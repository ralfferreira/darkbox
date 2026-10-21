use std::collections::VecDeque;

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use axum::response::sse::{Event, KeepAlive, Sse};
use darkbox_contracts::*;
use darkbox_storage::{NewRun, NewStep};
use serde::Deserialize;
use serde_json::json;

use crate::routes::core::parse_id;
use crate::{ApiError, AppState, Auth, PrincipalPermissions};

#[derive(Debug, Deserialize)]
pub struct RunsParams {
    pub limit: Option<i64>,
    pub assessment_id: Option<String>,
    pub after: Option<i64>,
}

pub async fn list_runs(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Query(params): Query<RunsParams>,
) -> Result<Json<Vec<RunSummary>>, ApiError> {
    let id = parse_id::<AssessmentId>(&id)?;
    Ok(Json(
        state
            .db
            .list_runs(auth.workspace_id, id, params.limit.unwrap_or(50))
            .await?,
    ))
}

pub async fn list_runs_all(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Query(params): Query<RunsParams>,
) -> Result<Json<Vec<RunSummary>>, ApiError> {
    let id = params
        .assessment_id
        .ok_or_else(|| ApiError::invalid("assessment_id is required"))?;
    let id = parse_id::<AssessmentId>(&id)?;
    state.db.assessment_summary(auth.workspace_id, id).await?;
    Ok(Json(
        state
            .db
            .list_runs(auth.workspace_id, id, params.limit.unwrap_or(50))
            .await?,
    ))
}

pub async fn create_run(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Json(req): Json<CreateRunRequest>,
) -> Result<Json<RunSummary>, ApiError> {
    auth.require_write()?;
    let assessment = state
        .db
        .get_assessment(auth.workspace_id, req.assessment_id)
        .await?;
    let scope = state
        .db
        .get_scope(auth.workspace_id, req.assessment_id, None)
        .await?;
    if let Some(identity_id) = req.identity_id {
        state
            .db
            .get_identity(auth.workspace_id, req.assessment_id, identity_id)
            .await?;
    }
    for identity_id in &req.identity_ids {
        state
            .db
            .get_identity(auth.workspace_id, req.assessment_id, *identity_id)
            .await?;
    }
    if let Some(flow_id) = req.flow_id {
        state
            .db
            .get_flow(auth.workspace_id, req.assessment_id, flow_id)
            .await?;
    }
    let budget = match &req.budget_override {
        Some(override_budget) => {
            if override_budget.max_requests > scope.policy.limits.max_requests
                || override_budget.max_browser_steps > scope.policy.limits.max_browser_steps
                || override_budget.max_duration_seconds > scope.policy.limits.max_duration_seconds
            {
                return Err(ApiError::blocked(
                    "a run budget cannot exceed the policy budget",
                ));
            }
            override_budget.clone()
        }
        None => scope.policy.limits.clone(),
    };

    let steps = build_steps(&state, &auth, &req, &assessment).await?;
    if steps.is_empty() {
        return Err(ApiError::invalid("this run has no executable steps"));
    }
    let idempotency_key = if req.idempotency_key.trim().is_empty() {
        uuid::Uuid::now_v7().to_string()
    } else {
        req.idempotency_key.clone()
    };
    let run = state
        .db
        .create_run(
            auth.workspace_id,
            NewRun {
                assessment_id: req.assessment_id,
                kind: req.kind,
                policy: scope.policy.clone(),
                policy_version: scope.version,
                budget,
                procedure_key: req.procedure_key.clone(),
                flow_id: req.flow_id,
                identity_id: req.identity_id,
                idempotency_key: idempotency_key.clone(),
                note: req.note.clone(),
                steps,
            },
        )
        .await?;
    state
        .db
        .audit(
            auth.workspace_id,
            Some(req.assessment_id),
            &auth.operator,
            "run.create",
            &run.id.to_string(),
            &req.note,
            &idempotency_key,
        )
        .await?;
    state.kick_worker();
    Ok(Json(state.db.run_summary(auth.workspace_id, run.id).await?))
}

async fn build_steps(
    state: &AppState,
    auth: &darkbox_storage::Principal,
    req: &CreateRunRequest,
    assessment: &darkbox_domain::Assessment,
) -> Result<Vec<NewStep>, ApiError> {
    let mut steps = Vec::new();
    match req.kind {
        RunKind::Discovery => {
            if req.endpoint_ids.is_empty() {
                return Err(ApiError::invalid(
                    "discovery runs need endpoint_ids; use import-urls for new URLs",
                ));
            }
            steps.push(NewStep {
                kind: StepKind::HttpProbe,
                operation: json!({ "endpoint_ids": req.endpoint_ids }),
                summary: format!("observe {} candidates", req.endpoint_ids.len()),
            });
        }
        RunKind::Observation => {
            if req.endpoint_ids.is_empty() {
                return Err(ApiError::invalid(
                    "observation runs need at least one endpoint_id",
                ));
            }
            for endpoint_id in req.endpoint_ids.iter().take(50) {
                state
                    .db
                    .endpoint_by_id(auth.workspace_id, req.assessment_id, *endpoint_id)
                    .await?;
                steps.push(NewStep {
                    kind: StepKind::HttpObserve,
                    operation: json!({ "endpoint_id": endpoint_id }),
                    summary: format!("observe endpoint {endpoint_id}"),
                });
            }
        }
        RunKind::Session => {
            let identity_id = req
                .identity_id
                .ok_or_else(|| ApiError::invalid("session runs need identity_id"))?;
            let base_url = req
                .base_url
                .clone()
                .unwrap_or_else(|| state.config.lab_url.clone());
            steps.push(NewStep {
                kind: StepKind::BrowserSession,
                operation: json!({
                    "identity_id": identity_id,
                    "tenant": req.tenant,
                    "base_url": base_url,
                    "login_path": req.login_path.clone().unwrap_or_else(|| "/login".into()),
                }),
                summary: format!("establish isolated session for {identity_id}"),
            });
        }
        RunKind::FlowReproduction => {
            let flow_id = req
                .flow_id
                .ok_or_else(|| ApiError::invalid("flow reproduction needs flow_id"))?;
            let flow = state
                .db
                .get_flow(auth.workspace_id, req.assessment_id, flow_id)
                .await?;
            steps.push(NewStep {
                kind: StepKind::BrowserFlow,
                operation: json!({
                    "flow_id": flow_id,
                    "base_url": req.base_url.clone().unwrap_or_else(|| state.config.lab_url.clone()),
                }),
                summary: format!("reproduce flow '{}'", flow.name),
            });
        }
        RunKind::Procedure => {
            let key = req
                .procedure_key
                .clone()
                .ok_or_else(|| ApiError::invalid("procedure runs need procedure_key"))?;
            let procedure = state.db.get_procedure(&key).await?;
            match procedure.id.as_str() {
                "http.configuration_inventory" => {
                    if req.endpoint_ids.is_empty() {
                        return Err(ApiError::invalid(
                            "configuration inventory needs at least one endpoint",
                        ));
                    }
                    for endpoint_id in req.endpoint_ids.iter().take(50) {
                        steps.push(NewStep {
                            kind: StepKind::ProcedureStep,
                            operation: json!({
                                "procedure_key": key,
                                "target_kind": "endpoint",
                                "target_id": endpoint_id,
                                "identity_id": req.identity_id,
                            }),
                            summary: format!("{} on {}", procedure.name, endpoint_id),
                        });
                    }
                }
                "role.visibility_comparison" => {
                    if req.identity_ids.len() < 2 {
                        return Err(ApiError::invalid(
                            "visibility comparison needs two identities",
                        ));
                    }
                    steps.push(NewStep {
                        kind: StepKind::ProcedureStep,
                        operation: json!({
                            "procedure_key": key,
                            "target_kind": "assessment",
                            "target_id": req.assessment_id,
                            "identity_ids": req.identity_ids,
                            "checks": req.checks,
                            "base_url": req.base_url.clone().unwrap_or_else(|| state.config.lab_url.clone()),
                        }),
                        summary: "compare visibility across two identities".into(),
                    });
                }
                "flow.reproduction" => {
                    let flow_id = req
                        .flow_id
                        .ok_or_else(|| ApiError::invalid("flow reproduction needs flow_id"))?;
                    state
                        .db
                        .get_flow(auth.workspace_id, req.assessment_id, flow_id)
                        .await?;
                    steps.push(NewStep {
                        kind: StepKind::ProcedureStep,
                        operation: json!({
                            "procedure_key": key,
                            "target_kind": "flow",
                            "target_id": flow_id,
                            "flow_id": flow_id,
                            "identity_id": req.identity_id,
                        }),
                        summary: "reproduce flow and record a test result".into(),
                    });
                }
                other => {
                    return Err(ApiError::new(
                        axum::http::StatusCode::CONFLICT,
                        "capability_unavailable",
                        format!("procedure {other} has no deterministic executor"),
                    ));
                }
            }
            let _ = assessment;
        }
        RunKind::Reconciliation => {
            steps.push(NewStep {
                kind: StepKind::Reconcile,
                operation: json!({ "note": req.note }),
                summary: "record operator reconciliation".into(),
            });
        }
        RunKind::NetworkScan => {
            let mut targets = req.targets.clone();
            if targets.is_empty() {
                targets = state
                    .db
                    .distinct_hosts(auth.workspace_id, req.assessment_id)
                    .await?;
            }
            targets.retain(|target| !target.trim().is_empty());
            if targets.is_empty() {
                return Err(ApiError::invalid(
                    "network scans need at least one host or an inventory with hosts",
                ));
            }
            let ports = req
                .params
                .get("ports")
                .and_then(|value| value.as_array())
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|value| value.as_u64())
                        .filter_map(|value| u16::try_from(value).ok())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            steps.push(NewStep {
                kind: StepKind::NetworkScan,
                operation: json!({ "targets": targets, "ports": ports }),
                summary: format!("nmap TCP connect scan on {} hosts", targets.len()),
            });
        }
        RunKind::FirecrawlMap => {
            let target = req
                .targets
                .first()
                .cloned()
                .or_else(|| req.base_url.clone())
                .ok_or_else(|| ApiError::invalid("firecrawl map needs a target URL"))?;
            steps.push(NewStep {
                kind: StepKind::FirecrawlMap,
                operation: json!({
                    "targets": [target],
                    "limit": req.params.get("limit").and_then(|value| value.as_u64()).unwrap_or(100),
                    "search": req.params.get("search").and_then(|value| value.as_str()),
                }),
                summary: "discover URLs through Firecrawl".into(),
            });
        }
        RunKind::FirecrawlScrape => {
            if req.targets.is_empty() {
                return Err(ApiError::invalid("firecrawl scrape needs at least one URL"));
            }
            steps.push(NewStep {
                kind: StepKind::FirecrawlScrape,
                operation: json!({
                    "targets": req.targets,
                    "objective": if req.note.trim().is_empty() {
                        "Observe the page for an authorized assessment."
                    } else {
                        req.note.as_str()
                    },
                    "max_urls": req.params.get("max_urls").and_then(|value| value.as_u64()).unwrap_or(5),
                }),
                summary: "render and capture pages through the managed browser".into(),
            });
        }
        RunKind::BurpImport => {
            steps.push(NewStep {
                kind: StepKind::BurpImport,
                operation: json!({
                    "limit": req.params.get("limit").and_then(|value| value.as_u64()).unwrap_or(100),
                }),
                summary: "import proxy history from Burp over MCP".into(),
            });
        }
    }
    Ok(steps)
}

pub async fn get_run(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
) -> Result<Json<RunDetail>, ApiError> {
    let id = parse_id::<RunId>(&id)?;
    let summary = state.db.run_summary(auth.workspace_id, id).await?;
    let steps = state.db.run_step_dtos(id).await?;
    let events = state.db.events_for_run(id, 200).await?;
    Ok(Json(RunDetail {
        run: summary,
        steps,
        events,
    }))
}

#[derive(Debug, Deserialize, Default)]
pub struct CancelRequest {
    #[serde(default)]
    pub reason: String,
}

pub async fn cancel_run(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<CancelRequest>,
) -> Result<Json<RunSummary>, ApiError> {
    auth.require_write()?;
    let id = parse_id::<RunId>(&id)?;
    let reason = if req.reason.trim().is_empty() {
        "cancelled by operator"
    } else {
        &req.reason
    };
    let summary = state
        .db
        .request_cancel(auth.workspace_id, id, reason, &auth.operator)
        .await?;
    state.kick_worker();
    Ok(Json(summary))
}

#[derive(Debug, Deserialize, Default)]
pub struct ResumeRequest {
    #[serde(default)]
    pub reconcile_unknown: bool,
    #[serde(default)]
    pub note: String,
}

pub async fn resume_run(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<ResumeRequest>,
) -> Result<Json<RunSummary>, ApiError> {
    auth.require_write()?;
    let id = parse_id::<RunId>(&id)?;
    let summary = state
        .db
        .resume_run(auth.workspace_id, id, req.reconcile_unknown, &req.note)
        .await?;
    state
        .db
        .audit(
            auth.workspace_id,
            Some(summary.assessment_id),
            &auth.operator,
            "run.resume",
            &id.to_string(),
            &req.note,
            &id.to_string(),
        )
        .await?;
    state.kick_worker();
    Ok(Json(summary))
}

pub async fn run_events(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Query(_params): Query<RunsParams>,
) -> Result<Json<Vec<EventDto>>, ApiError> {
    let id = parse_id::<RunId>(&id)?;
    let summary = state.db.run_summary(auth.workspace_id, id).await?;
    let _ = summary;
    Ok(Json(state.db.events_for_run(id, 200).await?))
}

pub async fn assessment_events(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Query(params): Query<RunsParams>,
    headers: HeaderMap,
) -> Result<axum::response::Response, ApiError> {
    let id = parse_id::<AssessmentId>(&id)?;
    state.db.assessment_summary(auth.workspace_id, id).await?;
    let after = params.after.unwrap_or(0);
    let wants_sse = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.contains("text/event-stream"))
        .unwrap_or(false);
    if !wants_sse {
        let events = state
            .db
            .events_since(auth.workspace_id, id, after, 200)
            .await?;
        return Ok(Json(events).into_response());
    }

    struct StreamState {
        db: darkbox_storage::Db,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        cursor: i64,
        buffer: VecDeque<EventDto>,
    }
    let stream_state = StreamState {
        db: state.db.clone(),
        workspace_id: auth.workspace_id,
        assessment_id: id,
        cursor: after,
        buffer: VecDeque::new(),
    };
    let stream = futures::stream::unfold(stream_state, |mut st| async move {
        loop {
            if let Some(event) = st.buffer.pop_front() {
                st.cursor = event.seq;
                let sse_event = Event::default()
                    .id(event.seq.to_string())
                    .event(event.event_type.clone())
                    .json_data(&event)
                    .ok()?;
                return Some((Ok::<_, std::convert::Infallible>(sse_event), st));
            }
            match st
                .db
                .events_since(st.workspace_id, st.assessment_id, st.cursor, 100)
                .await
            {
                Ok(events) => st.buffer.extend(events),
                Err(_) => {
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
            if st.buffer.is_empty() {
                tokio::time::sleep(std::time::Duration::from_millis(700)).await;
            }
        }
    });
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response())
}
