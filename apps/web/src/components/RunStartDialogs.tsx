import { useEffect, useId, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import {
  useCreateRun,
  useEligibility,
  useFlows,
  useIdentities,
  useInventory,
} from '../api/queries';
import { errorMessage } from '../api/client';
import type { FlowDto } from '../api/types';
import { newIdempotencyKey } from '../lib/id';
import { Dialog } from './Dialog';
import { Field, SelectInput, Textarea } from './Form';
import { Status } from './Status';
import { useToast } from './Toast';

export function FlowRunDialog({
  open,
  onClose,
  assessmentId,
  flow,
}: {
  open: boolean;
  onClose: () => void;
  assessmentId: string;
  flow?: FlowDto;
}) {
  const flows = useFlows(assessmentId);
  const mutation = useCreateRun(assessmentId);
  const navigate = useNavigate();
  const { notify } = useToast();
  const formId = useId();
  const uid = useId();
  const [flowId, setFlowId] = useState(flow?.id ?? '');
  const [note, setNote] = useState('');

  useEffect(() => {
    if (open) {
      setFlowId(flow?.id ?? '');
      setNote('');
      mutation.reset();
    }
    // Reset only when the dialog opens.
  }, [open, flow?.id]);

  const available = flows.data ?? [];
  const selected = flow ?? available.find((item) => item.id === flowId);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!selected) return;
    mutation.mutate(
      {
        assessment_id: assessmentId,
        kind: 'flow_reproduction',
        idempotency_key: newIdempotencyKey('flow'),
        flow_id: selected.id,
        procedure_key: null,
        endpoint_ids: [],
        identity_id: null,
        budget_override: null,
        note: note.trim(),
      },
      {
        onSuccess: (run) => {
          notify('Flow reproduction queued.', 'success');
          onClose();
          navigate(`/assessments/${assessmentId}/runs/${run.id}`);
        },
      },
    );
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Reproduce flow"
      description="Queues a run against the flow's recorded steps and scope."
      footer={
        <>
          <button type="button" className="btn" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            form={formId}
            className="btn btn-primary"
            disabled={!selected || mutation.isPending}
          >
            {mutation.isPending ? 'Queueing…' : 'Reproduce'}
          </button>
        </>
      }
    >
      <form id={formId} onSubmit={submit} className="form-stack">
        {flow ? (
          <div className="selected-flow">
            <div className="selected-flow-name">{flow.name}</div>
            <div className="selected-flow-meta">
              <Status value={flow.state} />
              <span className="mono">v{flow.version}</span>
              <span className="mono">{flow.steps.length} steps</span>
            </div>
          </div>
        ) : (
          <Field label="Flow" htmlFor={`${uid}-flow`}>
            <SelectInput
              id={`${uid}-flow`}
              value={flowId}
              onChange={(event) => setFlowId(event.target.value)}
              required
            >
              <option value="">Select a flow</option>
              {available.map((item) => (
                <option key={item.id} value={item.id}>
                  {item.name} · {item.state}
                </option>
              ))}
            </SelectInput>
            {flows.isError ? (
              <p className="field-hint is-error">{errorMessage(flows.error)}</p>
            ) : null}
            {available.length === 0 && !flows.isLoading ? (
              <p className="field-hint">No flows recorded for this assessment.</p>
            ) : null}
          </Field>
        )}
        <Field label="Note" htmlFor={`${uid}-flow-note`} hint="Optional. Attached to the run.">
          <Textarea
            id={`${uid}-flow-note`}
            value={note}
            onChange={(event) => setNote(event.target.value)}
            rows={2}
            placeholder="Why this reproduction now."
          />
        </Field>
        {mutation.isError ? (
          <p className="form-error" role="alert">
            {errorMessage(mutation.error)}
          </p>
        ) : null}
      </form>
    </Dialog>
  );
}

export function ProcedureRunDialog({
  open,
  onClose,
  assessmentId,
  procedureKey,
}: {
  open: boolean;
  onClose: () => void;
  assessmentId: string;
  procedureKey?: string;
}) {
  const eligibility = useEligibility(assessmentId);
  const inventory = useInventory(assessmentId, { state: 'observed', limit: 200 });
  const flows = useFlows(assessmentId);
  const identities = useIdentities(assessmentId);
  const mutation = useCreateRun(assessmentId);
  const navigate = useNavigate();
  const { notify } = useToast();
  const formId = useId();
  const uid = useId();
  const [selectedKey, setSelectedKey] = useState(procedureKey ?? '');
  const [note, setNote] = useState('');
  const [targetEndpoint, setTargetEndpoint] = useState('');
  const [targetFlow, setTargetFlow] = useState('');
  const [identityA, setIdentityA] = useState('');
  const [identityB, setIdentityB] = useState('');

  useEffect(() => {
    if (open) {
      setSelectedKey(procedureKey ?? '');
      setNote('');
      setTargetEndpoint('');
      setTargetFlow('');
      setIdentityA('');
      setIdentityB('');
      mutation.reset();
    }
    // Reset only when the dialog opens.
  }, [open, procedureKey]);

  const rows = eligibility.data ?? [];
  const selected = rows.find((item) => item.procedure_key === selectedKey);
  const procedureId = selected?.procedure_key.split('@')[0] ?? '';
  const needsEndpoint = procedureId === 'http.configuration_inventory';
  const needsFlow = procedureId === 'flow.reproduction';
  const needsIdentities = procedureId === 'role.visibility_comparison';
  const targetReady = needsEndpoint
    ? Boolean(targetEndpoint)
    : needsFlow
      ? Boolean(targetFlow)
      : needsIdentities
        ? Boolean(identityA && identityB && identityA !== identityB)
        : true;
  const canRun = Boolean(selected?.eligible) && targetReady;

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!selected || !selected.eligible) return;
    mutation.mutate(
      {
        assessment_id: assessmentId,
        kind: 'procedure',
        idempotency_key: newIdempotencyKey('procedure'),
        flow_id: needsFlow ? targetFlow : null,
        procedure_key: selected.procedure_key,
        endpoint_ids: needsEndpoint ? [targetEndpoint] : [],
        identity_id: null,
        identity_ids: needsIdentities ? [identityA, identityB] : [],
        budget_override: null,
        note: note.trim(),
      },
      {
        onSuccess: (run) => {
          notify('Procedure run queued.', 'success');
          onClose();
          navigate(`/assessments/${assessmentId}/runs/${run.id}`);
        },
      },
    );
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Run procedure"
      description="Eligibility is evaluated against the effective scope and available capabilities."
      footer={
        <>
          <button type="button" className="btn" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            form={formId}
            className="btn btn-primary"
            disabled={!canRun || mutation.isPending}
          >
            {mutation.isPending ? 'Queueing…' : 'Run procedure'}
          </button>
        </>
      }
    >
      <form id={formId} onSubmit={submit} className="form-stack">
        <Field label="Procedure" htmlFor={`${uid}-procedure`}>
          <SelectInput
            id={`${uid}-procedure`}
            value={selectedKey}
            onChange={(event) => setSelectedKey(event.target.value)}
            required
          >
            <option value="">Select an eligible procedure</option>
            {rows.map((item) => (
              <option key={item.procedure_key} value={item.procedure_key} disabled={!item.eligible}>
                {item.procedure_name} · {item.eligible ? `${item.target_count} targets` : 'blocked'}
              </option>
            ))}
          </SelectInput>
          {eligibility.isError ? (
            <p className="field-hint is-error">{errorMessage(eligibility.error)}</p>
          ) : null}
          {rows.length === 0 && !eligibility.isLoading ? (
            <p className="field-hint">No procedures are available.</p>
          ) : null}
        </Field>
        {needsEndpoint ? (
          <Field
            label="Endpoint"
            htmlFor={`${uid}-procedure-endpoint`}
            hint="Configuration inventory records headers for one observed endpoint."
          >
            <SelectInput
              id={`${uid}-procedure-endpoint`}
              value={targetEndpoint}
              onChange={(event) => setTargetEndpoint(event.target.value)}
              required
            >
              <option value="">Select an observed endpoint</option>
              {(inventory.data?.endpoints ?? []).map((endpoint) => (
                <option key={endpoint.id} value={endpoint.id}>
                  {endpoint.method} {endpoint.host}
                  {endpoint.path}
                </option>
              ))}
            </SelectInput>
          </Field>
        ) : null}
        {needsFlow ? (
          <Field
            label="Flow"
            htmlFor={`${uid}-procedure-flow`}
            hint="The recorded identity and tenant are reused."
          >
            <SelectInput
              id={`${uid}-procedure-flow`}
              value={targetFlow}
              onChange={(event) => setTargetFlow(event.target.value)}
              required
            >
              <option value="">Select a flow</option>
              {(flows.data ?? []).map((flow) => (
                <option key={flow.id} value={flow.id}>
                  {flow.name} · {flow.state}
                </option>
              ))}
            </SelectInput>
          </Field>
        ) : null}
        {needsIdentities ? (
          <div className="form-grid two">
            <Field label="First identity" htmlFor={`${uid}-identity-a`}>
              <SelectInput
                id={`${uid}-identity-a`}
                value={identityA}
                onChange={(event) => setIdentityA(event.target.value)}
                required
              >
                <option value="">Select identity</option>
                {(identities.data ?? []).map((identity) => (
                  <option key={identity.id} value={identity.id}>
                    {identity.label} · {identity.role_label}
                  </option>
                ))}
              </SelectInput>
            </Field>
            <Field
              label="Second identity"
              htmlFor={`${uid}-identity-b`}
              hint="Both sessions run in isolated contexts."
            >
              <SelectInput
                id={`${uid}-identity-b`}
                value={identityB}
                onChange={(event) => setIdentityB(event.target.value)}
                required
              >
                <option value="">Select identity</option>
                {(identities.data ?? []).map((identity) => (
                  <option key={identity.id} value={identity.id} disabled={identity.id === identityA}>
                    {identity.label} · {identity.role_label}
                  </option>
                ))}
              </SelectInput>
            </Field>
          </div>
        ) : null}
        {selected && !selected.eligible ? (
          <div className="blocker-list">
            {selected.blockers.map((blocker) => (
              <p key={`${blocker.code}-${blocker.detail}`} className="blocker">
                <span className="mono">{blocker.code}</span>
                {blocker.detail}
              </p>
            ))}
          </div>
        ) : null}
        <Field label="Note" htmlFor={`${uid}-procedure-note`} hint="Optional. Attached to the run.">
          <Textarea
            id={`${uid}-procedure-note`}
            value={note}
            onChange={(event) => setNote(event.target.value)}
            rows={2}
          />
        </Field>
        {mutation.isError ? (
          <p className="form-error" role="alert">
            {errorMessage(mutation.error)}
          </p>
        ) : null}
      </form>
    </Dialog>
  );
}
