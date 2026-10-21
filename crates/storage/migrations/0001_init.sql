-- DarkBox initial schema. Every operational row carries workspace_id and,
-- where it belongs to a piece of work, assessment_id.

CREATE TABLE workspaces (
    id uuid PRIMARY KEY,
    name text NOT NULL,
    retention_days integer NOT NULL DEFAULT 30,
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE api_tokens (
    token_hash text PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    operator text NOT NULL,
    role text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    revoked_at timestamptz
);

CREATE TABLE companies (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    name text NOT NULL,
    description text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE assessments (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    company_id uuid NOT NULL REFERENCES companies(id) ON DELETE CASCADE,
    name text NOT NULL,
    environment text NOT NULL,
    objective text NOT NULL,
    state text NOT NULL DEFAULT 'active',
    scope_version bigint NOT NULL DEFAULT 1,
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX assessments_workspace_idx ON assessments(workspace_id, updated_at DESC);

CREATE TABLE scope_versions (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    assessment_id uuid NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    version bigint NOT NULL,
    policy jsonb NOT NULL,
    note text NOT NULL DEFAULT '',
    created_by text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (assessment_id, version)
);

CREATE TABLE assets (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    assessment_id uuid NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    kind text NOT NULL,
    canonical_id text NOT NULL,
    environment text NOT NULL,
    relation_evidence text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (assessment_id, kind, canonical_id)
);

CREATE TABLE endpoints (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    assessment_id uuid NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    asset_id uuid NOT NULL REFERENCES assets(id) ON DELETE CASCADE,
    scheme text NOT NULL,
    host text NOT NULL,
    port integer NOT NULL,
    method text NOT NULL,
    path text NOT NULL,
    query_names jsonb NOT NULL DEFAULT '[]',
    record_key text NOT NULL,
    state text NOT NULL DEFAULT 'candidate',
    source text NOT NULL,
    source_detail text NOT NULL DEFAULT '',
    run_id uuid,
    identity_id uuid,
    group_pattern text NOT NULL,
    group_version integer NOT NULL DEFAULT 1,
    observed_at timestamptz,
    recorded_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (assessment_id, record_key)
);
CREATE INDEX endpoints_inventory_idx ON endpoints(assessment_id, state, recorded_at DESC);
CREATE INDEX endpoints_group_idx ON endpoints(assessment_id, group_pattern);

CREATE TABLE observations (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    assessment_id uuid NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    subject_kind text NOT NULL,
    subject_id uuid NOT NULL,
    property text NOT NULL,
    value jsonb NOT NULL,
    source text NOT NULL,
    confidence double precision NOT NULL DEFAULT 1.0,
    evidence_id uuid,
    observed_at timestamptz NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX observations_subject_idx ON observations(assessment_id, subject_kind, subject_id);

CREATE TABLE target_tenants (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    assessment_id uuid NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    key text NOT NULL,
    label text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (assessment_id, key)
);

CREATE TABLE identities (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    assessment_id uuid NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    label text NOT NULL,
    role_label text NOT NULL,
    tenant_keys jsonb NOT NULL DEFAULT '[]',
    capabilities jsonb NOT NULL DEFAULT '[]',
    secret_ref text,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (assessment_id, label)
);

CREATE TABLE browser_sessions (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    assessment_id uuid NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    identity_id uuid NOT NULL REFERENCES identities(id) ON DELETE CASCADE,
    tenant_key text,
    context_key text NOT NULL,
    state text NOT NULL DEFAULT 'unknown',
    expires_at timestamptz,
    created_at timestamptz NOT NULL DEFAULT now(),
    last_seen_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE actions (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    assessment_id uuid NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    intent text NOT NULL,
    endpoint_id uuid REFERENCES endpoints(id) ON DELETE SET NULL,
    effect_class text NOT NULL,
    state text NOT NULL DEFAULT 'candidate',
    preconditions jsonb NOT NULL DEFAULT '[]',
    expected_effect text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE flows (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    assessment_id uuid NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    name text NOT NULL,
    state text NOT NULL DEFAULT 'proposed',
    version bigint NOT NULL DEFAULT 1,
    identity_id uuid REFERENCES identities(id) ON DELETE SET NULL,
    tenant_key text,
    preconditions jsonb NOT NULL DEFAULT '[]',
    evidence_ids uuid[] NOT NULL DEFAULT '{}',
    observed_at timestamptz,
    recorded_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE flow_steps (
    flow_id uuid NOT NULL REFERENCES flows(id) ON DELETE CASCADE,
    ordinal integer NOT NULL,
    action_id uuid REFERENCES actions(id) ON DELETE SET NULL,
    intent text NOT NULL,
    expected_result text NOT NULL DEFAULT '',
    endpoint_id uuid REFERENCES endpoints(id) ON DELETE SET NULL,
    confirmed boolean NOT NULL DEFAULT false,
    PRIMARY KEY (flow_id, ordinal)
);

CREATE TABLE evidences (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    assessment_id uuid NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    kind text NOT NULL,
    classification text NOT NULL,
    object_key text NOT NULL,
    sha256 text NOT NULL,
    size_bytes bigint NOT NULL,
    truncated boolean NOT NULL DEFAULT false,
    sanitized boolean NOT NULL DEFAULT true,
    content_type text NOT NULL DEFAULT 'application/octet-stream',
    run_id uuid,
    step_id uuid,
    identity_id uuid,
    summary text NOT NULL DEFAULT '',
    source text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now(),
    retention_until timestamptz
);
CREATE INDEX evidences_list_idx ON evidences(assessment_id, created_at DESC);

CREATE TABLE runs (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    assessment_id uuid NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    kind text NOT NULL,
    state text NOT NULL DEFAULT 'queued',
    policy_version bigint NOT NULL,
    policy jsonb NOT NULL,
    budget jsonb NOT NULL,
    spent_requests bigint NOT NULL DEFAULT 0,
    spent_browser_steps bigint NOT NULL DEFAULT 0,
    spent_model_calls bigint NOT NULL DEFAULT 0,
    spent_cost_micros bigint NOT NULL DEFAULT 0,
    spent_artifact_bytes bigint NOT NULL DEFAULT 0,
    procedure_key text,
    flow_id uuid REFERENCES flows(id) ON DELETE SET NULL,
    identity_id uuid REFERENCES identities(id) ON DELETE SET NULL,
    idempotency_key text NOT NULL,
    reason text,
    effect_unknown boolean NOT NULL DEFAULT false,
    note text NOT NULL DEFAULT '',
    created_at timestamptz NOT NULL DEFAULT now(),
    started_at timestamptz,
    finished_at timestamptz,
    updated_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (workspace_id, idempotency_key)
);
CREATE INDEX runs_queue_idx ON runs(state, created_at);
CREATE INDEX runs_assessment_idx ON runs(assessment_id, created_at DESC);

CREATE TABLE run_steps (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    assessment_id uuid NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    run_id uuid NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    ordinal integer NOT NULL,
    kind text NOT NULL,
    state text NOT NULL DEFAULT 'pending',
    effect_status text NOT NULL DEFAULT 'not_dispatched',
    operation jsonb NOT NULL DEFAULT '{}',
    summary text NOT NULL DEFAULT '',
    attempt integer NOT NULL DEFAULT 0,
    lease_owner text,
    lease_expires_at timestamptz,
    started_at timestamptz,
    finished_at timestamptz,
    UNIQUE (run_id, ordinal)
);
CREATE INDEX run_steps_queue_idx ON run_steps(state, lease_expires_at);

CREATE TABLE step_attempts (
    id uuid PRIMARY KEY,
    step_id uuid NOT NULL REFERENCES run_steps(id) ON DELETE CASCADE,
    attempt integer NOT NULL,
    lease_owner text NOT NULL,
    lease_expires_at timestamptz NOT NULL,
    effect_status text NOT NULL,
    started_at timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz,
    result jsonb,
    UNIQUE (step_id, attempt)
);

CREATE TABLE procedures (
    id text NOT NULL,
    version text NOT NULL,
    name text NOT NULL,
    purpose text NOT NULL,
    target_types jsonb NOT NULL,
    effect_class text NOT NULL,
    required_capabilities jsonb NOT NULL,
    preconditions jsonb NOT NULL,
    manifest jsonb NOT NULL,
    published_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (id, version)
);

CREATE TABLE test_results (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    assessment_id uuid NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    procedure_key text NOT NULL,
    target_kind text NOT NULL,
    target_id text NOT NULL,
    identity_id uuid,
    state text NOT NULL,
    conclusion text NOT NULL DEFAULT '',
    evidence_ids uuid[] NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE findings (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    assessment_id uuid NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    title text NOT NULL,
    state text NOT NULL DEFAULT 'hypothesis',
    severity text NOT NULL DEFAULT 'info',
    claim text NOT NULL,
    impact text NOT NULL DEFAULT '',
    endpoint_id uuid REFERENCES endpoints(id) ON DELETE SET NULL,
    evidence_ids uuid[] NOT NULL DEFAULT '{}',
    test_result_ids uuid[] NOT NULL DEFAULT '{}',
    created_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE finding_revisions (
    finding_id uuid NOT NULL REFERENCES findings(id) ON DELETE CASCADE,
    revision integer NOT NULL,
    state text NOT NULL,
    note text NOT NULL DEFAULT '',
    actor text NOT NULL,
    at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (finding_id, revision)
);

CREATE TABLE memory_pages (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    assessment_id uuid NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    slug text NOT NULL,
    title text NOT NULL,
    version integer NOT NULL,
    body_markdown text NOT NULL,
    state text NOT NULL DEFAULT 'synthesis',
    references_json jsonb NOT NULL DEFAULT '[]',
    validity text NOT NULL DEFAULT 'current',
    created_by text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now(),
    UNIQUE (assessment_id, slug, version)
);
CREATE INDEX memory_pages_search_idx ON memory_pages
    USING gin (to_tsvector('simple', title || ' ' || body_markdown));

CREATE TABLE audit_events (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    assessment_id uuid,
    actor text NOT NULL,
    operation text NOT NULL,
    target text NOT NULL DEFAULT '',
    reason text NOT NULL DEFAULT '',
    correlation_id text NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT now()
);

CREATE TABLE events (
    seq bigserial PRIMARY KEY,
    event_id uuid NOT NULL UNIQUE,
    schema_version integer NOT NULL,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    assessment_id uuid NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    run_id uuid,
    step_id uuid,
    event_type text NOT NULL,
    occurred_at timestamptz NOT NULL,
    recorded_at timestamptz NOT NULL DEFAULT now(),
    correlation_id text NOT NULL,
    payload jsonb NOT NULL DEFAULT '{}'
);
CREATE INDEX events_run_idx ON events(run_id, seq);
CREATE INDEX events_assessment_idx ON events(assessment_id, seq);

CREATE TABLE exports (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    assessment_id uuid NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    object_key text NOT NULL,
    manifest_sha256 text NOT NULL,
    size_bytes bigint NOT NULL,
    item_count bigint NOT NULL,
    include_evidence boolean NOT NULL DEFAULT false,
    created_by text NOT NULL,
    created_at timestamptz NOT NULL DEFAULT now()
);

-- Atomic budget reservation. Returns the new spent value, or -1 when the
-- reservation would exceed the run budget. The row lock makes concurrent
-- workers serialize on the same budget.
CREATE OR REPLACE FUNCTION darkbox_reserve_budget(p_run uuid, p_kind text, p_amount bigint)
RETURNS bigint LANGUAGE plpgsql AS $$
DECLARE
    v_budget jsonb;
    v_spent bigint;
    v_limit bigint;
BEGIN
    SELECT budget INTO v_budget FROM runs WHERE id = p_run FOR UPDATE;
    IF NOT FOUND THEN
        RETURN -1;
    END IF;

    IF p_kind = 'requests' THEN
        v_limit := (v_budget->>'max_requests')::bigint;
        UPDATE runs SET spent_requests = spent_requests + p_amount, updated_at = now()
            WHERE id = p_run AND spent_requests + p_amount <= v_limit
            RETURNING spent_requests INTO v_spent;
    ELSIF p_kind = 'browser_steps' THEN
        v_limit := (v_budget->>'max_browser_steps')::bigint;
        UPDATE runs SET spent_browser_steps = spent_browser_steps + p_amount, updated_at = now()
            WHERE id = p_run AND spent_browser_steps + p_amount <= v_limit
            RETURNING spent_browser_steps INTO v_spent;
    ELSIF p_kind = 'model_calls' THEN
        v_limit := (v_budget->>'max_model_calls')::bigint;
        UPDATE runs SET spent_model_calls = spent_model_calls + p_amount, updated_at = now()
            WHERE id = p_run AND spent_model_calls + p_amount <= v_limit
            RETURNING spent_model_calls INTO v_spent;
    ELSIF p_kind = 'cost_micros' THEN
        v_limit := (v_budget->>'max_cost_micros')::bigint;
        UPDATE runs SET spent_cost_micros = spent_cost_micros + p_amount, updated_at = now()
            WHERE id = p_run AND spent_cost_micros + p_amount <= v_limit
            RETURNING spent_cost_micros INTO v_spent;
    ELSIF p_kind = 'artifact_bytes' THEN
        v_limit := (v_budget->>'max_artifact_bytes')::bigint;
        UPDATE runs SET spent_artifact_bytes = spent_artifact_bytes + p_amount, updated_at = now()
            WHERE id = p_run AND spent_artifact_bytes + p_amount <= v_limit
            RETURNING spent_artifact_bytes INTO v_spent;
    ELSE
        RAISE EXCEPTION 'unknown budget kind %', p_kind;
    END IF;

    RETURN COALESCE(v_spent, -1);
END;
$$;
