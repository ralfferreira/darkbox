use darkbox_contracts::*;
use darkbox_domain::{Assessment, EndpointKey, ScopeVersion};
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::{Db, DbError, Result, map_json};

pub struct NewAssessment {
    pub company_id: CompanyId,
    pub name: String,
    pub environment: String,
    pub objective: String,
    pub policy: ScopePolicy,
    pub targets: Vec<EndpointKey>,
    pub actor: String,
}

impl Db {
    pub async fn list_companies(&self, workspace_id: WorkspaceId) -> Result<Vec<CompanyDto>> {
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'id', c.id, 'name', c.name, 'description', c.description,
                'assessment_count', (SELECT count(*) FROM assessments a WHERE a.company_id = c.id),
                'last_activity_at', (SELECT max(a.updated_at) FROM assessments a WHERE a.company_id = c.id),
                'created_at', c.created_at
             ) AS row
             FROM companies c WHERE c.workspace_id = $1 ORDER BY c.created_at",
        )
        .bind(workspace_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(map_json).collect()
    }

    pub async fn create_company(
        &self,
        workspace_id: WorkspaceId,
        name: &str,
        description: &str,
    ) -> Result<CompanyId> {
        let name = name.trim();
        if name.is_empty() {
            return Err(DbError::Invalid("company name is required".into()));
        }
        let id = CompanyId::new();
        sqlx::query(
            "INSERT INTO companies (id, workspace_id, name, description) VALUES ($1,$2,$3,$4)",
        )
        .bind(id.as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(name)
        .bind(description)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn get_company(
        &self,
        workspace_id: WorkspaceId,
        id: CompanyId,
    ) -> Result<CompanyDto> {
        let row = sqlx::query(
            "SELECT jsonb_build_object(
                'id', c.id, 'name', c.name, 'description', c.description,
                'assessment_count', (SELECT count(*) FROM assessments a WHERE a.company_id = c.id),
                'last_activity_at', (SELECT max(a.updated_at) FROM assessments a WHERE a.company_id = c.id),
                'created_at', c.created_at
             ) AS row
             FROM companies c WHERE c.workspace_id = $1 AND c.id = $2",
        )
        .bind(workspace_id.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| DbError::NotFound("company".into()))?;
        map_json(&row)
    }

    pub async fn create_assessment(
        &self,
        workspace_id: WorkspaceId,
        new: NewAssessment,
    ) -> Result<Assessment> {
        new.policy.validate().map_err(DbError::Invalid)?;
        let mut tx = self.pool.begin().await?;
        let company: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM companies WHERE workspace_id=$1 AND id=$2")
                .bind(workspace_id.as_uuid())
                .bind(new.company_id.as_uuid())
                .fetch_optional(&mut *tx)
                .await?;
        if company.is_none() {
            return Err(DbError::NotFound("company".into()));
        }
        let id = AssessmentId::new();
        sqlx::query(
            "INSERT INTO assessments (id, workspace_id, company_id, name, environment, objective, scope_version)
             VALUES ($1,$2,$3,$4,$5,$6,1)",
        )
        .bind(id.as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(new.company_id.as_uuid())
        .bind(&new.name)
        .bind(&new.environment)
        .bind(&new.objective)
        .execute(&mut *tx)
        .await?;

        let scope_id = ScopeVersionId::new();
        sqlx::query(
            "INSERT INTO scope_versions (id, workspace_id, assessment_id, version, policy, note, created_by)
             VALUES ($1,$2,$3,1,$4,$5,$6)",
        )
        .bind(scope_id.as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(id.as_uuid())
        .bind(serde_json::to_value(&new.policy)?)
        .bind("initial scope")
        .bind(&new.actor)
        .execute(&mut *tx)
        .await?;

        for key in &new.targets {
            let asset_id = ensure_asset(&mut tx, workspace_id, id, key, &new.environment).await?;
            insert_candidate(
                &mut tx,
                workspace_id,
                id,
                asset_id,
                key,
                "operator",
                "initial target",
                None,
                None,
            )
            .await?;
        }

        sqlx::query(
            "INSERT INTO audit_events (id, workspace_id, assessment_id, actor, operation, target, reason, correlation_id)
             VALUES ($1,$2,$3,$4,'assessment.create',$5,$6,$7)",
        )
        .bind(AuditEventId::new().as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(id.as_uuid())
        .bind(&new.actor)
        .bind(&new.name)
        .bind(&new.objective)
        .bind(Uuid::now_v7().to_string())
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        self.get_assessment(workspace_id, id).await
    }

    pub async fn get_assessment(
        &self,
        workspace_id: WorkspaceId,
        id: AssessmentId,
    ) -> Result<Assessment> {
        let row = sqlx::query(
            "SELECT jsonb_build_object(
                'id', a.id, 'workspace_id', a.workspace_id, 'company_id', a.company_id,
                'name', a.name, 'environment', a.environment, 'objective', a.objective,
                'state', a.state, 'scope_version', a.scope_version,
                'created_at', a.created_at, 'updated_at', a.updated_at
             ) AS row
             FROM assessments a WHERE a.workspace_id=$1 AND a.id=$2",
        )
        .bind(workspace_id.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| DbError::NotFound("assessment".into()))?;
        map_json(&row)
    }

    pub async fn list_assessments(
        &self,
        workspace_id: WorkspaceId,
        company_id: Option<CompanyId>,
    ) -> Result<Vec<AssessmentSummary>> {
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'id', a.id, 'company_id', a.company_id, 'company_name', c.name,
                'name', a.name, 'environment', a.environment, 'objective', a.objective,
                'state', a.state, 'scope_version', a.scope_version,
                'endpoint_count', (SELECT count(*) FROM endpoints e WHERE e.assessment_id=a.id),
                'observed_count', (SELECT count(*) FROM endpoints e WHERE e.assessment_id=a.id AND e.state='observed'),
                'run_count', (SELECT count(*) FROM runs r WHERE r.assessment_id=a.id),
                'open_findings', (SELECT count(*) FROM findings f WHERE f.assessment_id=a.id AND f.state NOT IN ('rejected','remediated')),
                'last_activity_at', GREATEST(a.updated_at, COALESCE((SELECT max(ev.recorded_at) FROM events ev WHERE ev.assessment_id=a.id), a.updated_at)),
                'created_at', a.created_at
             ) AS row
             FROM assessments a JOIN companies c ON c.id=a.company_id
             WHERE a.workspace_id=$1 AND ($2::uuid IS NULL OR a.company_id=$2)
             ORDER BY a.updated_at DESC",
        )
        .bind(workspace_id.as_uuid())
        .bind(company_id.map(|c| c.as_uuid()))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(map_json).collect()
    }

    pub async fn assessment_summary(
        &self,
        workspace_id: WorkspaceId,
        id: AssessmentId,
    ) -> Result<AssessmentSummary> {
        let list = self.list_assessments(workspace_id, None).await?;
        list.into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| DbError::NotFound("assessment".into()))
    }

    pub async fn get_scope(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        version: Option<i64>,
    ) -> Result<ScopeVersion> {
        let version = match version {
            Some(version) => version,
            None => {
                self.get_assessment(workspace_id, assessment_id)
                    .await?
                    .scope_version
            }
        };
        let row = sqlx::query(
            "SELECT jsonb_build_object(
                'id', s.id, 'workspace_id', s.workspace_id, 'assessment_id', s.assessment_id,
                'version', s.version, 'policy', s.policy, 'note', s.note,
                'created_by', s.created_by, 'created_at', s.created_at
             ) AS row
             FROM scope_versions s
             WHERE s.workspace_id=$1 AND s.assessment_id=$2 AND s.version=$3",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(version)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| DbError::NotFound("scope version".into()))?;
        map_json(&row)
    }

    pub async fn list_scope_versions(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
    ) -> Result<Vec<ScopeVersion>> {
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'id', s.id, 'workspace_id', s.workspace_id, 'assessment_id', s.assessment_id,
                'version', s.version, 'policy', s.policy, 'note', s.note,
                'created_by', s.created_by, 'created_at', s.created_at
             ) AS row
             FROM scope_versions s
             WHERE s.workspace_id=$1 AND s.assessment_id=$2 ORDER BY s.version DESC",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(map_json).collect()
    }

    pub async fn create_scope_version(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        policy: &ScopePolicy,
        note: &str,
        actor: &str,
    ) -> Result<ScopeVersion> {
        policy.validate().map_err(DbError::Invalid)?;
        let mut tx = self.pool.begin().await?;
        let current: i64 = sqlx::query_scalar(
            "SELECT scope_version FROM assessments WHERE workspace_id=$1 AND id=$2 FOR UPDATE",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| DbError::NotFound("assessment".into()))?;
        let version = current + 1;
        sqlx::query(
            "INSERT INTO scope_versions (id, workspace_id, assessment_id, version, policy, note, created_by)
             VALUES ($1,$2,$3,$4,$5,$6,$7)",
        )
        .bind(ScopeVersionId::new().as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(version)
        .bind(serde_json::to_value(policy)?)
        .bind(note)
        .bind(actor)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE assessments SET scope_version=$3, updated_at=now() WHERE workspace_id=$1 AND id=$2")
            .bind(workspace_id.as_uuid())
            .bind(assessment_id.as_uuid())
            .bind(version)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.get_scope(workspace_id, assessment_id, Some(version))
            .await
    }

    pub async fn coverage(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
    ) -> Result<CoverageDto> {
        let row = sqlx::query(
            "SELECT
                (SELECT count(*) FROM endpoints WHERE assessment_id=$2) AS candidates,
                (SELECT count(*) FROM endpoints WHERE assessment_id=$2 AND state='observed') AS observed,
                (SELECT count(*) FROM endpoints WHERE assessment_id=$2 AND state='excluded') AS excluded,
                (SELECT count(*) FROM endpoints WHERE assessment_id=$2 AND state='unreachable') AS unreachable,
                (SELECT count(*) FROM flows WHERE assessment_id=$2 AND state='reproduced') AS flows_reproduced,
                (SELECT count(*) FROM flows WHERE assessment_id=$2) AS flows_known,
                (SELECT count(*) FROM findings WHERE assessment_id=$2 AND state='confirmed') AS confirmed,
                (SELECT count(*) FROM findings WHERE assessment_id=$2 AND state='needs_review') AS review,
                (SELECT count(*) FROM evidences WHERE assessment_id=$2) AS evidence_count,
                (SELECT COALESCE(sum(size_bytes),0)::bigint FROM evidences WHERE assessment_id=$2) AS evidence_bytes
             FROM assessments WHERE workspace_id=$1 AND id=$2",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| DbError::NotFound("assessment".into()))?;
        Ok(CoverageDto {
            known_candidates: row.try_get("candidates")?,
            observed_endpoints: row.try_get("observed")?,
            excluded_endpoints: row.try_get("excluded")?,
            unreachable_endpoints: row.try_get("unreachable")?,
            flows_reproduced: row.try_get("flows_reproduced")?,
            flows_known: row.try_get("flows_known")?,
            findings_confirmed: row.try_get("confirmed")?,
            findings_needs_review: row.try_get("review")?,
            evidence_count: row.try_get("evidence_count")?,
            evidence_bytes: row.try_get("evidence_bytes")?,
        })
    }

    pub async fn touch_assessment(&self, assessment_id: AssessmentId) -> Result<()> {
        sqlx::query("UPDATE assessments SET updated_at=now() WHERE id=$1")
            .bind(assessment_id.as_uuid())
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

pub(crate) async fn ensure_asset(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: WorkspaceId,
    assessment_id: AssessmentId,
    key: &EndpointKey,
    environment: &str,
) -> Result<AssetId> {
    let canonical = key.origin();
    let existing: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM assets WHERE assessment_id=$1 AND kind='origin' AND canonical_id=$2",
    )
    .bind(assessment_id.as_uuid())
    .bind(&canonical)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(id) = existing {
        return Ok(id.into());
    }
    let id = AssetId::new();
    sqlx::query(
        "INSERT INTO assets (id, workspace_id, assessment_id, kind, canonical_id, environment, relation_evidence)
         VALUES ($1,$2,$3,'origin',$4,$5,'observed at initial target')",
    )
    .bind(id.as_uuid())
    .bind(workspace_id.as_uuid())
    .bind(assessment_id.as_uuid())
    .bind(&canonical)
    .bind(environment)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn insert_candidate(
    tx: &mut Transaction<'_, Postgres>,
    workspace_id: WorkspaceId,
    assessment_id: AssessmentId,
    asset_id: AssetId,
    key: &EndpointKey,
    source: &str,
    source_detail: &str,
    run_id: Option<RunId>,
    identity_id: Option<IdentityId>,
) -> Result<EndpointId> {
    let record_key = key.record_key();
    let existing: Option<Uuid> =
        sqlx::query_scalar("SELECT id FROM endpoints WHERE assessment_id=$1 AND record_key=$2")
            .bind(assessment_id.as_uuid())
            .bind(&record_key)
            .fetch_optional(&mut **tx)
            .await?;
    if let Some(id) = existing {
        return Ok(id.into());
    }
    let id = EndpointId::new();
    let group = key.route_group();
    sqlx::query(
        "INSERT INTO endpoints (id, workspace_id, assessment_id, asset_id, scheme, host, port, method, path,
            query_names, record_key, state, source, source_detail, run_id, identity_id, group_pattern, group_version)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'candidate',$12,$13,$14,$15,$16,$17)",
    )
    .bind(id.as_uuid())
    .bind(workspace_id.as_uuid())
    .bind(assessment_id.as_uuid())
    .bind(asset_id.as_uuid())
    .bind(&key.scheme)
    .bind(&key.host)
    .bind(key.port as i32)
    .bind(&key.method)
    .bind(&key.path)
    .bind(serde_json::to_value(&key.query_names)?)
    .bind(&record_key)
    .bind(source)
    .bind(source_detail)
    .bind(run_id.map(|r| r.as_uuid()))
    .bind(identity_id.map(|i| i.as_uuid()))
    .bind(&group.pattern)
    .bind(group.version)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

impl Db {
    pub async fn upsert_asset(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        kind: AssetKind,
        canonical_id: &str,
        relation: &str,
    ) -> Result<AssetId> {
        let assessment = self.get_assessment(workspace_id, assessment_id).await?;
        let existing: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM assets WHERE assessment_id=$1 AND kind=$2 AND canonical_id=$3",
        )
        .bind(assessment_id.as_uuid())
        .bind(kind.as_str())
        .bind(canonical_id)
        .fetch_optional(&self.pool)
        .await?;
        if let Some(id) = existing {
            return Ok(id.into());
        }
        let id = AssetId::new();
        sqlx::query(
            "INSERT INTO assets (id, workspace_id, assessment_id, kind, canonical_id, environment, relation_evidence)
             VALUES ($1,$2,$3,$4,$5,$6,$7)
             ON CONFLICT (assessment_id, kind, canonical_id) DO NOTHING",
        )
        .bind(id.as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(kind.as_str())
        .bind(canonical_id)
        .bind(&assessment.environment)
        .bind(relation)
        .execute(&self.pool)
        .await?;
        let id: Uuid = sqlx::query_scalar(
            "SELECT id FROM assets WHERE assessment_id=$1 AND kind=$2 AND canonical_id=$3",
        )
        .bind(assessment_id.as_uuid())
        .bind(kind.as_str())
        .bind(canonical_id)
        .fetch_one(&self.pool)
        .await?;
        Ok(id.into())
    }

    pub async fn distinct_hosts(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
    ) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT host FROM endpoints
             WHERE workspace_id=$1 AND assessment_id=$2 ORDER BY host",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows)
    }
}
