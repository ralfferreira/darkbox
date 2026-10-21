import { useEffect, useId, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useCapabilities, useCreateRun, useInventory } from '../api/queries';
import { errorMessage } from '../api/client';
import type { Capability, RunKind } from '../api/types';
import { newIdempotencyKey } from '../lib/id';
import { Dialog } from './Dialog';
import { Field, SelectInput, TextInput, Textarea } from './Form';
import { Status } from './Status';
import { useToast } from './Toast';

type ToolMode = 'network_scan' | 'firecrawl_map' | 'firecrawl_scrape' | 'burp_import';

const TOOLS: Array<{ mode: ToolMode; label: string; capability: string }> = [
  { mode: 'network_scan', label: 'Network scan (Nmap)', capability: 'network.nmap' },
  { mode: 'firecrawl_map', label: 'Firecrawl map', capability: 'web.firecrawl' },
  { mode: 'firecrawl_scrape', label: 'Firecrawl browser scrape', capability: 'web.firecrawl' },
  { mode: 'burp_import', label: 'Import Burp history', capability: 'import.burp' },
];

export function IntegrationRunDialog({
  open,
  onClose,
  assessmentId,
}: {
  open: boolean;
  onClose: () => void;
  assessmentId: string;
}) {
  const capabilities = useCapabilities();
  const inventory = useInventory(assessmentId, { limit: 500 });
  const mutation = useCreateRun(assessmentId);
  const navigate = useNavigate();
  const { notify } = useToast();
  const formId = useId();
  const uid = useId();
  const [mode, setMode] = useState<ToolMode>('network_scan');
  const [targets, setTargets] = useState('');
  const [url, setUrl] = useState('');
  const [ports, setPorts] = useState('');
  const [limit, setLimit] = useState('100');
  const [search, setSearch] = useState('');
  const [objective, setObjective] = useState('');
  const [validation, setValidation] = useState<string | null>(null);

  const tool = TOOLS.find((item) => item.mode === mode) ?? TOOLS[0];
  const capability: Capability | undefined = capabilities.data?.adapters.find(
    (adapter) => adapter.id === tool.capability,
  );

  useEffect(() => {
    if (!open) return;
    setMode('network_scan');
    setTargets('');
    setUrl('');
    setPorts('');
    setLimit('100');
    setSearch('');
    setObjective('');
    setValidation(null);
    mutation.reset();
    // Reset only when the dialog opens.
  }, [open]);

  useEffect(() => {
    if (!open || mode !== 'network_scan' || targets.trim()) return;
    const hosts = [
      ...new Set((inventory.data?.endpoints ?? []).map((endpoint) => endpoint.host)),
    ];
    if (hosts.length > 0) setTargets(hosts.join('\n'));
  }, [open, mode, inventory.data, targets]);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!capability || capability.state === 'unavailable') {
      setValidation('This adapter is not available on this installation.');
      return;
    }
    const parsedPorts = ports
      .split(',')
      .map((value) => Number(value.trim()))
      .filter((value) => Number.isInteger(value) && value > 0 && value <= 65_535);
    const targetList = targets
      .split('\n')
      .map((value) => value.trim())
      .filter(Boolean);
    let bodyTargets: string[] = [];
    let params: Record<string, unknown> = {};
    if (mode === 'network_scan') {
      bodyTargets = targetList;
      if (bodyTargets.length === 0) {
        setValidation('Add at least one host.');
        return;
      }
      params = { ports: parsedPorts };
    } else if (mode === 'firecrawl_map') {
      if (!url.trim()) {
        setValidation('Add the URL to map.');
        return;
      }
      bodyTargets = [url.trim()];
      params = { limit: Number(limit) || 100, search: search.trim() || null };
    } else if (mode === 'firecrawl_scrape') {
      if (!url.trim()) {
        setValidation('Add at least one URL.');
        return;
      }
      bodyTargets = url
        .split('\n')
        .map((value) => value.trim())
        .filter(Boolean);
      params = { max_urls: 5 };
    } else {
      params = { limit: Number(limit) || 100 };
    }
    setValidation(null);
    const kind: RunKind = mode;
    mutation.mutate(
      {
        assessment_id: assessmentId,
        kind,
        idempotency_key: newIdempotencyKey(mode),
        flow_id: null,
        procedure_key: null,
        endpoint_ids: [],
        identity_id: null,
        budget_override: null,
        note: objective.trim(),
        targets: bodyTargets,
        params,
      },
      {
        onSuccess: (run) => {
          notify(`${tool.label} queued.`, 'success');
          onClose();
          navigate(`/assessments/${assessmentId}/runs/${run.id}`);
        },
      },
    );
  };

  const unavailable = !capability || capability.state === 'unavailable';
  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Run integration"
      description="Optional tooling with explicit capability checks. Nmap uses only policy-approved ports."
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
            disabled={unavailable || mutation.isPending}
          >
            {mutation.isPending ? 'Queueing…' : 'Queue run'}
          </button>
        </>
      }
    >
      <form id={formId} onSubmit={submit} className="form-stack">
        <Field label="Tool" htmlFor={`${uid}-tool`}>
          <SelectInput
            id={`${uid}-tool`}
            value={mode}
            onChange={(event) => setMode(event.target.value as ToolMode)}
          >
            {TOOLS.map((item) => (
              <option key={item.mode} value={item.mode}>
                {item.label}
              </option>
            ))}
          </SelectInput>
        </Field>

        {capability ? (
          <div className="notice" data-tone={unavailable ? 'amber' : undefined}>
            <Status value={capability.state} />
            <span>{capability.detail}</span>
          </div>
        ) : null}

        {mode === 'network_scan' ? (
          <>
            <Field
              label="Hosts"
              htmlFor={`${uid}-targets`}
              hint="One host per line. These are the only hosts passed to Nmap."
            >
              <Textarea
                id={`${uid}-targets`}
                value={targets}
                onChange={(event) => setTargets(event.target.value)}
                rows={3}
              />
            </Field>
            <Field
              label="Ports"
              htmlFor={`${uid}-ports`}
              hint="Comma separated. Leave empty to use the policy port list; ports outside it are dropped."
            >
              <TextInput
                id={`${uid}-ports`}
                value={ports}
                onChange={(event) => setPorts(event.target.value)}
                placeholder="8080, 8443"
                inputMode="numeric"
              />
            </Field>
          </>
        ) : null}

        {mode === 'firecrawl_map' ? (
          <>
            <Field label="Site URL" htmlFor={`${uid}-url`} hint="In-scope public URL.">
              <TextInput
                id={`${uid}-url`}
                value={url}
                onChange={(event) => setUrl(event.target.value)}
                placeholder="https://app.example.com/"
              />
            </Field>
            <div className="form-grid two">
              <Field label="Limit" htmlFor={`${uid}-limit`}>
                <TextInput
                  id={`${uid}-limit`}
                  value={limit}
                  onChange={(event) => setLimit(event.target.value)}
                  inputMode="numeric"
                />
              </Field>
              <Field label="Search filter" htmlFor={`${uid}-search`} hint="Optional.">
                <TextInput
                  id={`${uid}-search`}
                  value={search}
                  onChange={(event) => setSearch(event.target.value)}
                  placeholder="admin, api"
                />
              </Field>
            </div>
          </>
        ) : null}

        {mode === 'firecrawl_scrape' ? (
          <Field
            label="URLs"
            htmlFor={`${uid}-url`}
            hint="One per line, up to five. Firecrawl renders the page and returns Markdown as evidence."
          >
            <Textarea
              id={`${uid}-url`}
              value={url}
              onChange={(event) => setUrl(event.target.value)}
              rows={3}
              placeholder={'https://app.example.com/\nhttps://app.example.com/pricing'}
            />
          </Field>
        ) : null}

        {mode === 'burp_import' ? (
          <Field
            label="History limit"
            htmlFor={`${uid}-limit`}
            hint="Imports proxy history only. It does not create new target traffic."
          >
            <TextInput
              id={`${uid}-limit`}
              value={limit}
              onChange={(event) => setLimit(event.target.value)}
              inputMode="numeric"
            />
          </Field>
        ) : null}

        {mode !== 'burp_import' ? (
          <Field
            label="Objective"
            htmlFor={`${uid}-objective`}
            hint="Optional. Kept in the run record and used as scrape guidance."
          >
            <Textarea
              id={`${uid}-objective`}
              value={objective}
              onChange={(event) => setObjective(event.target.value)}
              rows={2}
            />
          </Field>
        ) : null}

        {validation || mutation.isError ? (
          <p className="form-error" role="alert">
            {validation ?? errorMessage(mutation.error)}
          </p>
        ) : null}
      </form>
    </Dialog>
  );
}
