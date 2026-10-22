-- Session 1: interactive browser sessions, correlated actions, and usage
-- settlement. Existing rows keep working with explicit defaults.

ALTER TABLE identities
    ADD COLUMN login_config jsonb NOT NULL DEFAULT '{}'::jsonb;

ALTER TABLE browser_sessions
    ADD COLUMN engine text NOT NULL DEFAULT 'unknown',
    ADD COLUMN last_observation jsonb,
    ADD COLUMN closed_at timestamptz;

CREATE UNIQUE INDEX browser_sessions_context_key_idx
    ON browser_sessions(context_key);

ALTER TABLE flow_steps
    ADD COLUMN action_json jsonb NOT NULL DEFAULT '{}'::jsonb;

CREATE TABLE agent_actions (
    id uuid PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    assessment_id uuid NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    run_id uuid NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    session_id uuid REFERENCES browser_sessions(id) ON DELETE SET NULL,
    ordinal integer NOT NULL,
    kind text NOT NULL,
    action jsonb NOT NULL DEFAULT '{}'::jsonb,
    state text NOT NULL DEFAULT 'running',
    effect_status text NOT NULL DEFAULT 'not_applicable',
    summary text NOT NULL DEFAULT '',
    observation jsonb,
    evidence_ids uuid[] NOT NULL DEFAULT '{}',
    started_at timestamptz NOT NULL DEFAULT now(),
    finished_at timestamptz,
    UNIQUE (run_id, ordinal)
);
CREATE INDEX agent_actions_run_idx ON agent_actions(run_id, ordinal);

-- One row per provider call. The reservation is idempotent; the settlement
-- updates the same row and never reserves a second model call.
CREATE TABLE agent_model_calls (
    call_id text PRIMARY KEY,
    workspace_id uuid NOT NULL REFERENCES workspaces(id) ON DELETE CASCADE,
    assessment_id uuid NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    run_id uuid NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    model text NOT NULL DEFAULT '',
    reserved_at timestamptz NOT NULL DEFAULT now(),
    settled_at timestamptz,
    input_tokens bigint,
    output_tokens bigint,
    total_tokens bigint,
    cost_micros bigint,
    usage_known boolean NOT NULL DEFAULT false
);
CREATE INDEX agent_model_calls_run_idx ON agent_model_calls(run_id, reserved_at);

-- Adjust already-reserved cost after the provider reports real token usage.
-- The reservation stays inside the budget; an overrun is rejected atomically.
CREATE OR REPLACE FUNCTION darkbox_adjust_cost(p_run uuid, p_delta bigint)
RETURNS bigint LANGUAGE plpgsql AS $$
DECLARE
    v_budget jsonb;
    v_limit bigint;
    v_spent bigint;
BEGIN
    IF p_delta <= 0 THEN
        UPDATE runs SET spent_cost_micros = GREATEST(0, spent_cost_micros + p_delta), updated_at=now()
            WHERE id=p_run RETURNING spent_cost_micros INTO v_spent;
        RETURN COALESCE(v_spent, -1);
    END IF;
    SELECT budget INTO v_budget FROM runs WHERE id=p_run FOR UPDATE;
    IF NOT FOUND THEN
        RETURN -1;
    END IF;
    v_limit := (v_budget->>'max_cost_micros')::bigint;
    UPDATE runs SET spent_cost_micros = spent_cost_micros + p_delta, updated_at=now()
        WHERE id=p_run AND spent_cost_micros + p_delta <= v_limit
        RETURNING spent_cost_micros INTO v_spent;
    RETURN COALESCE(v_spent, -1);
END;
$$;
