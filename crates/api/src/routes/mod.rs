use axum::Json;
use axum::extract::State;
use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use darkbox_contracts::*;
use serde_json::json;

use crate::{ApiError, AppState, Auth};

pub mod agent;
pub mod core;
pub mod evidence;
pub mod findings;
pub mod memory;
pub mod runs;

pub use core::*;
pub use evidence::*;
pub use findings::*;
pub use memory::*;
pub use runs::*;

pub async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(json!({
        "status": if state.db.healthy().await { "ok" } else { "degraded" },
        "version": env!("CARGO_PKG_VERSION"),
        "service": "darkbox-api",
    }))
}

pub async fn session(State(_state): State<AppState>, Auth(auth): Auth) -> Json<SessionDto> {
    Json(SessionDto {
        workspace_id: auth.workspace_id,
        workspace_name: auth.workspace_name,
        role: auth.role,
        operator: auth.operator.clone(),
        can_write: auth.role.can_write(),
    })
}

pub async fn capabilities(
    State(state): State<AppState>,
    Auth(_auth): Auth,
) -> Result<Json<CapabilitiesDto>, ApiError> {
    let model_provider = if std::env::var("DARKBOX_MODEL_API_KEY").is_ok() {
        std::env::var("DARKBOX_MODEL").unwrap_or_else(|_| "openai-compatible".into())
    } else {
        "none".into()
    };
    Ok(Json(CapabilitiesDto {
        adapters: darkbox_execution::capabilities(state.executor.config()),
        model_provider,
        lab_url: state.config.lab_url.clone(),
        raw_evidence_enabled: state.config.raw_evidence_enabled,
    }))
}

/// Serves the built single-page app for client-side routes. API paths are never
/// rewritten, so an unknown `/v1/...` request still returns a JSON 404.
pub async fn spa_index(State(state): State<AppState>, uri: Uri) -> Response {
    let path = uri.path();
    if path.starts_with("/v1/") || path == "/v1" || path == "/health" {
        return ApiError::not_found("route").into_response();
    }
    let Some(dist) = &state.config.web_dist else {
        return ApiError::not_found("route").into_response();
    };
    match tokio::fs::read_to_string(dist.join("index.html")).await {
        Ok(html) => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
            html,
        )
            .into_response(),
        Err(_) => ApiError::not_found("index").into_response(),
    }
}
