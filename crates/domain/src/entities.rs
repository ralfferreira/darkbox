use chrono::{DateTime, Utc};
use darkbox_contracts::api::MemoryReferenceDto;
use darkbox_contracts::policy::ScopePolicy;
use darkbox_contracts::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub retention_days: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Company {
    pub id: CompanyId,
    pub workspace_id: WorkspaceId,
    pub name: String,
    pub description: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assessment {
    pub id: AssessmentId,
    pub workspace_id: WorkspaceId,
    pub company_id: CompanyId,
    pub name: String,
    pub environment: String,
    pub objective: String,
    pub state: String,
    pub scope_version: i64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScopeVersion {
    pub id: ScopeVersionId,
    pub workspace_id: WorkspaceId,
    pub assessment_id: AssessmentId,
    pub version: i64,
    pub policy: ScopePolicy,
    pub note: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Asset {
    pub id: AssetId,
    pub workspace_id: WorkspaceId,
    pub assessment_id: AssessmentId,
    pub kind: AssetKind,
    pub canonical_id: String,
    pub environment: String,
    pub relation_evidence: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointRecord {
    pub id: EndpointId,
    pub workspace_id: WorkspaceId,
    pub assessment_id: AssessmentId,
    pub asset_id: AssetId,
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub method: String,
    pub path: String,
    pub query_names: Vec<String>,
    pub state: EndpointState,
    pub source: String,
    pub source_detail: String,
    pub run_id: Option<RunId>,
    pub identity_id: Option<IdentityId>,
    pub group_pattern: String,
    pub group_version: i32,
    pub observed_at: Option<DateTime<Utc>>,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestIdentity {
    pub id: IdentityId,
    pub workspace_id: WorkspaceId,
    pub assessment_id: AssessmentId,
    pub label: String,
    pub role_label: String,
    pub tenant_keys: Vec<String>,
    pub capabilities: Vec<String>,
    pub secret_ref: Option<String>,
    #[serde(default)]
    pub login_config: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetTenant {
    pub id: TenantId,
    pub workspace_id: WorkspaceId,
    pub assessment_id: AssessmentId,
    pub key: String,
    pub label: String,
    pub created_at: DateTime<Utc>,
}

/// A browser session fixes one identity and one effective tenant. Historical
/// records keep the context that produced them.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowserSession {
    pub id: BrowserSessionId,
    pub workspace_id: WorkspaceId,
    pub assessment_id: AssessmentId,
    pub identity_id: IdentityId,
    pub tenant_key: Option<String>,
    pub context_key: String,
    pub state: String,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub id: ActionId,
    pub workspace_id: WorkspaceId,
    pub assessment_id: AssessmentId,
    pub intent: String,
    pub endpoint_id: Option<EndpointId>,
    pub effect_class: ActionClass,
    pub state: String,
    pub preconditions: Vec<String>,
    pub expected_effect: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Flow {
    pub id: FlowId,
    pub workspace_id: WorkspaceId,
    pub assessment_id: AssessmentId,
    pub name: String,
    pub state: FlowState,
    pub version: i64,
    pub identity_id: Option<IdentityId>,
    pub tenant_key: Option<String>,
    pub preconditions: Vec<String>,
    pub evidence_ids: Vec<EvidenceId>,
    pub observed_at: Option<DateTime<Utc>>,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlowStep {
    pub ordinal: i64,
    pub action_id: Option<ActionId>,
    pub intent: String,
    pub expected_result: String,
    pub endpoint_id: Option<EndpointId>,
    pub confirmed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    pub id: ObservationId,
    pub workspace_id: WorkspaceId,
    pub assessment_id: AssessmentId,
    pub subject_kind: String,
    pub subject_id: String,
    pub property: String,
    pub value: serde_json::Value,
    pub source: String,
    pub confidence: f64,
    pub evidence_id: Option<EvidenceId>,
    pub observed_at: DateTime<Utc>,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvidenceRecord {
    pub id: EvidenceId,
    pub workspace_id: WorkspaceId,
    pub assessment_id: AssessmentId,
    pub kind: EvidenceKind,
    pub classification: EvidenceClassification,
    pub object_key: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub truncated: bool,
    pub sanitized: bool,
    pub content_type: String,
    pub run_id: Option<RunId>,
    pub step_id: Option<StepId>,
    pub identity_id: Option<IdentityId>,
    pub summary: String,
    pub source: String,
    pub created_at: DateTime<Utc>,
    pub retention_until: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    pub id: RunId,
    pub workspace_id: WorkspaceId,
    pub assessment_id: AssessmentId,
    pub kind: RunKind,
    pub state: RunState,
    pub policy_version: i64,
    pub policy: ScopePolicy,
    pub budget: Budget,
    pub spent: BudgetSpent,
    pub procedure_key: Option<ProcedureKey>,
    pub flow_id: Option<FlowId>,
    pub identity_id: Option<IdentityId>,
    pub idempotency_key: String,
    pub reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunStepRecord {
    pub id: StepId,
    pub run_id: RunId,
    pub ordinal: i64,
    pub kind: StepKind,
    pub state: String,
    pub effect_status: EffectStatus,
    pub operation: serde_json::Value,
    pub summary: String,
    pub attempt: i64,
    pub started_at: Option<DateTime<Utc>>,
    pub finished_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepAttempt {
    pub id: uuid::Uuid,
    pub step_id: StepId,
    pub attempt: i64,
    pub lease_owner: String,
    pub lease_expires_at: DateTime<Utc>,
    pub effect_status: EffectStatus,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub result: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcedureRecord {
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
    pub published_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub id: ResultId,
    pub workspace_id: WorkspaceId,
    pub assessment_id: AssessmentId,
    pub procedure_key: ProcedureKey,
    pub target_kind: String,
    pub target_id: String,
    pub identity_id: Option<IdentityId>,
    pub state: TestResultState,
    pub conclusion: String,
    pub evidence_ids: Vec<EvidenceId>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub id: FindingId,
    pub workspace_id: WorkspaceId,
    pub assessment_id: AssessmentId,
    pub title: String,
    pub state: FindingState,
    pub severity: String,
    pub claim: String,
    pub impact: String,
    pub endpoint_id: Option<EndpointId>,
    pub evidence_ids: Vec<EvidenceId>,
    pub test_result_ids: Vec<ResultId>,
    pub revisions: Vec<FindingRevision>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FindingRevision {
    pub revision: i64,
    pub state: FindingState,
    pub note: String,
    pub actor: String,
    pub at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryPage {
    pub id: MemoryPageId,
    pub workspace_id: WorkspaceId,
    pub assessment_id: AssessmentId,
    pub slug: String,
    pub title: String,
    pub version: i64,
    pub body_markdown: String,
    pub state: String,
    pub references: Vec<MemoryReferenceDto>,
    pub validity: String,
    pub created_by: String,
    pub created_at: DateTime<Utc>,
}
