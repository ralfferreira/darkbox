use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use darkbox_contracts::*;
use darkbox_domain::Run;
use darkbox_evidence::sanitize_text;
use serde_json::{Value, json};

use crate::bridge::{AdapterRequest, AdapterResult, call_adapter};
use crate::{ExecError, ExecResult, Executor, Outcome, StepOutcomeState, operation_str};

impl Executor {
    pub(crate) async fn browser_session(
        &self,
        run: &Run,
        step: &darkbox_domain::RunStepRecord,
        _cancelled: Arc<AtomicBool>,
    ) -> ExecResult<Outcome> {
        let identity_id: IdentityId = operation_str(step, "identity_id")
            .ok_or_else(|| ExecError::Invalid("identity_id is required".into()))?
            .parse()
            .map_err(|_| ExecError::Invalid("invalid identity_id".into()))?;
        let identity = self
            .db
            .get_identity(run.workspace_id, run.assessment_id, identity_id)
            .await?;
        let tenant = operation_str(step, "tenant").map(str::to_string);
        if let Some(tenant) = &tenant
            && !identity.tenant_keys.iter().any(|key| key == tenant)
        {
            return Err(ExecError::Blocked(format!(
                "identity {} is not a member of tenant {tenant}",
                identity.label
            )));
        }
        let base_url = operation_str(step, "base_url")
            .ok_or_else(|| ExecError::Invalid("base_url is required".into()))?;
        let opened = self
            .browser_open(
                run,
                &json!({
                    "url": base_url,
                    "identity_id": identity_id,
                    "tenant": tenant,
                }),
            )
            .await?;
        if opened.get("status").and_then(Value::as_str) != Some("ok") {
            return Ok(Outcome::blocked(
                "browser_session_blocked",
                opened
                    .get("summary")
                    .and_then(Value::as_str)
                    .unwrap_or("browser session could not be opened"),
            ));
        }
        let context_key = opened
            .get("session")
            .and_then(|value| value.get("context_key"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let mut login = identity.login_config.clone();
        if login.is_null() || !login.is_object() {
            login = json!({});
        }
        if login.get("login_url").is_none() {
            let login_path = operation_str(step, "login_path").unwrap_or("/login");
            if let Some(object) = login.as_object_mut() {
                object.insert(
                    "login_url".into(),
                    json!(format!(
                        "{}{}",
                        base_url.trim_end_matches('/'),
                        if login_path.starts_with('/') {
                            login_path.to_string()
                        } else {
                            format!("/{login_path}")
                        }
                    )),
                );
            }
        }
        let result = self
            .browser_command(run, &context_key, "login", &json!({ "login": login }))
            .await?;
        let status = result
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("error");
        let login_state = result
            .get("login_state")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let evidence_ids: Vec<EvidenceId> = result
            .get("evidence_ids")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .filter_map(|value| value.parse::<EvidenceId>().ok())
                    .collect()
            })
            .unwrap_or_default();
        let state = match (status, login_state) {
            ("ok", "authenticated") => StepOutcomeState::Succeeded,
            ("blocked", _) => StepOutcomeState::Blocked,
            _ => StepOutcomeState::Failed,
        };
        Ok(Outcome {
            state,
            effect: EffectStatus::NotApplicable,
            summary: result
                .get("summary")
                .and_then(Value::as_str)
                .unwrap_or("browser login")
                .to_string(),
            result: json!({
                "status": if state == StepOutcomeState::Succeeded { "succeeded" } else { "blocked" },
                "session": {
                    "context_key": context_key,
                    "state": login_state,
                    "tenant": tenant,
                },
                "evidence_ids": evidence_ids,
            }),
        })
    }

    pub(crate) async fn browser_flow(
        &self,
        run: &Run,
        step: &darkbox_domain::RunStepRecord,
        _cancelled: Arc<AtomicBool>,
    ) -> ExecResult<Outcome> {
        let flow_id: FlowId = match operation_str(step, "flow_id") {
            Some(raw) => raw
                .parse()
                .map_err(|_| ExecError::Invalid("invalid flow_id".into()))?,
            None => run
                .flow_id
                .ok_or_else(|| ExecError::Invalid("flow_id is required".into()))?,
        };
        let flow = self
            .db
            .get_flow(run.workspace_id, run.assessment_id, flow_id)
            .await?;
        let identity_id = flow
            .identity_id
            .or(run.identity_id)
            .ok_or_else(|| ExecError::Blocked("identity_required".into()))?;
        let identity = self
            .db
            .get_identity(run.workspace_id, run.assessment_id, identity_id)
            .await?;
        let (session_state, _) = self.db.session_state(identity_id).await?;
        if session_state == "expired" {
            self.db
                .update_flow_observation(flow_id, FlowState::Blocked, &[], &[])
                .await?;
            return Err(ExecError::Blocked("identity_expired".into()));
        }
        if flow.steps.iter().all(|item| {
            item.action_json.is_null()
                || item
                    .action_json
                    .as_object()
                    .map(|object| object.is_empty())
                    .unwrap_or(true)
        }) {
            self.db
                .update_flow_observation(flow_id, FlowState::Blocked, &[], &[])
                .await?;
            return Ok(Outcome::blocked(
                "flow_has_no_recorded_actions",
                "the flow has no recorded browser actions; reproduce it from a new exploration instead of replaying guessed GET requests",
            ));
        }
        let mut start_url = flow.steps.iter().find_map(|item| {
            item.action_json
                .get("url")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        });
        if start_url.is_none()
            && let Some(endpoint_id) = flow.steps.iter().find_map(|item| item.endpoint_id)
        {
            let endpoint = self
                .db
                .endpoint_by_id(run.workspace_id, run.assessment_id, endpoint_id)
                .await?;
            start_url = Some(format!(
                "{}://{}:{}{}",
                endpoint.scheme, endpoint.host, endpoint.port, endpoint.path
            ));
        }
        let Some(start_url) = start_url else {
            return Err(ExecError::Blocked(
                "flow has no recorded starting URL".into(),
            ));
        };
        let opened = self
            .browser_open(
                run,
                &json!({
                    "url": start_url,
                    "identity_id": identity_id,
                    "tenant": flow.tenant_key,
                }),
            )
            .await?;
        let status = opened
            .get("status")
            .and_then(|value| value.as_str())
            .unwrap_or("error");
        if status != "ok" {
            self.db
                .update_flow_observation(flow_id, FlowState::Blocked, &[], &[])
                .await?;
            return Ok(Outcome::blocked(
                "flow_session_blocked",
                opened
                    .get("summary")
                    .and_then(|value| value.as_str())
                    .unwrap_or("flow session could not be opened"),
            ));
        }
        let context_key = opened
            .get("session")
            .and_then(|value| value.get("context_key"))
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_string();
        let mut evidence_ids: Vec<EvidenceId> = opened
            .get("evidence_ids")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|value| value.as_str())
                    .filter_map(|value| value.parse().ok())
                    .collect()
            })
            .unwrap_or_default();
        let mut confirmed = Vec::new();
        let mut failed_reason = None;
        for item in &flow.steps {
            let action = &item.action_json;
            let kind = action
                .get("type")
                .or_else(|| action.get("kind"))
                .and_then(|value| value.as_str())
                .unwrap_or("click");
            if kind == "open" || action.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                if item.confirmed {
                    confirmed.push(item.ordinal);
                }
                continue;
            }
            let result = if kind == "login" {
                self.browser_command(run, &context_key, "login", &json!({ "login": {} }))
                    .await?
            } else {
                self.browser_command(run, &context_key, "act", &json!({ "action": action }))
                    .await?
            };
            if let Some(ids) = result
                .get("evidence_ids")
                .and_then(|value| value.as_array())
            {
                evidence_ids.extend(
                    ids.iter()
                        .filter_map(|value| value.as_str())
                        .filter_map(|value| value.parse::<EvidenceId>().ok()),
                );
            }
            let result_status = result
                .get("status")
                .and_then(|value| value.as_str())
                .unwrap_or("error");
            if result_status == "blocked" || result_status == "outcome_unknown" {
                failed_reason = Some(format!(
                    "step {} could not be confirmed: {}",
                    item.ordinal,
                    result
                        .get("summary")
                        .and_then(|value| value.as_str())
                        .unwrap_or("blocked")
                ));
                break;
            }
            let observation = result.get("observation").cloned().unwrap_or(Value::Null);
            let observed_text = observation
                .get("text")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let observed_url = observation
                .get("url")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let expectation = item.expected_result.trim();
            let matched = if matches!(
                kind,
                "click" | "fill" | "select" | "wait" | "scroll" | "press" | "back"
            ) {
                true
            } else {
                expectation.is_empty()
                    || observed_text.contains(expectation)
                    || observed_url.contains(expectation)
            };
            if matched {
                confirmed.push(item.ordinal);
            } else {
                failed_reason = Some(format!(
                    "step {} did not expose the recorded expected result",
                    item.ordinal
                ));
            }
        }
        let _ = self.browser_close(run, &context_key).await;
        let state = match &failed_reason {
            None => FlowState::Reproduced,
            Some(reason) => {
                if reason.contains("login") || reason.contains("authenticated") {
                    FlowState::Blocked
                } else {
                    FlowState::Observed
                }
            }
        };
        self.db
            .update_flow_observation(flow_id, state, &evidence_ids, &confirmed)
            .await?;
        let outcome_state = match state {
            FlowState::Reproduced => StepOutcomeState::Succeeded,
            FlowState::Blocked => StepOutcomeState::Blocked,
            _ => StepOutcomeState::Failed,
        };
        Ok(Outcome {
            state: outcome_state,
            effect: EffectStatus::NotApplicable,
            summary: match failed_reason {
                Some(reason) => {
                    format!("flow {flow_id} did not fully reproduce: {reason}")
                }
                None => format!(
                    "flow {flow_id} reproduced with {}/{} confirmed steps in a real browser",
                    confirmed.len(),
                    flow.steps.len()
                ),
            },
            result: json!({
                "flow_id": flow_id,
                "confirmed": confirmed,
                "evidence_ids": evidence_ids,
                "state": state,
                "identity": identity.label,
            }),
        })
    }

    pub(crate) async fn browser_call(
        &self,
        run: &Run,
        step: &darkbox_domain::RunStepRecord,
        kind: &str,
        payload: &Value,
        cancelled: Arc<AtomicBool>,
    ) -> ExecResult<AdapterResult> {
        let request = AdapterRequest {
            schema_version: 1,
            request_id: step.id.to_string(),
            kind: kind.to_string(),
            workspace_id: run.workspace_id.to_string(),
            assessment_id: run.assessment_id.to_string(),
            run_id: run.id.to_string(),
            step_id: step.id.to_string(),
            scope_version: run.policy_version,
            policy: &run.policy,
            payload,
            limits: crate::bridge::AdapterLimits {
                timeout_seconds: self.config().request_timeout_seconds * 6,
                max_output_bytes: 8 * 1024 * 1024,
                max_steps: 50,
            },
        };
        match call_adapter(&self.config().bridge, &request, cancelled).await {
            Ok(result) => Ok(result),
            Err(crate::bridge::BridgeError::Cancelled) => {
                Err(ExecError::Blocked("cancelled".into()))
            }
            Err(error) => Err(ExecError::Bridge(error)),
        }
    }

    pub(crate) async fn apply_browser_payload(
        &self,
        run: &Run,
        step: &darkbox_domain::RunStepRecord,
        identity_id: Option<IdentityId>,
        payload: &Value,
    ) -> ExecResult<(Outcome, Vec<EvidenceId>)> {
        let mut evidence_ids = Vec::new();
        if let Some(items) = payload.get("evidence").and_then(|v| v.as_array()) {
            for item in items {
                let summary = item
                    .get("summary")
                    .and_then(|v| v.as_str())
                    .unwrap_or("browser capture");
                let text = item.get("text").and_then(|v| v.as_str()).unwrap_or("");
                let sanitized = sanitize_text(text, 256 * 1024);
                let id = self
                    .store_evidence_text(
                        run,
                        step.id,
                        identity_id,
                        EvidenceKind::HttpExchange,
                        summary,
                        &sanitized.text,
                    )
                    .await?;
                evidence_ids.push(id);
            }
        }
        if let Some(items) = payload.get("observations").and_then(|v| v.as_array()) {
            for item in items {
                let subject_kind = item
                    .get("subject_kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("endpoint");
                let Some(subject_id) = item
                    .get("subject_id")
                    .and_then(|v| v.as_str())
                    .and_then(|v| v.parse::<uuid::Uuid>().ok())
                else {
                    continue;
                };
                let property = item
                    .get("property")
                    .and_then(|v| v.as_str())
                    .unwrap_or("browser.observation");
                let value = item.get("value").cloned().unwrap_or(Value::Null);
                let confidence = item
                    .get("confidence")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.8);
                self.add_observation(
                    run,
                    subject_kind,
                    subject_id,
                    property,
                    value,
                    confidence,
                    evidence_ids.last().copied(),
                )
                .await?;
            }
        }
        let status = payload
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("succeeded");
        let summary = payload
            .get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("browser step completed")
            .to_string();
        let state = match status {
            "blocked" => StepOutcomeState::Blocked,
            "failed" | "error" => StepOutcomeState::Failed,
            _ => StepOutcomeState::Succeeded,
        };
        Ok((
            Outcome {
                state,
                effect: EffectStatus::NotApplicable,
                summary,
                result: payload.clone(),
            },
            evidence_ids,
        ))
    }
}
