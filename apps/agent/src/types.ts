export interface RunSummary {
  id: string;
  assessment_id: string;
  kind: string;
  state: string;
  policy_version: number;
  reason: string | null;
  effect_unknown: boolean;
  spent: {
    requests: number;
    browser_steps: number;
    model_calls: number;
    cost_micros: number;
    artifact_bytes: number;
  };
  budget: {
    max_requests: number;
    max_browser_steps: number;
    max_model_calls: number;
    max_cost_micros: number;
    max_duration_seconds: number;
  };
}

export interface ContextSection {
  title: string;
  body: string;
  reference_kind: string | null;
}

export interface ContextPackage {
  assessment_id: string;
  objective: string;
  policy_version: number;
  identity_id: string | null;
  token_budget: number;
  estimated_tokens: number;
  sections: ContextSection[];
  omitted: string[];
  references: Array<{ kind: string; id: string; label: string }>;
}

export interface Evidence {
  id: string;
  run_id: string | null;
  summary: string;
  size_bytes: number;
  classification: string;
}
