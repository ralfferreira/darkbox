use axum::Json;
use axum::extract::{Path, State};
use darkbox_contracts::*;
use darkbox_memory::ContextBuilder;
use darkbox_storage::{NewRun, NewStep};
use serde_json::{Value, json};

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
    let max_steps = req.max_steps.clamp(1, 12);
    let token_budget = req.token_budget.clamp(500, 16_000);
    let start_url = match req.start_url.clone() {
        Some(url) if !url.trim().is_empty() => Some(url),
        _ => {
            let page = state
                .db
                .inventory(
                    auth.workspace_id,
                    assessment_id,
                    &darkbox_storage::InventoryQuery {
                        limit: 1,
                        ..Default::default()
                    },
                )
                .await?;
            page.endpoints.first().map(|endpoint| {
                format!(
                    "{}://{}:{}{}",
                    endpoint.scheme, endpoint.host, endpoint.port, endpoint.path
                )
            })
        }
    };
    let start_url = start_url.or_else(|| Some(state.config.lab_url.clone()));
    if let Some(url) = &start_url
        && !url.starts_with("http://")
        && !url.starts_with("https://")
    {
        return Err(ApiError::invalid("start_url must be an HTTP URL"));
    }
    if let Some(previous) = req.continue_from {
        let previous_run = state.db.get_run(auth.workspace_id, previous).await?;
        if previous_run.assessment_id != assessment_id || previous_run.kind != RunKind::Agent {
            return Err(ApiError::invalid(
                "continue_from must be an agent objective in this assessment",
            ));
        }
    }
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
                        "continue_from": req.continue_from,
                        "start_url": start_url,
                        "model": req.model.clone().unwrap_or_else(|| std::env::var("DARKBOX_MODEL")
                            .unwrap_or_else(|_| "configured-by-agent-service".into())),
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
    let mut context =
        ContextBuilder::new(&state.db, auth.workspace_id, assessment_id, req.identity_id)
            .with_budget(token_budget)
            .build()
            .await
            .map_err(|error| ApiError::invalid(error.to_string()))?;
    if let Some(url) = &start_url {
        context.sections.insert(
            1,
            ContextSectionDto {
                title: "Starting URL".into(),
                body: format!(
                    "{url}\n\nOpen this URL first in a real browser. Paths, controls and filters must be discovered from the rendered page, not assumed."
                ),
                reference_kind: None,
            },
        );
    }
    if let Some(previous) = req.continue_from {
        let actions = state.db.list_agent_actions(previous).await?;
        let previous_run = state.db.get_run(auth.workspace_id, previous).await?;
        let previous_summary = state.db.run_summary(auth.workspace_id, previous).await?;
        let mut body = format!(
            "This objective continues run {previous} ({}). Do not repeat an action whose result is unknown.\n",
            previous_run.state
        );
        for action in actions.iter().rev().take(25).rev() {
            body.push_str(&format!(
                "- {} `{}` [{}] {} — {}\n",
                action.ordinal,
                action.kind,
                action.state,
                action
                    .observation
                    .as_ref()
                    .and_then(|value| value.get("url"))
                    .and_then(|value| value.as_str())
                    .unwrap_or(""),
                action.summary
            ));
        }
        if previous_summary.effect_unknown {
            body.push_str("\nAn external effect has an unknown outcome. Reconcile it before repeating that action.\n");
        }
        context.sections.insert(
            1,
            ContextSectionDto {
                title: "Objective continuation".into(),
                body,
                reference_kind: Some("run".into()),
            },
        );
        context.references.push(MemoryReferenceDto {
            kind: "run".into(),
            id: previous.to_string(),
            label: "continued objective".into(),
        });
    }
    Ok(Json(AgentTurnResponse {
        run: state.db.run_summary(auth.workspace_id, run.id).await?,
        context,
        start_url: start_url.clone(),
    }))
}

const ALLOWED_AGENT_EVENTS: &[&str] = &[
    "agent.message",
    "agent.tool_call",
    "agent.tool_result",
    "agent.usage",
    "agent.note",
    "agent.decision",
    "agent.action",
    "agent.browser_session",
    "agent.progress",
    "agent.checkpoint",
    "agent.flow",
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
    let outcome = req.outcome.trim().to_lowercase();
    if !matches!(
        outcome.as_str(),
        "succeeded" | "partial" | "blocked" | "inconclusive"
    ) {
        return Err(ApiError::invalid(
            "outcome must be succeeded, partial, blocked or inconclusive",
        ));
    }
    if req.next_steps.len() > 10 || req.evidence_ids.len() > 20 {
        return Err(ApiError::invalid(
            "too many next steps or evidence references",
        ));
    }
    if outcome == "succeeded" && req.evidence_ids.is_empty() {
        return Err(ApiError::conflict(
            "a successful objective needs at least one evidence id from this run",
        ));
    }
    let (assessment_id, identity_id, run_state, policy_version, operation) = state
        .db
        .agent_run_context(auth.workspace_id, run_id)
        .await?;
    if run_state != "running" {
        return Err(ApiError::conflict("agent run is not running"));
    }
    let mut evidence_ids = Vec::new();
    for evidence_id in &req.evidence_ids {
        let record = state
            .db
            .get_evidence(auth.workspace_id, assessment_id, *evidence_id)
            .await?;
        if record.run_id != Some(run_id) {
            return Err(ApiError::conflict(format!(
                "evidence {evidence_id} does not belong to this objective"
            )));
        }
        evidence_ids.push(*evidence_id);
    }
    let events = state.db.events_for_run(run_id, 1000).await?;
    let actions = state.db.list_agent_actions(run_id).await?;
    let decisions: Vec<String> = events
        .iter()
        .filter(|event| event.event_type == "agent.decision")
        .filter_map(|event| {
            event
                .payload
                .get("summary")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .collect();
    let mut flow_id = None;
    if !actions.is_empty() {
        let steps: Vec<CreateFlowStep> = actions
            .iter()
            .filter(|action| {
                !matches!(
                    action.kind.as_str(),
                    "observe" | "screenshot" | "traffic" | "close"
                )
            })
            .map(|action| CreateFlowStep {
                intent: format!("{}: {}", action.kind, action.summary),
                expected_result: action
                    .observation
                    .as_ref()
                    .and_then(|value| value.get("url"))
                    .and_then(|value| value.as_str())
                    .unwrap_or("observed page")
                    .to_string(),
                endpoint_id: None,
                action_class: if action.kind == "login" {
                    Some(ActionClass::Session)
                } else {
                    Some(ActionClass::Query)
                },
                action_json: action.action.clone(),
                confirmed: action.state == "succeeded",
            })
            .collect();
        if !steps.is_empty() {
            let flow = state
                .db
                .create_flow(
                    auth.workspace_id,
                    assessment_id,
                    &CreateFlowRequest {
                        name: format!("Discovered objective {run_id}"),
                        identity_id,
                        tenant_key: None,
                        preconditions: vec![format!("policy v{policy_version}")],
                        steps,
                        state: Some(if outcome == "succeeded" {
                            FlowState::Reproduced
                        } else {
                            FlowState::Observed
                        }),
                        evidence_ids: evidence_ids.clone(),
                    },
                    "agent",
                )
                .await?;
            flow_id = Some(flow.id);
        }
    }
    let mut body = String::new();
    body.push_str("# Agent objective\n\n");
    body.push_str(&format!(
        "Objective: {}\n\nOutcome: **{outcome}**\n\n",
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
    if !decisions.is_empty() {
        body.push_str("## Decisions\n\n");
        for decision in decisions.iter().take(20) {
            body.push_str(&format!("- {decision}\n"));
        }
        body.push('\n');
    }
    body.push_str("## Action trail\n\n");
    for action in &actions {
        body.push_str(&format!(
            "- {} `{}` [{}] {} — {}\n",
            action.ordinal,
            action.kind,
            action.state,
            action
                .observation
                .as_ref()
                .and_then(|value| value.get("url"))
                .and_then(|value| value.as_str())
                .unwrap_or(""),
            action.summary
        ));
        if !action.evidence_ids.is_empty() {
            body.push_str(&format!(
                "  evidence: {}\n",
                action
                    .evidence_ids
                    .iter()
                    .map(|id| id.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    if !req.next_steps.is_empty() {
        body.push_str("\n## Next steps\n\n");
        for step in &req.next_steps {
            body.push_str(&format!("- {step}\n"));
        }
    }
    if outcome != "succeeded" {
        body.push_str(
            "\nThis objective is not complete. The checkpoint above preserves what was observed.\n",
        );
    }
    body.push_str(
        "\n_This page is a derived narrative. Observations and evidence remain canonical; a hypothesis is not a confirmed finding._\n",
    );
    let mut references = vec![MemoryReferenceDto {
        kind: "run".into(),
        id: run_id.to_string(),
        label: "agent objective".into(),
    }];
    for evidence_id in &evidence_ids {
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
    if let Some(flow_id) = flow_id {
        references.push(MemoryReferenceDto {
            kind: "flow".into(),
            id: flow_id.to_string(),
            label: "discovered flow".into(),
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
            &format!("Agent objective {short}"),
            &body,
            &references,
            if outcome == "succeeded" {
                "current"
            } else {
                "checkpoint"
            },
            "agent-service",
        )
        .await?;
    let persisted_outcome = if outcome == "succeeded" {
        "succeeded"
    } else {
        "failed"
    };
    let reason = if outcome == "succeeded" {
        format!(
            "agent summary: {}",
            summary.chars().take(200).collect::<String>()
        )
    } else {
        format!("agent_{outcome}: {summary}")
    };
    state
        .db
        .finish_agent_run(
            auth.workspace_id,
            run_id,
            persisted_outcome,
            Some(&reason),
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
    Ok(Json(AgentFinishResponse { run, page, flow_id }))
}

pub async fn usage(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<AgentUsageRequest>,
) -> Result<Json<RunSummary>, ApiError> {
    auth.require_write()?;
    let run_id = parse_id::<RunId>(&id)?;
    let (assessment_id, _, run_state, _, _) = state
        .db
        .agent_run_context(auth.workspace_id, run_id)
        .await?;
    if run_state != "running" {
        return Err(ApiError::conflict("agent run is not running"));
    }
    let call_id = req
        .call_id
        .clone()
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ApiError::invalid("call_id is required for usage accounting"))?;
    let usage_known = req
        .usage_known
        .unwrap_or(req.total_tokens > 0 || req.input_tokens > 0 || req.output_tokens > 0);
    if req.phase == "reserve" {
        if !state
            .db
            .reserve_model_call(
                auth.workspace_id,
                assessment_id,
                run_id,
                &call_id,
                &req.model,
            )
            .await?
        {
            return Err(ApiError::blocked("budget_exhausted: model_calls"));
        }
        state
            .db
            .append_agent_event(
                auth.workspace_id,
                run_id,
                "agent.usage",
                json!({
                    "phase": "reserve",
                    "call_id": call_id,
                    "model": req.model,
                    "reserved_model_call": 1,
                    "usage_known": false,
                }),
            )
            .await?;
        return Ok(Json(state.db.run_summary(auth.workspace_id, run_id).await?));
    }
    let input_rate = std::env::var("DARKBOX_MODEL_INPUT_MICROS_PER_MTOK")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0);
    let output_rate = std::env::var("DARKBOX_MODEL_OUTPUT_MICROS_PER_MTOK")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(0);
    let cost = if usage_known {
        Some(
            req.input_tokens.max(0) * input_rate / 1_000_000
                + req.output_tokens.max(0) * output_rate / 1_000_000,
        )
    } else {
        None
    };
    let settled = state
        .db
        .settle_model_call(
            run_id,
            &call_id,
            usage_known.then_some(req.input_tokens.max(0)),
            usage_known.then_some(req.output_tokens.max(0)),
            usage_known.then_some(req.total_tokens.max(0)),
            cost,
            usage_known,
        )
        .await?;
    if let Some(settled) = settled {
        state
            .db
            .append_agent_event(
                auth.workspace_id,
                run_id,
                "agent.usage",
                json!({
                    "phase": "settle",
                    "call_id": call_id,
                    "model": req.model,
                    "usage_known": usage_known,
                    "input_tokens": settled.get("input_tokens"),
                    "output_tokens": settled.get("output_tokens"),
                    "total_tokens": settled.get("total_tokens"),
                    "cost_micros": settled.get("cost_micros"),
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
    let (_, _, run_state, _, _) = state
        .db
        .agent_run_context(auth.workspace_id, run_id)
        .await?;
    if run_state != "running" {
        return Ok(Json(state.db.run_summary(auth.workspace_id, run_id).await?));
    }
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
    let _ = save_checkpoint(&state, &auth, run_id, reason).await;
    Ok(Json(state.db.run_summary(auth.workspace_id, run_id).await?))
}

pub async fn checkpoint(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let run_id = parse_id::<RunId>(&id)?;
    let (assessment_id, identity_id, run_state, _, operation) = state
        .db
        .agent_run_context(auth.workspace_id, run_id)
        .await?;
    let actions = state.db.list_agent_actions(run_id).await?;
    let pages = state
        .db
        .list_memory_pages(auth.workspace_id, assessment_id)
        .await?;
    let run_text = run_id.to_string();
    let short = &run_text[run_text.len().saturating_sub(8)..];
    let expected = format!("checkpoint-{short}");
    let checkpoint = pages
        .iter()
        .find(|page| page.slug == expected)
        .or_else(|| {
            pages
                .iter()
                .find(|page| page.validity == "checkpoint" && page.slug.ends_with(short))
        })
        .cloned();
    Ok(Json(json!({
        "run_id": run_id,
        "state": run_state,
        "objective": operation.get("objective").cloned().unwrap_or(Value::Null),
        "identity_id": identity_id,
        "actions": actions,
        "checkpoint": checkpoint,
    })))
}

pub(crate) async fn save_checkpoint(
    state: &AppState,
    auth: &darkbox_storage::Principal,
    run_id: RunId,
    reason: &str,
) -> Result<String, ApiError> {
    let (assessment_id, identity_id, _, policy_version, operation) = state
        .db
        .agent_run_context(auth.workspace_id, run_id)
        .await?;
    let actions = state.db.list_agent_actions(run_id).await?;
    let events = state.db.events_for_run(run_id, 1000).await?;
    let decisions: Vec<String> = events
        .iter()
        .filter(|event| event.event_type == "agent.decision")
        .filter_map(|event| {
            event
                .payload
                .get("summary")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .collect();
    let evidence_ids: Vec<String> = actions
        .iter()
        .flat_map(|action| action.evidence_ids.iter().map(|id| id.to_string()))
        .collect();
    let mut body = String::new();
    body.push_str("# Objective checkpoint\\n\\n");
    body.push_str(&format!(
        "Objective: {}\\n\\nCheckpoint reason: `{reason}`\\n\\nPolicy version: {}\\n\\nIdentity: {}\\n\\n",
        operation
            .get("objective")
            .and_then(|value| value.as_str())
            .unwrap_or(""),
        policy_version,
        identity_id
            .map(|id| id.to_string())
            .unwrap_or_else(|| "none".into())
    ));
    body.push_str("## Completed actions\\n\\n");
    if actions.is_empty() {
        body.push_str("No browser action was completed.\\n\\n");
    }
    for action in &actions {
        body.push_str(&format!(
            "- {} `{}` [{}] {} — {}\\n",
            action.ordinal,
            action.kind,
            action.state,
            action
                .observation
                .as_ref()
                .and_then(|value| value.get("url"))
                .and_then(|value| value.as_str())
                .unwrap_or(""),
            action.summary
        ));
    }
    if !decisions.is_empty() {
        body.push_str("\\n## Decisions\\n\\n");
        for decision in decisions.iter().take(20) {
            body.push_str(&format!("- {decision}\\n"));
        }
    }
    body.push_str("\\n## Pending work\\n\\n");
    body.push_str(
        "The objective was interrupted. A new session may continue from the observed state below; it must not repeat an effect whose outcome is unknown.\\n\\n",
    );
    if !evidence_ids.is_empty() {
        body.push_str("## Evidence\\n\\n");
        for id in evidence_ids.iter().take(40) {
            body.push_str(&format!("- evidence `{id}`\\n"));
        }
    }
    let run_text = run_id.to_string();
    let short = &run_text[run_text.len().saturating_sub(8)..];
    let page = state
        .db
        .save_memory_page(
            auth.workspace_id,
            assessment_id,
            &format!("checkpoint-{short}"),
            &format!("Objective checkpoint {short}"),
            &body,
            &[MemoryReferenceDto {
                kind: "run".into(),
                id: run_id.to_string(),
                label: "interrupted objective".into(),
            }],
            "checkpoint",
            "agent-service",
        )
        .await?;
    state
        .db
        .append_agent_event(
            auth.workspace_id,
            run_id,
            "agent.checkpoint",
            json!({
                "slug": page.slug,
                "reason": reason,
                "actions": actions.len(),
                "evidence_ids": evidence_ids,
            }),
        )
        .await
        .ok();
    Ok(page.slug)
}
