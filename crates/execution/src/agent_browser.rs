//! Interactive browser operations used by the agent loop.
//!
//! Every operation stays inside one agent run: the run is the objective, its
//! pinned policy is sent to the adapter, and its budget pays for the step.
//! Partial actions are recorded by the API; this module produces the semantic
//! result and stores the evidence.

use base64::Engine as _;
use darkbox_contracts::*;
use darkbox_domain::Run;
use darkbox_evidence::sanitize_text;
use serde_json::{Value, json};

use crate::browser_runtime::OpenContext;
use crate::{ExecError, ExecResult, Executor};

impl Executor {
    pub async fn browser_open(&self, run: &Run, args: &Value) -> ExecResult<Value> {
        let identity_id: IdentityId = args
            .get("identity_id")
            .or_else(|| args.get("identity"))
            .and_then(Value::as_str)
            .ok_or_else(|| ExecError::Invalid("identity_id is required to open a browser".into()))?
            .parse()
            .map_err(|_| ExecError::Invalid("invalid identity_id".into()))?;
        let identity = self
            .db
            .get_identity(run.workspace_id, run.assessment_id, identity_id)
            .await?;
        let tenant = args
            .get("tenant")
            .and_then(Value::as_str)
            .map(str::to_string);
        if let Some(tenant) = &tenant
            && !identity.tenant_keys.iter().any(|key| key == tenant)
        {
            return Err(ExecError::Blocked(format!(
                "identity {} is not a member of tenant {tenant}",
                identity.label
            )));
        }
        let url = args
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| ExecError::Invalid("url is required to open a browser".into()))?;
        let (_, decision) = self
            .authorize_url(
                run,
                url,
                ActionClass::Session,
                darkbox_policy::TargetRole::NavigationDependency,
            )
            .await?;
        if !decision.allowed {
            return Err(ExecError::Blocked(format!(
                "browser navigation denied by {}",
                decision.rule
            )));
        }
        if !self.db.reserve_budget(run.id, "browser_steps", 1).await? {
            return Err(ExecError::Blocked("budget_exhausted: browser_steps".into()));
        }
        let context_key = format!(
            "ctx-{}-{}-{}",
            run.id,
            identity_id,
            uuid::Uuid::now_v7().simple()
        );
        let identity_payload = json!({
            "label": identity.label,
            "role": identity.role_label,
            "tenant_keys": identity.tenant_keys,
            "secret_ref": identity.secret_ref,
            "login_config": identity.login_config,
        });
        let opened = self
            .browser_runtime()
            .open(OpenContext {
                context_key: &context_key,
                workspace_id: run.workspace_id,
                assessment_id: run.assessment_id,
                run_id: run.id,
                policy: &run.policy,
                identity: identity_payload,
                tenant: tenant.clone(),
                url,
            })
            .await
            .map_err(|error| ExecError::Blocked(format!("browser_open: {error}")))?;
        let status = opened
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("error");
        if status != "ok" {
            return Ok(json!({
                "status": "blocked",
                "summary": opened.get("summary").and_then(Value::as_str).unwrap_or("browser could not open the page"),
                "reason": opened.get("error").cloned().unwrap_or(Value::Null),
                "engine": opened.get("engine").cloned().unwrap_or(Value::Null),
            }));
        }
        let engine = opened
            .get("engine")
            .and_then(|v| v.get("engine"))
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        let session_id = self
            .db
            .record_session(
                run.workspace_id,
                run.assessment_id,
                identity_id,
                tenant.as_deref(),
                &context_key,
                "open",
                None,
            )
            .await?;
        self.db
            .update_session_engine(run.workspace_id, &context_key, &engine)
            .await?;
        let (evidence_ids, observation) = self
            .capture_browser_page(run, Some(identity_id), &opened, true, "browser open")
            .await?;
        if let Some(observation) = &observation {
            self.db
                .update_session_observation(run.workspace_id, &context_key, observation)
                .await?;
        }
        Ok(json!({
            "status": "ok",
            "summary": opened.get("summary").and_then(Value::as_str).unwrap_or("browser opened"),
            "engine": opened.get("engine").cloned().unwrap_or(Value::Null),
            "session": {
                "session_id": session_id,
                "context_key": context_key,
                "identity_id": identity_id,
                "identity_label": identity.label,
                "tenant": tenant,
                "state": "open",
            },
            "observation": observation,
            "evidence_ids": evidence_ids,
        }))
    }

    pub async fn browser_command(
        &self,
        run: &Run,
        session_key: &str,
        kind: &str,
        args: &Value,
    ) -> ExecResult<Value> {
        let session = self
            .db
            .browser_session_by_context(run.workspace_id, run.assessment_id, session_key)
            .await?
            .ok_or_else(|| ExecError::Blocked("browser_session_not_found".into()))?;
        let identity_id = Some(session.identity_id);
        if kind == "navigate"
            && let Some(url) = args.get("url").and_then(Value::as_str)
        {
            {
                let (_, decision) = self
                    .authorize_url(
                        run,
                        url,
                        ActionClass::Query,
                        darkbox_policy::TargetRole::TestTarget,
                    )
                    .await?;
                if !decision.allowed {
                    return Ok(serde_json::json!({
                        "status": "blocked",
                        "summary": format!("navigation denied by {} before dispatch", decision.rule),
                        "reason": decision.rule,
                        "observation": Value::Null,
                    }));
                }
            }
        }
        if kind == "login" {
            let login = args.get("login").cloned().unwrap_or(Value::Null);
            if let Some(url) = login.get("login_url").and_then(Value::as_str) {
                let (_, decision) = self
                    .authorize_url(
                        run,
                        url,
                        ActionClass::Session,
                        darkbox_policy::TargetRole::NavigationDependency,
                    )
                    .await?;
                if !decision.allowed {
                    return Ok(serde_json::json!({
                        "status": "blocked",
                        "summary": format!("login navigation denied by {} before dispatch", decision.rule),
                        "reason": decision.rule,
                        "observation": Value::Null,
                    }));
                }
            }
        }
        if !self.db.reserve_budget(run.id, "browser_steps", 1).await? {
            return Ok(serde_json::json!({
                "status": "blocked",
                "summary": "browser step budget exhausted before dispatch",
                "reason": "budget_exhausted: browser_steps",
                "observation": Value::Null,
            }));
        }
        let response = self
            .browser_runtime()
            .command(
                kind,
                session_key,
                run.workspace_id,
                run.assessment_id,
                run.id,
                &run.policy,
                args.clone(),
            )
            .await
            .map_err(|error| ExecError::Blocked(format!("browser_{kind}: {error}")))?;
        let state = response
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("error");
        if state == "blocked" {
            if response.get("error").and_then(Value::as_str) == Some("session_lost") {
                let _ = self
                    .db
                    .update_session_state(run.workspace_id, session_key, "lost")
                    .await;
            }
            if let Some(login_state) = response.get("login_state").and_then(Value::as_str) {
                let db_state = match login_state {
                    "authenticated" => "authenticated",
                    "login_failed" => "expired",
                    "expired" | "login_required" => "expired",
                    "lost" => "lost",
                    _ => "unknown",
                };
                self.db
                    .update_session_state(run.workspace_id, session_key, db_state)
                    .await?;
            }
            return Ok(json!({
                "status": "blocked",
                "summary": response.get("summary").and_then(Value::as_str).unwrap_or("browser action blocked"),
                "reason": response.get("error").cloned().unwrap_or(Value::Null),
                "login_state": response.get("login_state").cloned().unwrap_or(Value::Null),
                "observation": response.get("observation").cloned().unwrap_or(Value::Null),
            }));
        }
        if state == "outcome_unknown" {
            let (evidence_ids, observation) = self
                .capture_browser_page(run, identity_id, &response, true, "browser outcome unknown")
                .await?;
            return Ok(json!({
                "status": "outcome_unknown",
                "summary": response.get("summary").and_then(Value::as_str).unwrap_or("external result unknown"),
                "reason": response.get("error").cloned().unwrap_or(Value::Null),
                "observation": observation,
                "evidence_ids": evidence_ids,
            }));
        }
        let take_screenshot = matches!(kind, "login" | "act" | "navigate" | "screenshot");
        let (evidence_ids, observation) = self
            .capture_browser_page(run, identity_id, &response, take_screenshot, kind)
            .await?;
        if let Some(observation) = &observation {
            self.db
                .update_session_observation(run.workspace_id, session_key, observation)
                .await?;
        }
        let login_state = response
            .get("login_state")
            .or_else(|| response.get("session").and_then(|s| s.get("state")))
            .and_then(Value::as_str)
            .map(str::to_string);
        if let Some(login_state) = &login_state {
            let db_state = match login_state.as_str() {
                "authenticated" => "authenticated",
                "login_failed" => "expired",
                "expired" | "login_required" => "expired",
                "lost" => "lost",
                _ => "unknown",
            };
            self.db
                .update_session_state(run.workspace_id, session_key, db_state)
                .await?;
        }
        let identity_label = self
            .db
            .get_identity(run.workspace_id, run.assessment_id, session.identity_id)
            .await
            .map(|identity| identity.label)
            .unwrap_or_else(|_| session.identity_id.to_string());
        let mut out = response;
        if let Some(object) = out.as_object_mut() {
            object.remove("screenshot_b64");
            object.insert("evidence_ids".into(), json!(evidence_ids));
            if let Some(observation) = observation {
                object.insert("observation".into(), observation);
            }
            object.insert(
                "session".into(),
                json!({
                    "session_id": session.id,
                    "context_key": session_key,
                    "identity_id": identity_id,
                    "identity_label": identity_label,
                    "tenant": session.tenant_key,
                    "state": login_state.clone().unwrap_or_else(|| "unknown".into()),
                }),
            );
        }
        Ok(out)
    }

    pub async fn browser_traffic(&self, run: &Run, session_key: &str) -> ExecResult<Value> {
        let session = self
            .db
            .browser_session_by_context(run.workspace_id, run.assessment_id, session_key)
            .await?
            .ok_or_else(|| ExecError::Blocked("browser_session_not_found".into()))?;
        let response = self
            .browser_runtime()
            .command(
                "traffic",
                session_key,
                run.workspace_id,
                run.assessment_id,
                run.id,
                &run.policy,
                json!({}),
            )
            .await
            .map_err(|error| ExecError::Blocked(format!("browser_traffic: {error}")))?;
        let trace = serde_json::to_string_pretty(&response).unwrap_or_default();
        let sanitized = sanitize_text(&trace, 256 * 1024);
        let evidence = self
            .store_evidence_text(
                run,
                StepId::new(),
                Some(session.identity_id),
                EvidenceKind::BrowserTrace,
                "browser traffic",
                &sanitized.text,
            )
            .await?;
        Ok(json!({
            "status": "ok",
            "summary": response.get("summary").and_then(Value::as_str).unwrap_or("browser traffic"),
            "traffic": response.get("traffic").cloned().unwrap_or(Value::Null),
            "blocked_requests": response.get("blocked_requests").cloned().unwrap_or(Value::Null),
            "evidence_ids": [evidence],
        }))
    }

    pub async fn browser_close(&self, run: &Run, session_key: &str) -> ExecResult<Value> {
        let closed = self
            .browser_runtime()
            .close(session_key)
            .await
            .map_err(|error| ExecError::Blocked(format!("browser_close: {error}")))?;
        self.db
            .update_session_state(run.workspace_id, session_key, "closed")
            .await?;
        Ok(json!({
            "status": "ok",
            "summary": if closed { "browser context closed" } else { "browser context was already gone" },
        }))
    }

    pub async fn browser_abort(&self, session_key: &str) {
        let _ = self.browser_runtime().abort(session_key).await;
    }

    async fn capture_browser_page(
        &self,
        run: &Run,
        identity_id: Option<IdentityId>,
        response: &Value,
        take_screenshot: bool,
        label: &str,
    ) -> ExecResult<(Vec<EvidenceId>, Option<Value>)> {
        let mut evidence_ids = Vec::new();
        let observation = response.get("observation").cloned();
        if let Some(observation) = &observation {
            let trace = json!({
                "label": label,
                "url": observation.get("url"),
                "title": observation.get("title"),
                "ready_state": observation.get("ready_state"),
                "headings": observation.get("headings"),
                "interactive": observation.get("interactive"),
                "tables": observation.get("tables"),
                "auth": observation.get("auth"),
                "blocked_requests": observation.get("blocked_requests"),
                "text": observation.get("text"),
            });
            let text = serde_json::to_string_pretty(&trace).unwrap_or_default();
            let sanitized = sanitize_text(&text, 256 * 1024);
            let id = self
                .store_evidence_text(
                    run,
                    StepId::new(),
                    identity_id,
                    EvidenceKind::BrowserTrace,
                    &format!(
                        "{label}: {}",
                        observation
                            .get("url")
                            .and_then(Value::as_str)
                            .unwrap_or("page")
                    ),
                    &sanitized.text,
                )
                .await?;
            evidence_ids.push(id);
            self.record_page_observation(run, identity_id, observation, id)
                .await?;
        }
        if take_screenshot
            && let Some(encoded) = response.get("screenshot_b64").and_then(Value::as_str)
            && let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded)
        {
            let id = self
                .store_evidence_bytes(
                    run,
                    Some(StepId::new()),
                    identity_id,
                    EvidenceKind::BrowserScreenshot,
                    &format!("{label} screenshot"),
                    &bytes,
                    response
                        .get("content_type")
                        .and_then(Value::as_str)
                        .unwrap_or("image/jpeg"),
                )
                .await?;
            evidence_ids.push(id);
        }
        Ok((evidence_ids, observation))
    }

    async fn record_page_observation(
        &self,
        run: &Run,
        identity_id: Option<IdentityId>,
        observation: &Value,
        evidence_id: EvidenceId,
    ) -> ExecResult<()> {
        let Some(url) = observation.get("url").and_then(Value::as_str) else {
            return Ok(());
        };
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok(());
        }
        let Ok(key) = darkbox_domain::EndpointKey::parse(url, "GET") else {
            return Ok(());
        };
        let endpoint = self
            .db
            .upsert_candidate(
                run.workspace_id,
                run.assessment_id,
                &key,
                "browser",
                "observed through an interactive session",
                Some(run.id),
                identity_id,
            )
            .await?;
        self.db
            .mark_observed(
                run.workspace_id,
                run.assessment_id,
                endpoint,
                EndpointState::Observed,
                identity_id,
                "browser",
            )
            .await?;
        self.add_observation(
            run,
            "endpoint",
            endpoint.as_uuid(),
            "browser.page",
            json!({
                "url": url,
                "title": observation.get("title"),
                "interactive_count": observation
                    .get("interactive")
                    .and_then(Value::as_array)
                    .map(|items| items.len())
                    .unwrap_or(0),
                "evidence": evidence_id,
            }),
            0.95,
            Some(evidence_id),
        )
        .await?;
        if let Some(auth) = observation.get("auth") {
            self.add_observation(
                run,
                "endpoint",
                endpoint.as_uuid(),
                "browser.auth",
                auth.clone(),
                0.9,
                Some(evidence_id),
            )
            .await?;
        }
        Ok(())
    }

    pub fn operation_value<'a>(&self, value: &'a Value, key: &str) -> Option<&'a str> {
        value.get(key).and_then(Value::as_str)
    }
}
