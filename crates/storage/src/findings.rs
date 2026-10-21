use chrono::Utc;
use darkbox_contracts::*;
use darkbox_domain::findings::review_transition;
use sqlx::Row;

use crate::{Db, DbError, Result, map_json};

impl Db {
    pub async fn create_finding(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        req: &CreateFindingRequest,
        actor: &str,
    ) -> Result<FindingId> {
        if req.claim.trim().is_empty() {
            return Err(DbError::Invalid("a finding needs a claim".into()));
        }
        let state = if req.from_hypothesis {
            FindingState::Hypothesis
        } else {
            FindingState::NeedsReview
        };
        let id = FindingId::new();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO findings (id, workspace_id, assessment_id, title, state, severity, claim, impact,
                endpoint_id, evidence_ids, test_result_ids)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
        )
        .bind(id.as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(&req.title)
        .bind(state.as_str())
        .bind(&req.severity)
        .bind(&req.claim)
        .bind(&req.impact)
        .bind(req.endpoint_id.map(|e| e.as_uuid()))
        .bind(req.evidence_ids.iter().map(|e| e.as_uuid()).collect::<Vec<_>>())
        .bind(req.test_result_ids.iter().map(|e| e.as_uuid()).collect::<Vec<_>>())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO finding_revisions (finding_id, revision, state, note, actor)
             VALUES ($1,1,$2,$3,$4)",
        )
        .bind(id.as_uuid())
        .bind(state.as_str())
        .bind(if req.from_hypothesis {
            "recorded as hypothesis"
        } else {
            "recorded for review"
        })
        .bind(actor)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(id)
    }

    pub async fn list_findings(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        state: Option<&str>,
    ) -> Result<Vec<FindingDto>> {
        let rows = sqlx::query(
            "SELECT jsonb_build_object(
                'id', f.id, 'title', f.title, 'state', f.state, 'severity', f.severity,
                'claim', f.claim, 'impact', f.impact, 'endpoint_id', f.endpoint_id,
                'evidence_ids', f.evidence_ids, 'test_result_ids', f.test_result_ids,
                'revisions', COALESCE((
                    SELECT jsonb_agg(jsonb_build_object(
                        'revision', r.revision, 'state', r.state, 'note', r.note,
                        'actor', r.actor, 'at', r.at) ORDER BY r.revision)
                    FROM finding_revisions r WHERE r.finding_id=f.id), '[]'),
                'created_at', f.created_at, 'updated_at', f.updated_at
             ) AS row FROM findings f
             WHERE f.workspace_id=$1 AND f.assessment_id=$2
               AND ($3::text IS NULL OR f.state=$3)
             ORDER BY f.updated_at DESC",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(state)
        .fetch_all(&self.pool)
        .await?;
        rows.iter().map(map_json).collect()
    }

    pub async fn get_finding(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        id: FindingId,
    ) -> Result<FindingDto> {
        let row = sqlx::query(
            "SELECT jsonb_build_object(
                'id', f.id, 'title', f.title, 'state', f.state, 'severity', f.severity,
                'claim', f.claim, 'impact', f.impact, 'endpoint_id', f.endpoint_id,
                'evidence_ids', f.evidence_ids, 'test_result_ids', f.test_result_ids,
                'revisions', COALESCE((
                    SELECT jsonb_agg(jsonb_build_object(
                        'revision', r.revision, 'state', r.state, 'note', r.note,
                        'actor', r.actor, 'at', r.at) ORDER BY r.revision)
                    FROM finding_revisions r WHERE r.finding_id=f.id), '[]'),
                'created_at', f.created_at, 'updated_at', f.updated_at
             ) AS row FROM findings f
             WHERE f.workspace_id=$1 AND f.assessment_id=$2 AND f.id=$3",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&self.pool)
        .await?
        .ok_or_else(|| DbError::NotFound("finding".into()))?;
        map_json(&row)
    }

    pub async fn review_finding(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        id: FindingId,
        next: FindingState,
        note: &str,
        actor: &str,
    ) -> Result<FindingDto> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT state, cardinality(evidence_ids) AS evidence_count, test_result_ids
             FROM findings WHERE workspace_id=$1 AND assessment_id=$2 AND id=$3 FOR UPDATE",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(id.as_uuid())
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| DbError::NotFound("finding".into()))?;
        let current: FindingState = row
            .try_get::<String, _>("state")?
            .parse()
            .map_err(DbError::Invalid)?;
        review_transition(current, next).map_err(|e| DbError::Conflict(e.to_string()))?;
        let evidence_count: i32 = row.try_get("evidence_count")?;
        if next == FindingState::Confirmed {
            if evidence_count == 0 {
                return Err(DbError::Conflict(
                    "a confirmed finding needs at least one evidence item".into(),
                ));
            }
            let result_ids: Vec<uuid::Uuid> = row.try_get("test_result_ids")?;
            if !result_ids.is_empty() {
                let rows = sqlx::query("SELECT state FROM test_results WHERE id = ANY($1)")
                    .bind(&result_ids)
                    .fetch_all(&mut *tx)
                    .await?;
                let states: Vec<TestResultState> = rows
                    .iter()
                    .map(|r| {
                        r.try_get::<String, _>("state")
                            .map_err(DbError::from)
                            .and_then(|s| s.parse().map_err(DbError::Invalid))
                    })
                    .collect::<Result<_>>()?;
                if states
                    .iter()
                    .all(|s| matches!(s, TestResultState::Inconclusive | TestResultState::Blocked))
                {
                    return Err(DbError::Conflict(
                        "inconclusive or blocked results cannot sustain a confirmation".into(),
                    ));
                }
            }
        }
        let revision: i32 = sqlx::query_scalar(
            "SELECT COALESCE(max(revision),0)+1 FROM finding_revisions WHERE finding_id=$1",
        )
        .bind(id.as_uuid())
        .fetch_one(&mut *tx)
        .await?;
        sqlx::query("UPDATE findings SET state=$2, updated_at=now() WHERE id=$1")
            .bind(id.as_uuid())
            .bind(next.as_str())
            .execute(&mut *tx)
            .await?;
        sqlx::query(
            "INSERT INTO finding_revisions (finding_id, revision, state, note, actor)
             VALUES ($1,$2,$3,$4,$5)",
        )
        .bind(id.as_uuid())
        .bind(revision)
        .bind(next.as_str())
        .bind(note)
        .bind(actor)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        self.get_finding(workspace_id, assessment_id, id).await
    }

    pub async fn finding_counts(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
    ) -> Result<Vec<(String, i64)>> {
        let rows = sqlx::query(
            "SELECT state, count(*) AS n FROM findings
             WHERE workspace_id=$1 AND assessment_id=$2 GROUP BY state",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| Ok((row.try_get("state")?, row.try_get("n")?)))
            .collect()
    }

    pub async fn finding_revision_count(&self, id: FindingId) -> Result<i64> {
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM finding_revisions WHERE finding_id=$1")
                .bind(id.as_uuid())
                .fetch_one(&self.pool)
                .await?;
        Ok(count)
    }

    pub async fn now(&self) -> chrono::DateTime<Utc> {
        Utc::now()
    }
}

impl Db {
    pub async fn finding_scope(&self, id: FindingId) -> Result<(WorkspaceId, AssessmentId)> {
        let row = sqlx::query("SELECT workspace_id, assessment_id FROM findings WHERE id=$1")
            .bind(id.as_uuid())
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| DbError::NotFound("finding".into()))?;
        Ok((
            row.try_get::<uuid::Uuid, _>("workspace_id")?.into(),
            row.try_get::<uuid::Uuid, _>("assessment_id")?.into(),
        ))
    }
}
