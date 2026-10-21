use darkbox_contracts::*;

use crate::{Db, DbError, Result, map_json};

const PAGE_JSON: &str = "jsonb_build_object(
    'id', m.id, 'workspace_id', m.workspace_id, 'assessment_id', m.assessment_id,
    'slug', m.slug, 'title', m.title, 'version', m.version, 'body_markdown', m.body_markdown,
    'state', m.state, 'references', m.references_json, 'validity', m.validity,
    'created_by', m.created_by, 'created_at', m.created_at
)";

impl Db {
    #[allow(clippy::too_many_arguments)]
    pub async fn save_memory_page(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        slug: &str,
        title: &str,
        body: &str,
        references: &[MemoryReferenceDto],
        validity: &str,
        actor: &str,
    ) -> Result<MemoryPageDto> {
        let version: i32 = sqlx::query_scalar(
            "SELECT COALESCE(max(version),0)+1 FROM memory_pages WHERE assessment_id=$1 AND slug=$2",
        )
        .bind(assessment_id.as_uuid())
        .bind(slug)
        .fetch_one(&self.pool)
        .await?;
        sqlx::query(
            "INSERT INTO memory_pages (id, workspace_id, assessment_id, slug, title, version,
                body_markdown, references_json, validity, created_by)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(MemoryPageId::new().as_uuid())
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(slug)
        .bind(title)
        .bind(version)
        .bind(body)
        .bind(serde_json::to_value(references)?)
        .bind(validity)
        .bind(actor)
        .execute(&self.pool)
        .await?;
        self.get_memory_page(workspace_id, assessment_id, slug)
            .await
    }

    pub async fn get_memory_page(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        slug: &str,
    ) -> Result<MemoryPageDto> {
        let sql = format!(
            "SELECT {PAGE_JSON} AS row FROM memory_pages m
             WHERE m.workspace_id=$1 AND m.assessment_id=$2 AND m.slug=$3
             ORDER BY m.version DESC LIMIT 1"
        );
        let row = sqlx::query(&sql)
            .bind(workspace_id.as_uuid())
            .bind(assessment_id.as_uuid())
            .bind(slug)
            .fetch_optional(&self.pool)
            .await?
            .ok_or_else(|| DbError::NotFound("memory page".into()))?;
        map_json(&row)
    }

    pub async fn list_memory_pages(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
    ) -> Result<Vec<MemoryPageDto>> {
        let sql = format!(
            "SELECT {PAGE_JSON} AS row FROM (
                SELECT DISTINCT ON (slug) * FROM memory_pages
                WHERE workspace_id=$1 AND assessment_id=$2
                ORDER BY slug, version DESC
             ) m ORDER BY m.created_at DESC"
        );
        let rows = sqlx::query(&sql)
            .bind(workspace_id.as_uuid())
            .bind(assessment_id.as_uuid())
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(map_json).collect()
    }

    pub async fn search_memory(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        query: &str,
        limit: i64,
    ) -> Result<Vec<MemoryPageDto>> {
        let sql = format!(
            "SELECT {PAGE_JSON} AS row FROM (
                SELECT DISTINCT ON (m.slug) m.*
                FROM memory_pages m
                WHERE m.workspace_id=$1 AND m.assessment_id=$2
                ORDER BY m.slug, m.version DESC
             ) m
             WHERE ($3 = '' OR to_tsvector('simple', m.title || ' ' || m.body_markdown) @@ plainto_tsquery('simple', $3)
                    OR m.body_markdown ILIKE '%' || $3 || '%' OR m.title ILIKE '%' || $3 || '%')
             ORDER BY ts_rank(to_tsvector('simple', m.title || ' ' || m.body_markdown), plainto_tsquery('simple', $3)) DESC,
                      m.created_at DESC
             LIMIT $4"
        );
        let rows = sqlx::query(&sql)
            .bind(workspace_id.as_uuid())
            .bind(assessment_id.as_uuid())
            .bind(query)
            .bind(limit.clamp(1, 50))
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(map_json).collect()
    }

    pub async fn memory_page_versions(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        slug: &str,
    ) -> Result<Vec<MemoryPageDto>> {
        let sql = format!(
            "SELECT {PAGE_JSON} AS row FROM memory_pages m
             WHERE m.workspace_id=$1 AND m.assessment_id=$2 AND m.slug=$3
             ORDER BY m.version DESC"
        );
        let rows = sqlx::query(&sql)
            .bind(workspace_id.as_uuid())
            .bind(assessment_id.as_uuid())
            .bind(slug)
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(map_json).collect()
    }

    pub async fn purge_memory_page_versions(
        &self,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        slug: &str,
        keep: i32,
    ) -> Result<u64> {
        let result = sqlx::query(
            "DELETE FROM memory_pages WHERE workspace_id=$1 AND assessment_id=$2 AND slug=$3
             AND version NOT IN (
                SELECT version FROM memory_pages WHERE assessment_id=$2 AND slug=$3
                ORDER BY version DESC LIMIT $4)",
        )
        .bind(workspace_id.as_uuid())
        .bind(assessment_id.as_uuid())
        .bind(slug)
        .bind(keep)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }

    pub async fn count_memory_versions(&self, assessment_id: AssessmentId) -> Result<i64> {
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM memory_pages WHERE assessment_id=$1")
                .bind(assessment_id.as_uuid())
                .fetch_one(&self.pool)
                .await?;
        Ok(count)
    }
}

impl Db {
    pub async fn purge_all_memory_versions(&self, keep: i32) -> Result<u64> {
        let result = sqlx::query(
            "DELETE FROM memory_pages m
             WHERE m.version < (
                SELECT max(version) FROM memory_pages x
                WHERE x.assessment_id=m.assessment_id AND x.slug=m.slug
             ) - $1 + 1",
        )
        .bind(keep)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected())
    }
}
