use darkbox_contracts::*;
use sqlx::Row;

use crate::{Db, DbError, Result, map_json};

#[derive(Debug, Clone)]
pub struct NewProcedure {
    pub key: ProcedureKey,
    pub id: String,
    pub version: String,
    pub name: String,
    pub purpose: String,
    pub target_types: Vec<String>,
    pub effect_class: ActionClass,
    pub required_capabilities: Vec<String>,
    pub preconditions: Vec<String>,
    pub manifest: serde_json::Value,
}

impl Db {
    pub async fn upsert_procedure(&self, new: NewProcedure) -> Result<()> {
        sqlx::query(
            "INSERT INTO procedures (id, version, name, purpose, target_types, effect_class,
                required_capabilities, preconditions, manifest)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)
             ON CONFLICT (id, version) DO UPDATE SET
                name=EXCLUDED.name, purpose=EXCLUDED.purpose, target_types=EXCLUDED.target_types,
                effect_class=EXCLUDED.effect_class, required_capabilities=EXCLUDED.required_capabilities,
                preconditions=EXCLUDED.preconditions, manifest=EXCLUDED.manifest",
        )
        .bind(&new.id)
        .bind(&new.version)
        .bind(&new.name)
        .bind(&new.purpose)
        .bind(serde_json::to_value(&new.target_types)?)
        .bind(new.effect_class.as_str())
        .bind(serde_json::to_value(&new.required_capabilities)?)
        .bind(serde_json::to_value(&new.preconditions)?)
        .bind(&new.manifest)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_procedures(&self) -> Result<Vec<ProcedureDto>> {
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'key', p.id || '@' || p.version, 'id', p.id, 'version', p.version, 'name', p.name,
                'purpose', p.purpose, 'target_types', p.target_types, 'effect_class', p.effect_class,
                'required_capabilities', p.required_capabilities, 'preconditions', p.preconditions,
                'limits', p.manifest->'limits', 'manifest', p.manifest, 'published_at', p.published_at
             ) AS row FROM procedures p ORDER BY p.id, p.version DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(map_json).collect()
    }

    pub async fn get_procedure(&self, key: &ProcedureKey) -> Result<ProcedureDto> {
        let row = sqlx::query(
            "SELECT jsonb_build_object(
                'key', p.id || '@' || p.version, 'id', p.id, 'version', p.version, 'name', p.name,
                'purpose', p.purpose, 'target_types', p.target_types, 'effect_class', p.effect_class,
                'required_capabilities', p.required_capabilities, 'preconditions', p.preconditions,
                'limits', p.manifest->'limits', 'manifest', p.manifest, 'published_at', p.published_at
             ) AS row FROM procedures p WHERE p.id || '@' || p.version = $1",
        )
        .bind(&key.0)
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| DbError::NotFound("procedure".into()))?;
        map_json(&row)
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert_test_result(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        procedure_key: &ProcedureKey,
        target_kind: &str,
        target_id: &str,
        identity_id: Option<IdentityId>,
        state: TestResultState,
        conclusion: &str,
        evidence_ids: &[EvidenceId],
    ) -> Result<ResultId> {
        let id = ResultId::new();
        sqlx::query(
            "INSERT INTO test_results (id, workspace_id, assessment_id, procedure_key, target_kind,
                target_id, identity_id, state, conclusion, evidence_ids)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(id.as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(&procedure_key.0)
        .bind(target_kind)
        .bind(target_id)
        .bind(identity_id.map(|i| i.as_uuid()))
        .bind(state.as_str())
        .bind(conclusion)
        .bind(evidence_ids.iter().map(|e| e.as_uuid()).collect::<Vec<_>>())
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn list_test_results(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        limit: i64,
    ) -> Result<Vec<TestResultDto>> {
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'id', t.id, 'procedure_key', t.procedure_key, 'target_kind', t.target_kind,
                'target_id', t.target_id, 'identity_id', t.identity_id, 'state', t.state,
                'conclusion', t.conclusion, 'evidence_ids', t.evidence_ids, 'created_at', t.created_at
             ) AS row FROM test_results t
             WHERE t.workspace_id=$1 AND t.assessment_id=$2
             ORDER BY t.created_at DESC LIMIT $3",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(limit.clamp(1, 200))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(map_json).collect()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct EligibilityInput {
    pub procedure_key: ProcedureKey,
    pub procedure_name: String,
    pub required_capabilities: Vec<String>,
    #[serde(skip)]
    pub available_capabilities: Vec<String>,
    pub target_count: i64,
    pub requires_identity: bool,
    pub has_identity: bool,
}

impl Db {
    pub async fn eligibility_inputs(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
    ) -> Result<Vec<EligibilityInput>> {
        let rows = sqlx::query(
            "SELECT p.id, p.version, p.name, p.required_capabilities, p.target_types, p.preconditions,
                (SELECT count(*) FROM endpoints e WHERE e.assessment_id=$2 AND e.state='observed') AS target_count,
                EXISTS(SELECT 1 FROM identities i WHERE i.assessment_id=$2) AS has_identity
             FROM procedures p ORDER BY p.id",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let id: String = row.try_get("id")?;
                let version: String = row.try_get("version")?;
                let target_types: Vec<String> =
                    serde_json::from_value(row.try_get("target_types")?)?;
                let preconditions: Vec<String> =
                    serde_json::from_value(row.try_get("preconditions")?)?;
                let requires_identity = target_types.iter().any(|t| t == "flow")
                    || preconditions.iter().any(|p| p.contains("identity"));
                Ok(EligibilityInput {
                    procedure_key: ProcedureKey::new(&id, &version),
                    procedure_name: row.try_get("name")?,
                    required_capabilities: serde_json::from_value(
                        row.try_get("required_capabilities")?,
                    )?,
                    available_capabilities: Vec::new(),
                    target_count: row.try_get("target_count")?,
                    requires_identity,
                    has_identity: row.try_get("has_identity")?,
                })
            })
            .collect()
    }
}
