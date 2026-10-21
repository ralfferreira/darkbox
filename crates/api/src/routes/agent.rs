use axum::Json;
use axum::extract::{Path, State};
use darkbox_contracts::*;
use darkbox_memory::ContextBuilder;
use darkbox_storage::{NewRun, NewStep};
use serde_json::json;

use crate::routes::core::parse_id;
use crate::{ApiError, AppState, Auth, PrincipalPermissions};

pub async fn create_turn(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<AgentTurnRequest>,
) -> Result<Json<AgentTurnResponse>, ApiError> {
    auth.require_write()?;
    let assessment_id = parse_id::<AssessmentId>(&id)?;
    let objective = req.objective.trim();
    if objective.is_empty() {
        return Err(ApiError::invalid("an agent turn needs an objective"));
    }
    if objective.chars().count() > 4000 {
        return Err(ApiError::invalid("objective is too long"));
    }
    let assessment = state
        .db
        .get_assessment(auth.workspace_id, assessment_id)
        .await?;
    let scope = state
        .db
        .get_scope(auth.workspace_id, assessment_id, None)
        .await?;
    if let Some(identity_id) = req.identity_id {
        state
            .db
            .get_identity(auth.workspace_id, assessment_id, identity_id)
            .await?;
    }
    let max_steps = req.max_steps.clamp(1, 8);
    let token_budget = req.token_budget.clamp(500, 16_000);
    let mut budget = scope.policy.limits.clone();
    budget.max_model_calls = budget.max_model_calls.min(i64::from(max_steps) + 1).max(1);
    let idempotency_key = format!("agent-{}", uuid::Uuid::now_v7());
    let run = state
        .db
        .create_agent_run(
            auth.workspace_id,
            &NewRun {
                assessment_id,
                kind: RunKind::Agent,
                policy: scope.policy.clone(),
                policy_version: scope.version,
                budget,
                procedure_key: None,
                flow_id: None,
                identity_id: req.identity_id,
                idempotency_key: idempotency_key.clone(),
                note: objective.to_string(),
                steps: vec![NewStep {
                    kind: StepKind::AgentTurn,
                    operation: json!({
                        "objective": objective,
                        "identity_id": req.identity_id,
                        "max_steps": max_steps,
                        "token_budget": token_budget,
                        "model": std::env::var("DARKBOX_MODEL")
                            .unwrap_or_else(|_| "configured-by-agent-service".into()),
                    }),
                    summary: "agent turn owned by the TypeScript AI SDK service".into(),
                }],
            },
            &auth.operator,
        )
        .await?;
    state
        .db
        .audit(
            auth.workspace_id,
            Some(assessment.id),
            &auth.operator,
            "agent.turn.create",
            &run.id.to_string(),
            objective,
            &idempotency_key,
        )
        .await?;
    let context = ContextBuilder::new(&state.db, auth.workspace_id, assessment_id, req.identity_id)
        .with_budget(token_budget)
        .build()
        .await
        .map_err(|error| ApiError::invalid(error.to_string()))?;
    Ok(Json(AgentTurnResponse {
        run: state.db.run_summary(auth.workspace_id, run.id).await?,
        context,
    }))
}

const ALLOWED_AGENT_EVENTS: &[&str] = &[
    "agent.message",
    "agent.tool_call",
    "agent.tool_result",
    "agent.usage",
    "agent.note",
];

pub async fn append_event(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<AgentEventRequest>,
) -> Result<Json<EventDto>, ApiError> {
    auth.require_write()?;
    let run_id = parse_id::<RunId>(&id)?;
    if !ALLOWED_AGENT_EVENTS.contains(&req.event_type.as_str()) {
        return Err(ApiError::invalid("unsupported agent event type"));
    }
    let (assessment_id, _, run_state, _, _) = state
        .db
        .agent_run_context(auth.workspace_id, run_id)
        .await?;
    if run_state != "running" {
        return Err(ApiError::conflict("agent run is not running"));
    }
    let _ = assessment_id;
    let event = state
        .db
        .append_agent_event(auth.workspace_id, run_id, &req.event_type, req.payload)
        .await?;
    Ok(Json(event))
}

pub async fn finish(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<AgentFinishRequest>,
) -> Result<Json<AgentFinishResponse>, ApiError> {
    auth.require_write()?;
    let run_id = parse_id::<RunId>(&id)?;
    let summary = req.summary.trim();
    if summary.is_empty() {
        return Err(ApiError::invalid("agent finish needs a summary"));
    }
    if summary.chars().count() > 8000 {
        return Err(ApiError::invalid("agent summary is too long"));
    }
    if req.next_steps.len() > 10 || req.evidence_ids.len() > 20 {
        return Err(ApiError::invalid(
            "too many next steps or evidence references",
        ));
    }
    let (assessment_id, identity_id, run_state, policy_version, operation) = state
        .db
        .agent_run_context(auth.workspace_id, run_id)
        .await?;
    if run_state != "running" {
        return Err(ApiError::conflict("agent run is not running"));
    }
    for evidence_id in &req.evidence_ids {
        state
            .db
            .get_evidence(auth.workspace_id, assessment_id, *evidence_id)
            .await?;
    }
    let events = state.db.events_for_run(run_id, 500).await?;
    let mut body = String::new();
    body.push_str("# Agent turn\n\n");
    body.push_str(&format!(
        "Objective: {}\n\n",
        operation
            .get("objective")
            .and_then(|value| value.as_str())
            .unwrap_or("")
    ));
    body.push_str(&format!(
        "Model: {}\n\nPolicy version: {}\n\n",
        operation
            .get("model")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown"),
        policy_version
    ));
    body.push_str("## Summary\n\n");
    body.push_str(summary);
    body.push_str("\n\n");
    if !req.next_steps.is_empty() {
        body.push_str("## Next steps\n\n");
        for step in &req.next_steps {
            body.push_str(&format!("- {step}\n"));
        }
        body.push('\n');
    }
    body.push_str("## Tool trail\n\n");
    for event in events
        .iter()
        .filter(|event| event.event_type == "agent.tool_call")
    {
        let name = event
            .payload
            .get("name")
            .and_then(|value| value.as_str())
            .unwrap_or("tool");
        let tool_summary = event
            .payload
            .get("summary")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        let evidence = event
            .payload
            .get("evidence_ids")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|value| value.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        body.push_str(&format!("- `{name}`: {tool_summary}"));
        if !evidence.is_empty() {
            body.push_str(&format!(" (evidence: {evidence})"));
        }
        body.push('\n');
    }
    body.push_str(
        "\n_This page is a derived narrative. Observations and evidence remain canonical; a hypothesis is not a confirmed finding._\n",
    );
    let mut references = vec![MemoryReferenceDto {
        kind: "run".into(),
        id: run_id.to_string(),
        label: "agent turn".into(),
    }];
    for evidence_id in &req.evidence_ids {
        references.push(MemoryReferenceDto {
            kind: "evidence".into(),
            id: evidence_id.to_string(),
            label: "evidence".into(),
        });
    }
    if let Some(identity_id) = identity_id {
        references.push(MemoryReferenceDto {
            kind: "identity".into(),
            id: identity_id.to_string(),
            label: "agent identity".into(),
        });
    }
    let run_text = run_id.to_string();
    let short = &run_text[run_text.len().saturating_sub(8)..];
    let page = state
        .db
        .save_memory_page(
            auth.workspace_id,
            assessment_id,
            &format!("agent-{short}"),
            &format!("Agent turn {short}"),
            &body,
            &references,
            "current",
            "agent-service",
        )
        .await?;
    state
        .db
        .finish_agent_run(
            auth.workspace_id,
            run_id,
            "succeeded",
            Some(&format!(
                "agent summary: {}",
                summary.chars().take(200).collect::<String>()
            )),
            Some(summary),
            Some(&page.slug),
        )
        .await?;
    state
        .db
        .audit(
            auth.workspace_id,
            Some(assessment_id),
            &auth.operator,
            "agent.turn.finish",
            &run_id.to_string(),
            summary,
            &run_id.to_string(),
        )
        .await?;
    let run = state.db.run_summary(auth.workspace_id, run_id).await?;
    Ok(Json(AgentFinishResponse { run, page }))
}

pub async fn usage(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<AgentUsageRequest>,
) -> Result<Json<RunSummary>, ApiError> {
    auth.require_write()?;
    let run_id = parse_id::<RunId>(&id)?;
    let (_, _, run_state, _, _) = state
        .db
        .agent_run_context(auth.workspace_id, run_id)
        .await?;
    if run_state != "running" {
        return Err(ApiError::conflict("agent run is not running"));
    }
    if !state.db.reserve_budget(run_id, "model_calls", 1).await? {
        return Err(ApiError::blocked("budget_exhausted: model_calls"));
    }
    let input_rate = std::env::var("DARKBOX_MODEL_INPUT_MICROS_PER_MTOK")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0);
    let output_rate = std::env::var("DARKBOX_MODEL_OUTPUT_MICROS_PER_MTOK")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0);
    let cost = req.input_tokens.max(0) * input_rate / 1_000_000
        + req.output_tokens.max(0) * output_rate / 1_000_000;
    if cost > 0 && !state.db.reserve_budget(run_id, "cost_micros", cost).await? {
        return Err(ApiError::blocked("budget_exhausted: cost_micros"));
    }
    if req.total_tokens > 0 {
        state
            .db
            .append_agent_event(
                auth.workspace_id,
                run_id,
                "agent.usage",
                serde_json::json!({
                    "input_tokens": req.input_tokens,
                    "output_tokens": req.output_tokens,
                    "total_tokens": req.total_tokens,
                    "model": req.model,
                    "cost_micros": cost,
                }),
            )
            .await?;
    }
    Ok(Json(state.db.run_summary(auth.workspace_id, run_id).await?))
}

#[derive(Debug, serde::Deserialize)]
pub struct AbortRequest {
    #[serde(default)]
    pub reason: String,
}

pub async fn abort(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<AbortRequest>,
) -> Result<Json<RunSummary>, ApiError> {
    auth.require_write()?;
    let run_id = parse_id::<RunId>(&id)?;
    let reason = if req.reason.trim().is_empty() {
        "agent service aborted the turn"
    } else {
        req.reason.trim()
    };
    state
        .db
        .finish_agent_run(
            auth.workspace_id,
            run_id,
            "failed",
            Some(reason),
            None,
            None,
        )
        .await?;
    Ok(Json(state.db.run_summary(auth.workspace_id, run_id).await?))
}
