use sqlx::Row;

use crate::{Db, DbError, Principal, Result};

impl Db {
    pub async fn authenticate(&self, token_hash: &str) -> Result<Option<Principal>> {
        let row = sqlx::query(
            "SELECT t.workspace_id, w.name AS workspace_name, t.operator, t.role
             FROM api_tokens t JOIN workspaces w ON w.id = t.workspace_id
             WHERE t.token_hash = $1 AND t.revoked_at IS NULL",
        )
        .bind(token_hash)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let role: String = row.try_get("role")?;
        Ok(Some(Principal {
            workspace_id: row.try_get::<uuid::Uuid, _>("workspace_id")?.into(),
            workspace_name: row.try_get("workspace_name")?,
            operator: row.try_get("operator")?,
            role: role.parse().map_err(DbError::Invalid)?,
        }))
    }

    pub async fn create_workspace(
        &self,
        name: &str,
        retention_days: i32,
    ) -> Result<darkbox_contracts::WorkspaceId> {
        let id = darkbox_contracts::WorkspaceId::new();
        sqlx::query("INSERT INTO workspaces (id, name, retention_days) VALUES ($1,$2,$3)")
            .bind(id.as_uuid())
            .bind(name)
            .bind(retention_days)
            .execute(&self.pool)
            .await?;
        Ok(id)
    }

    pub async fn seed_token(
        &self,
        token_hash: &str,
        workspace_id: darkbox_contracts::WorkspaceId,
        operator: &str,
        role: darkbox_contracts::Role,
    ) -> Result<()> {
        sqlx::query(
            "INSERT INTO api_tokens (token_hash, workspace_id, operator, role)
             VALUES ($1,$2,$3,$4) ON CONFLICT (token_hash) DO UPDATE
             SET workspace_id = EXCLUDED.workspace_id, operator = EXCLUDED.operator, role = EXCLUDED.role",
        )
        .bind(token_hash)
        .bind(workspace_id.as_uuid())
        .bind(operator)
        .bind(role.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_workspaces(&self) -> Result<Vec<(darkbox_contracts::WorkspaceId, String)>> {
        let rows = sqlx::query("SELECT id, name FROM workspaces ORDER BY created_at")
            .fetch_all(&self.pool)
            .await?;
        rows.into_iter()
            .map(|row| {
                Ok((
                    row.try_get::<uuid::Uuid, _>("id")?.into(),
                    row.try_get("name")?,
                ))
            })
            .collect()
    }
}

impl Db {
    pub async fn workspace_by_name(
        &self,
        name: &str,
    ) -> Result<Option<darkbox_contracts::WorkspaceId>> {
        let row = sqlx::query("SELECT id FROM workspaces WHERE name=$1")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| row.get::<uuid::Uuid, _>("id").into()))
    }

    pub async fn company_by_name(
        &self,
        workspace_id: darkbox_contracts::WorkspaceId,
        name: &str,
    ) -> Result<Option<darkbox_contracts::CompanyId>> {
        let row = sqlx::query("SELECT id FROM companies WHERE workspace_id=$1 AND name=$2")
            .bind(workspace_id.as_uuid())
            .bind(name)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| row.get::<uuid::Uuid, _>("id").into()))
    }

    pub async fn assessment_by_name(
        &self,
        workspace_id: darkbox_contracts::WorkspaceId,
        company_id: darkbox_contracts::CompanyId,
        name: &str,
    ) -> Result<Option<darkbox_contracts::AssessmentId>> {
        let row = sqlx::query(
            "SELECT id FROM assessments WHERE workspace_id=$1 AND company_id=$2 AND name=$3",
        )
        .bind(workspace_id.as_uuid())
        .bind(company_id.as_uuid())
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| row.get::<uuid::Uuid, _>("id").into()))
    }

    pub async fn identity_by_label(
        &self,
        assessment_id: darkbox_contracts::AssessmentId,
        label: &str,
    ) -> Result<Option<darkbox_contracts::IdentityId>> {
        let row = sqlx::query("SELECT id FROM identities WHERE assessment_id=$1 AND label=$2")
            .bind(assessment_id.as_uuid())
            .bind(label)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|row| row.get::<uuid::Uuid, _>("id").into()))
    }
}
