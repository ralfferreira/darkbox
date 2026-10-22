-- Correction round: a real cost that exceeds the reservation and the run budget
-- is persisted instead of rolled back. The excess is recorded and new model
-- calls are blocked.

ALTER TABLE runs
    ADD COLUMN budget_blocked boolean NOT NULL DEFAULT false,
    ADD COLUMN cost_overage_micros bigint NOT NULL DEFAULT 0;

ALTER TABLE agent_model_calls
    ADD COLUMN overage_micros bigint NOT NULL DEFAULT 0;
