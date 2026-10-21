use axum::Json;
use axum::extract::{Path, Query, State};
use darkbox_contracts::*;
use serde::Deserialize;

use crate::routes::core::parse_id;
use crate::{ApiError, AppState, Auth, PrincipalPermissions};

#[derive(Debug, Deserialize)]
pub struct FindingsParams {
    pub state: Option<String>,
}

pub async fn list_findings(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Query(params): Query<FindingsParams>,
) -> Result<Json<Vec<FindingDto>>, ApiError> {
    let id = parse_id::<AssessmentId>(&id)?;
    state.db.assessment_summary(auth.workspace_id, id).await?;
    Ok(Json(
        state
            .db
            .list_findings(auth.workspace_id, id, params.state.as_deref())
            .await?,
    ))
}

pub async fn create_finding(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<CreateFindingRequest>,
) -> Result<Json<FindingDto>, ApiError> {
    auth.require_write()?;
    let id = parse_id::<AssessmentId>(&id)?;
    state.db.assessment_summary(auth.workspace_id, id).await?;
    if let Some(endpoint_id) = req.endpoint_id {
        state
            .db
            .endpoint_by_id(auth.workspace_id, id, endpoint_id)
            .await?;
    }
    for evidence_id in &req.evidence_ids {
        state
            .db
            .get_evidence(auth.workspace_id, id, *evidence_id)
            .await?;
    }
    let finding_id = state
        .db
        .create_finding(auth.workspace_id, id, &req, &auth.operator)
        .await?;
    state
        .db
        .audit(
            auth.workspace_id,
            Some(id),
            &auth.operator,
            "finding.create",
            &finding_id.to_string(),
            &req.title,
            &finding_id.to_string(),
        )
        .await?;
    Ok(Json(
        state
            .db
            .get_finding(auth.workspace_id, id, finding_id)
            .await?,
    ))
}

pub async fn get_finding(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
) -> Result<Json<FindingDto>, ApiError> {
    let finding_id = parse_id::<FindingId>(&id)?;
    let (workspace_id, assessment_id) = state.db.finding_scope(finding_id).await?;
    if workspace_id != auth.workspace_id {
        return Err(ApiError::not_found("finding"));
    }
    Ok(Json(
        state
            .db
            .get_finding(auth.workspace_id, assessment_id, finding_id)
            .await?,
    ))
}

pub async fn review_finding(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<ReviewFindingRequest>,
) -> Result<Json<FindingDto>, ApiError> {
    auth.require_review()?;
    let finding_id = parse_id::<FindingId>(&id)?;
    let (workspace_id, assessment_id) = state.db.finding_scope(finding_id).await?;
    if workspace_id != auth.workspace_id {
        return Err(ApiError::not_found("finding"));
    }
    let finding = state
        .db
        .review_finding(
            auth.workspace_id,
            assessment_id,
            finding_id,
            req.state,
            &req.note,
            &auth.operator,
        )
        .await?;
    state
        .db
        .audit(
            auth.workspace_id,
            Some(assessment_id),
            &auth.operator,
            "finding.review",
            &finding_id.to_string(),
            &format!("{}: {}", req.state, req.note),
            &finding_id.to_string(),
        )
        .await?;
    Ok(Json(finding))
}
