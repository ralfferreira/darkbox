-- Correction round: durable action intent, request grants, structured replay
-- assertions, and reservation/settlement accounting.

ALTER TABLE agent_actions
    ADD COLUMN authorization_json jsonb NOT NULL DEFAULT '{}'::jsonb,
    ADD COLUMN effect_class text NOT NULL DEFAULT 'query',
    ADD COLUMN idempotency_key text,
    ADD COLUMN lease_expires_at timestamptz,
    ADD COLUMN requests_reserved bigint NOT NULL DEFAULT 0,
    ADD COLUMN requests_used bigint NOT NULL DEFAULT 0,
    ADD COLUMN resolution text,
    ADD COLUMN resolved_at timestamptz,
    ADD COLUMN resolved_by text;

CREATE UNIQUE INDEX agent_actions_idempotency_idx
    ON agent_actions(run_id, idempotency_key)
    WHERE idempotency_key IS NOT NULL;

ALTER TABLE flows
    ADD COLUMN assertion jsonb;

ALTER TABLE flow_steps
    ADD COLUMN assertion jsonb;

ALTER TABLE agent_model_calls
    ADD COLUMN reserved_cost_micros bigint NOT NULL DEFAULT 0;
