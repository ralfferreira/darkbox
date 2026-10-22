import { useEffect, useId, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { errorMessage } from '../api/client';
import { useCapabilities, useCreateRun, useIdentities } from '../api/queries';
import { Dialog } from './Dialog';
import { Field, SelectInput, TextInput } from './Form';

export function IdentitySessionDialog({
  open,
  onClose,
  assessmentId,
  identityId: initialIdentity,
}: {
  open: boolean;
  onClose: () => void;
  assessmentId: string;
  identityId?: string;
}) {
  const identities = useIdentities(assessmentId);
  const capabilities = useCapabilities();
  const start = useCreateRun(assessmentId);
  const navigate = useNavigate();
  const uid = useId();
  const [identityId, setIdentityId] = useState('');
  const [tenant, setTenant] = useState('');
  const [baseUrl, setBaseUrl] = useState('');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    setIdentityId(initialIdentity ?? '');
    setTenant('');
    setBaseUrl(capabilities.data?.lab_url ?? 'http://127.0.0.1:8080');
    setError(null);
  }, [open, initialIdentity, capabilities.data?.lab_url]);

  const identity = (identities.data ?? []).find((item) => item.id === identityId);
  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    if (!identityId || !baseUrl.trim()) {
      setError('Choose an identity and a starting URL.');
      return;
    }
    try {
      const run = await start.mutateAsync({
        assessment_id: assessmentId,
        kind: 'session',
        idempotency_key: crypto.randomUUID(),
        identity_id: identityId,
        budget_override: null,
        tenant: tenant || identity?.tenant_keys[0] || null,
        base_url: baseUrl.trim(),
        login_path: '/login',
        targets: [],
        params: {},
        endpoint_ids: [],
        identity_ids: [],
        checks: [],
        flow_id: null,
        procedure_key: null,
        note: 'operator-started identity session',
      });
      onClose();
      navigate(`/assessments/${assessmentId}/runs/${run.id}`);
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Start identity session"
      description="Creates one isolated browser context for this identity. The login secret is resolved from the environment, never from this form."
      size="sm"
      footer={
        <>
          <button type="button" className="btn" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            form={`${uid}-form`}
            className="btn btn-primary"
            disabled={start.isPending || !identityId}
          >
            {start.isPending ? 'Starting…' : 'Start session'}
          </button>
        </>
      }
    >
      <form id={`${uid}-form`} onSubmit={submit} className="form-stack">
        <Field label="Identity" htmlFor={`${uid}-identity`}>
          <SelectInput
            id={`${uid}-identity`}
            value={identityId}
            onChange={(event) => {
              setIdentityId(event.target.value);
              const next = (identities.data ?? []).find((item) => item.id === event.target.value);
              setTenant(next?.tenant_keys[0] ?? '');
            }}
          >
            <option value="">Choose an identity</option>
            {(identities.data ?? []).map((item) => (
              <option key={item.id} value={item.id}>
                {item.label} · {item.role_label}
              </option>
            ))}
          </SelectInput>
        </Field>
        <div className="form-grid two">
          <Field label="Tenant" htmlFor={`${uid}-tenant`}>
            <SelectInput
              id={`${uid}-tenant`}
              value={tenant}
              onChange={(event) => setTenant(event.target.value)}
            >
              {(identity?.tenant_keys ?? []).map((key) => (
                <option key={key} value={key}>
                  {key}
                </option>
              ))}
            </SelectInput>
          </Field>
          <Field label="Starting URL" htmlFor={`${uid}-url`}>
            <TextInput
              id={`${uid}-url`}
              value={baseUrl}
              onChange={(event) => setBaseUrl(event.target.value)}
            />
          </Field>
        </div>
        {error ? (
          <p className="form-error" role="alert">
            {error}
          </p>
        ) : null}
      </form>
    </Dialog>
  );
}
