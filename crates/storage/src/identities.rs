use darkbox_contracts::*;
use darkbox_domain::TestIdentity;
use sqlx::Row;

use crate::{Db, DbError, Result, map_json};

impl Db {
    pub async fn create_identity(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        req: &CreateIdentityRequest,
    ) -> Result<IdentityId> {
        let id = IdentityId::new();
        sqlx::query(
            "INSERT INTO identities (id, workspace_id, assessment_id, label, role_label, tenant_keys, capabilities, secret_ref)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8)
             ON CONFLICT (assessment_id, label) DO NOTHING",
        )
        .bind(id.as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(req.label.trim())
        .bind(&req.role_label)
        .bind(serde_json::to_value(&req.tenant_keys)?)
        .bind(serde_json::to_value(&req.capabilities)?)
        .bind(&req.secret_ref)
        .execute(&self.pool)
        .await?;
        let existing: Option<uuid::Uuid> = sqlx::query_scalar(
            "SELECT id FROM identities WHERE workspace_id=$1 AND assessment_id=$2 AND label=$3",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(req.label.trim())
        .fetch_optional(&self.pool)
        .await?;
        existing
            .map(Into::into)
            .ok_or_else(|| DbError::Conflict("identity label already exists".into()))
    }

    pub async fn list_identities(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
    ) -> Result<Vec<IdentityDto>> {
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'id', i.id, 'label', i.label, 'role_label', i.role_label,
                'tenant_keys', i.tenant_keys, 'capabilities', i.capabilities,
                'has_secret_ref', i.secret_ref IS NOT NULL,
                'session_state', COALESCE(s.state, 'unknown'),
                'session_expires_at', s.expires_at,
                'created_at', i.created_at
             ) AS row
             FROM identities i
             LEFT JOIN LATERAL (
                SELECT state, expires_at FROM browser_sessions b
                WHERE b.identity_id=i.id ORDER BY b.last_seen_at DESC LIMIT 1
             ) s ON true
             WHERE i.workspace_id=$1 AND i.assessment_id=$2 ORDER BY i.created_at",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(map_json).collect()
    }

    pub async fn get_identity(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        id: IdentityId,
    ) -> Result<TestIdentity> {
        let row = sqlx::query(
            "SELECT jsonb_build_object(
                'id', i.id, 'workspace_id', i.workspace_id, 'assessment_id', i.assessment_id,
                'label', i.label, 'role_label', i.role_label, 'tenant_keys', i.tenant_keys,
                'capabilities', i.capabilities, 'secret_ref', i.secret_ref, 'created_at', i.created_at
             ) AS row FROM identities i
             WHERE i.workspace_id=$1 AND i.assessment_id=$2 AND i.id=$3",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| DbError::NotFound("identity".into()))?;
        map_json(&row)
    }

    pub async fn create_tenant(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        key: &str,
        label: &str,
    ) -> Result<TenantId> {
        let id = TenantId::new();
        sqlx::query(
            "INSERT INTO target_tenants (id, workspace_id, assessment_id, key, label)
             VALUES ($1,$2,$3,$4,$5) ON CONFLICT (assessment_id, key) DO NOTHING",
        )
        .bind(id.as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(key)
        .bind(label)
        .execute(&self.pool)
        .await?;
        let existing: Option<uuid::Uuid> =
            sqlx::query_scalar("SELECT id FROM target_tenants WHERE assessment_id=$1 AND key=$2")
                .bind(assessment_id.as_uuid())
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        existing
            .map(Into::into)
            .ok_or_else(|| DbError::Conflict("tenant".into()))
    }

    pub async fn list_tenants(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
    ) -> Result<Vec<TargetTenantDto>> {
        let rows = sqlx::query(
            "SELECT jsonb_build_object('id', t.id, 'key', t.key, 'label', t.label) AS row
             FROM target_tenants t WHERE t.workspace_id=$1 AND t.assessment_id=$2 ORDER BY t.key",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(map_json).collect()
    }

    /// Each identity and tenant gets its own isolated browser context.
    #[allow(clippy::too_many_arguments)]
    pub async fn record_session(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        identity_id: IdentityId,
        tenant_key: Option<&str>,
        context_key: &str,
        state: &str,
        expires_at: Option<chrono::DateTime<chrono::Utc>>,
    ) -> Result<BrowserSessionId> {
        let id = BrowserSessionId::new();
        sqlx::query(
            "INSERT INTO browser_sessions (id, workspace_id, assessment_id, identity_id, tenant_key,
                context_key, state, expires_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
        )
        .bind(id.as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(identity_id.as_uuid())
        .bind(tenant_key)
        .bind(context_key)
        .bind(state)
        .bind(expires_at)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn session_state(
        &self,
        identity_id: IdentityId,
    ) -> Result<(String, Option<chrono::DateTime<chrono::Utc>>)> {
        let row = sqlx::query(
            "SELECT state, expires_at FROM browser_sessions WHERE identity_id=$1
             ORDER BY last_seen_at DESC LIMIT 1",
        )
        .bind(identity_id.as_uuid())
        .fetch_optional(&self.pool)
        .await?;
        match row {
            Some(row) => Ok((row.try_get("state")?, row.try_get("expires_at")?)),
            None => Ok(("unknown".into(), None)),
        }
    }
}
