use axum::Json;
use axum::extract::{Path, Query, State};
use darkbox_contracts::*;
use darkbox_memory::ContextBuilder;
use serde::Deserialize;

use crate::routes::core::parse_id;
use crate::{ApiError, AppState, Auth, PrincipalPermissions};

pub async fn list_memory(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
) -> Result<Json<Vec<MemoryPageDto>>, ApiError> {
    let id = parse_id::<AssessmentId>(&id)?;
    state.db.assessment_summary(auth.workspace_id, id).await?;
    Ok(Json(
        state.db.list_memory_pages(auth.workspace_id, id).await?,
    ))
}

pub async fn search_memory(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<MemorySearchRequest>,
) -> Result<Json<Vec<MemoryPageDto>>, ApiError> {
    let id = parse_id::<AssessmentId>(&id)?;
    state.db.assessment_summary(auth.workspace_id, id).await?;
    let pages = state
        .db
        .search_memory(auth.workspace_id, id, &req.query, 20)
        .await?;
    let pages = if let Some(identity_id) = req.identity_id {
        let allowed = state
            .db
            .list_identities(auth.workspace_id, id)
            .await?
            .into_iter()
            .any(|identity| identity.id == identity_id);
        if !allowed {
            return Err(ApiError::forbidden(
                "identity does not belong to this assessment",
            ));
        }
        pages
    } else {
        pages
    };
    Ok(Json(pages))
}

pub async fn synthesize_memory(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
) -> Result<Json<MemoryPageDto>, ApiError> {
    auth.require_write()?;
    let id = parse_id::<AssessmentId>(&id)?;
    state.db.assessment_summary(auth.workspace_id, id).await?;
    let page = darkbox_memory::synthesize(&state.db, auth.workspace_id, id, &auth.operator)
        .await
        .map_err(|e| ApiError::invalid(e.to_string()))?;
    state
        .db
        .audit(
            auth.workspace_id,
            Some(id),
            &auth.operator,
            "memory.synthesize",
            &page.slug,
            "deterministic synthesis",
            &page.id.to_string(),
        )
        .await?;
    Ok(Json(page))
}

#[derive(Debug, Deserialize)]
pub struct ContextParams {
    pub token_budget: Option<i64>,
    pub identity_id: Option<String>,
}

pub async fn context(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Query(params): Query<ContextParams>,
) -> Result<Json<ContextPackageDto>, ApiError> {
    let id = parse_id::<AssessmentId>(&id)?;
    let assessment = state.db.assessment_summary(auth.workspace_id, id).await?;
    let _ = assessment;
    let identity_id = params
        .identity_id
        .map(|raw| parse_id::<IdentityId>(&raw))
        .transpose()?;
    if let Some(identity_id) = identity_id {
        let identities = state.db.list_identities(auth.workspace_id, id).await?;
        if !identities.iter().any(|identity| identity.id == identity_id) {
            return Err(ApiError::forbidden(
                "identity does not belong to this assessment",
            ));
        }
    }
    let package = ContextBuilder::new(&state.db, auth.workspace_id, id, identity_id)
        .with_budget(params.token_budget.unwrap_or(3000))
        .build()
        .await
        .map_err(|e| ApiError::invalid(e.to_string()))?;
    state
        .db
        .audit(
            auth.workspace_id,
            Some(id),
            &auth.operator,
            "memory.context",
            &id.to_string(),
            "",
            &uuid::Uuid::now_v7().to_string(),
        )
        .await?;
    Ok(Json(package))
}
