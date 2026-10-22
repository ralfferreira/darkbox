use axum::Json;
use axum::extract::{Path, State};
use darkbox_contracts::*;
use darkbox_domain::Run;
use darkbox_evidence::{sanitize_json_value, sanitize_text};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::routes::agent::save_checkpoint;
use crate::routes::core::parse_id;
use darkbox_storage::Principal;

use crate::{ApiError, AppState, Auth, PrincipalPermissions};

#[derive(Debug, Deserialize)]
pub struct OpenRequest {
    pub url: String,
    pub identity_id: IdentityId,
    #[serde(default)]
    pub tenant: Option<String>,
    #[serde(default)]
    pub decision: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ActRequest {
    pub session_key: String,
    pub action: Value,
    #[serde(default)]
    pub decision: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CommandRequest {
    pub session_key: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub login: Option<Value>,
    #[serde(default)]
    pub decision: Option<String>,
}

async fn running_run(
    state: &AppState,
    auth: &Principal,
    run_id: RunId,
) -> Result<(Run, AssessmentId), ApiError> {
    let run = state.db.get_run(auth.workspace_id, run_id).await?;
    if run.kind != RunKind::Agent {
        return Err(ApiError::invalid("browser actions belong to an agent run"));
    }
    if run.state != RunState::Running {
        return Err(ApiError::conflict(format!(
            "agent objective is {}; no new browser action may start",
            run.state
        )));
    }
    let (assessment_id, _, _, _, _) = state
        .db
        .agent_run_context(auth.workspace_id, run_id)
        .await?;
    Ok((run, assessment_id))
}

fn result_state(result: &Value) -> (&'static str, EffectStatus) {
    match result.get("status").and_then(Value::as_str) {
        Some("blocked") => ("blocked", EffectStatus::NotDispatched),
        Some("outcome_unknown") => ("failed", EffectStatus::OutcomeUnknown),
        Some("inconclusive") => ("failed", EffectStatus::NotApplicable),
        Some("failed") | Some("error") => ("failed", EffectStatus::NotApplicable),
        _ => ("succeeded", EffectStatus::NotApplicable),
    }
}

fn action_kind(action: &Value) -> String {
    action
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("act")
        .to_string()
}

#[allow(clippy::too_many_arguments)]
async fn prepare_action(
    state: &AppState,
    run: &Run,
    session_key: Option<&str>,
    kind: &str,
    action: &Value,
    effect_class: &str,
) -> Result<uuid::Uuid, ApiError> {
    let action = sanitize_json_value(action, 64 * 1024);
    let session_id = if let Some(key) = session_key {
        state
            .db
            .browser_session_by_context(run.workspace_id, run.assessment_id, key)
            .await?
            .map(|session| session.id)
    } else {
        None
    };
    Ok(state
        .db
        .start_agent_action(
            run.workspace_id,
            run.assessment_id,
            run.id,
            session_id,
            kind,
            &action,
            &json!({
                "phase": "prepared",
                "policy_version": run.policy_version,
            }),
            effect_class,
            None,
            45,
        )
        .await?)
}

#[allow(clippy::too_many_arguments)]
async fn record(
    state: &AppState,
    auth: &Principal,
    run: &Run,
    action_id: uuid::Uuid,
    session_key: Option<&str>,
    kind: &str,
    action: &Value,
    result: Value,
    decision: Option<String>,
) -> Result<Value, ApiError> {
    let result = sanitize_json_value(&result, 512 * 1024);
    let action = sanitize_json_value(action, 64 * 1024);
    let decision = decision.map(|value| sanitize_text(&value, 2_000).text);
    let mut stored_action = action;
    if let Some(object) = stored_action.as_object_mut() {
        if matches!(kind, "open" | "navigate" | "login") {
            object.insert("type".into(), json!(kind));
        }
        if let Some(key) = result
            .get("session")
            .and_then(|value| value.get("context_key"))
            .and_then(Value::as_str)
        {
            object.insert("context_key".into(), json!(key));
        }
    }
    let (action_state, effect) = result_state(&result);
    let mut observation = result.get("observation").cloned();
    if let Some(observation) = observation.as_mut().and_then(Value::as_object_mut) {
        if let Some(session) = result.get("session") {
            observation.insert("session".into(), session.clone());
        }
        if let Some(engine) = result.get("engine") {
            observation.insert("engine".into(), engine.clone());
        }
    }
    let evidence_ids: Vec<EvidenceId> = result
        .get("evidence_ids")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .filter_map(|value| value.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    let summary = result
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or("browser action");
    state
        .db
        .finish_agent_action(
            action_id,
            action_state,
            effect,
            summary,
            observation.as_ref(),
            &evidence_ids,
            Some(&stored_action),
        )
        .await?;
    let url = observation
        .as_ref()
        .and_then(|value| value.get("url"))
        .and_then(Value::as_str);
    let title = observation
        .as_ref()
        .and_then(|value| value.get("title"))
        .and_then(Value::as_str);
    let event_payload = json!({
        "action_id": action_id,
        "kind": kind,
        "action": stored_action,
        "state": action_state,
        "effect_status": effect.as_str(),
        "summary": summary,
        "url": url,
        "title": title,
        "evidence_ids": evidence_ids,
        "decision": decision,
        "session_key": session_key,
    });
    state
        .db
        .append_agent_event(
            auth.workspace_id,
            run.id,
            "agent.action",
            event_payload.clone(),
        )
        .await?;
    if let Some(decision) = event_payload.get("decision").and_then(Value::as_str)
        && !decision.trim().is_empty()
    {
        state
            .db
            .append_agent_event(
                auth.workspace_id,
                run.id,
                "agent.decision",
                json!({ "summary": decision, "action_id": action_id, "url": url }),
            )
            .await
            .ok();
    }
    if effect == EffectStatus::OutcomeUnknown {
        state
            .db
            .mark_agent_outcome_unknown(
                run.id,
                "browser action may have reached the application but its result was not observed",
            )
            .await?;
        save_checkpoint(state, auth, run.id, "outcome_unknown").await?;
    }
    Ok(json!({
        "action_id": action_id,
        "state": action_state,
        "effect_status": effect.as_str(),
        "result": result,
    }))
}

#[allow(clippy::too_many_arguments)]
async fn record_blocked(
    state: &AppState,
    auth: &Principal,
    run: &Run,
    action_id: uuid::Uuid,
    kind: &str,
    action: &Value,
    message: &str,
    decision: Option<String>,
) -> Result<(), ApiError> {
    let action = sanitize_json_value(action, 64 * 1024);
    let message = sanitize_text(message, 4_000).text;
    let (stored_effect, _) = state
        .db
        .agent_action_status(action_id)
        .await?
        .unwrap_or(("running".into(), "not_dispatched".into()));
    let effect = if stored_effect == "dispatched" {
        EffectStatus::OutcomeUnknown
    } else {
        EffectStatus::NotDispatched
    };
    state
        .db
        .finish_agent_action(
            action_id,
            "failed",
            effect,
            &message,
            None,
            &[],
            Some(&action),
        )
        .await?;
    if effect == EffectStatus::OutcomeUnknown {
        state
            .db
            .mark_agent_outcome_unknown(
                run.id,
                "browser action was dispatched but its result was not confirmed",
            )
            .await?;
        save_checkpoint(state, auth, run.id, "outcome_unknown").await?;
    }
    state
        .db
        .append_agent_event(
            auth.workspace_id,
            run.id,
            "agent.action",
            json!({
                "action_id": action_id,
                "kind": kind,
                "action": action,
                "state": "failed",
                "effect_status": effect.as_str(),
                "summary": message,
                "evidence_ids": [],
                "decision": decision,
            }),
        )
        .await?;
    Ok(())
}

pub async fn open(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<OpenRequest>,
) -> Result<Json<Value>, ApiError> {
    auth.require_write()?;
    let run_id = parse_id::<RunId>(&id)?;
    let (run, _) = running_run(&state, &auth, run_id).await?;
    let args = json!({
        "url": req.url,
        "identity_id": req.identity_id,
        "tenant": req.tenant,
    });
    let action_id = prepare_action(&state, &run, None, "open", &args, "query").await?;
    let result = match state
        .executor
        .browser_open(&run, &args, Some(action_id))
        .await
    {
        Ok(result) => result,
        Err(error) => {
            let message = error.to_string();
            record_blocked(
                &state,
                &auth,
                &run,
                action_id,
                "open",
                &args,
                &message,
                req.decision.clone(),
            )
            .await?;
            return Err(error.into());
        }
    };
    let recorded = record(
        &state,
        &auth,
        &run,
        action_id,
        None,
        "open",
        &args,
        result,
        req.decision,
    )
    .await?;
    Ok(Json(recorded))
}

pub async fn navigate(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<CommandRequest>,
) -> Result<Json<Value>, ApiError> {
    auth.require_write()?;
    let run_id = parse_id::<RunId>(&id)?;
    let (run, _) = running_run(&state, &auth, run_id).await?;
    let args = json!({ "url": req.url, "session_key": req.session_key });
    let action_id = prepare_action(
        &state,
        &run,
        Some(&req.session_key),
        "navigate",
        &args,
        "query",
    )
    .await?;
    let result = match state
        .executor
        .browser_command(&run, &req.session_key, "navigate", &args, Some(action_id))
        .await
    {
        Ok(result) => result,
        Err(error) => {
            let message = error.to_string();
            record_blocked(
                &state,
                &auth,
                &run,
                action_id,
                "navigate",
                &args,
                &message,
                req.decision.clone(),
            )
            .await?;
            return Err(error.into());
        }
    };
    let recorded = record(
        &state,
        &auth,
        &run,
        action_id,
        Some(&req.session_key),
        "navigate",
        &args,
        result,
        req.decision,
    )
    .await?;
    Ok(Json(recorded))
}

pub async fn observe(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<CommandRequest>,
) -> Result<Json<Value>, ApiError> {
    auth.require_write()?;
    let run_id = parse_id::<RunId>(&id)?;
    let (run, _) = running_run(&state, &auth, run_id).await?;
    let args = json!({ "session_key": req.session_key });
    let action_id = prepare_action(
        &state,
        &run,
        Some(&req.session_key),
        "observe",
        &args,
        "query",
    )
    .await?;
    let result = match state
        .executor
        .browser_command(&run, &req.session_key, "observe", &args, Some(action_id))
        .await
    {
        Ok(result) => result,
        Err(error) => {
            let message = error.to_string();
            record_blocked(
                &state,
                &auth,
                &run,
                action_id,
                "observe",
                &args,
                &message,
                req.decision.clone(),
            )
            .await?;
            return Err(error.into());
        }
    };
    let recorded = record(
        &state,
        &auth,
        &run,
        action_id,
        Some(&req.session_key),
        "observe",
        &args,
        result,
        req.decision,
    )
    .await?;
    Ok(Json(recorded))
}

pub async fn act(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<ActRequest>,
) -> Result<Json<Value>, ApiError> {
    auth.require_write()?;
    let run_id = parse_id::<RunId>(&id)?;
    let (run, _) = running_run(&state, &auth, run_id).await?;
    let kind = action_kind(&req.action);
    let args = json!({ "session_key": req.session_key, "action": req.action });
    let action_id = prepare_action(
        &state,
        &run,
        Some(&req.session_key),
        &kind,
        &req.action,
        "query",
    )
    .await?;
    let result = match state
        .executor
        .browser_command(&run, &req.session_key, "act", &args, Some(action_id))
        .await
    {
        Ok(result) => result,
        Err(error) => {
            let message = error.to_string();
            record_blocked(
                &state,
                &auth,
                &run,
                action_id,
                &kind,
                &req.action,
                &message,
                req.decision.clone(),
            )
            .await?;
            return Err(error.into());
        }
    };
    let recorded = record(
        &state,
        &auth,
        &run,
        action_id,
        Some(&req.session_key),
        &kind,
        &req.action,
        result,
        req.decision,
    )
    .await?;
    Ok(Json(recorded))
}

pub async fn login(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<CommandRequest>,
) -> Result<Json<Value>, ApiError> {
    auth.require_write()?;
    let run_id = parse_id::<RunId>(&id)?;
    let (run, _) = running_run(&state, &auth, run_id).await?;
    let login = req.login.unwrap_or(Value::Null);
    let args = json!({ "session_key": req.session_key, "login": login });
    let action_id = prepare_action(
        &state,
        &run,
        Some(&req.session_key),
        "login",
        &args,
        "session",
    )
    .await?;
    let result = match state
        .executor
        .browser_command(&run, &req.session_key, "login", &args, Some(action_id))
        .await
    {
        Ok(result) => result,
        Err(error) => {
            let message = error.to_string();
            record_blocked(
                &state,
                &auth,
                &run,
                action_id,
                "login",
                &args,
                &message,
                req.decision.clone(),
            )
            .await?;
            return Err(error.into());
        }
    };
    let recorded = record(
        &state,
        &auth,
        &run,
        action_id,
        Some(&req.session_key),
        "login",
        &args,
        result,
        req.decision,
    )
    .await?;
    Ok(Json(recorded))
}

pub async fn screenshot(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<CommandRequest>,
) -> Result<Json<Value>, ApiError> {
    auth.require_write()?;
    let run_id = parse_id::<RunId>(&id)?;
    let (run, _) = running_run(&state, &auth, run_id).await?;
    let args = json!({ "session_key": req.session_key });
    let action_id = prepare_action(
        &state,
        &run,
        Some(&req.session_key),
        "screenshot",
        &args,
        "query",
    )
    .await?;
    let result = match state
        .executor
        .browser_command(&run, &req.session_key, "screenshot", &args, Some(action_id))
        .await
    {
        Ok(result) => result,
        Err(error) => {
            let message = error.to_string();
            record_blocked(
                &state,
                &auth,
                &run,
                action_id,
                "screenshot",
                &args,
                &message,
                req.decision.clone(),
            )
            .await?;
            return Err(error.into());
        }
    };
    let recorded = record(
        &state,
        &auth,
        &run,
        action_id,
        Some(&req.session_key),
        "screenshot",
        &args,
        result,
        req.decision,
    )
    .await?;
    Ok(Json(recorded))
}

pub async fn traffic(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<CommandRequest>,
) -> Result<Json<Value>, ApiError> {
    auth.require_write()?;
    let run_id = parse_id::<RunId>(&id)?;
    let (run, _) = running_run(&state, &auth, run_id).await?;
    let traffic_args = json!({ "session_key": req.session_key });
    let action_id = prepare_action(
        &state,
        &run,
        Some(&req.session_key),
        "traffic",
        &traffic_args,
        "query",
    )
    .await?;
    let result = match state.executor.browser_traffic(&run, &req.session_key).await {
        Ok(result) => result,
        Err(error) => {
            let message = error.to_string();
            record_blocked(
                &state,
                &auth,
                &run,
                action_id,
                "traffic",
                &traffic_args,
                &message,
                req.decision.clone(),
            )
            .await?;
            return Err(error.into());
        }
    };
    let recorded = record(
        &state,
        &auth,
        &run,
        action_id,
        Some(&req.session_key),
        "traffic",
        &json!({ "session_key": req.session_key }),
        result,
        req.decision,
    )
    .await?;
    Ok(Json(recorded))
}

pub async fn close(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<CommandRequest>,
) -> Result<Json<Value>, ApiError> {
    auth.require_write()?;
    let run_id = parse_id::<RunId>(&id)?;
    let (run, _) = running_run(&state, &auth, run_id).await?;
    let close_args = json!({ "session_key": req.session_key });
    let action_id = prepare_action(
        &state,
        &run,
        Some(&req.session_key),
        "close",
        &close_args,
        "query",
    )
    .await?;
    let result = match state.executor.browser_close(&run, &req.session_key).await {
        Ok(result) => result,
        Err(error) => {
            let message = error.to_string();
            record_blocked(
                &state,
                &auth,
                &run,
                action_id,
                "close",
                &close_args,
                &message,
                None,
            )
            .await?;
            return Err(error.into());
        }
    };
    let recorded = record(
        &state,
        &auth,
        &run,
        action_id,
        Some(&req.session_key),
        "close",
        &json!({ "session_key": req.session_key }),
        result,
        None,
    )
    .await?;
    Ok(Json(recorded))
}

pub async fn progress(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let run_id = parse_id::<RunId>(&id)?;
    let run = state.db.run_summary(auth.workspace_id, run_id).await?;
    if run.kind != RunKind::Agent {
        return Err(ApiError::invalid("not an agent run"));
    }
    let actions = state.db.list_agent_actions(run_id).await?;
    let calls = state.db.list_model_calls(run_id).await?;
    let now_page = state
        .db
        .agent_run_context(auth.workspace_id, run_id)
        .await?;
    let evidence = state
        .db
        .list_evidence_for_run(auth.workspace_id, run.assessment_id, run_id, 200)
        .await?;
    let latest_observation = actions
        .iter()
        .rev()
        .find_map(|action| action.observation.clone());
    Ok(Json(json!({
        "run": run,
        "objective": now_page.4.get("objective").cloned().unwrap_or(Value::Null),
        "identity_id": now_page.1,
        "actions": actions,
        "model_calls": calls,
        "evidence": evidence,
        "current": {
            "url": latest_observation.as_ref().and_then(|o| o.get("url")).cloned().unwrap_or(Value::Null),
            "title": latest_observation.as_ref().and_then(|o| o.get("title")).cloned().unwrap_or(Value::Null),
            "observation": latest_observation,
        },
        "pending": if matches!(run.state, RunState::Running | RunState::Queued) { "exploring" } else { "none" },
        "blocked": if matches!(run.state, RunState::WaitingOperator | RunState::Interrupted) { "operator_attention" } else { "" },
    })))
}
