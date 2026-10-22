import { useEffect, useId, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { agentTurn, AGENT_URL, errorMessage, type AgentHealth } from '../api/client';
import { useIdentities } from '../api/queries';
import { Dialog } from './Dialog';
import { Field, SelectInput, TextInput, Textarea } from './Form';
import { useToast } from './Toast';

const STEP_OPTIONS = [3, 5, 8, 12];

export function AgentTurnDialog({
  open,
  onClose,
  assessmentId,
  continueFrom,
  initialObjective,
}: {
  open: boolean;
  onClose: () => void;
  assessmentId: string;
  continueFrom?: string | null;
  initialObjective?: string;
}) {
  const identities = useIdentities(assessmentId);
  const navigate = useNavigate();
  const { notify } = useToast();
  const formId = useId();
  const uid = useId();
  const [objective, setObjective] = useState('');
  const [identityId, setIdentityId] = useState('');
  const [startUrl, setStartUrl] = useState('');
  const [maxSteps, setMaxSteps] = useState('8');
  const [tokenBudget, setTokenBudget] = useState('6000');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [health, setHealth] = useState<AgentHealth | null>(null);
  const [healthError, setHealthError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setObjective(initialObjective ?? '');
    setIdentityId('');
    setStartUrl('');
    setMaxSteps('8');
    setTokenBudget('6000');
    setError(null);
    setBusy(false);
    setHealth(null);
    setHealthError(null);
    const controller = new AbortController();
    fetch(`${AGENT_URL}/health`, { signal: controller.signal })
      .then((response) => {
        if (!response.ok) throw new Error(`status ${response.status}`);
        return response.json() as Promise<AgentHealth>;
      })
      .then(setHealth)
      .catch((cause: unknown) => {
        if (cause instanceof DOMException && cause.name === 'AbortError') return;
        setHealthError(errorMessage(cause));
      });
    return () => controller.abort();
  }, [open, initialObjective]);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!objective.trim()) {
      setError('Describe the observable result the objective must establish.');
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const response = await agentTurn({
        assessment_id: assessmentId,
        objective: objective.trim(),
        identity_id: identityId || null,
        max_steps: Number(maxSteps) || 8,
        token_budget: Number(tokenBudget) || 6000,
        start_url: startUrl.trim() || undefined,
        continue_from: continueFrom ?? null,
      });
      notify('Objective created. The live exploration view is open.', 'success');
      onClose();
      navigate(`/assessments/${assessmentId}/runs/${response.run_id}`);
    } catch (cause) {
      setError(errorMessage(cause));
      setBusy(false);
    }
  };

  const unavailable = Boolean(healthError) || health?.status === 'missing_model_key';
  const planner = health?.planner === 'deterministic';

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={continueFrom ? 'Continue objective' : 'Ask the agent'}
      description="One objective, one live browser session. The view opens immediately and updates while the agent works."
      size="md"
      footer={
        <>
          <button type="button" className="btn" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            form={formId}
            className="btn btn-primary"
            disabled={busy || unavailable || !objective.trim()}
          >
            {busy ? 'Creating objective…' : 'Start exploration'}
          </button>
        </>
      }
    >
      <form id={formId} onSubmit={submit} className="form-stack">
        <div className="notice" data-tone={unavailable ? 'amber' : undefined}>
          {health ? (
            <>
              <span className="mono">
                {planner ? 'deterministic planner (labelled)' : health.model}
                {health.planner ? ' · planner mode' : ''}
              </span>
              <br />
              <span>
                {planner
                  ? 'No model provider is configured. This mode drives the same real browser loop and is labelled as deterministic in the run.'
                  : `Provider ready at ${AGENT_URL}. Up to ${health.max_steps} model steps.`}
              </span>
            </>
          ) : healthError ? (
            <>
              <strong>Agent service unavailable</strong>
              <p>
                Start it with <span className="mono">cd apps/agent &amp;&amp; npm start</span> and
                set an OpenAI-compatible model key.
              </p>
              <p className="mono quiet">{healthError}</p>
            </>
          ) : (
            <span className="mono">checking agent service…</span>
          )}
        </div>

        {continueFrom ? (
          <div className="notice">
            <strong>Continuing from a persisted checkpoint</strong>
            <p>
              The new objective receives the previous actions, observed URLs and evidence. It must
              not repeat an effect whose outcome is unknown.
            </p>
          </div>
        ) : null}

        <Field
          label="Objective"
          htmlFor={`${uid}-objective`}
          hint="State the functional result and the confirmation you expect. Do not provide selectors, click sequences or internal routes."
        >
          <Textarea
            id={`${uid}-objective`}
            value={objective}
            onChange={(event) => setObjective(event.target.value)}
            rows={4}
            required
            placeholder="Sign in with the configured identity, open the reports area, apply period 2026-08 with minimum margin 19, and confirm the displayed result."
          />
        </Field>

        <Field
          label="Starting URL"
          htmlFor={`${uid}-start`}
          hint="Only the entry point. Paths and controls must be discovered from the rendered page."
        >
          <TextInput
            id={`${uid}-start`}
            value={startUrl}
            onChange={(event) => setStartUrl(event.target.value)}
            placeholder="http://127.0.0.1:8080/"
          />
        </Field>

        <div className="form-grid two">
          <Field
            label="Identity"
            htmlFor={`${uid}-identity`}
            hint="Each identity gets its own browser context."
          >
            <SelectInput
              id={`${uid}-identity`}
              value={identityId}
              onChange={(event) => setIdentityId(event.target.value)}
            >
              <option value="">No identity</option>
              {(identities.data ?? []).map((identity) => (
                <option key={identity.id} value={identity.id}>
                  {identity.label} · {identity.role_label}
                </option>
              ))}
            </SelectInput>
          </Field>
          <Field label="Max model steps" htmlFor={`${uid}-steps`} hint="Hard stop, independent of the model.">
            <SelectInput
              id={`${uid}-steps`}
              value={maxSteps}
              onChange={(event) => setMaxSteps(event.target.value)}
            >
              {STEP_OPTIONS.map((steps) => (
                <option key={steps} value={steps}>
                  {steps}
                </option>
              ))}
            </SelectInput>
          </Field>
        </div>

        <Field
          label="Context token budget"
          htmlFor={`${uid}-tokens`}
          hint="Limits the context package. The provider still enforces its own context window."
        >
          <TextInput
            id={`${uid}-tokens`}
            value={tokenBudget}
            onChange={(event) => setTokenBudget(event.target.value)}
            inputMode="numeric"
          />
        </Field>

        {error ? (
          <p className="form-error" role="alert">
            {error}
          </p>
        ) : null}
      </form>
    </Dialog>
  );
}
