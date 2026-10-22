//! HTTP API for DarkBox.

use std::sync::Arc;

use axum::Router;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::{HeaderValue, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use darkbox_evidence::BlobStore;
use darkbox_execution::{ExecError, Executor, ExecutorConfig};
use darkbox_storage::{Db, DbError, Principal};
use serde_json::json;
use sha2::{Digest, Sha256};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;

mod routes;

#[derive(Clone)]
pub struct AppState {
    pub db: Db,
    pub executor: Arc<Executor>,
    pub config: ApiConfig,
}

#[derive(Clone, Debug)]
pub struct ApiConfig {
    pub inline_worker: bool,
    pub lab_url: String,
    pub raw_evidence_enabled: bool,
    pub web_dist: Option<std::path::PathBuf>,
}

impl ApiConfig {
    pub fn from_env() -> Self {
        Self {
            inline_worker: std::env::var("DARKBOX_INLINE_WORKER")
                .map(|v| v != "0")
                .unwrap_or(true),
            lab_url: std::env::var("DARKBOX_LAB_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8080".into()),
            raw_evidence_enabled: std::env::var("DARKBOX_RAW_EVIDENCE")
                .map(|v| v == "1")
                .unwrap_or(false),
            web_dist: std::env::var("DARKBOX_WEB_DIST")
                .ok()
                .map(std::path::PathBuf::from)
                .filter(|path| path.is_dir()),
        }
    }
}

impl AppState {
    pub async fn new(db: Db, config: ApiConfig) -> anyhow::Result<Self> {
        let executor = Arc::new(Executor::new(db.clone(), ExecutorConfig::from_env())?);
        Ok(Self {
            db,
            executor,
            config,
        })
    }

    pub fn store(&self) -> &dyn BlobStore {
        self.executor.store()
    }

    pub fn kick_worker(&self) {
        if !self.config.inline_worker {
            return;
        }
        let executor = self.executor.clone();
        tokio::spawn(async move {
            executor.worker_loop(None, true).await;
        });
    }
}

#[derive(Debug)]
pub struct ApiError {
    pub status: StatusCode,
    pub code: &'static str,
    pub message: String,
    pub detail: serde_json::Value,
}

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
            detail: json!({}),
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, "invalid", message)
    }
    pub fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "missing or invalid bearer token",
        )
    }
    pub fn not_found(what: &str) -> Self {
        Self::new(
            StatusCode::NOT_FOUND,
            "not_found",
            format!("{what} not found"),
        )
    }
    pub fn conflict(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "conflict", message)
    }
    pub fn blocked(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, "blocked", message)
    }
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", message)
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for ApiError {}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = json!({
            "error": {
                "code": self.code,
                "message": self.message,
                "detail": self.detail,
            }
        });
        (self.status, axum::Json(body)).into_response()
    }
}

impl From<DbError> for ApiError {
    fn from(error: DbError) -> Self {
        match &error {
            DbError::NotFound(what) => Self::not_found(what),
            DbError::Conflict(message) => Self::conflict(message.clone()),
            DbError::Invalid(message) => Self::invalid(message.clone()),
            DbError::Sqlx(sqlx::Error::RowNotFound) => Self::not_found("record"),
            other => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "internal",
                message: other.to_string(),
                detail: json!({}),
            },
        }
    }
}

impl From<ExecError> for ApiError {
    fn from(error: ExecError) -> Self {
        match error {
            ExecError::CapabilityUnavailable(message) => {
                Self::new(StatusCode::CONFLICT, "capability_unavailable", message)
            }
            ExecError::Blocked(message) => Self::blocked(message),
            ExecError::Invalid(message) => Self::invalid(message),
            other => Self {
                status: StatusCode::INTERNAL_SERVER_ERROR,
                code: "internal",
                message: other.to_string(),
                detail: json!({}),
            },
        }
    }
}

pub struct Auth(pub Principal);

impl FromRequestParts<AppState> for Auth {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(ApiError::unauthorized)?;
        let token = header
            .strip_prefix("Bearer ")
            .or_else(|| header.strip_prefix("bearer "))
            .ok_or_else(ApiError::unauthorized)?
            .trim();
        let hash = hex::encode(Sha256::digest(token.as_bytes()));
        let principal = state
            .db
            .authenticate(&hash)
            .await
            .map_err(ApiError::from)?
            .ok_or_else(ApiError::unauthorized)?;
        Ok(Auth(principal))
    }
}

pub trait PrincipalPermissions {
    fn require_write(&self) -> Result<(), ApiError>;
    fn require_review(&self) -> Result<(), ApiError>;
}

impl PrincipalPermissions for Principal {
    fn require_write(&self) -> Result<(), ApiError> {
        if self.role.can_write() {
            Ok(())
        } else {
            Err(ApiError::forbidden("role cannot modify this resource"))
        }
    }
    fn require_review(&self) -> Result<(), ApiError> {
        if self.role.can_review() {
            Ok(())
        } else {
            Err(ApiError::forbidden("role cannot review findings"))
        }
    }
}

pub fn router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE, header::ACCEPT]);

    let mut app = Router::new()
        .route("/health", get(routes::health))
        .route("/v1/session", get(routes::session))
        .route(
            "/v1/companies",
            get(routes::list_companies).post(routes::create_company),
        )
        .route("/v1/companies/{id}", get(routes::get_company))
        .route(
            "/v1/assessments",
            get(routes::list_assessments).post(routes::create_assessment),
        )
        .route("/v1/assessments/{id}", get(routes::get_assessment))
        .route(
            "/v1/assessments/{id}/scope-versions",
            get(routes::list_scope_versions).post(routes::create_scope_version),
        )
        .route(
            "/v1/assessments/{id}/scope-preview",
            post(routes::preview_scope),
        )
        .route(
            "/v1/assessments/{id}/agent/turns",
            post(routes::agent::create_turn),
        )
        .route(
            "/v1/runs/{id}/agent/events",
            post(routes::agent::append_event),
        )
        .route("/v1/runs/{id}/agent/usage", post(routes::agent::usage))
        .route("/v1/runs/{id}/agent/finish", post(routes::agent::finish))
        .route("/v1/runs/{id}/agent/abort", post(routes::agent::abort))
        .route(
            "/v1/runs/{id}/agent/reconcile",
            post(routes::agent::reconcile),
        )
        .route(
            "/v1/runs/{id}/agent/checkpoint",
            get(routes::agent::checkpoint),
        )
        .route("/v1/runs/{id}/browser/open", post(routes::browser::open))
        .route(
            "/v1/runs/{id}/browser/navigate",
            post(routes::browser::navigate),
        )
        .route(
            "/v1/runs/{id}/browser/observe",
            post(routes::browser::observe),
        )
        .route("/v1/runs/{id}/browser/act", post(routes::browser::act))
        .route("/v1/runs/{id}/browser/login", post(routes::browser::login))
        .route(
            "/v1/runs/{id}/browser/screenshot",
            post(routes::browser::screenshot),
        )
        .route(
            "/v1/runs/{id}/browser/traffic",
            post(routes::browser::traffic),
        )
        .route("/v1/runs/{id}/browser/close", post(routes::browser::close))
        .route(
            "/v1/runs/{id}/browser/progress",
            get(routes::browser::progress),
        )
        .route(
            "/v1/assessments/{id}/identities/{identity_id}",
            post(routes::update_identity),
        )
        .route("/v1/capabilities/browser", get(routes::browser_capability))
        .route("/v1/assessments/{id}/inventory", get(routes::inventory))
        .route(
            "/v1/assessments/{id}/import-urls",
            post(routes::import_urls),
        )
        .route(
            "/v1/assessments/{id}/observations",
            get(routes::observations),
        )
        .route(
            "/v1/assessments/{id}/identities",
            get(routes::list_identities).post(routes::create_identity),
        )
        .route(
            "/v1/assessments/{id}/tenants",
            get(routes::list_tenants).post(routes::create_tenant),
        )
        .route(
            "/v1/assessments/{id}/flows",
            get(routes::list_flows).post(routes::create_flow),
        )
        .route("/v1/assessments/{id}/actions", post(routes::create_action))
        .route("/v1/assessments/{id}/runs", get(routes::list_runs))
        .route("/v1/assessments/{id}/evidence", get(routes::list_evidence))
        .route("/v1/assessments/{id}/memory", get(routes::list_memory))
        .route(
            "/v1/assessments/{id}/memory/search",
            post(routes::search_memory),
        )
        .route(
            "/v1/assessments/{id}/memory/synthesize",
            post(routes::synthesize_memory),
        )
        .route("/v1/assessments/{id}/context", get(routes::context))
        .route(
            "/v1/assessments/{id}/findings",
            get(routes::list_findings).post(routes::create_finding),
        )
        .route("/v1/assessments/{id}/results", get(routes::list_results))
        .route("/v1/assessments/{id}/eligibility", get(routes::eligibility))
        .route(
            "/v1/assessments/{id}/events",
            get(routes::assessment_events),
        )
        .route("/v1/assessments/{id}/exports", post(routes::create_export))
        .route("/v1/procedures", get(routes::list_procedures))
        .route(
            "/v1/runs",
            get(routes::list_runs_all).post(routes::create_run),
        )
        .route("/v1/runs/{id}", get(routes::get_run))
        .route("/v1/runs/{id}/cancel", post(routes::cancel_run))
        .route("/v1/runs/{id}/resume", post(routes::resume_run))
        .route("/v1/runs/{id}/events", get(routes::run_events))
        .route("/v1/evidence/{id}", get(routes::get_evidence))
        .route("/v1/evidence/{id}/content", get(routes::evidence_content))
        .route("/v1/findings/{id}", get(routes::get_finding))
        .route("/v1/findings/{id}/review", post(routes::review_finding))
        .route("/v1/exports/{id}/download", get(routes::download_export))
        .route("/v1/capabilities", get(routes::capabilities));

    if let Some(web_dist) = &state.config.web_dist {
        app = app
            .nest_service("/assets", ServeDir::new(web_dist.join("assets")))
            .route_service("/favicon.svg", ServeFile::new(web_dist.join("favicon.svg")))
            .fallback(routes::spa_index);
        tracing::info!(path = %web_dist.display(), "serving operator interface");
    }

    app.with_state(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
}

pub async fn serve(state: AppState, bind: &str) -> anyhow::Result<()> {
    let recovery_db = state.db.clone();
    tokio::spawn(async move {
        if let Err(error) = recovery_db.recover_stale_agent_actions(true).await {
            tracing::warn!(error = %error, "initial agent action recovery failed");
        }
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            match recovery_db.recover_stale_agent_actions(false).await {
                Ok(0) => {}
                Ok(count) => tracing::warn!(count, "recovered stale agent actions"),
                Err(error) => tracing::warn!(error = %error, "agent action recovery failed"),
            }
        }
    });
    let app = router(state);
    let listener = tokio::net::TcpListener::bind(bind).await?;
    tracing::info!(bind, "darkbox api listening");
    axum::serve(listener, app).await?;
    Ok(())
}

pub(crate) fn no_store() -> [(header::HeaderName, HeaderValue); 2] {
    [
        (header::CACHE_CONTROL, HeaderValue::from_static("no-store")),
        (
            header::X_CONTENT_TYPE_OPTIONS,
            HeaderValue::from_static("nosniff"),
        ),
    ]
}
