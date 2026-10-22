// Exact mirrors of crates/contracts/src/api.rs and the domain types it references.
// IDs are transparent UUIDs in Rust, so they serialize as strings.
// ProcedureKey is transparent and serializes as "<id>@<version>".

export type Id = string;
export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };

export type Role = 'admin' | 'operator' | 'reviewer' | 'reader';

export type EndpointState = 'candidate' | 'observed' | 'unreachable' | 'excluded';
export type FlowState = 'proposed' | 'observed' | 'reproduced' | 'stale' | 'blocked';
export type TestResultState =
  | 'not_applicable'
  | 'passed'
  | 'failed'
  | 'inconclusive'
  | 'error'
  | 'blocked';
export type FindingState =
  | 'hypothesis'
  | 'needs_review'
  | 'confirmed'
  | 'rejected'
  | 'remediated'
  | 'reopened';
export type ActionClass =
  | 'query'
  | 'session'
  | 'reversible_change'
  | 'external_effect'
  | 'destructive';
export type EvidenceKind =
  | 'http_exchange'
  | 'http_response_body'
  | 'browser_screenshot'
  | 'browser_trace'
  | 'tool_output'
  | 'export_manifest';
export type EvidenceClassification = 'sanitized' | 'raw_restricted' | 'metadata_only';
export type RunKind =
  | 'discovery'
  | 'observation'
  | 'session'
  | 'flow_reproduction'
  | 'procedure'
  | 'reconciliation'
  | 'network_scan'
  | 'firecrawl_map'
  | 'firecrawl_scrape'
  | 'burp_import'
  | 'agent';
export type StepKind =
  | 'http_observe'
  | 'http_probe'
  | 'discovery_import'
  | 'discovery_crawler'
  | 'browser_session'
  | 'browser_flow'
  | 'procedure_step'
  | 'reconcile'
  | 'network_scan'
  | 'firecrawl_map'
  | 'firecrawl_scrape'
  | 'burp_import'
  | 'agent_turn';
export type EffectStatus =
  | 'not_dispatched'
  | 'dispatched'
  | 'confirmed'
  | 'outcome_unknown'
  | 'not_applicable';
export type RunState =
  | 'queued'
  | 'running'
  | 'waiting_operator'
  | 'succeeded'
  | 'failed'
  | 'interrupted'
  | 'cancelling'
  | 'cancelled';
export type DenyCode =
  | 'out_of_scope'
  | 'excluded'
  | 'action_class_denied'
  | 'outside_window'
  | 'private_network_denied'
  | 'auxiliary_target_only'
  | 'capability_unavailable'
  | 'identity_expired'
  | 'budget_exhausted'
  | 'capture_failed'
  | 'external_result_unknown'
  | 'tool_error'
  | 'cancelled'
  | 'unsupported_scheme'
  | 'port_not_allowed'
  | 'path_not_allowed'
  | 'host_not_included';
export type CapabilityState = 'available' | 'unavailable' | 'degraded';

export const TERMINAL_RUN_STATES: readonly RunState[] = ['succeeded', 'failed', 'cancelled'];

export function isTerminalRun(state: RunState): boolean {
  return TERMINAL_RUN_STATES.includes(state);
}

export interface Budget {
  max_duration_seconds: number;
  max_requests: number;
  max_browser_steps: number;
  max_model_calls: number;
  max_cost_micros: number;
  max_artifact_bytes: number;
  max_concurrency_per_origin: number;
}

export interface BudgetSpent {
  requests: number;
  browser_steps: number;
  model_calls: number;
  cost_micros: number;
  overage_micros: number;
  artifact_bytes: number;
}

export interface TimeWindow {
  start: string;
  end: string;
}

export interface AuxiliaryDestination {
  host: string;
  ports: number[];
  schemes: string[];
}

export interface ScopePolicy {
  include_hosts: string[];
  exclude_hosts: string[];
  include_paths: string[];
  exclude_paths: string[];
  allowed_schemes: string[];
  allowed_ports: number[];
  allow_private_network: boolean;
  window: TimeWindow | null;
  action_classes: ActionClass[];
  navigation_dependencies: AuxiliaryDestination[];
  test_targets: AuxiliaryDestination[];
  limits: Budget;
  retain_raw: boolean;
}

export interface ScopeDecision {
  allowed: boolean;
  code: DenyCode | null;
  rule: string;
  policy_version: number;
  target: string;
  action_class: ActionClass;
}

export interface EventDto {
  event_id: Id;
  schema_version: number;
  workspace_id: Id;
  assessment_id: Id;
  run_id: Id | null;
  step_id: Id | null;
  event_type: string;
  occurred_at: string;
  recorded_at: string;
  correlation_id: string;
  seq: number;
  payload: JsonValue;
}

export interface SessionDto {
  workspace_id: Id;
  workspace_name: string;
  role: Role;
  operator: string;
  can_write: boolean;
}

export interface CompanyDto {
  id: Id;
  name: string;
  description: string;
  assessment_count: number;
  last_activity_at: string | null;
  created_at: string;
}

export interface CreateCompanyRequest {
  name: string;
  description: string;
}

export interface CreateAssessmentRequest {
  company_id: Id;
  name: string;
  environment: string;
  objective: string;
  initial_targets: string[];
  policy: ScopePolicy;
}

export interface AssessmentSummary {
  id: Id;
  company_id: Id;
  company_name: string;
  name: string;
  environment: string;
  objective: string;
  state: string;
  scope_version: number;
  endpoint_count: number;
  observed_count: number;
  run_count: number;
  open_findings: number;
  last_activity_at: string;
  created_at: string;
}

export interface ScopeVersionDto {
  id: Id;
  version: number;
  policy: ScopePolicy;
  created_at: string;
  created_by: string;
  note: string;
}

export interface AssessmentDetail {
  assessment: AssessmentSummary;
  scope: ScopeVersionDto;
  identities: IdentityDto[];
  recent_runs: RunSummary[];
  coverage: CoverageDto;
}

export interface CoverageDto {
  known_candidates: number;
  observed_endpoints: number;
  excluded_endpoints: number;
  unreachable_endpoints: number;
  flows_reproduced: number;
  flows_known: number;
  findings_confirmed: number;
  findings_needs_review: number;
  evidence_count: number;
  evidence_bytes: number;
}

export interface IdentityDto {
  id: Id;
  label: string;
  role_label: string;
  tenant_keys: string[];
  capabilities: string[];
  has_secret_ref: boolean;
  login_config: JsonValue;
  session_state: string;
  session_expires_at: string | null;
  created_at: string;
}

export interface CreateIdentityRequest {
  label: string;
  role_label: string;
  tenant_keys: string[];
  capabilities: string[];
  secret_ref: string | null;
  login_config?: JsonValue;
}

export interface UpdateIdentityRequest {
  role_label?: string;
  tenant_keys?: string[];
  capabilities?: string[];
  secret_ref?: string | null;
  login_config?: JsonValue;
}

export interface TargetTenantDto {
  id: Id;
  key: string;
  label: string;
}

export interface EndpointDto {
  id: Id;
  asset_id: Id;
  scheme: string;
  host: string;
  port: number;
  method: string;
  path: string;
  query_names: string[];
  state: EndpointState;
  source: string;
  run_id: Id | null;
  group_pattern: string | null;
  identity_id: Id | null;
  observed_at: string | null;
  recorded_at: string;
}

export interface InventoryResponse {
  endpoints: EndpointDto[];
  total: number;
  counts: Record<string, number>;
  gaps: GapDto[];
  policy_version: number;
}

export interface GapDto {
  kind: string;
  detail: string;
  target: string | null;
}

export interface ImportUrlsRequest {
  urls: string[];
  source: string | null;
}

export interface ObservationDto {
  id: Id;
  subject_kind: string;
  subject_id: string;
  property: string;
  value: JsonValue;
  source: string;
  confidence: number;
  evidence_id: Id | null;
  observed_at: string;
  recorded_at: string;
}

export interface FlowStepDto {
  ordinal: number;
  action_id: Id | null;
  intent: string;
  expected_result: string;
  endpoint_id: Id | null;
  confirmed: boolean;
}

export interface FlowDto {
  id: Id;
  name: string;
  state: FlowState;
  version: number;
  identity_id: Id | null;
  tenant_key: string | null;
  steps: FlowStepDto[];
  evidence_ids: Id[];
  preconditions: string[];
  observed_at: string | null;
  recorded_at: string;
}

export interface CreateFlowStep {
  intent: string;
  expected_result: string;
  endpoint_id: Id | null;
  action_class: ActionClass | null;
  action_json?: JsonValue;
  confirmed?: boolean;
}

export interface CreateFlowRequest {
  name: string;
  identity_id: Id | null;
  tenant_key: string | null;
  preconditions: string[];
  steps: CreateFlowStep[];
  state?: FlowState;
  evidence_ids?: Id[];
}

export interface CreateActionRequest {
  intent: string;
  endpoint_id: Id | null;
  action_class: ActionClass;
  preconditions: string[];
  expected_effect: string;
}

export interface ActionDto {
  id: Id;
  intent: string;
  endpoint_id: Id | null;
  effect_class: ActionClass;
  state: string;
  preconditions: string[];
  expected_effect: string;
  created_at: string;
}

export interface CreateRunRequest {
  assessment_id: Id;
  kind: RunKind;
  idempotency_key: string;
  flow_id: Id | null;
  procedure_key: string | null;
  endpoint_ids: Id[];
  identity_id: Id | null;
  budget_override: Budget | null;
  note: string;
  targets?: string[];
  params?: Record<string, unknown>;
  base_url?: string | null;
  tenant?: string | null;
  identity_ids?: Id[];
  checks?: string[];
  login_path?: string | null;
}

export interface RunSummary {
  id: Id;
  assessment_id: Id;
  kind: RunKind;
  state: RunState;
  procedure_key: string | null;
  policy_version: number;
  budget: Budget;
  spent: BudgetSpent;
  effect_unknown: boolean;
  budget_blocked: boolean;
  reason: string | null;
  idempotency_key: string;
  created_at: string;
  started_at: string | null;
  finished_at: string | null;
  step_count: number;
  steps_done: number;
}

export interface RunStepDto {
  id: Id;
  ordinal: number;
  kind: StepKind;
  state: string;
  attempt: number;
  effect_status: EffectStatus;
  summary: string;
  started_at: string | null;
  finished_at: string | null;
}

export interface RunDetail {
  run: RunSummary;
  steps: RunStepDto[];
  events: EventDto[];
}

export interface CancelRunRequest {
  reason: string;
}

export interface ResumeRunRequest {
  reconcile_unknown: boolean;
  note: string;
}

export interface EvidenceDto {
  id: Id;
  assessment_id: Id;
  kind: EvidenceKind;
  classification: EvidenceClassification;
  sha256: string;
  size_bytes: number;
  truncated: boolean;
  sanitized: boolean;
  content_type: string;
  run_id: Id | null;
  step_id: Id | null;
  identity_id: Id | null;
  summary: string;
  created_at: string;
  retention_until: string | null;
  access_url: string | null;
}

export interface MemoryPageDto {
  id: Id;
  slug: string;
  title: string;
  version: number;
  body_markdown: string;
  state: string;
  references: MemoryReferenceDto[];
  validity: string;
  created_by: string;
  created_at: string;
}

export interface MemoryReferenceDto {
  kind: string;
  id: string;
  label: string;
}

export interface MemorySearchRequest {
  query: string;
  token_budget: number | null;
  identity_id: Id | null;
}

export interface ContextPackageDto {
  assessment_id: Id;
  objective: string;
  policy_version: number;
  identity_id: Id | null;
  generated_at: string;
  token_budget: number;
  estimated_tokens: number;
  sections: ContextSectionDto[];
  omitted: string[];
  references: MemoryReferenceDto[];
}

export interface ContextSectionDto {
  title: string;
  body: string;
  reference_kind: string | null;
}

export interface FindingDto {
  id: Id;
  title: string;
  state: FindingState;
  severity: string;
  claim: string;
  impact: string;
  endpoint_id: Id | null;
  evidence_ids: Id[];
  test_result_ids: Id[];
  revisions: FindingRevisionDto[];
  created_at: string;
  updated_at: string;
}

export interface FindingRevisionDto {
  revision: number;
  state: FindingState;
  note: string;
  actor: string;
  at: string;
}

export interface CreateFindingRequest {
  title: string;
  severity: string;
  claim: string;
  impact: string;
  endpoint_id: Id | null;
  evidence_ids: Id[];
  test_result_ids: Id[];
  from_hypothesis: boolean;
}

export interface ReviewFindingRequest {
  state: FindingState;
  note: string;
}

export interface TestResultDto {
  id: Id;
  procedure_key: string;
  target_kind: string;
  target_id: string;
  identity_id: Id | null;
  state: TestResultState;
  conclusion: string;
  evidence_ids: Id[];
  created_at: string;
}

export interface ProcedureDto {
  key: string;
  id: string;
  version: string;
  name: string;
  purpose: string;
  target_types: string[];
  effect_class: ActionClass;
  required_capabilities: string[];
  preconditions: string[];
  limits: JsonValue;
  manifest: JsonValue;
  published_at: string;
}

export interface EligibilityDto {
  procedure_key: string;
  procedure_name: string;
  eligible: boolean;
  target_count: number;
  blockers: BlockerDto[];
  required_capabilities: string[];
}

export interface BlockerDto {
  code: string;
  detail: string;
}

export interface ExportRequest {
  include_evidence: boolean;
  note: string;
}

export interface ExportDto {
  id: string;
  assessment_id: Id;
  item_count: number;
  manifest_sha256: string;
  object_key: string;
  size_bytes: number;
  include_evidence: boolean;
  created_at: string;
  download_url: string;
}

export interface Capability {
  id: string;
  provider: string;
  version: string;
  state: CapabilityState;
  detail: string;
}

export interface CapabilitiesDto {
  adapters: Capability[];
  model_provider: string;
  lab_url: string;
  raw_evidence_enabled: boolean;
}

export interface AgentTurnResponse {
  accepted: boolean;
  run_id: string;
  planner: string | null;
  model: string;
  run?: RunSummary;
}

export interface AgentActionDto {
  id: Id;
  run_id: Id;
  session_id: Id | null;
  ordinal: number;
  kind: string;
  action: JsonValue;
  state: string;
  effect_status: EffectStatus;
  summary: string;
  observation: JsonValue;
  evidence_ids: Id[];
  started_at: string;
  finished_at: string | null;
}

export interface AgentProgress {
  run: RunSummary;
  objective: JsonValue;
  identity_id: Id | null;
  actions: AgentActionDto[];
  model_calls: Array<{
    call_id: string;
    model: string;
    settled_at: string | null;
    input_tokens: number | null;
    output_tokens: number | null;
    total_tokens: number | null;
    cost_micros: number | null;
    usage_known: boolean;
  }>;
  evidence: EvidenceDto[];
  current: {
    url: JsonValue;
    title: JsonValue;
    observation: JsonValue;
  };
  pending: string;
  blocked: string;
}
