use chrono::Utc;
use darkbox_contracts::*;
use darkbox_domain::Run;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::events::insert_event_tx;
use crate::{Db, DbError, Result};

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
            "UPDATE runs SET state=$2, reason=$3, finished_at=now(), updated_at=now(),
                effect_unknown=false WHERE id=$1",
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
}
