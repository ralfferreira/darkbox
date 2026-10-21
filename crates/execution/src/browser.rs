use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use darkbox_contracts::*;
use darkbox_domain::Run;
use darkbox_evidence::sanitize_text;
use darkbox_policy::TargetRole;
use serde_json::{Value, json};
use url::Url;

use crate::bridge::{AdapterRequest, AdapterResult, call_adapter};
use crate::{ExecError, ExecResult, Executor, Outcome, StepOutcomeState, operation_str};

impl Executor {
    pub(crate) async fn browser_session(
        &self,
        run: &Run,
        step: &darkbox_domain::RunStepRecord,
        cancelled: Arc<AtomicBool>,
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
        let (_, decision) = self
            .authorize_url(
                run,
                base_url,
                ActionClass::Session,
                TargetRole::NavigationDependency,
            )
            .await?;
        if !decision.allowed {
            return Err(ExecError::Blocked(format!(
                "session base URL denied by {}",
                decision.rule
            )));
        }
        if !self.db.reserve_budget(run.id, "browser_steps", 1).await? {
            return Err(ExecError::Blocked("budget_exhausted: browser_steps".into()));
        }
        let payload = json!({
            "identity": {
                "label": identity.label,
                "role": identity.role_label,
                "tenant_keys": identity.tenant_keys,
                "secret_ref": identity.secret_ref,
            },
            "tenant": tenant,
            "base_url": base_url,
            "login_path": operation_str(step, "login_path").unwrap_or("/login"),
            "username": operation_str(step, "username").unwrap_or(&identity.label),
        });
        let result = self
            .browser_call(run, step, "browser.session", &payload, cancelled)
            .await?;
        let (mut outcome, _evidence) = self
            .apply_browser_payload(run, step, Some(identity_id), &result.payload)
            .await?;
        if outcome.state == StepOutcomeState::Succeeded {
            let session = result
                .payload
                .get("session")
                .cloned()
                .unwrap_or(Value::Null);
            let state = session
                .get("state")
                .and_then(|v| v.as_str())
                .unwrap_or("authenticated");
            let context_key = session
                .get("context_key")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("ctx-{}-{}", identity_id, uuid::Uuid::now_v7()));
            let expires_at = session
                .get("expires_at")
                .and_then(|v| v.as_str())
                .and_then(|v| chrono::DateTime::parse_from_rfc3339(v).ok())
                .map(|v| v.with_timezone(&chrono::Utc));
            self.db
                .record_session(
                    run.workspace_id,
                    run.assessment_id,
                    identity_id,
                    tenant.as_deref(),
                    &context_key,
                    state,
                    expires_at,
                )
                .await?;
            outcome.summary = format!("session {} for {}", state, identity.label);
        }
        Ok(outcome)
    }

    pub(crate) async fn browser_flow(
        &self,
        run: &Run,
        step: &darkbox_domain::RunStepRecord,
        cancelled: Arc<AtomicBool>,
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
        let mut steps = Vec::new();
        for step_def in &flow.steps {
            let Some(endpoint_id) = step_def.endpoint_id else {
                continue;
            };
            let endpoint = self
                .db
                .endpoint_by_id(run.workspace_id, run.assessment_id, endpoint_id)
                .await?;
            let url = format!(
                "{}://{}:{}{}",
                endpoint.scheme, endpoint.host, endpoint.port, endpoint.path
            );
            let (_, decision) = self
                .authorize_url(run, &url, ActionClass::Query, TargetRole::TestTarget)
                .await?;
            if !decision.allowed {
                return Err(ExecError::Blocked(format!(
                    "flow step {} denied by {}",
                    step_def.ordinal, decision.rule
                )));
            }
            steps.push(json!({
                "ordinal": step_def.ordinal,
                "intent": step_def.intent,
                "expected_result": step_def.expected_result,
                "method": "GET",
                "url": url,
            }));
        }
        if steps.is_empty() {
            return Err(ExecError::Invalid(
                "flow has no endpoint steps to reproduce".into(),
            ));
        }
        if !self
            .db
            .reserve_budget(run.id, "browser_steps", steps.len() as i64)
            .await?
        {
            return Err(ExecError::Blocked("budget_exhausted: browser_steps".into()));
        }
        let base_url = steps
            .first()
            .and_then(|v| v.get("url"))
            .and_then(|v| v.as_str())
            .and_then(|v| Url::parse(v).ok())
            .map(|u| {
                format!(
                    "{}://{}:{}",
                    u.scheme(),
                    u.host_str().unwrap_or(""),
                    u.port_or_known_default().unwrap_or(80)
                )
            })
            .unwrap_or_default();
        let payload = json!({
            "identity": {
                "label": identity.label,
                "role": identity.role_label,
                "tenant_keys": identity.tenant_keys,
                "secret_ref": identity.secret_ref,
            },
            "tenant": flow.tenant_key,
            "base_url": base_url,
            "login_path": operation_str(step, "login_path").unwrap_or("/login"),
            "username": operation_str(step, "username").unwrap_or(&identity.label),
            "flow": { "id": flow.id, "name": flow.name },
            "steps": steps,
        });
        let result = self
            .browser_call(run, step, "browser.flow", &payload, cancelled)
            .await?;
        let (outcome, evidence_ids) = self
            .apply_browser_payload(run, step, Some(identity_id), &result.payload)
            .await?;
        let confirmed: Vec<i64> = result
            .payload
            .get("steps")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter(|item| {
                        item.get("confirmed")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false)
                    })
                    .filter_map(|item| item.get("ordinal").and_then(|v| v.as_i64()))
                    .collect()
            })
            .unwrap_or_default();
        let state = match outcome.state {
            StepOutcomeState::Succeeded => FlowState::Reproduced,
            StepOutcomeState::Blocked => FlowState::Blocked,
            _ => FlowState::Observed,
        };
        self.db
            .update_flow_observation(flow.id, state, &evidence_ids, &confirmed)
            .await?;
        Ok(outcome)
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
