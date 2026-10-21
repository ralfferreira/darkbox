use darkbox_contracts::*;
use sqlx::{Postgres, Transaction};

use crate::{Db, Result, map_json};

pub(crate) async fn insert_event_tx(
    tx: &mut Transaction<'_, Postgres>,
    event: &EventEnvelope,
) -> Result<()> {
    sqlx::query(
        "INSERT INTO events (event_id, schema_version, workspace_id, assessment_id, run_id, step_id,
            event_type, occurred_at, recorded_at, correlation_id, payload)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    )
    .bind(event.event_id.as_uuid())
    .bind(event.schema_version as i32)
    .bind(event.workspace_id.as_uuid())
    .bind(event.assessment_id.as_uuid())
    .bind(event.run_id.map(|r| r.as_uuid()))
    .bind(event.step_id.map(|s| s.as_uuid()))
    .bind(&event.event_type)
    .bind(event.occurred_at)
    .bind(event.recorded_at)
    .bind(&event.correlation_id)
    .bind(&event.payload)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

impl Db {
    pub async fn events_since(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        cursor: i64,
        limit: i64,
    ) -> Result<Vec<EventDto>> {
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'event_id', e.event_id, 'schema_version', e.schema_version,
                'workspace_id', e.workspace_id, 'assessment_id', e.assessment_id,
                'run_id', e.run_id, 'step_id', e.step_id, 'event_type', e.event_type,
                'occurred_at', e.occurred_at, 'recorded_at', e.recorded_at,
                'correlation_id', e.correlation_id, 'seq', e.seq, 'payload', e.payload
             ) AS row FROM events e
             WHERE e.workspace_id=$1 AND e.assessment_id=$2 AND e.seq > $3
             ORDER BY e.seq LIMIT $4",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(cursor)
        .bind(limit.clamp(1, 500))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(map_json).collect()
    }

    pub async fn events_for_run(&self, run_id: RunId, limit: i64) -> Result<Vec<EventDto>> {
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'event_id', e.event_id, 'schema_version', e.schema_version,
                'workspace_id', e.workspace_id, 'assessment_id', e.assessment_id,
                'run_id', e.run_id, 'step_id', e.step_id, 'event_type', e.event_type,
                'occurred_at', e.occurred_at, 'recorded_at', e.recorded_at,
                'correlation_id', e.correlation_id, 'seq', e.seq, 'payload', e.payload
             ) AS row FROM events e
             WHERE e.run_id=$1 ORDER BY e.seq LIMIT $2",
        )
        .bind(run_id.as_uuid())
        .bind(limit.clamp(1, 500))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(map_json).collect()
    }

    pub async fn latest_event_seq(&self, assessment_id: AssessmentId) -> Result<i64> {
        let seq: Option<i64> =
            sqlx::query_scalar("SELECT max(seq) FROM events WHERE assessment_id=$1")
                .bind(assessment_id.as_uuid())
                .fetch_one(&self.pool)
                .await?;
        Ok(seq.unwrap_or(0))
    }

    pub async fn recent_events_for_workspace(
        &self,
        workspace_id: WorkspaceId,
        limit: i64,
    ) -> Result<Vec<EventDto>> {
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'event_id', e.event_id, 'schema_version', e.schema_version,
                'workspace_id', e.workspace_id, 'assessment_id', e.assessment_id,
                'run_id', e.run_id, 'step_id', e.step_id, 'event_type', e.event_type,
                'occurred_at', e.occurred_at, 'recorded_at', e.recorded_at,
                'correlation_id', e.correlation_id, 'seq', e.seq, 'payload', e.payload
             ) AS row FROM events e
             WHERE e.workspace_id=$1 ORDER BY e.seq DESC LIMIT $2",
        )
        .bind(workspace_id.as_uuid())
        .bind(limit.clamp(1, 200))
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(map_json).collect()
    }
}
