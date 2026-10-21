use chrono::Utc;
use darkbox_contracts::*;
use darkbox_domain::EndpointKey;
use sqlx::Row;

use crate::assessments::{ensure_asset, insert_candidate};
use crate::{Db, DbError, Result, map_json};

#[derive(Debug, Clone)]
pub struct InventoryQuery {
    pub state: Option<String>,
    pub host: Option<String>,
    pub identity_id: Option<IdentityId>,
    pub source: Option<String>,
    pub text: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

impl Default for InventoryQuery {
    fn default() -> Self {
        Self {
            state: None,
            host: None,
            identity_id: None,
            source: None,
            text: None,
            limit: 100,
            offset: 0,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct InventoryPage {
    pub endpoints: Vec<EndpointDto>,
    pub total: i64,
    pub counts: std::collections::BTreeMap<String, i64>,
}

impl Db {
    pub async fn inventory(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        query: &InventoryQuery,
    ) -> Result<InventoryPage> {
        let limit = query.limit.clamp(1, 500);
        let offset = query.offset.max(0);
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'id', e.id, 'asset_id', e.asset_id, 'scheme', e.scheme, 'host', e.host,
                'port', e.port, 'method', e.method, 'path', e.path, 'query_names', e.query_names,
                'state', e.state, 'source', e.source, 'run_id', e.run_id,
                'group_pattern', e.group_pattern, 'identity_id', e.identity_id,
                'observed_at', e.observed_at, 'recorded_at', e.recorded_at
             ) AS row
             FROM endpoints e
             WHERE e.workspace_id=$1 AND e.assessment_id=$2
               AND ($3::text IS NULL OR e.state=$3)
               AND ($4::text IS NULL OR e.host ILIKE '%' || $4 || '%')
               AND ($5::uuid IS NULL OR e.identity_id=$5)
               AND ($6::text IS NULL OR e.source=$6)
               AND ($7::text IS NULL OR e.host ILIKE '%' || $7 || '%' OR e.path ILIKE '%' || $7 || '%' OR e.method ILIKE '%' || $7 || '%')
             ORDER BY e.recorded_at DESC, e.path
             LIMIT $8 OFFSET $9",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(query.state.as_deref())
        .bind(query.host.as_deref())
        .bind(query.identity_id.map(|i| i.as_uuid()))
        .bind(query.source.as_deref())
        .bind(query.text.as_deref())
        .bind(limit)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;
        let endpoints: Vec<EndpointDto> = rows.iter().map(map_json).collect::<Result<_>>()?;

        let total: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM endpoints e
             WHERE e.workspace_id=$1 AND e.assessment_id=$2
               AND ($3::text IS NULL OR e.state=$3)
               AND ($4::text IS NULL OR e.host ILIKE '%' || $4 || '%')
               AND ($5::uuid IS NULL OR e.identity_id=$5)
               AND ($6::text IS NULL OR e.source=$6)
               AND ($7::text IS NULL OR e.host ILIKE '%' || $7 || '%' OR e.path ILIKE '%' || $7 || '%' OR e.method ILIKE '%' || $7 || '%')",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(query.state.as_deref())
        .bind(query.host.as_deref())
        .bind(query.identity_id.map(|i| i.as_uuid()))
        .bind(query.source.as_deref())
        .bind(query.text.as_deref())
        .fetch_one(&self.pool)
        .await?;

        let count_rows = sqlx::query(
            "SELECT state, count(*) AS n FROM endpoints
             WHERE workspace_id=$1 AND assessment_id=$2 GROUP BY state",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        let mut counts = std::collections::BTreeMap::new();
        for row in count_rows {
            counts.insert(
                row.try_get::<String, _>("state")?,
                row.try_get::<i64, _>("n")?,
            );
        }

        Ok(InventoryPage {
            endpoints,
            total,
            counts,
        })
    }

    pub async fn endpoint_by_id(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        id: EndpointId,
    ) -> Result<EndpointDto> {
        let row = sqlx::query(
            "SELECT jsonb_build_object(
                'id', e.id, 'asset_id', e.asset_id, 'scheme', e.scheme, 'host', e.host,
                'port', e.port, 'method', e.method, 'path', e.path, 'query_names', e.query_names,
                'state', e.state, 'source', e.source, 'run_id', e.run_id,
                'group_pattern', e.group_pattern, 'identity_id', e.identity_id,
                'observed_at', e.observed_at, 'recorded_at', e.recorded_at
             ) AS row
             FROM endpoints e WHERE e.workspace_id=$1 AND e.assessment_id=$2 AND e.id=$3",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| DbError::NotFound("endpoint".into()))?;
        map_json(&row)
    }

    /// Inserts a candidate, preserving the first source that reported it.
    #[allow(clippy::too_many_arguments)]
    pub async fn upsert_candidate(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        key: &EndpointKey,
        source: &str,
        source_detail: &str,
        run_id: Option<RunId>,
        identity_id: Option<IdentityId>,
    ) -> Result<EndpointId> {
        let assessment = self.get_assessment(workspace_id, assessment_id).await?;
        let mut tx = self.pool.begin().await?;
        let asset_id = ensure_asset(
            &mut tx,
            workspace_id,
            assessment_id,
            key,
            &assessment.environment,
        )
        .await?;
        let id = insert_candidate(
            &mut tx,
            workspace_id,
            assessment_id,
            asset_id,
            key,
            source,
            source_detail,
            run_id,
            identity_id,
        )
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn mark_observed(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        id: EndpointId,
        state: EndpointState,
        identity_id: Option<IdentityId>,
        source: &str,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE endpoints SET state=$4,
                observed_at = CASE WHEN $4='observed' THEN $5 ELSE observed_at END,
                identity_id = COALESCE($6, identity_id),
                source = CASE WHEN state='candidate' THEN $7 ELSE source END
             WHERE workspace_id=$1 AND assessment_id=$2 AND id=$3",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(id.as_uuid())
        .bind(state.as_str())
        .bind(Utc::now())
        .bind(identity_id.map(|i| i.as_uuid()))
        .bind(source)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_endpoint_state(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        id: EndpointId,
        state: EndpointState,
    ) -> Result<()> {
        sqlx::query(
            "UPDATE endpoints SET state=$4 WHERE workspace_id=$1 AND assessment_id=$2 AND id=$3",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(id.as_uuid())
        .bind(state.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn add_observation(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        subject_kind: &str,
        subject_id: uuid::Uuid,
        property: &str,
        value: serde_json::Value,
        source: &str,
        confidence: f64,
        evidence_id: Option<EvidenceId>,
    ) -> Result<ObservationId> {
        let id = ObservationId::new();
        sqlx::query(
            "INSERT INTO observations (id, workspace_id, assessment_id, subject_kind, subject_id,
                property, value, source, confidence, evidence_id, observed_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,now())",
        )
        .bind(id.as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(subject_kind)
        .bind(subject_id)
        .bind(property)
        .bind(value)
        .bind(source)
        .bind(confidence)
        .bind(evidence_id.map(|e| e.as_uuid()))
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn observations_for(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        subject_kind: &str,
        subject_id: uuid::Uuid,
        limit: i64,
    ) -> Result<Vec<ObservationDto>> {
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'id', o.id, 'subject_kind', o.subject_kind, 'subject_id', o.subject_id,
                'property', o.property, 'value', o.value, 'source', o.source,
                'confidence', o.confidence, 'evidence_id', o.evidence_id,
                'observed_at', o.observed_at, 'recorded_at', o.recorded_at
             ) AS row
             FROM observations o
             WHERE o.workspace_id=$1 AND o.assessment_id=$2 AND o.subject_kind=$3 AND o.subject_id=$4
             ORDER BY o.observed_at DESC LIMIT $5",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(subject_kind)
        .bind(subject_id)
        .bind(limit.clamp(1, 500))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(map_json).collect()
    }

    pub async fn recent_observations(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        limit: i64,
    ) -> Result<Vec<ObservationDto>> {
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'id', o.id, 'subject_kind', o.subject_kind, 'subject_id', o.subject_id,
                'property', o.property, 'value', o.value, 'source', o.source,
                'confidence', o.confidence, 'evidence_id', o.evidence_id,
                'observed_at', o.observed_at, 'recorded_at', o.recorded_at
             ) AS row
             FROM observations o
             WHERE o.workspace_id=$1 AND o.assessment_id=$2
             ORDER BY o.recorded_at DESC LIMIT $3",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(limit.clamp(1, 500))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(map_json).collect()
    }
}
