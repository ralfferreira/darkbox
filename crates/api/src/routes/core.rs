use std::str::FromStr;

use axum::Json;
use axum::extract::{Path, Query, State};
use darkbox_contracts::*;
use darkbox_domain::EndpointKey;
use darkbox_execution::eligibility_blockers;
use darkbox_policy::{PolicyEngine, RequestContext, TargetRole};
use darkbox_storage::{InventoryQuery, NewAssessment, NewRun, NewStep};
use serde::Deserialize;
use serde_json::json;

use crate::{ApiError, AppState, Auth, PrincipalPermissions};

pub async fn list_companies(
    State(state): State<AppState>,
    Auth(auth): Auth,
) -> Result<Json<Vec<CompanyDto>>, ApiError> {
    Ok(Json(state.db.list_companies(auth.workspace_id).await?))
}

pub async fn create_company(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Json(req): Json<CreateCompanyRequest>,
) -> Result<Json<CompanyDto>, ApiError> {
    auth.require_write()?;
    let id = state
        .db
        .create_company(auth.workspace_id, &req.name, &req.description)
        .await?;
    state
        .db
        .audit(
            auth.workspace_id,
            None,
            &auth.operator,
            "company.create",
            &req.name,
            "",
            &uuid::Uuid::now_v7().to_string(),
        )
        .await?;
    Ok(Json(state.db.get_company(auth.workspace_id, id).await?))
}

pub async fn get_company(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
) -> Result<Json<CompanyDto>, ApiError> {
    let id = parse_id::<CompanyId>(&id)?;
    Ok(Json(state.db.get_company(auth.workspace_id, id).await?))
}

pub async fn list_assessments(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Query(params): Query<ListAssessmentsParams>,
) -> Result<Json<Vec<AssessmentSummary>>, ApiError> {
    let company = params
        .company_id
        .map(|raw| parse_id::<CompanyId>(&raw))
        .transpose()?;
    Ok(Json(
        state
            .db
            .list_assessments(auth.workspace_id, company)
            .await?,
    ))
}

#[derive(Debug, Deserialize)]
pub struct ListAssessmentsParams {
    pub company_id: Option<String>,
}

pub async fn create_assessment(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Json(req): Json<CreateAssessmentRequest>,
) -> Result<Json<AssessmentSummary>, ApiError> {
    auth.require_write()?;
    let mut targets = Vec::new();
    for raw in &req.initial_targets {
        targets.push(
            EndpointKey::parse(raw, "GET")
                .map_err(|e| ApiError::invalid(format!("invalid target {raw}: {e}")))?,
        );
    }
    let assessment = state
        .db
        .create_assessment(
            auth.workspace_id,
            NewAssessment {
                company_id: req.company_id,
                name: req.name,
                environment: req.environment,
                objective: req.objective,
                policy: req.policy,
                targets,
                actor: auth.operator.clone(),
            },
        )
        .await?;
    state
        .db
        .audit(
            auth.workspace_id,
            Some(assessment.id),
            &auth.operator,
            "assessment.create",
            &assessment.name,
            "",
            &uuid::Uuid::now_v7().to_string(),
        )
        .await?;
    Ok(Json(
        state
            .db
            .assessment_summary(auth.workspace_id, assessment.id)
            .await?,
    ))
}

pub async fn get_assessment(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
) -> Result<Json<AssessmentDetail>, ApiError> {
    let id = parse_id::<AssessmentId>(&id)?;
    let assessment = state.db.assessment_summary(auth.workspace_id, id).await?;
    let scope = state.db.get_scope(auth.workspace_id, id, None).await?;
    let identities = state.db.list_identities(auth.workspace_id, id).await?;
    let recent_runs = state.db.list_runs(auth.workspace_id, id, 8).await?;
    let coverage = state.db.coverage(auth.workspace_id, id).await?;
    Ok(Json(AssessmentDetail {
        assessment,
        scope: ScopeVersionDto {
            id: scope.id,
            version: scope.version,
            policy: scope.policy,
            created_at: scope.created_at,
            created_by: scope.created_by,
            note: scope.note,
        },
        identities,
        recent_runs,
        coverage,
    }))
}

pub async fn list_scope_versions(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
) -> Result<Json<Vec<ScopeVersionDto>>, ApiError> {
    let id = parse_id::<AssessmentId>(&id)?;
    let versions = state.db.list_scope_versions(auth.workspace_id, id).await?;
    Ok(Json(
        versions
            .into_iter()
            .map(|version| ScopeVersionDto {
                id: version.id,
                version: version.version,
                policy: version.policy,
                created_at: version.created_at,
                created_by: version.created_by,
                note: version.note,
            })
            .collect(),
    ))
}

#[derive(Debug, Deserialize)]
pub struct CreateScopeRequest {
    pub policy: ScopePolicy,
    #[serde(default)]
    pub note: String,
}

pub async fn create_scope_version(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<CreateScopeRequest>,
) -> Result<Json<ScopeVersionDto>, ApiError> {
    auth.require_write()?;
    let id = parse_id::<AssessmentId>(&id)?;
    let scope = state
        .db
        .create_scope_version(
            auth.workspace_id,
            id,
            &req.policy,
            &req.note,
            &auth.operator,
        )
        .await?;
    let updated = state
        .executor
        .browser_update_policy(auth.workspace_id, id, &scope.policy)
        .await
        .unwrap_or(0);
    tracing::info!(assessment = %id, sessions = updated, "pushed current scope to open browser contexts");
    state
        .db
        .audit(
            auth.workspace_id,
            Some(id),
            &auth.operator,
            "scope.version.create",
            &format!("v{}", scope.version),
            &req.note,
            &uuid::Uuid::now_v7().to_string(),
        )
        .await?;
    Ok(Json(ScopeVersionDto {
        id: scope.id,
        version: scope.version,
        policy: scope.policy,
        created_at: scope.created_at,
        created_by: scope.created_by,
        note: scope.note,
    }))
}

#[derive(Debug, Deserialize)]
pub struct PreviewRequest {
    pub targets: Vec<String>,
}

pub async fn preview_scope(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<PreviewRequest>,
) -> Result<Json<Vec<ScopeDecision>>, ApiError> {
    let id = parse_id::<AssessmentId>(&id)?;
    let scope = state.db.get_scope(auth.workspace_id, id, None).await?;
    let engine = PolicyEngine::new(&scope.policy, scope.version);
    let mut decisions = Vec::new();
    for raw in req.targets.iter().take(200) {
        let key = match EndpointKey::parse(raw, "GET") {
            Ok(key) => key,
            Err(error) => {
                decisions.push(ScopeDecision {
                    allowed: false,
                    code: Some(DenyCode::OutOfScope),
                    rule: format!("invalid_url: {error}"),
                    policy_version: scope.version,
                    target: raw.clone(),
                    action_class: ActionClass::Query,
                });
                continue;
            }
        };
        let ctx = RequestContext {
            scheme: &key.scheme,
            host: &key.host,
            port: key.port,
            path: &key.path,
            action_class: ActionClass::Query,
            role: TargetRole::TestTarget,
            resolved_ips: Vec::new(),
            now: chrono::Utc::now(),
        };
        decisions.push(engine.evaluate(&ctx));
    }
    Ok(Json(decisions))
}

#[derive(Debug, Deserialize)]
pub struct InventoryParams {
    pub state: Option<String>,
    pub host: Option<String>,
    pub identity_id: Option<String>,
    pub source: Option<String>,
    pub q: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn inventory(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Query(params): Query<InventoryParams>,
) -> Result<Json<InventoryResponse>, ApiError> {
    let id = parse_id::<AssessmentId>(&id)?;
    let assessment = state.db.assessment_summary(auth.workspace_id, id).await?;
    let identity_id = params
        .identity_id
        .map(|raw| parse_id::<IdentityId>(&raw))
        .transpose()?;
    let page = state
        .db
        .inventory(
            auth.workspace_id,
            id,
            &InventoryQuery {
                state: params.state,
                host: params.host,
                identity_id,
                source: params.source,
                text: params.q,
                limit: params.limit.unwrap_or(100),
                offset: params.offset.unwrap_or(0),
            },
        )
        .await?;
    let coverage = state.db.coverage(auth.workspace_id, id).await?;
    let runs = state.db.list_runs(auth.workspace_id, id, 20).await?;
    let mut gaps = Vec::new();
    let candidates = page.counts.get("candidate").copied().unwrap_or(0);
    if candidates > 0 {
        gaps.push(GapDto {
            kind: "candidate_not_observed".into(),
            detail: format!("{candidates} candidates have not been observed yet"),
            target: None,
        });
    }
    if coverage.unreachable_endpoints > 0 {
        gaps.push(GapDto {
            kind: "unreachable".into(),
            detail: format!(
                "{} endpoints were unreachable at last attempt; that is not proof of removal",
                coverage.unreachable_endpoints
            ),
            target: None,
        });
    }
    if coverage.excluded_endpoints > 0 {
        gaps.push(GapDto {
            kind: "excluded".into(),
            detail: format!(
                "{} destinations were excluded by policy before access",
                coverage.excluded_endpoints
            ),
            target: None,
        });
    }
    for run in &runs {
        if matches!(run.state, RunState::WaitingOperator | RunState::Interrupted) {
            gaps.push(GapDto {
                kind: "run_attention".into(),
                detail: format!(
                    "run {} is {} ({})",
                    run.id,
                    run.state,
                    run.reason.as_deref().unwrap_or("no reason recorded")
                ),
                target: Some(run.id.to_string()),
            });
        }
    }
    Ok(Json(InventoryResponse {
        endpoints: page.endpoints,
        total: page.total,
        counts: page.counts,
        gaps,
        policy_version: assessment.scope_version,
    }))
}

#[derive(Debug, Deserialize)]
pub struct ObservationsParams {
    pub limit: Option<i64>,
}

pub async fn observations(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Query(params): Query<ObservationsParams>,
) -> Result<Json<Vec<ObservationDto>>, ApiError> {
    let id = parse_id::<AssessmentId>(&id)?;
    state.db.assessment_summary(auth.workspace_id, id).await?;
    Ok(Json(
        state
            .db
            .recent_observations(auth.workspace_id, id, params.limit.unwrap_or(50))
            .await?,
    ))
}

pub async fn import_urls(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<ImportUrlsRequest>,
) -> Result<Json<RunSummary>, ApiError> {
    auth.require_write()?;
    let id = parse_id::<AssessmentId>(&id)?;
    if req.urls.is_empty() {
        return Err(ApiError::invalid("at least one URL is required"));
    }
    let assessment = state.db.get_assessment(auth.workspace_id, id).await?;
    let scope = state.db.get_scope(auth.workspace_id, id, None).await?;
    for raw in &req.urls {
        EndpointKey::parse(raw, "GET")
            .map_err(|e| ApiError::invalid(format!("invalid URL {raw}: {e}")))?;
    }
    let dedupe = format!("import-{}", uuid::Uuid::now_v7());
    let run = state
        .db
        .create_run(
            auth.workspace_id,
            NewRun {
                assessment_id: id,
                kind: RunKind::Discovery,
                policy: scope.policy.clone(),
                policy_version: scope.version,
                budget: scope.policy.limits.clone(),
                procedure_key: None,
                flow_id: None,
                identity_id: None,
                idempotency_key: dedupe,
                note: format!(
                    "import {} URLs from {}",
                    req.urls.len(),
                    req.source.as_deref().unwrap_or("operator")
                ),
                steps: vec![NewStep {
                    kind: StepKind::DiscoveryImport,
                    operation: json!({
                        "urls": req.urls,
                        "observe": true,
                        "max_urls": 100,
                        "source": req.source.unwrap_or_else(|| "operator".into()),
                    }),
                    summary: "import URLs and observe allowed targets".into(),
                }],
            },
        )
        .await?;
    state
        .db
        .audit(
            auth.workspace_id,
            Some(id),
            &auth.operator,
            "discovery.import",
            &run.id.to_string(),
            "",
            &run.id.to_string(),
        )
        .await?;
    state.kick_worker();
    let _ = assessment;
    Ok(Json(state.db.run_summary(auth.workspace_id, run.id).await?))
}

pub async fn list_identities(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
) -> Result<Json<Vec<IdentityDto>>, ApiError> {
    let id = parse_id::<AssessmentId>(&id)?;
    Ok(Json(state.db.list_identities(auth.workspace_id, id).await?))
}

pub async fn create_identity(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<CreateIdentityRequest>,
) -> Result<Json<IdentityDto>, ApiError> {
    auth.require_write()?;
    let id = parse_id::<AssessmentId>(&id)?;
    let identity_id = state
        .db
        .create_identity(auth.workspace_id, id, &req)
        .await?;
    let identities = state.db.list_identities(auth.workspace_id, id).await?;
    identities
        .into_iter()
        .find(|identity| identity.id == identity_id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("identity"))
}

pub async fn update_identity(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path((id, identity_id)): Path<(String, String)>,
    Json(req): Json<UpdateIdentityRequest>,
) -> Result<Json<IdentityDto>, ApiError> {
    auth.require_write()?;
    let assessment_id = parse_id::<AssessmentId>(&id)?;
    let identity_id = parse_id::<IdentityId>(&identity_id)?;
    state
        .db
        .update_identity(auth.workspace_id, assessment_id, identity_id, &req)
        .await?;
    let identities = state
        .db
        .list_identities(auth.workspace_id, assessment_id)
        .await?;
    identities
        .into_iter()
        .find(|identity| identity.id == identity_id)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("identity"))
}

pub async fn list_tenants(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
) -> Result<Json<Vec<TargetTenantDto>>, ApiError> {
    let id = parse_id::<AssessmentId>(&id)?;
    Ok(Json(state.db.list_tenants(auth.workspace_id, id).await?))
}

#[derive(Debug, Deserialize)]
pub struct CreateTenantRequest {
    pub key: String,
    pub label: String,
}

pub async fn create_tenant(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<CreateTenantRequest>,
) -> Result<Json<TargetTenantDto>, ApiError> {
    auth.require_write()?;
    let id = parse_id::<AssessmentId>(&id)?;
    state
        .db
        .create_tenant(auth.workspace_id, id, &req.key, &req.label)
        .await?;
    let tenants = state.db.list_tenants(auth.workspace_id, id).await?;
    tenants
        .into_iter()
        .find(|tenant| tenant.key == req.key)
        .map(Json)
        .ok_or_else(|| ApiError::not_found("tenant"))
}

pub async fn list_flows(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
) -> Result<Json<Vec<FlowDto>>, ApiError> {
    let id = parse_id::<AssessmentId>(&id)?;
    Ok(Json(state.db.list_flows(auth.workspace_id, id).await?))
}

pub async fn create_flow(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<CreateFlowRequest>,
) -> Result<Json<FlowDto>, ApiError> {
    auth.require_write()?;
    let id = parse_id::<AssessmentId>(&id)?;
    let flow = state
        .db
        .create_flow(auth.workspace_id, id, &req, &auth.operator)
        .await?;
    state
        .db
        .audit(
            auth.workspace_id,
            Some(id),
            &auth.operator,
            "flow.create",
            &flow.name,
            "",
            &uuid::Uuid::now_v7().to_string(),
        )
        .await?;
    Ok(Json(flow))
}

pub async fn create_action(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<CreateActionRequest>,
) -> Result<Json<ActionDto>, ApiError> {
    auth.require_write()?;
    let id = parse_id::<AssessmentId>(&id)?;
    if !req.action_class.default_enabled() {
        return Err(ApiError::blocked(format!(
            "action class {} is disabled in the initial release",
            req.action_class
        )));
    }
    if let Some(endpoint_id) = req.endpoint_id {
        state
            .db
            .endpoint_by_id(auth.workspace_id, id, endpoint_id)
            .await?;
    }
    Ok(Json(
        state.db.create_action(auth.workspace_id, id, &req).await?,
    ))
}

pub async fn list_procedures(
    State(state): State<AppState>,
    Auth(_auth): Auth,
) -> Result<Json<Vec<ProcedureDto>>, ApiError> {
    Ok(Json(state.db.list_procedures().await?))
}

pub async fn eligibility(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
) -> Result<Json<Vec<EligibilityDto>>, ApiError> {
    let id = parse_id::<AssessmentId>(&id)?;
    state.db.assessment_summary(auth.workspace_id, id).await?;
    let capabilities = darkbox_execution::capabilities(state.executor.config());
    let available: Vec<String> = capabilities
        .iter()
        .filter(|capability| capability.state == CapabilityState::Available)
        .map(|capability| capability.id.clone())
        .collect();
    let mut inputs = state.db.eligibility_inputs(auth.workspace_id, id).await?;
    for input in &mut inputs {
        input.available_capabilities = available.clone();
    }
    Ok(Json(
        inputs
            .into_iter()
            .map(|input| {
                let blockers = eligibility_blockers(&input);
                EligibilityDto {
                    procedure_key: input.procedure_key,
                    procedure_name: input.procedure_name,
                    eligible: blockers.is_empty(),
                    target_count: input.target_count,
                    blockers,
                    required_capabilities: input.required_capabilities,
                }
            })
            .collect(),
    ))
}

pub async fn list_results(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Query(params): Query<ObservationsParams>,
) -> Result<Json<Vec<TestResultDto>>, ApiError> {
    let id = parse_id::<AssessmentId>(&id)?;
    Ok(Json(
        state
            .db
            .list_test_results(auth.workspace_id, id, params.limit.unwrap_or(100))
            .await?,
    ))
}

pub(crate) fn parse_id<T>(raw: &str) -> Result<T, ApiError>
where
    T: FromStr,
{
    raw.parse::<T>()
        .map_err(|_| ApiError::invalid(format!("invalid identifier: {raw}")))
}
