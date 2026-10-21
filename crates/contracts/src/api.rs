use serde::{Deserialize, Serialize};

use crate::policy::ScopePolicy;
use crate::states::{ActionClass, Budget, FindingState, TestResultState};
use crate::{
    AssessmentId, EvidenceId, FindingId, IdentityId, MemoryPageId, RunId, ScopeVersionId, StepId,
    WorkspaceId,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventDto {
    pub event_id: crate::EventId,
    pub schema_version: u32,
    pub workspace_id: WorkspaceId,
    pub assessment_id: AssessmentId,
    pub run_id: Option<RunId>,
    pub step_id: Option<StepId>,
    pub event_type: String,
    pub occurred_at: chrono::DateTime<chrono::Utc>,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
    pub correlation_id: String,
    pub seq: i64,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionDto {
    pub workspace_id: WorkspaceId,
    pub workspace_name: String,
    pub role: crate::Role,
    pub operator: String,
    pub can_write: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompanyDto {
    pub id: crate::CompanyId,
    pub name: String,
    pub description: String,
    pub assessment_count: i64,
    pub last_activity_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateCompanyRequest {
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateAssessmentRequest {
    pub company_id: crate::CompanyId,
    pub name: String,
    pub environment: String,
    pub objective: String,
    #[serde(default)]
    pub initial_targets: Vec<String>,
    pub policy: ScopePolicy,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssessmentSummary {
    pub id: AssessmentId,
    pub company_id: crate::CompanyId,
    pub company_name: String,
    pub name: String,
    pub environment: String,
    pub objective: String,
    pub state: String,
    pub scope_version: i64,
    pub endpoint_count: i64,
    pub observed_count: i64,
    pub run_count: i64,
    pub open_findings: i64,
    pub last_activity_at: chrono::DateTime<chrono::Utc>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScopeVersionDto {
    pub id: ScopeVersionId,
    pub version: i64,
    pub policy: ScopePolicy,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub created_by: String,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AssessmentDetail {
    pub assessment: AssessmentSummary,
    pub scope: ScopeVersionDto,
    pub identities: Vec<IdentityDto>,
    pub recent_runs: Vec<RunSummary>,
    pub coverage: CoverageDto,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoverageDto {
    pub known_candidates: i64,
    pub observed_endpoints: i64,
    pub excluded_endpoints: i64,
    pub unreachable_endpoints: i64,
    pub flows_reproduced: i64,
    pub flows_known: i64,
    pub findings_confirmed: i64,
    pub findings_needs_review: i64,
    pub evidence_count: i64,
    pub evidence_bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IdentityDto {
    pub id: crate::IdentityId,
    pub label: String,
    pub role_label: String,
    pub tenant_keys: Vec<String>,
    pub capabilities: Vec<String>,
    pub has_secret_ref: bool,
    pub session_state: String,
    pub session_expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateIdentityRequest {
    pub label: String,
    pub role_label: String,
    #[serde(default)]
    pub tenant_keys: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub secret_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TargetTenantDto {
    pub id: crate::TenantId,
    pub key: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EndpointDto {
    pub id: crate::EndpointId,
    pub asset_id: crate::AssetId,
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub method: String,
    pub path: String,
    pub query_names: Vec<String>,
    pub state: crate::EndpointState,
    pub source: String,
    pub run_id: Option<RunId>,
    pub group_pattern: Option<String>,
    pub identity_id: Option<crate::IdentityId>,
    pub observed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InventoryResponse {
    pub endpoints: Vec<EndpointDto>,
    pub total: i64,
    pub counts: std::collections::BTreeMap<String, i64>,
    pub gaps: Vec<GapDto>,
    pub policy_version: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GapDto {
    pub kind: String,
    pub detail: String,
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ImportUrlsRequest {
    pub urls: Vec<String>,
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservationDto {
    pub id: crate::ObservationId,
    pub subject_kind: String,
    pub subject_id: String,
    pub property: String,
    pub value: serde_json::Value,
    pub source: String,
    pub confidence: f64,
    pub evidence_id: Option<EvidenceId>,
    pub observed_at: chrono::DateTime<chrono::Utc>,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowStepDto {
    pub ordinal: i64,
    pub action_id: Option<crate::ActionId>,
    pub intent: String,
    pub expected_result: String,
    pub endpoint_id: Option<crate::EndpointId>,
    pub confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FlowDto {
    pub id: crate::FlowId,
    pub name: String,
    pub state: crate::FlowState,
    pub version: i64,
    pub identity_id: Option<crate::IdentityId>,
    pub tenant_key: Option<String>,
    pub steps: Vec<FlowStepDto>,
    pub evidence_ids: Vec<EvidenceId>,
    pub preconditions: Vec<String>,
    pub observed_at: Option<chrono::DateTime<chrono::Utc>>,
    pub recorded_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateFlowRequest {
    pub name: String,
    pub identity_id: Option<crate::IdentityId>,
    pub tenant_key: Option<String>,
    #[serde(default)]
    pub preconditions: Vec<String>,
    #[serde(default)]
    pub steps: Vec<CreateFlowStep>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateFlowStep {
    pub intent: String,
    pub expected_result: String,
    pub endpoint_id: Option<crate::EndpointId>,
    #[serde(default)]
    pub action_class: Option<ActionClass>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateActionRequest {
    pub intent: String,
    pub endpoint_id: Option<crate::EndpointId>,
    pub action_class: ActionClass,
    #[serde(default)]
    pub preconditions: Vec<String>,
    #[serde(default)]
    pub expected_effect: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionDto {
    pub id: crate::ActionId,
    pub intent: String,
    pub endpoint_id: Option<crate::EndpointId>,
    pub effect_class: ActionClass,
    pub state: String,
    pub preconditions: Vec<String>,
    pub expected_effect: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateRunRequest {
    pub assessment_id: AssessmentId,
    pub kind: crate::RunKind,
    pub idempotency_key: String,
    #[serde(default)]
    pub flow_id: Option<crate::FlowId>,
    #[serde(default)]
    pub procedure_key: Option<crate::ProcedureKey>,
    #[serde(default)]
    pub endpoint_ids: Vec<crate::EndpointId>,
    #[serde(default)]
    pub identity_id: Option<crate::IdentityId>,
    #[serde(default)]
    pub budget_override: Option<Budget>,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub tenant: Option<String>,
    #[serde(default)]
    pub identity_ids: Vec<IdentityId>,
    #[serde(default)]
    pub checks: Vec<String>,
    #[serde(default)]
    pub login_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunSummary {
    pub id: RunId,
    pub assessment_id: AssessmentId,
    pub kind: crate::RunKind,
    pub state: crate::RunState,
    pub procedure_key: Option<crate::ProcedureKey>,
    pub policy_version: i64,
    pub budget: Budget,
    pub spent: crate::BudgetSpent,
    pub effect_unknown: bool,
    pub reason: Option<String>,
    pub idempotency_key: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
    pub step_count: i64,
    pub steps_done: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunStepDto {
    pub id: StepId,
    pub ordinal: i64,
    pub kind: crate::StepKind,
    pub state: String,
    pub attempt: i64,
    pub effect_status: crate::EffectStatus,
    pub summary: String,
    pub started_at: Option<chrono::DateTime<chrono::Utc>>,
    pub finished_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunDetail {
    pub run: RunSummary,
    pub steps: Vec<RunStepDto>,
    pub events: Vec<EventDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CancelRunRequest {
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResumeRunRequest {
    #[serde(default)]
    pub reconcile_unknown: bool,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvidenceDto {
    pub id: EvidenceId,
    pub assessment_id: AssessmentId,
    pub kind: crate::EvidenceKind,
    pub classification: crate::EvidenceClassification,
    pub sha256: String,
    pub size_bytes: i64,
    pub truncated: bool,
    pub sanitized: bool,
    pub content_type: String,
    pub run_id: Option<RunId>,
    pub step_id: Option<StepId>,
    pub identity_id: Option<crate::IdentityId>,
    pub summary: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub retention_until: Option<chrono::DateTime<chrono::Utc>>,
    pub access_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryPageDto {
    pub id: MemoryPageId,
    pub slug: String,
    pub title: String,
    pub version: i64,
    pub body_markdown: String,
    pub state: String,
    pub references: Vec<MemoryReferenceDto>,
    pub validity: String,
    pub created_by: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryReferenceDto {
    pub kind: String,
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemorySearchRequest {
    pub query: String,
    #[serde(default)]
    pub token_budget: Option<i64>,
    #[serde(default)]
    pub identity_id: Option<crate::IdentityId>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextPackageDto {
    pub assessment_id: AssessmentId,
    pub objective: String,
    pub policy_version: i64,
    pub identity_id: Option<crate::IdentityId>,
    pub generated_at: chrono::DateTime<chrono::Utc>,
    pub token_budget: i64,
    pub estimated_tokens: i64,
    pub sections: Vec<ContextSectionDto>,
    pub omitted: Vec<String>,
    pub references: Vec<MemoryReferenceDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextSectionDto {
    pub title: String,
    pub body: String,
    pub reference_kind: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FindingDto {
    pub id: FindingId,
    pub title: String,
    pub state: FindingState,
    pub severity: String,
    pub claim: String,
    pub impact: String,
    pub endpoint_id: Option<crate::EndpointId>,
    pub evidence_ids: Vec<EvidenceId>,
    pub test_result_ids: Vec<crate::ResultId>,
    pub revisions: Vec<FindingRevisionDto>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FindingRevisionDto {
    pub revision: i64,
    pub state: FindingState,
    pub note: String,
    pub actor: String,
    pub at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CreateFindingRequest {
    pub title: String,
    pub severity: String,
    pub claim: String,
    pub impact: String,
    #[serde(default)]
    pub endpoint_id: Option<crate::EndpointId>,
    #[serde(default)]
    pub evidence_ids: Vec<EvidenceId>,
    #[serde(default)]
    pub test_result_ids: Vec<crate::ResultId>,
    #[serde(default)]
    pub from_hypothesis: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReviewFindingRequest {
    pub state: FindingState,
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestResultDto {
    pub id: crate::ResultId,
    pub procedure_key: crate::ProcedureKey,
    pub target_kind: String,
    pub target_id: String,
    pub identity_id: Option<crate::IdentityId>,
    pub state: TestResultState,
    pub conclusion: String,
    pub evidence_ids: Vec<EvidenceId>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProcedureDto {
    pub key: crate::ProcedureKey,
    pub id: String,
    pub version: String,
    pub name: String,
    pub purpose: String,
    pub target_types: Vec<String>,
    pub effect_class: ActionClass,
    pub required_capabilities: Vec<String>,
    pub preconditions: Vec<String>,
    pub limits: serde_json::Value,
    pub manifest: serde_json::Value,
    pub published_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EligibilityDto {
    pub procedure_key: crate::ProcedureKey,
    pub procedure_name: String,
    pub eligible: bool,
    pub target_count: i64,
    pub blockers: Vec<BlockerDto>,
    pub required_capabilities: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BlockerDto {
    pub code: String,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportRequest {
    #[serde(default)]
    pub include_evidence: bool,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportDto {
    pub id: uuid::Uuid,
    pub assessment_id: AssessmentId,
    pub item_count: i64,
    pub manifest_sha256: String,
    pub object_key: String,
    pub size_bytes: i64,
    pub include_evidence: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub download_url: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilitiesDto {
    pub adapters: Vec<crate::Capability>,
    pub model_provider: String,
    pub lab_url: String,
    pub raw_evidence_enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcceptanceCriterion {
    pub id: String,
    pub title: String,
    pub status: String,
    pub detail: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AcceptanceReport {
    pub generated_at: chrono::DateTime<chrono::Utc>,
    pub lab_version: String,
    pub criteria: Vec<AcceptanceCriterion>,
    pub passed: i64,
    pub failed: i64,
    pub skipped: i64,
}
