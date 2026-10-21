use darkbox_contracts::*;
use darkbox_domain::EvidenceRecord;
use sqlx::Row;

use crate::{Db, DbError, Result, map_json};

#[derive(Debug, Clone)]
pub struct NewEvidence {
    pub kind: EvidenceKind,
    pub classification: EvidenceClassification,
    pub object_key: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub truncated: bool,
    pub sanitized: bool,
    pub content_type: String,
    pub run_id: Option<RunId>,
    pub step_id: Option<StepId>,
    pub identity_id: Option<IdentityId>,
    pub summary: String,
    pub source: String,
}

impl Db {
    pub async fn insert_evidence(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        new: NewEvidence,
        retention_days: Option<i32>,
    ) -> Result<EvidenceId> {
        let id = EvidenceId::new();
        sqlx::query(
            "INSERT INTO evidences (id, workspace_id, assessment_id, kind, classification, object_key,
                sha256, size_bytes, truncated, sanitized, content_type, run_id, step_id, identity_id,
                summary, source, retention_until)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,
                CASE WHEN $17::int IS NULL THEN NULL ELSE now() + make_interval(days => $17) END)",
        )
        .bind(id.as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(new.kind.as_str())
        .bind(new.classification.as_str())
        .bind(&new.object_key)
        .bind(&new.sha256)
        .bind(new.size_bytes)
        .bind(new.truncated)
        .bind(new.sanitized)
        .bind(&new.content_type)
        .bind(new.run_id.map(|r| r.as_uuid()))
        .bind(new.step_id.map(|s| s.as_uuid()))
        .bind(new.identity_id.map(|i| i.as_uuid()))
        .bind(&new.summary)
        .bind(&new.source)
        .bind(retention_days)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn get_evidence(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        id: EvidenceId,
    ) -> Result<EvidenceRecord> {
        let row = sqlx::query(
            "SELECT jsonb_build_object(
                'id', e.id, 'workspace_id', e.workspace_id, 'assessment_id', e.assessment_id,
                'kind', e.kind, 'classification', e.classification, 'object_key', e.object_key,
                'sha256', e.sha256, 'size_bytes', e.size_bytes, 'truncated', e.truncated,
                'sanitized', e.sanitized, 'content_type', e.content_type, 'run_id', e.run_id,
                'step_id', e.step_id, 'identity_id', e.identity_id, 'summary', e.summary,
                'source', e.source, 'created_at', e.created_at, 'retention_until', e.retention_until
             ) AS row FROM evidences e
             WHERE e.workspace_id=$1 AND e.assessment_id=$2 AND e.id=$3",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| DbError::NotFound("evidence".into()))?;
        map_json(&row)
    }

    pub async fn list_evidence(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        kind: Option<&str>,
        limit: i64,
        offset: i64,
    ) -> Result<Vec<EvidenceDto>> {
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'id', e.id, 'assessment_id', e.assessment_id, 'kind', e.kind,
                'classification', e.classification, 'sha256', e.sha256, 'size_bytes', e.size_bytes,
                'truncated', e.truncated, 'sanitized', e.sanitized, 'content_type', e.content_type,
                'run_id', e.run_id, 'step_id', e.step_id, 'identity_id', e.identity_id,
                'summary', e.summary, 'created_at', e.created_at, 'retention_until', e.retention_until,
                'access_url', NULL
             ) AS row FROM evidences e
             WHERE e.workspace_id=$1 AND e.assessment_id=$2
               AND ($3::text IS NULL OR e.kind=$3)
             ORDER BY e.created_at DESC LIMIT $4 OFFSET $5",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(kind)
        .bind(limit.clamp(1, 200))
        .bind(offset.max(0))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(map_json).collect()
    }

    pub async fn evidence_workspace_of(
        &self,
        id: EvidenceId,
    ) -> Result<(WorkspaceId, AssessmentId, String)> {
        let row = sqlx::query(
            "SELECT workspace_id, assessment_id, object_key FROM evidences WHERE id=$1",
        )
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| DbError::NotFound("evidence".into()))?;
        Ok((
            row.try_get::<uuid::Uuid, _>("workspace_id")?.into(),
            row.try_get::<uuid::Uuid, _>("assessment_id")?.into(),
            row.try_get("object_key")?,
        ))
    }

    pub async fn delete_evidence(&self, id: EvidenceId) -> Result<()> {
        sqlx::query("DELETE FROM evidences WHERE id=$1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn expired_evidence(&self, limit: i64) -> Result<Vec<(EvidenceId, String)>> {
        let rows = sqlx::query(
            "SELECT id, object_key FROM evidences
             WHERE retention_until IS NOT NULL AND retention_until < now() LIMIT $1",
        )
        .bind(limit.clamp(1, 1000))
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok((
                    row.try_get::<uuid::Uuid, _>("id")?.into(),
                    row.try_get("object_key")?,
                ))
            })
            .collect()
    }
}

#[derive(Debug, Clone)]
pub struct ExportRecord {
    pub id: uuid::Uuid,
    pub assessment_id: AssessmentId,
    pub object_key: String,
    pub manifest_sha256: String,
    pub size_bytes: i64,
    pub item_count: i64,
    pub include_evidence: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

impl Db {
    #[allow(clippy::too_many_arguments)]
    pub async fn create_export(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        object_key: &str,
        manifest_sha256: &str,
        size_bytes: i64,
        item_count: i64,
        include_evidence: bool,
        actor: &str,
    ) -> Result<ExportRecord> {
        let id = uuid::Uuid::now_v7();
        sqlx::query(
            "INSERT INTO exports (id, workspace_id, assessment_id, object_key, manifest_sha256,
                size_bytes, item_count, include_evidence, created_by)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)",
        )
        .bind(id)
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(object_key)
        .bind(manifest_sha256)
        .bind(size_bytes)
        .bind(item_count)
        .bind(include_evidence)
        .bind(actor)
        .execute(&self.pool)
        .await?;
        Ok(ExportRecord {
            id,
            assessment_id,
            object_key: object_key.to_string(),
            manifest_sha256: manifest_sha256.to_string(),
            size_bytes,
            item_count,
            include_evidence,
            created_at: chrono::Utc::now(),
        })
    }

    pub async fn get_export(
        &self,
        workspace_id: WorkspaceId,
        id: uuid::Uuid,
    ) -> Result<Option<ExportRecord>> {
        let row = sqlx::query(
            "SELECT id, assessment_id, object_key, manifest_sha256, size_bytes, item_count,
                include_evidence, created_at
             FROM exports WHERE workspace_id=$1 AND id=$2",
        )
        .bind(workspace_id.as_uuid())
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            Ok(ExportRecord {
                id: row.try_get("id")?,
                assessment_id: row.try_get::<uuid::Uuid, _>("assessment_id")?.into(),
                object_key: row.try_get("object_key")?,
                manifest_sha256: row.try_get("manifest_sha256")?,
                size_bytes: row.try_get("size_bytes")?,
                item_count: row.try_get("item_count")?,
                include_evidence: row.try_get("include_evidence")?,
                created_at: row.try_get("created_at")?,
            })
        })
        .transpose()
    }
}

impl Db {
    pub async fn expire_evidence_now(&self, id: EvidenceId) -> Result<()> {
        sqlx::query("UPDATE evidences SET retention_until = now() - interval '1 day' WHERE id=$1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
