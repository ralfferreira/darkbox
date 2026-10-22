use chrono::Utc;
use darkbox_contracts::*;
use darkbox_domain::Run;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::events::insert_event_tx;
use crate::{Db, DbError, Result, map_json};

const AGENT_LEASE_SECONDS: i64 = 900;

impl Db {
    /// Creates an agent run already owned by the TypeScript agent service.
    /// The Rust worker never executes the step; it exists as the durable turn
    /// record, lease and transcript anchor.
    pub async fn create_agent_run(
        &self,
        workspace_id: WorkspaceId,
        new: &crate::NewRun,
        actor: &str,
    ) -> Result<Run> {
        let existing: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM runs WHERE workspace_id=$1 AND idempotency_key=$2")
                .bind(workspace_id.as_uuid())
                .bind(&new.idempotency_key)
                .fetch_optional(&self.pool)
                .await?;
        if let Some(id) = existing {
            return self.get_run(workspace_id, id.into()).await;
        }
        if new.kind != RunKind::Agent || new.steps.len() != 1 {
            return Err(DbError::Invalid("agent runs need exactly one step".into()));
        }
        self.get_assessment(workspace_id, new.assessment_id).await?;
        let run_id = RunId::new();
        let step_id = StepId::new();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO runs (id, workspace_id, assessment_id, kind, state, policy_version, policy,
                budget, procedure_key, flow_id, identity_id, idempotency_key, note, started_at)
             VALUES ($1,$2,$3,'agent','running',$4,$5,$6,NULL,NULL,$7,$8,$9,now())",
        )
        .bind(run_id.as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(new.assessment_id.as_uuid())
        .bind(new.policy_version)
        .bind(serde_json::to_value(&new.policy)?)
        .bind(serde_json::to_value(&new.budget)?)
        .bind(new.identity_id.map(|id| id.as_uuid()))
        .bind(&new.idempotency_key)
        .bind(&new.note)
        .execute(&mut *tx)
        .await?;
        let step = &new.steps[0];
        sqlx::query(
            "INSERT INTO run_steps (id, workspace_id, assessment_id, run_id, ordinal, kind, state,
                operation, summary, attempt, lease_owner, lease_expires_at, started_at)
             VALUES ($1,$2,$3,$4,0,$5,'running',$6,$7,1,$8,now() + make_interval(secs => $9),now())",
        )
        .bind(step_id.as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(new.assessment_id.as_uuid())
        .bind(run_id.as_uuid())
        .bind(step.kind.as_str())
        .bind(&step.operation)
        .bind(&step.summary)
        .bind(actor)
        .bind(AGENT_LEASE_SECONDS)
        .execute(&mut *tx)
        .await?;
        insert_event_tx(
            &mut tx,
            &EventEnvelope::new(
                workspace_id,
                new.assessment_id,
                Some(run_id),
                Some(step_id),
                "run.created",
                new.idempotency_key.clone(),
                serde_json::json!({
                    "kind": "agent",
                    "policy_version": new.policy_version,
                    "steps": 1,
                }),
            ),
        )
        .await?;
        insert_event_tx(
            &mut tx,
            &EventEnvelope::new(
                workspace_id,
                new.assessment_id,
                Some(run_id),
                Some(step_id),
                "agent.started",
                run_id.to_string(),
                serde_json::json!({
                    "objective": step.operation.get("objective"),
                    "max_steps": step.operation.get("max_steps"),
                    "token_budget": step.operation.get("token_budget"),
                    "model": step.operation.get("model"),
                    "actor": actor,
                }),
            ),
        )
        .await?;
        tx.commit().await?;
        self.get_run(workspace_id, run_id).await
    }

    async fn agent_run_row(
        &self,
        tx: &mut Transaction<'_, Postgres>,
        workspace_id: WorkspaceId,
        run_id: RunId,
    ) -> Result<(Uuid, Uuid, Uuid, String)> {
        let row = sqlx::query(
            "SELECT id, assessment_id, (SELECT id FROM run_steps WHERE run_id=runs.id LIMIT 1) AS step_id, kind
             FROM runs WHERE workspace_id=$1 AND id=$2 FOR UPDATE",
        )
        .bind(workspace_id.as_uuid())
        .bind(run_id.as_uuid())
        .fetch_optional(&mut **tx)
        .await?
        .ok_or_else(|| DbError::NotFound("agent run".into()))?;
        Ok((
            row.try_get("id")?,
            row.try_get("assessment_id")?,
            row.try_get::<Option<Uuid>, _>("step_id")?
                .ok_or_else(|| DbError::NotFound("agent step".into()))?,
            row.try_get("kind")?,
        ))
    }

    pub async fn append_agent_event(
        &self,
        workspace_id: WorkspaceId,
        run_id: RunId,
        event_type: &str,
        payload: serde_json::Value,
    ) -> Result<EventDto> {
        let mut tx = self.pool.begin().await?;
        let (_, assessment_id, step_id, kind) =
            self.agent_run_row(&mut tx, workspace_id, run_id).await?;
        if kind != "agent" {
            return Err(DbError::Invalid("not an agent run".into()));
        }
        sqlx::query(
            "UPDATE run_steps SET lease_expires_at=now() + make_interval(secs => $2)
             WHERE id=$1 AND state='running'",
        )
        .bind(step_id)
        .bind(AGENT_LEASE_SECONDS)
        .execute(&mut *tx)
        .await?;
        let event = EventEnvelope::new(
            workspace_id,
            assessment_id.into(),
            Some(run_id),
            Some(step_id.into()),
            event_type,
            run_id.to_string(),
            payload,
        );
        insert_event_tx(&mut tx, &event).await?;
        tx.commit().await?;
        Ok(EventDto {
            event_id: event.event_id,
            schema_version: event.schema_version,
            workspace_id,
            assessment_id: assessment_id.into(),
            run_id: Some(run_id),
            step_id: Some(step_id.into()),
            event_type: event.event_type,
            occurred_at: event.occurred_at,
            recorded_at: event.recorded_at,
            correlation_id: event.correlation_id,
            seq: 0,
            payload: event.payload,
        })
    }

    pub async fn finish_agent_run(
        &self,
        workspace_id: WorkspaceId,
        run_id: RunId,
        outcome: &str,
        reason: Option<&str>,
        summary: Option<&str>,
        memory_slug: Option<&str>,
    ) -> Result<RunSummary> {
        let mut tx = self.pool.begin().await?;
        let (_, assessment_id, step_id, kind) =
            self.agent_run_row(&mut tx, workspace_id, run_id).await?;
        if kind != "agent" {
            return Err(DbError::Invalid("not an agent run".into()));
        }
        let step_state = if outcome == "succeeded" {
            "succeeded"
        } else {
            "failed"
        };
        sqlx::query(
            "UPDATE run_steps SET state=$2, summary=COALESCE($3, summary), finished_at=now(),
                lease_owner=NULL, lease_expires_at=NULL WHERE id=$1",
        )
        .bind(step_id)
        .bind(step_state)
        .bind(reason)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE runs SET state=$2, reason=$3, finished_at=now(), updated_at=now()
             WHERE id=$1",
        )
        .bind(run_id.as_uuid())
        .bind(if outcome == "succeeded" {
            "succeeded"
        } else {
            "failed"
        })
        .bind(reason)
        .execute(&mut *tx)
        .await?;
        insert_event_tx(
            &mut tx,
            &EventEnvelope::new(
                workspace_id,
                assessment_id.into(),
                Some(run_id),
                Some(step_id.into()),
                if outcome == "succeeded" {
                    "agent.finished"
                } else {
                    "agent.failed"
                },
                run_id.to_string(),
                serde_json::json!({
                    "outcome": outcome,
                    "reason": reason,
                    "summary": summary,
                    "memory_slug": memory_slug,
                }),
            ),
        )
        .await?;
        tx.commit().await?;
        self.run_summary(workspace_id, run_id).await
    }

    pub async fn agent_run_context(
        &self,
        workspace_id: WorkspaceId,
        run_id: RunId,
    ) -> Result<(
        AssessmentId,
        Option<IdentityId>,
        String,
        i64,
        serde_json::Value,
    )> {
        let row = sqlx::query(
            "SELECT assessment_id, identity_id, state, policy_version,
                (SELECT operation FROM run_steps WHERE run_id=runs.id LIMIT 1) AS operation
             FROM runs WHERE workspace_id=$1 AND id=$2 AND kind='agent'",
        )
        .bind(workspace_id.as_uuid())
        .bind(run_id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| DbError::NotFound("agent run".into()))?;
        Ok((
            row.try_get::<Uuid, _>("assessment_id")?.into(),
            row.try_get::<Option<Uuid>, _>("identity_id")?
                .map(Into::into),
            row.try_get("state")?,
            row.try_get("policy_version")?,
            row.try_get("operation")?,
        ))
    }

    pub async fn expire_agent_lease_if_needed(&self, run_id: RunId) -> Result<bool> {
        let changed = sqlx::query(
            "UPDATE runs SET state='failed', reason='agent_service_lease_expired', finished_at=now()
             WHERE id=$1 AND kind='agent' AND state='running'
               AND EXISTS (SELECT 1 FROM run_steps WHERE run_id=$1 AND state='running'
                           AND lease_expires_at < now())",
        )
        .bind(run_id.as_uuid())
        .execute(&self.pool)
        .await?;
        Ok(changed.rows_affected() == 1)
    }

    pub async fn agent_now(&self) -> chrono::DateTime<Utc> {
        Utc::now()
    }

    pub async fn start_agent_action(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        run_id: RunId,
        session_id: Option<BrowserSessionId>,
        kind: &str,
        action: &serde_json::Value,
    ) -> Result<uuid::Uuid> {
        let id = uuid::Uuid::now_v7();
        let ordinal: i32 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(ordinal), -1) + 1 FROM agent_actions WHERE run_id=$1",
        )
        .bind(run_id.as_uuid())
        .fetch_one(&self.pool)
        .await?;
        sqlx::query(
            "INSERT INTO agent_actions (id, workspace_id, assessment_id, run_id, session_id,
                ordinal, kind, action, state, effect_status)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,'running','not_applicable')",
        )
        .bind(id)
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(run_id.as_uuid())
        .bind(session_id.map(|value| value.as_uuid()))
        .bind(ordinal)
        .bind(kind)
        .bind(action)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)]
    pub async fn finish_agent_action(
        &self,
        id: uuid::Uuid,
        state: &str,
        effect_status: EffectStatus,
        summary: &str,
        observation: Option<&serde_json::Value>,
        evidence_ids: &[EvidenceId],
    ) -> Result<()> {
        sqlx::query(
            "UPDATE agent_actions SET state=$2, effect_status=$3, summary=$4,
                observation=$5, evidence_ids=$6, finished_at=now()
             WHERE id=$1",
        )
        .bind(id)
        .bind(state)
        .bind(effect_status.as_str())
        .bind(summary)
        .bind(observation)
        .bind(
            evidence_ids
                .iter()
                .map(|id| id.as_uuid())
                .collect::<Vec<_>>(),
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn list_agent_actions(&self, run_id: RunId) -> Result<Vec<AgentActionDto>> {
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'id', a.id, 'run_id', a.run_id, 'session_id', a.session_id,
                'ordinal', a.ordinal, 'kind', a.kind, 'action', a.action,
                'state', a.state, 'effect_status', a.effect_status,
                'summary', a.summary, 'observation', a.observation,
                'evidence_ids', to_jsonb(a.evidence_ids), 'started_at', a.started_at,
                'finished_at', a.finished_at
             ) AS row FROM agent_actions a WHERE a.run_id=$1 ORDER BY a.ordinal",
        )
        .bind(run_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(map_json).collect()
    }

    pub async fn agent_session_keys(&self, run_id: RunId) -> Result<Vec<String>> {
        let rows = sqlx::query_scalar::<_, String>(
            "SELECT DISTINCT COALESCE(a.action->>'context_key', '')
             FROM agent_actions a WHERE a.run_id=$1 AND a.action->>'context_key' IS NOT NULL",
        )
        .bind(run_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().filter(|key| !key.is_empty()).collect())
    }

    /// Reserves one model call before the provider is contacted. Repeating the
    /// same call id is idempotent, so a retry never books a second call.
    pub async fn reserve_model_call(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        run_id: RunId,
        call_id: &str,
        model: &str,
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let inserted = sqlx::query(
            "INSERT INTO agent_model_calls
                (call_id, workspace_id, assessment_id, run_id, model)
             VALUES ($1,$2,$3,$4,$5)
             ON CONFLICT (call_id) DO NOTHING",
        )
        .bind(call_id)
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(run_id.as_uuid())
        .bind(model)
        .execute(&mut *tx)
        .await?
        .rows_affected()
            == 1;
        if !inserted {
            tx.commit().await?;
            return Ok(true);
        }
        let spent: i64 = sqlx::query_scalar("SELECT darkbox_reserve_budget($1,'model_calls',1)")
            .bind(run_id.as_uuid())
            .fetch_one(&mut *tx)
            .await?;
        if spent < 0 {
            sqlx::query("DELETE FROM agent_model_calls WHERE call_id=$1")
                .bind(call_id)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
            return Ok(false);
        }
        tx.commit().await?;
        Ok(true)
    }

    /// Settles the real usage against the earlier reservation. Returns the
    /// settled row, or None when the call was already settled.
    #[allow(clippy::too_many_arguments)]
    pub async fn settle_model_call(
        &self,
        run_id: RunId,
        call_id: &str,
        input_tokens: Option<i64>,
        output_tokens: Option<i64>,
        total_tokens: Option<i64>,
        cost_micros: Option<i64>,
        usage_known: bool,
    ) -> Result<Option<serde_json::Value>> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT cost_micros, settled_at FROM agent_model_calls
             WHERE call_id=$1 AND run_id=$2 FOR UPDATE",
        )
        .bind(call_id)
        .bind(run_id.as_uuid())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| DbError::NotFound("model call reservation".into()))?;
        let already: Option<chrono::DateTime<Utc>> = row.try_get("settled_at")?;
        if already.is_some() {
            tx.commit().await?;
            return Ok(None);
        }
        let reserved: i64 = row.try_get::<Option<i64>, _>("cost_micros")?.unwrap_or(0);
        let actual = if usage_known { cost_micros } else { None };
        let delta = actual.unwrap_or(0) - reserved;
        if delta != 0 {
            let spent: i64 = sqlx::query_scalar("SELECT darkbox_adjust_cost($1,$2)")
                .bind(run_id.as_uuid())
                .bind(delta)
                .fetch_one(&mut *tx)
                .await?;
            if spent < 0 {
                tx.rollback().await?;
                return Err(DbError::Conflict(
                    "reported cost exceeds the run budget".into(),
                ));
            }
        }
        sqlx::query(
            "UPDATE agent_model_calls SET settled_at=now(), input_tokens=$2,
                output_tokens=$3, total_tokens=$4, cost_micros=$5, usage_known=$6
             WHERE call_id=$1",
        )
        .bind(call_id)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(total_tokens)
        .bind(actual)
        .bind(usage_known)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(serde_json::json!({
            "call_id": call_id,
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "total_tokens": total_tokens,
            "cost_micros": actual,
            "usage_known": usage_known,
            "reserved_cost_micros": reserved,
        })))
    }

    pub async fn list_model_calls(&self, run_id: RunId) -> Result<Vec<serde_json::Value>> {
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'call_id', call_id, 'model', model, 'reserved_at', reserved_at,
                'settled_at', settled_at, 'input_tokens', input_tokens,
                'output_tokens', output_tokens, 'total_tokens', total_tokens,
                'cost_micros', cost_micros, 'usage_known', usage_known
             ) AS row FROM agent_model_calls WHERE run_id=$1 ORDER BY reserved_at",
        )
        .bind(run_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(map_json).collect()
    }

    /// Stops an agent objective without retrying an effect whose result was
    /// never observed. The action that reached the application stays marked.
    pub async fn mark_agent_outcome_unknown(&self, run_id: RunId, reason: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "UPDATE runs SET state='waiting_operator', effect_unknown=true, reason=$2, updated_at=now()
             WHERE id=$1 AND state='running'",
        )
        .bind(run_id.as_uuid())
        .bind(reason)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE run_steps SET state='failed', effect_status='outcome_unknown',
                summary=$2, finished_at=now(), lease_owner=NULL, lease_expires_at=NULL
             WHERE run_id=$1 AND state='running'",
        )
        .bind(run_id.as_uuid())
        .bind(reason)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn finalize_agent_cancel(
        &self,
        workspace_id: WorkspaceId,
        run_id: RunId,
        reason: &str,
    ) -> Result<RunSummary> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT assessment_id FROM runs WHERE workspace_id=$1 AND id=$2 AND kind='agent'",
        )
        .bind(workspace_id.as_uuid())
        .bind(run_id.as_uuid())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| DbError::NotFound("agent run".into()))?;
        let assessment_id: Uuid = row.try_get("assessment_id")?;
        sqlx::query(
            "UPDATE run_steps SET state='skipped',
                summary=CASE WHEN effect_status='dispatched' THEN 'cancelled; effect may have reached the target'
                             ELSE 'cancelled before dispatch' END,
                finished_at=now(), lease_owner=NULL, lease_expires_at=NULL
             WHERE run_id=$1 AND state IN ('pending','running')",
        )
        .bind(run_id.as_uuid())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE runs SET state='cancelled', reason=$2, finished_at=now(), updated_at=now()
             WHERE id=$1",
        )
        .bind(run_id.as_uuid())
        .bind(reason)
        .execute(&mut *tx)
        .await?;
        insert_event_tx(
            &mut tx,
            &EventEnvelope::new(
                workspace_id,
                assessment_id.into(),
                Some(run_id),
                None,
                "agent.cancelled",
                run_id.to_string(),
                serde_json::json!({ "reason": reason, "partial_results_preserved": true }),
            ),
        )
        .await?;
        tx.commit().await?;
        self.run_summary(workspace_id, run_id).await
    }

    pub async fn last_checkpoint_for_assessment(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
    ) -> Result<Option<(RunId, String)>> {
        let row = sqlx::query(
            "SELECT id, COALESCE(reason, note, '') AS checkpoint
             FROM runs WHERE workspace_id=$1 AND assessment_id=$2 AND kind='agent'
               AND state IN ('failed','cancelled','waiting_operator','interrupted')
             ORDER BY updated_at DESC LIMIT 1",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(|row| {
            (
                row.get::<uuid::Uuid, _>("id").into(),
                row.get::<String, _>("checkpoint"),
            )
        }))
    }
}
