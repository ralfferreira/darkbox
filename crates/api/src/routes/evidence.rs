use axum::Json;
use axum::body::Body;
use axum::extract::{Path, Query, State};
use axum::http::{StatusCode, header};
use axum::response::Response;
use darkbox_contracts::*;
use darkbox_storage::NewEvidence;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::routes::core::parse_id;
use crate::{ApiError, AppState, Auth, PrincipalPermissions, no_store};

#[derive(Debug, Deserialize)]
pub struct EvidenceParams {
    pub kind: Option<String>,
    pub run_id: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn list_evidence(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Query(params): Query<EvidenceParams>,
) -> Result<Json<Vec<EvidenceDto>>, ApiError> {
    let id = parse_id::<AssessmentId>(&id)?;
    state.db.assessment_summary(auth.workspace_id, id).await?;
    let mut items = if let Some(run_id) = params.run_id.as_deref() {
        let run_id = parse_id::<RunId>(run_id)?;
        state
            .db
            .list_evidence_for_run(auth.workspace_id, id, run_id, params.limit.unwrap_or(300))
            .await?
    } else {
        state
            .db
            .list_evidence(
                auth.workspace_id,
                id,
                params.kind.as_deref(),
                params.limit.unwrap_or(100),
                params.offset.unwrap_or(0),
            )
            .await?
    };
    for item in &mut items {
        item.access_url = Some(format!("/v1/evidence/{}/content", item.id));
    }
    Ok(Json(items))
}

pub async fn get_evidence(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
) -> Result<Json<EvidenceDto>, ApiError> {
    let id = parse_id::<EvidenceId>(&id)?;
    let (workspace_id, assessment_id, _) = state.db.evidence_workspace_of(id).await?;
    if workspace_id != auth.workspace_id {
        return Err(ApiError::not_found("evidence"));
    }
    let record = state
        .db
        .get_evidence(auth.workspace_id, assessment_id, id)
        .await?;
    Ok(Json(EvidenceDto {
        id: record.id,
        assessment_id: record.assessment_id,
        kind: record.kind,
        classification: record.classification,
        sha256: record.sha256,
        size_bytes: record.size_bytes,
        truncated: record.truncated,
        sanitized: record.sanitized,
        content_type: record.content_type,
        run_id: record.run_id,
        step_id: record.step_id,
        identity_id: record.identity_id,
        summary: record.summary,
        created_at: record.created_at,
        retention_until: record.retention_until,
        access_url: Some(format!("/v1/evidence/{}/content", record.id)),
    }))
}

pub async fn evidence_content(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id = parse_id::<EvidenceId>(&id)?;
    let (workspace_id, assessment_id, object_key) = state.db.evidence_workspace_of(id).await?;
    if workspace_id != auth.workspace_id {
        return Err(ApiError::not_found("evidence"));
    }
    let record = state
        .db
        .get_evidence(auth.workspace_id, assessment_id, id)
        .await?;
    if record.classification == EvidenceClassification::RawRestricted
        && !state.config.raw_evidence_enabled
    {
        return Err(ApiError::forbidden(
            "raw restricted evidence is disabled by configuration",
        ));
    }
    let bytes = state
        .store()
        .get(&object_key)
        .await
        .map_err(|_| ApiError::not_found("evidence object"))?;
    state
        .db
        .audit(
            auth.workspace_id,
            Some(assessment_id),
            &auth.operator,
            "evidence.read",
            &id.to_string(),
            "",
            &id.to_string(),
        )
        .await?;
    let mut response = Response::new(Body::from(bytes));
    *response.status_mut() = StatusCode::OK;
    for (name, value) in no_store() {
        response.headers_mut().insert(name, value);
    }
    let content_type = record.content_type.clone();
    let extension = if content_type.starts_with("image/") {
        "jpg"
    } else {
        "txt"
    };
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_str(&content_type)
            .unwrap_or_else(|_| header::HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&format!("inline; filename=\"{id}.{extension}\""))
            .unwrap_or_else(|_| header::HeaderValue::from_static("inline")),
    );
    Ok(response)
}

#[derive(Debug, Deserialize, Default)]
pub struct ExportPayload {
    #[serde(default)]
    pub include_evidence: bool,
    #[serde(default)]
    pub note: String,
}

pub async fn create_export(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
    Json(req): Json<ExportPayload>,
) -> Result<Json<ExportDto>, ApiError> {
    auth.require_write()?;
    let id = parse_id::<AssessmentId>(&id)?;
    let assessment = state.db.assessment_summary(auth.workspace_id, id).await?;
    let scope = state.db.get_scope(auth.workspace_id, id, None).await?;
    let endpoints = state
        .db
        .inventory(
            auth.workspace_id,
            id,
            &darkbox_storage::InventoryQuery {
                limit: 5000,
                ..Default::default()
            },
        )
        .await?;
    let flows = state.db.list_flows(auth.workspace_id, id).await?;
    let findings = state.db.list_findings(auth.workspace_id, id, None).await?;
    let pages = state.db.list_memory_pages(auth.workspace_id, id).await?;
    let evidence = state
        .db
        .list_evidence(auth.workspace_id, id, None, 5000, 0)
        .await?;
    let results = state
        .db
        .list_test_results(auth.workspace_id, id, 1000)
        .await?;

    let mut evidence_items = Vec::new();
    for item in &evidence {
        let mut entry = json!({
            "id": item.id,
            "kind": item.kind,
            "classification": item.classification,
            "sha256": item.sha256,
            "size_bytes": item.size_bytes,
            "sanitized": item.sanitized,
            "summary": item.summary,
            "run_id": item.run_id,
            "created_at": item.created_at,
        });
        if req.include_evidence {
            let record = state
                .db
                .get_evidence(auth.workspace_id, id, item.id)
                .await?;
            if let Ok(bytes) = state.store().get(&record.object_key).await {
                entry["content"] = json!(String::from_utf8_lossy(&bytes));
            } else {
                entry["content"] = json!(null);
                entry["missing_object"] = json!(true);
            }
        }
        evidence_items.push(entry);
    }

    let document = json!({
        "schema_version": 1,
        "generated_at": chrono::Utc::now(),
        "workspace_id": auth.workspace_id,
        "assessment": assessment,
        "scope": {
            "version": scope.version,
            "policy": scope.policy,
        },
        "inventory": endpoints.endpoints,
        "inventory_total": endpoints.total,
        "flows": flows,
        "findings": findings,
        "test_results": results,
        "memory": pages,
        "evidence": evidence_items,
        "note": req.note,
        "limitations": [
            "Sanitized export: request and response secrets were removed at ingestion.",
            "Absence of a result is not evidence of absence of a vulnerability.",
            "Observations remain the canonical source; narrative memory is derived.",
        ],
    });
    let bytes =
        serde_json::to_vec_pretty(&document).map_err(|e| ApiError::invalid(e.to_string()))?;
    let hash = hex::encode(Sha256::digest(&bytes));
    let key = format!("export_{}.json", uuid::Uuid::now_v7().simple());
    state
        .store()
        .put(&key, &bytes)
        .await
        .map_err(|e| ApiError::invalid(e.to_string()))?;
    let export = state
        .db
        .create_export(
            auth.workspace_id,
            id,
            &key,
            &hash,
            bytes.len() as i64,
            (endpoints.total + flows.len() as i64 + findings.len() as i64 + evidence.len() as i64)
                as i64,
            req.include_evidence,
            &auth.operator,
        )
        .await?;
    state
        .db
        .audit(
            auth.workspace_id,
            Some(id),
            &auth.operator,
            "export.create",
            &export.id.to_string(),
            &req.note,
            &export.id.to_string(),
        )
        .await?;
    Ok(Json(ExportDto {
        id: export.id,
        assessment_id: id,
        item_count: export.item_count,
        manifest_sha256: export.manifest_sha256,
        object_key: export.object_key,
        size_bytes: export.size_bytes,
        include_evidence: export.include_evidence,
        created_at: export.created_at,
        download_url: format!("/v1/exports/{}/download", export.id),
    }))
}

pub async fn download_export(
    State(state): State<AppState>,
    Auth(auth): Auth,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let id: uuid::Uuid = id
        .parse()
        .map_err(|_| ApiError::invalid("invalid export id"))?;
    let export = state
        .db
        .get_export(auth.workspace_id, id)
        .await?
        .ok_or_else(|| ApiError::not_found("export"))?;
    let bytes = state
        .store()
        .get(&export.object_key)
        .await
        .map_err(|_| ApiError::not_found("export object"))?;
    state
        .db
        .audit(
            auth.workspace_id,
            Some(export.assessment_id),
            &auth.operator,
            "export.download",
            &id.to_string(),
            "",
            &id.to_string(),
        )
        .await?;
    let mut response = Response::new(Body::from(bytes));
    for (name, value) in no_store() {
        response.headers_mut().insert(name, value);
    }
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        header::HeaderValue::from_str(&format!(
            "attachment; filename=\"darkbox-export-{id}.json\""
        ))
        .unwrap_or_else(|_| header::HeaderValue::from_static("attachment")),
    );
    Ok(response)
}

#[allow(dead_code)]
pub(crate) fn empty_evidence() -> NewEvidence {
    NewEvidence {
        kind: EvidenceKind::ToolOutput,
        classification: EvidenceClassification::MetadataOnly,
        object_key: String::new(),
        sha256: String::new(),
        size_bytes: 0,
        truncated: false,
        sanitized: true,
        content_type: "text/plain".into(),
        run_id: None,
        step_id: None,
        identity_id: None,
        summary: String::new(),
        source: String::new(),
    }
}
