import { useEffect, useId, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { agentTurn, AGENT_URL, errorMessage, type AgentHealth } from '../api/client';
import { useIdentities } from '../api/queries';
import { Dialog } from './Dialog';
import { Field, SelectInput, TextInput, Textarea } from './Form';
import { useToast } from './Toast';

const STEP_OPTIONS = [2, 4, 6];

export function AgentTurnDialog({
  open,
  onClose,
  assessmentId,
}: {
  open: boolean;
  onClose: () => void;
  assessmentId: string;
}) {
  const identities = useIdentities(assessmentId);
  const navigate = useNavigate();
  const { notify } = useToast();
  const formId = useId();
  const uid = useId();
  const [objective, setObjective] = useState('');
  const [identityId, setIdentityId] = useState('');
  const [maxSteps, setMaxSteps] = useState('4');
  const [tokenBudget, setTokenBudget] = useState('4000');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [health, setHealth] = useState<AgentHealth | null>(null);
  const [healthError, setHealthError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setObjective('');
    setIdentityId('');
    setMaxSteps('4');
    setTokenBudget('4000');
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
  }, [open]);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!objective.trim()) {
      setError('Describe the objective for this turn.');
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const response = await agentTurn({
        assessment_id: assessmentId,
        objective: objective.trim(),
        identity_id: identityId || null,
        max_steps: Number(maxSteps) || 4,
        token_budget: Number(tokenBudget) || 4000,
      });
      notify('Agent turn finished. Review the transcript and evidence.', 'success');
      onClose();
      navigate(`/assessments/${assessmentId}/runs/${response.run.id}`);
    } catch (cause) {
      setError(errorMessage(cause));
      setBusy(false);
    }
  };

  const unavailable = Boolean(healthError) || health?.status === 'missing_model_key';

  return (
    <Dialog
      open={open}
      onClose={busy ? () => undefined : onClose}
      title="Ask the agent"
      description="A bounded turn with typed tools. The agent can only call operations the platform mediates and can never open a shell."
      size="md"
      footer={
        <>
          <button type="button" className="btn" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button
            type="submit"
            form={formId}
            className="btn btn-primary"
            disabled={busy || unavailable || !objective.trim()}
          >
            {busy ? 'Agent is working…' : 'Start turn'}
          </button>
        </>
      }
    >
      <form id={formId} onSubmit={submit} className="form-stack">
        <div className="notice" data-tone={unavailable ? 'amber' : undefined}>
          {health ? (
            <>
              <span className="mono">agent · {health.model}</span>
              <br />
              <span>
                Ready at <span className="mono">{AGENT_URL}</span>. Up to {health.max_steps} model
                steps per turn by default.
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

        <Field
          label="Objective"
          htmlFor={`${uid}-objective`}
          hint="What should this turn establish or continue? Be specific and evidence-oriented."
        >
          <Textarea
            id={`${uid}-objective`}
            value={objective}
            onChange={(event) => setObjective(event.target.value)}
            rows={4}
            required
            placeholder="Inspect the candidate endpoints, observe the most relevant one, and leave a checkpoint for the next session."
          />
        </Field>

        <div className="form-grid two">
          <Field label="Identity" htmlFor={`${uid}-identity`} hint="Optional. Isolates the session context.">
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

        {busy ? (
          <p className="field-hint">
            The turn may take up to two minutes. It is recorded as a run; closing this panel does
            not cancel it.
          </p>
        ) : null}

        {error ? (
          <p className="form-error" role="alert">
            {error}
          </p>
        ) : null}
      </form>
    </Dialog>
  );
}
