use darkbox_contracts::*;
use darkbox_domain::{Run, RunStepRecord};
use sqlx::Row;
use uuid::Uuid;

use crate::events::insert_event_tx;
use crate::{Db, DbError, Result, map_json};

pub const RUN_JSON: &str = "to_jsonb(r) || jsonb_build_object(
    'spent', jsonb_build_object(
        'requests', r.spent_requests,
        'browser_steps', r.spent_browser_steps,
        'model_calls', r.spent_model_calls,
        'cost_micros', r.spent_cost_micros,
        'overage_micros', r.cost_overage_micros,
        'artifact_bytes', r.spent_artifact_bytes
    ),
    'budget_blocked', r.budget_blocked
)";

#[derive(Debug, Clone)]
pub struct NewStep {
    pub kind: StepKind,
    pub operation: serde_json::Value,
    pub summary: String,
}

#[derive(Debug, Clone)]
pub struct NewRun {
    pub assessment_id: AssessmentId,
    pub kind: RunKind,
    pub policy: ScopePolicy,
    pub policy_version: i64,
    pub budget: Budget,
    pub procedure_key: Option<ProcedureKey>,
    pub flow_id: Option<FlowId>,
    pub identity_id: Option<IdentityId>,
    pub idempotency_key: String,
    pub note: String,
    pub steps: Vec<NewStep>,
}

#[derive(Debug, Clone)]
pub struct ClaimedStep {
    pub run: Run,
    pub step: RunStepRecord,
}

impl Db {
    pub async fn create_run(&self, workspace_id: WorkspaceId, new: NewRun) -> Result<Run> {
        if new.idempotency_key.trim().is_empty() {
            return Err(DbError::Invalid("idempotency_key is required".into()));
        }
        if new.steps.is_empty() {
            return Err(DbError::Invalid("a run needs at least one step".into()));
        }
        let existing: Option<Uuid> =
            sqlx::query_scalar("SELECT id FROM runs WHERE workspace_id=$1 AND idempotency_key=$2")
                .bind(workspace_id.as_uuid())
                .bind(&new.idempotency_key)
                .fetch_optional(&self.pool)
                .await?;
        if let Some(id) = existing {
            return self.get_run(workspace_id, id.into()).await;
        }

        let assessment = self.get_assessment(workspace_id, new.assessment_id).await?;
        let run_id = RunId::new();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO runs (id, workspace_id, assessment_id, kind, state, policy_version, policy, budget,
                procedure_key, flow_id, identity_id, idempotency_key, note)
             VALUES ($1,$2,$3,$4,'queued',$5,$6,$7,$8,$9,$10,$11,$12)",
        )
        .bind(run_id.as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(new.assessment_id.as_uuid())
        .bind(new.kind.as_str())
        .bind(new.policy_version)
        .bind(serde_json::to_value(&new.policy)?)
        .bind(serde_json::to_value(&new.budget)?)
        .bind(new.procedure_key.as_ref().map(|k| k.0.clone()))
        .bind(new.flow_id.map(|f| f.as_uuid()))
        .bind(new.identity_id.map(|i| i.as_uuid()))
        .bind(&new.idempotency_key)
        .bind(&new.note)
        .execute(&mut *tx)
        .await?;

        for (index, step) in new.steps.iter().enumerate() {
            sqlx::query(
                "INSERT INTO run_steps (id, workspace_id, assessment_id, run_id, ordinal, kind, operation, summary)
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
            )
            .bind(StepId::new().as_uuid())
            .bind(workspace_id.as_uuid())
            .bind(new.assessment_id.as_uuid())
            .bind(run_id.as_uuid())
            .bind(index as i32)
            .bind(step.kind.as_str())
            .bind(&step.operation)
            .bind(&step.summary)
            .execute(&mut *tx)
            .await?;
        }

        insert_event_tx(
            &mut tx,
            &EventEnvelope::new(
                workspace_id,
                new.assessment_id,
                Some(run_id),
                None,
                "run.created",
                new.idempotency_key.clone(),
                serde_json::json!({
                    "kind": new.kind.as_str(),
                    "steps": new.steps.len(),
                    "policy_version": new.policy_version,
                    "procedure": new.procedure_key.as_ref().map(|k| k.0.clone()),
                }),
            ),
        )
        .await?;
        sqlx::query("UPDATE assessments SET updated_at=now() WHERE id=$1")
            .bind(assessment.id.as_uuid())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        self.get_run(workspace_id, run_id).await
    }

    pub async fn get_run(&self, workspace_id: WorkspaceId, run_id: RunId) -> Result<Run> {
        let sql =
            format!("SELECT {RUN_JSON} AS row FROM runs r WHERE r.workspace_id=$1 AND r.id=$2");
        let row = sqlx::query(&sql)
            .bind(workspace_id.as_uuid())
            .bind(run_id.as_uuid())
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| DbError::NotFound("run".into()))?;
        map_json(&row)
    }

    pub async fn run_summary(
        &self,
        workspace_id: WorkspaceId,
        run_id: RunId,
    ) -> Result<RunSummary> {
        let row = sqlx::query(
            "SELECT jsonb_build_object(
                'id', r.id, 'assessment_id', r.assessment_id, 'kind', r.kind, 'state', r.state,
                'procedure_key', r.procedure_key, 'policy_version', r.policy_version,
                'budget', r.budget,
                'spent', jsonb_build_object(
                    'requests', r.spent_requests, 'browser_steps', r.spent_browser_steps,
                    'model_calls', r.spent_model_calls, 'cost_micros', r.spent_cost_micros,
                    'overage_micros', r.cost_overage_micros,
                    'artifact_bytes', r.spent_artifact_bytes),
                'effect_unknown', r.effect_unknown, 'budget_blocked', r.budget_blocked,
                'reason', r.reason,
                'idempotency_key', r.idempotency_key,
                'created_at', r.created_at, 'started_at', r.started_at, 'finished_at', r.finished_at,
                'step_count', (SELECT count(*) FROM run_steps s WHERE s.run_id=r.id),
                'steps_done', (SELECT count(*) FROM run_steps s WHERE s.run_id=r.id AND s.state IN ('succeeded','failed','skipped','blocked'))
             ) AS row
             FROM runs r WHERE r.workspace_id=$1 AND r.id=$2",
        )
        .bind(workspace_id.as_uuid())
        .bind(run_id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| DbError::NotFound("run".into()))?;
        map_json(&row)
    }

    pub async fn list_runs(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        limit: i64,
    ) -> Result<Vec<RunSummary>> {
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'id', r.id, 'assessment_id', r.assessment_id, 'kind', r.kind, 'state', r.state,
                'procedure_key', r.procedure_key, 'policy_version', r.policy_version,
                'budget', r.budget,
                'spent', jsonb_build_object(
                    'requests', r.spent_requests, 'browser_steps', r.spent_browser_steps,
                    'model_calls', r.spent_model_calls, 'cost_micros', r.spent_cost_micros,
                    'overage_micros', r.cost_overage_micros,
                    'artifact_bytes', r.spent_artifact_bytes),
                'effect_unknown', r.effect_unknown, 'budget_blocked', r.budget_blocked,
                'reason', r.reason,
                'idempotency_key', r.idempotency_key,
                'created_at', r.created_at, 'started_at', r.started_at, 'finished_at', r.finished_at,
                'step_count', (SELECT count(*) FROM run_steps s WHERE s.run_id=r.id),
                'steps_done', (SELECT count(*) FROM run_steps s WHERE s.run_id=r.id AND s.state IN ('succeeded','failed','skipped','blocked'))
             ) AS row
             FROM runs r WHERE r.workspace_id=$1 AND r.assessment_id=$2
             ORDER BY r.created_at DESC LIMIT $3",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(limit.clamp(1, 200))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(map_json).collect()
    }

    pub async fn run_steps(&self, run_id: RunId) -> Result<Vec<RunStepRecord>> {
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'id', s.id, 'run_id', s.run_id, 'ordinal', s.ordinal, 'kind', s.kind,
                'state', s.state, 'effect_status', s.effect_status, 'operation', s.operation,
                'summary', s.summary, 'attempt', s.attempt,
                'started_at', s.started_at, 'finished_at', s.finished_at
             ) AS row FROM run_steps s WHERE s.run_id=$1 ORDER BY s.ordinal",
        )
        .bind(run_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(map_json).collect()
    }

    pub async fn run_step_dtos(&self, run_id: RunId) -> Result<Vec<RunStepDto>> {
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'id', s.id, 'ordinal', s.ordinal, 'kind', s.kind, 'state', s.state,
                'attempt', s.attempt, 'effect_status', s.effect_status, 'summary', s.summary,
                'started_at', s.started_at, 'finished_at', s.finished_at
             ) AS row FROM run_steps s WHERE s.run_id=$1 ORDER BY s.ordinal",
        )
        .bind(run_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(map_json).collect()
    }

    pub async fn claim_step(
        &self,
        worker_id: &str,
        lease_seconds: i64,
        assessment_id: Option<AssessmentId>,
    ) -> Result<Option<ClaimedStep>> {
        let mut tx = self.pool.begin().await?;
        let candidate: Option<Uuid> = sqlx::query_scalar(
            "SELECT s.id FROM run_steps s JOIN runs r ON r.id=s.run_id
             WHERE s.state='pending' AND r.state IN ('queued','running')
               AND ($1::uuid IS NULL OR r.assessment_id=$1)
               AND NOT EXISTS (
                   SELECT 1 FROM run_steps earlier
                   WHERE earlier.run_id=s.run_id AND earlier.ordinal < s.ordinal
                     AND earlier.state NOT IN ('succeeded','failed','skipped','blocked'))
             ORDER BY s.run_id, s.ordinal
             FOR UPDATE OF s SKIP LOCKED LIMIT 1",
        )
        .bind(assessment_id.map(|a| a.as_uuid()))
        .fetch_optional(&mut *tx)
        .await?;
        let Some(step_id) = candidate else {
            tx.commit().await?;
            return Ok(None);
        };
        sqlx::query(
            "UPDATE run_steps SET state='running', attempt=attempt+1, lease_owner=$2,
                lease_expires_at=now() + make_interval(secs => $3),
                started_at=COALESCE(started_at, now())
             WHERE id=$1",
        )
        .bind(step_id)
        .bind(worker_id)
        .bind(lease_seconds)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "UPDATE runs SET state='running', started_at=COALESCE(started_at, now()), updated_at=now()
             WHERE id=(SELECT run_id FROM run_steps WHERE id=$1) AND state='queued'",
        )
        .bind(step_id)
        .execute(&mut *tx)
        .await?;
        let run_id: Uuid = sqlx::query_scalar("SELECT run_id FROM run_steps WHERE id=$1")
            .bind(step_id)
            .fetch_one(&mut *tx)
            .await?;
        tx.commit().await?;

        let run = self
            .get_run(self.run_workspace(run_id).await?, run_id.into())
            .await?;
        let steps = self.run_steps(run.id).await?;
        let step = steps
            .into_iter()
            .find(|s| s.id.as_uuid() == step_id)
            .ok_or_else(|| DbError::NotFound("step".into()))?;
        Ok(Some(ClaimedStep { run, step }))
    }

    async fn run_workspace(&self, run_id: Uuid) -> Result<WorkspaceId> {
        let id: Uuid = sqlx::query_scalar("SELECT workspace_id FROM runs WHERE id=$1")
            .bind(run_id)
            .fetch_one(&self.pool)
            .await?;
        Ok(id.into())
    }

    pub async fn heartbeat(
        &self,
        step_id: StepId,
        worker_id: &str,
        lease_seconds: i64,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE run_steps SET lease_expires_at=now() + make_interval(secs => $3)
             WHERE id=$1 AND lease_owner=$2 AND state='running'",
        )
        .bind(step_id.as_uuid())
        .bind(worker_id)
        .bind(lease_seconds)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn mark_step_dispatched(
        &self,
        step_id: StepId,
        worker_id: &str,
        intent: &str,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE run_steps SET effect_status='dispatched'
             WHERE id=$1 AND lease_owner=$2 AND state='running'
               AND effect_status IN ('not_dispatched','not_applicable')",
        )
        .bind(step_id.as_uuid())
        .bind(worker_id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() == 1 {
            let run_id: Uuid = sqlx::query_scalar("SELECT run_id FROM run_steps WHERE id=$1")
                .bind(step_id.as_uuid())
                .fetch_one(&self.pool)
                .await?;
            self.add_event(EventEnvelope::new(
                self.run_workspace(run_id).await?,
                self.run_assessment(run_id.into()).await?,
                Some(run_id.into()),
                Some(step_id),
                "step.dispatched",
                step_id.to_string(),
                serde_json::json!({ "intent": intent }),
            ))
            .await?;
        }
        Ok(result.rows_affected() == 1)
    }

    async fn run_assessment(&self, run_id: RunId) -> Result<AssessmentId> {
        let id: Uuid = sqlx::query_scalar("SELECT assessment_id FROM runs WHERE id=$1")
            .bind(run_id.as_uuid())
            .fetch_one(&self.pool)
            .await?;
        Ok(id.into())
    }

    /// Finishes a step. A worker that lost its lease cannot write.
    pub async fn finish_step(
        &self,
        step_id: StepId,
        worker_id: &str,
        state: &str,
        effect_status: EffectStatus,
        summary: &str,
        result: serde_json::Value,
    ) -> Result<bool> {
        let mut tx = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE run_steps SET state=$3, effect_status=$4, summary=$5, finished_at=now(),
                lease_owner=NULL, lease_expires_at=NULL
             WHERE id=$1 AND lease_owner=$2 AND state='running'",
        )
        .bind(step_id.as_uuid())
        .bind(worker_id)
        .bind(state)
        .bind(effect_status.as_str())
        .bind(summary)
        .execute(&mut *tx)
        .await?;
        if updated.rows_affected() == 0 {
            tx.rollback().await?;
            return Ok(false);
        }
        let run_id: Uuid = sqlx::query_scalar("SELECT run_id FROM run_steps WHERE id=$1")
            .bind(step_id.as_uuid())
            .fetch_one(&mut *tx)
            .await?;
        let assessment_id: Uuid = sqlx::query_scalar("SELECT assessment_id FROM runs WHERE id=$1")
            .bind(run_id)
            .fetch_one(&mut *tx)
            .await?;
        let workspace_id: Uuid = sqlx::query_scalar("SELECT workspace_id FROM runs WHERE id=$1")
            .bind(run_id)
            .fetch_one(&mut *tx)
            .await?;
        insert_event_tx(
            &mut tx,
            &EventEnvelope::new(
                workspace_id.into(),
                assessment_id.into(),
                Some(run_id.into()),
                Some(step_id),
                format!("step.{state}"),
                step_id.to_string(),
                serde_json::json!({ "summary": summary, "effect_status": effect_status.as_str(), "result": result }),
            ),
        )
        .await?;
        tx.commit().await?;
        Ok(true)
    }

    /// Refunds the unused part of a request grant after a browser command. The
    /// grant was reserved before dispatch; actual requests are settled here.
    pub async fn settle_requests(&self, run_id: RunId, reserved: i64, used: i64) -> Result<i64> {
        let refund = (reserved - used).max(0);
        if refund == 0 {
            let spent: i64 = sqlx::query_scalar("SELECT spent_requests FROM runs WHERE id=$1")
                .bind(run_id.as_uuid())
                .fetch_one(&self.pool)
                .await?;
            return Ok(spent);
        }
        let spent: i64 = sqlx::query_scalar(
            "UPDATE runs SET spent_requests=GREATEST(0, spent_requests - $2), updated_at=now()
             WHERE id=$1 RETURNING spent_requests",
        )
        .bind(run_id.as_uuid())
        .bind(refund)
        .fetch_one(&self.pool)
        .await?;
        Ok(spent)
    }

    pub async fn reserve_budget(&self, run_id: RunId, kind: &str, amount: i64) -> Result<bool> {
        let spent: i64 = sqlx::query_scalar("SELECT darkbox_reserve_budget($1,$2,$3)")
            .bind(run_id.as_uuid())
            .bind(kind)
            .bind(amount)
            .fetch_one(&self.pool)
            .await?;
        Ok(spent >= 0)
    }

    pub async fn set_run_state(
        &self,
        run_id: RunId,
        expected: &[RunState],
        next: RunState,
        reason: Option<&str>,
    ) -> Result<bool> {
        let expected: Vec<String> = expected.iter().map(|s| s.as_str().to_string()).collect();
        let result = sqlx::query(
            "UPDATE runs SET state=$2, reason=COALESCE($3, reason), updated_at=now(),
                finished_at=CASE WHEN $2 IN ('succeeded','failed','cancelled') THEN now() ELSE finished_at END
             WHERE id=$1 AND state = ANY($4)",
        )
        .bind(run_id.as_uuid())
        .bind(next.as_str())
        .bind(reason)
        .bind(&expected)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    pub async fn request_cancel(
        &self,
        workspace_id: WorkspaceId,
        run_id: RunId,
        reason: &str,
        actor: &str,
    ) -> Result<RunSummary> {
        let run = self.get_run(workspace_id, run_id).await?;
        if run.state.is_terminal() {
            return Err(DbError::Conflict("run already finished".into()));
        }
        self.set_run_state(
            run_id,
            &[
                RunState::Queued,
                RunState::Running,
                RunState::WaitingOperator,
                RunState::Interrupted,
            ],
            RunState::Cancelling,
            Some(reason),
        )
        .await?;
        sqlx::query(
            "UPDATE run_steps SET state='skipped', summary='cancelled before dispatch', finished_at=now()
             WHERE run_id=$1 AND state='pending'",
        )
        .bind(run_id.as_uuid())
        .execute(&self.pool)
        .await?;
        let running: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM run_steps WHERE run_id=$1 AND state='running'",
        )
        .bind(run_id.as_uuid())
        .fetch_one(&self.pool)
        .await?;
        if running == 0 {
            self.set_run_state(
                run_id,
                &[RunState::Cancelling],
                RunState::Cancelled,
                Some(reason),
            )
            .await?;
        }
        self.add_event(EventEnvelope::new(
            workspace_id,
            run.assessment_id,
            Some(run_id),
            None,
            "run.cancelling",
            run_id.to_string(),
            serde_json::json!({ "reason": reason, "actor": actor, "running_steps": running }),
        ))
        .await?;
        self.run_summary(workspace_id, run_id).await
    }

    pub async fn resume_run(
        &self,
        workspace_id: WorkspaceId,
        run_id: RunId,
        reconcile_unknown: bool,
        note: &str,
    ) -> Result<RunSummary> {
        let run = self.get_run(workspace_id, run_id).await?;
        if run.state != RunState::Interrupted && run.state != RunState::WaitingOperator {
            return Err(DbError::Conflict(format!(
                "run cannot resume from {}",
                run.state
            )));
        }
        let unknown: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM run_steps WHERE run_id=$1 AND effect_status='outcome_unknown'",
        )
        .bind(run_id.as_uuid())
        .fetch_one(&self.pool)
        .await?;
        if unknown > 0 && !reconcile_unknown {
            return Err(DbError::Conflict(
                "outcome_unknown steps require explicit reconciliation".into(),
            ));
        }
        let mut tx = self.pool.begin().await?;
        if reconcile_unknown {
            sqlx::query(
                "UPDATE run_steps SET state='skipped', effect_status='outcome_unknown',
                    summary='reconciled by operator: not repeated', finished_at=now()
                 WHERE run_id=$1 AND effect_status='outcome_unknown'",
            )
            .bind(run_id.as_uuid())
            .execute(&mut *tx)
            .await?;
            sqlx::query("UPDATE runs SET effect_unknown=false WHERE id=$1")
                .bind(run_id.as_uuid())
                .execute(&mut *tx)
                .await?;
        }
        sqlx::query("UPDATE runs SET state='queued', reason=NULL, updated_at=now() WHERE id=$1")
            .bind(run_id.as_uuid())
            .execute(&mut *tx)
            .await?;
        insert_event_tx(
            &mut tx,
            &EventEnvelope::new(
                workspace_id,
                run.assessment_id,
                Some(run_id),
                None,
                "run.resumed",
                run_id.to_string(),
                serde_json::json!({ "note": note, "reconciled": reconcile_unknown }),
            ),
        )
        .await?;
        tx.commit().await?;
        let pending: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM run_steps WHERE run_id=$1 AND state IN ('pending','running')",
        )
        .bind(run_id.as_uuid())
        .fetch_one(&self.pool)
        .await?;
        if pending == 0 {
            self.set_run_state(
                run_id,
                &[RunState::Queued],
                RunState::Failed,
                Some("outcome_unknown reconciled without retry; no eligible steps remain"),
            )
            .await?;
        }
        self.run_summary(workspace_id, run_id).await
    }

    /// Recovers steps whose worker vanished. A dispatched effect whose result
    /// was never recorded becomes `outcome_unknown`; the run waits for
    /// reconciliation instead of repeating it.
    pub async fn recover_expired_leases(&self) -> Result<u64> {
        let rows = sqlx::query(
            "SELECT id, run_id, workspace_id, assessment_id, effect_status FROM run_steps
             WHERE state='running' AND lease_expires_at < now()",
        )
        .fetch_all(&self.pool)
        .await?;
        let mut recovered = 0u64;
        for row in rows {
            let step_id: Uuid = row.try_get("id")?;
            let run_id: Uuid = row.try_get("run_id")?;
            let workspace_id: Uuid = row.try_get("workspace_id")?;
            let assessment_id: Uuid = row.try_get("assessment_id")?;
            let effect: String = row.try_get("effect_status")?;
            let mut tx = self.pool.begin().await?;
            if effect == EffectStatus::Dispatched.as_str() {
                sqlx::query(
                    "UPDATE run_steps SET state='failed', effect_status='outcome_unknown',
                        summary='worker lost after dispatch; result unknown', finished_at=now(),
                        lease_owner=NULL, lease_expires_at=NULL WHERE id=$1 AND state='running'",
                )
                .bind(step_id)
                .execute(&mut *tx)
                .await?;
                sqlx::query(
                    "UPDATE runs SET state='waiting_operator', effect_unknown=true,
                        reason='external_result_unknown', updated_at=now() WHERE id=$1",
                )
                .bind(run_id)
                .execute(&mut *tx)
                .await?;
                insert_event_tx(
                    &mut tx,
                    &EventEnvelope::new(
                        workspace_id.into(),
                        assessment_id.into(),
                        Some(run_id.into()),
                        Some(step_id.into()),
                        "step.outcome_unknown",
                        step_id.to_string(),
                        serde_json::json!({ "reason": "external_result_unknown" }),
                    ),
                )
                .await?;
            } else {
                sqlx::query(
                    "UPDATE run_steps SET state='pending', lease_owner=NULL, lease_expires_at=NULL,
                        summary='lease expired before dispatch; eligible for retry' WHERE id=$1 AND state='running'",
                )
                .bind(step_id)
                .execute(&mut *tx)
                .await?;
            }
            tx.commit().await?;
            recovered += 1;
        }
        Ok(recovered)
    }

    pub async fn add_event(&self, event: EventEnvelope) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        insert_event_tx(&mut tx, &event).await?;
        tx.commit().await?;
        Ok(())
    }
}

pub async fn finish_run_if_done(db: &Db, run_id: RunId) -> Result<Option<RunState>> {
    let rows = sqlx::query("SELECT state FROM run_steps WHERE run_id=$1")
        .bind(run_id.as_uuid())
        .fetch_all(db.pool())
        .await?;
    let mut pending = false;
    let mut failed = false;
    for row in rows {
        let state: String = row.try_get("state")?;
        if matches!(state.as_str(), "pending" | "running") {
            pending = true;
        }
        if state == "failed" {
            failed = true;
        }
        if state == "blocked" {
            failed = true;
        }
    }
    let unknown: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM run_steps WHERE run_id=$1 AND effect_status='outcome_unknown'",
    )
    .bind(run_id.as_uuid())
    .fetch_one(db.pool())
    .await?;
    let effect_unknown = unknown > 0;
    if pending || effect_unknown {
        return Ok(None);
    }
    let next = if failed {
        RunState::Failed
    } else {
        RunState::Succeeded
    };
    let changed = db
        .set_run_state(run_id, &[RunState::Running], next, None)
        .await?;
    Ok(changed.then_some(next))
}

impl Db {
    pub async fn force_expire_lease(&self, step_id: StepId) -> Result<()> {
        sqlx::query(
            "UPDATE run_steps SET lease_expires_at = now() - interval '1 minute' WHERE id=$1",
        )
        .bind(step_id.as_uuid())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn set_run_budget_for_test(&self, run_id: RunId, budget: &Budget) -> Result<()> {
        sqlx::query("UPDATE runs SET budget=$2 WHERE id=$1")
            .bind(run_id.as_uuid())
            .bind(serde_json::to_value(budget)?)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}
