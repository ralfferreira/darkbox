use darkbox_contracts::*;

use crate::{Db, DbError, Result, map_json};

impl Db {
    pub async fn create_action(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        req: &CreateActionRequest,
    ) -> Result<ActionDto> {
        let id = ActionId::new();
        sqlx::query(
            "INSERT INTO actions (id, workspace_id, assessment_id, intent, endpoint_id, effect_class,
                preconditions, expected_effect)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
        )
        .bind(id.as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(&req.intent)
        .bind(req.endpoint_id.map(|e| e.as_uuid()))
        .bind(req.action_class.as_str())
        .bind(serde_json::to_value(&req.preconditions)?)
        .bind(&req.expected_effect)
        .execute(&self.pool)
        .await?;
        let row = sqlx::query(
            "SELECT jsonb_build_object(
                'id', a.id, 'intent', a.intent, 'endpoint_id', a.endpoint_id,
                'effect_class', a.effect_class, 'state', a.state, 'preconditions', a.preconditions,
                'expected_effect', a.expected_effect, 'created_at', a.created_at
             ) AS row FROM actions a WHERE a.id=$1",
        )
        .bind(id.as_uuid())
        .fetch_one(&self.pool)
        .await?;
        map_json(&row)
    }

    pub async fn create_flow(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        req: &CreateFlowRequest,
        actor: &str,
    ) -> Result<FlowDto> {
        if req.name.trim().is_empty() || req.steps.is_empty() {
            return Err(DbError::Invalid(
                "a flow needs a name and at least one step".into(),
            ));
        }
        if let Some(identity_id) = req.identity_id {
            self.get_identity(workspace_id, assessment_id, identity_id)
                .await?;
        }
        let id = FlowId::new();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO flows (id, workspace_id, assessment_id, name, state, version, identity_id,
                tenant_key, preconditions)
             VALUES ($1,$2,$3,$4,'proposed',1,$5,$6,$7)",
        )
        .bind(id.as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(&req.name)
        .bind(req.identity_id.map(|i| i.as_uuid()))
        .bind(&req.tenant_key)
        .bind(serde_json::to_value(&req.preconditions)?)
        .execute(&mut *tx)
        .await?;
        for (index, step) in req.steps.iter().enumerate() {
            let action_id = ActionId::new().as_uuid();
            sqlx::query(
                "INSERT INTO actions (id, workspace_id, assessment_id, intent, endpoint_id, effect_class,
                    preconditions, expected_effect)
                 VALUES ($1,$2,$3,$4,$5,$6,'[]',$7)",
            )
            .bind(action_id)
            .bind(workspace_id.as_uuid())
            .bind(assessment_id.as_uuid())
            .bind(&step.intent)
            .bind(step.endpoint_id.map(|e| e.as_uuid()))
            .bind(step.action_class.unwrap_or(ActionClass::Query).as_str())
            .bind(&step.expected_result)
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO flow_steps (flow_id, ordinal, action_id, intent, expected_result, endpoint_id)
                 VALUES ($1,$2,$3,$4,$5,$6)",
            )
            .bind(id.as_uuid())
            .bind(index as i32)
            .bind(action_id)
            .bind(&step.intent)
            .bind(&step.expected_result)
            .bind(step.endpoint_id.map(|e| e.as_uuid()))
            .execute(&mut *tx)
            .await?;
        }
        sqlx::query(
            "INSERT INTO audit_events (id, workspace_id, assessment_id, actor, operation, target, reason, correlation_id)
             VALUES ($1,$2,$3,$4,'flow.create',$5,'',$6)",
        )
        .bind(AuditEventId::new().as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(actor)
        .bind(&req.name)
        .bind(uuid::Uuid::now_v7().to_string())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.get_flow(workspace_id, assessment_id, id).await
    }

    pub async fn list_flows(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
    ) -> Result<Vec<FlowDto>> {
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'id', f.id, 'name', f.name, 'state', f.state, 'version', f.version,
                'identity_id', f.identity_id, 'tenant_key', f.tenant_key,
                'steps', COALESCE((
                    SELECT jsonb_agg(jsonb_build_object(
                        'ordinal', s.ordinal, 'action_id', s.action_id, 'intent', s.intent,
                        'expected_result', s.expected_result, 'endpoint_id', s.endpoint_id,
                        'confirmed', s.confirmed) ORDER BY s.ordinal)
                    FROM flow_steps s WHERE s.flow_id=f.id), '[]'),
                'evidence_ids', f.evidence_ids, 'preconditions', f.preconditions,
                'observed_at', f.observed_at, 'recorded_at', f.recorded_at
             ) AS row FROM flows f
             WHERE f.workspace_id=$1 AND f.assessment_id=$2 ORDER BY f.recorded_at DESC",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(map_json).collect()
    }

    pub async fn get_flow(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        id: FlowId,
    ) -> Result<FlowDto> {
        let row = sqlx::query(
            "SELECT jsonb_build_object(
                'id', f.id, 'name', f.name, 'state', f.state, 'version', f.version,
                'identity_id', f.identity_id, 'tenant_key', f.tenant_key,
                'steps', COALESCE((
                    SELECT jsonb_agg(jsonb_build_object(
                        'ordinal', s.ordinal, 'action_id', s.action_id, 'intent', s.intent,
                        'expected_result', s.expected_result, 'endpoint_id', s.endpoint_id,
                        'confirmed', s.confirmed) ORDER BY s.ordinal)
                    FROM flow_steps s WHERE s.flow_id=f.id), '[]'),
                'evidence_ids', f.evidence_ids, 'preconditions', f.preconditions,
                'observed_at', f.observed_at, 'recorded_at', f.recorded_at
             ) AS row FROM flows f
             WHERE f.workspace_id=$1 AND f.assessment_id=$2 AND f.id=$3",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| DbError::NotFound("flow".into()))?;
        map_json(&row)
    }

    pub async fn update_flow_observation(
        &self,
        flow_id: FlowId,
        state: FlowState,
        evidence_ids: &[EvidenceId],
        confirmed_ordinals: &[i64],
    ) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE flows SET state=$2, evidence_ids=$3,
                observed_at = CASE WHEN $2 IN ('observed','reproduced') THEN now() ELSE observed_at END
             WHERE id=$1",
        )
        .bind(flow_id.as_uuid())
        .bind(state.as_str())
        .bind(evidence_ids.iter().map(|e| e.as_uuid()).collect::<Vec<_>>())
        .execute(&mut *tx)
        .await?;
        for ordinal in confirmed_ordinals {
            sqlx::query("UPDATE flow_steps SET confirmed=true WHERE flow_id=$1 AND ordinal=$2")
                .bind(flow_id.as_uuid())
                .bind(*ordinal as i32)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    pub async fn workspace_retention(&self, workspace_id: WorkspaceId) -> Result<i32> {
        let days: i32 = sqlx::query_scalar("SELECT retention_days FROM workspaces WHERE id=$1")
            .bind(workspace_id.as_uuid())
            .fetch_one(&self.pool)
            .await?;
        Ok(days)
    }

    pub async fn set_effect_unknown(&self, run_id: RunId) -> Result<()> {
        sqlx::query("UPDATE runs SET effect_unknown=true, updated_at=now() WHERE id=$1")
            .bind(run_id.as_uuid())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn run_state(&self, run_id: RunId) -> Result<RunState> {
        let state: String = sqlx::query_scalar("SELECT state FROM runs WHERE id=$1")
            .bind(run_id.as_uuid())
            .fetch_one(&self.pool)
            .await?;
        state.parse().map_err(DbError::Invalid)
    }
}
