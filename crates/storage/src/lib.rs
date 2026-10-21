//! PostgreSQL persistence.
//!
//! Queries are explicit SQL. Rows are mapped through `to_jsonb` into the same
//! structs the rest of the core uses, so there is one definition of the model.

use std::time::Duration;

use darkbox_contracts::*;
use darkbox_domain::ScopeVersion;
use darkbox_policy::PolicyEngine;
use serde::de::DeserializeOwned;
use sqlx::Row;
use sqlx::postgres::{PgPool, PgPoolOptions};
use uuid::Uuid;

mod assessments;
mod auth;
mod events;
mod evidence;
mod findings;
mod flows;
mod identities;
mod inventory;
mod memory;
mod procedures;
mod runs;

pub use assessments::NewAssessment;
pub use evidence::*;
pub use inventory::{InventoryPage, InventoryQuery};
pub use procedures::*;
pub use runs::*;

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Migrate(#[from] sqlx::migrate::MigrateError),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("invalid: {0}")]
    Invalid(String),
}

pub type Result<T> = std::result::Result<T, DbError>;

#[derive(Clone)]
pub struct Db {
    pool: PgPool,
}

#[derive(Debug, Clone)]
pub struct Principal {
    pub workspace_id: WorkspaceId,
    pub workspace_name: String,
    pub operator: String,
    pub role: Role,
}

impl Db {
    pub async fn connect(url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(12)
            .acquire_timeout(Duration::from_secs(5))
            .connect(url)
            .await?;
        Ok(Self { pool })
    }

    pub fn from_pool(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn migrate(&self) -> Result<()> {
        sqlx::migrate!("./migrations").run(&self.pool).await?;
        Ok(())
    }

    pub async fn healthy(&self) -> bool {
        sqlx::query("SELECT 1").execute(&self.pool).await.is_ok()
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn audit(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: Option<AssessmentId>,
        actor: &str,
        operation: &str,
        target: &str,
        reason: &str,
        correlation_id: &str,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO audit_events (id, workspace_id, assessment_id, actor, operation, target, reason, correlation_id)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
        )
        .bind(AuditEventId::new().as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.map(|v| v.as_uuid()))
        .bind(actor)
        .bind(operation)
        .bind(target)
        .bind(reason)
        .bind(correlation_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn ping_workspace(&self, workspace_id: WorkspaceId) -> Result<()> {
        let exists: Option<Uuid> = sqlx::query_scalar("SELECT id FROM workspaces WHERE id=$1")
            .bind(workspace_id.as_uuid())
            .fetch_optional(&self.pool)
            .await?;
        exists
            .map(|_| ())
            .ok_or_else(|| DbError::NotFound("workspace".into()))
    }
}

fn map_json<T: DeserializeOwned>(row: &sqlx::postgres::PgRow) -> Result<T> {
    let value: serde_json::Value = row.try_get("row")?;
    Ok(serde_json::from_value(value)?)
}

pub fn policy_engine(version: &ScopeVersion) -> PolicyEngine<'_> {
    PolicyEngine::new(&version.policy, version.version)
}
