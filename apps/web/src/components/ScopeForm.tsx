import { useId, useState } from 'react';
import type { ActionClass, ScopePolicy } from '../api/types';
import {
  ACTION_CLASSES,
  ACTION_CLASS_LABELS,
  joinLines,
  joinPorts,
  splitCsv,
  splitLines,
  splitPorts,
} from '../lib/scope';
import { Checkbox, TextInput, Toggle } from './Form';

interface Draft {
  includeHosts: string;
  excludeHosts: string;
  includePaths: string;
  excludePaths: string;
  schemes: string;
  ports: string;
  allowPrivateNetwork: boolean;
  actionClasses: ActionClass[];
  retainRaw: boolean;
  maxDurationSeconds: string;
  maxRequests: string;
  maxBrowserSteps: string;
  maxModelCalls: string;
  maxCostUsd: string;
  maxArtifactBytes: string;
  maxConcurrency: string;
}

function draftFrom(policy: ScopePolicy): Draft {
  return {
    includeHosts: joinLines(policy.include_hosts),
    excludeHosts: joinLines(policy.exclude_hosts),
    includePaths: joinLines(policy.include_paths),
    excludePaths: joinLines(policy.exclude_paths),
    schemes: policy.allowed_schemes.join(', '),
    ports: joinPorts(policy.allowed_ports),
    allowPrivateNetwork: policy.allow_private_network,
    actionClasses: [...policy.action_classes],
    retainRaw: policy.retain_raw,
    maxDurationSeconds: String(policy.limits.max_duration_seconds),
    maxRequests: String(policy.limits.max_requests),
    maxBrowserSteps: String(policy.limits.max_browser_steps),
    maxModelCalls: String(policy.limits.max_model_calls),
    maxCostUsd: String(policy.limits.max_cost_micros / 1_000_000),
    maxArtifactBytes: String(policy.limits.max_artifact_bytes),
    maxConcurrency: String(policy.limits.max_concurrency_per_origin),
  };
}

function numberValue(value: string): number {
  const parsed = Number(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function buildPolicy(draft: Draft, base: ScopePolicy): ScopePolicy {
  return {
    include_hosts: splitLines(draft.includeHosts),
    exclude_hosts: splitLines(draft.excludeHosts),
    include_paths: splitLines(draft.includePaths),
    exclude_paths: splitLines(draft.excludePaths),
    allowed_schemes: splitCsv(draft.schemes),
    allowed_ports: splitPorts(draft.ports),
    allow_private_network: draft.allowPrivateNetwork,
    window: base.window,
    action_classes: ACTION_CLASSES.filter((actionClass) =>
      draft.actionClasses.includes(actionClass),
    ),
    navigation_dependencies: base.navigation_dependencies,
    test_targets: base.test_targets,
    limits: {
      max_duration_seconds: numberValue(draft.maxDurationSeconds),
      max_requests: numberValue(draft.maxRequests),
      max_browser_steps: numberValue(draft.maxBrowserSteps),
      max_model_calls: numberValue(draft.maxModelCalls),
      max_cost_micros: Math.round(numberValue(draft.maxCostUsd) * 1_000_000),
      max_artifact_bytes: numberValue(draft.maxArtifactBytes),
      max_concurrency_per_origin: numberValue(draft.maxConcurrency),
    },
    retain_raw: draft.retainRaw,
  };
}

export function ScopeForm({
  initialPolicy,
  onChange,
  disabled = false,
}: {
  initialPolicy: ScopePolicy;
  onChange: (policy: ScopePolicy) => void;
  disabled?: boolean;
}) {
  const [draft, setDraft] = useState<Draft>(() => draftFrom(initialPolicy));
  const uid = useId();
  const baseRef = useState(initialPolicy)[0];

  const update = (patch: Partial<Draft>) => {
    const next = { ...draft, ...patch };
    setDraft(next);
    onChange(buildPolicy(next, baseRef));
  };

  const toggleActionClass = (actionClass: ActionClass) => {
    update({
      actionClasses: draft.actionClasses.includes(actionClass)
        ? draft.actionClasses.filter((item) => item !== actionClass)
        : [...draft.actionClasses, actionClass],
    });
  };

  return (
    <div className="scope-form">
      <div className="form-section">
        <h3 className="form-section-title">Hosts</h3>
        <div className="form-grid two">
          <div className="field">
            <label htmlFor={`${uid}-include-hosts`}>Include hosts</label>
            <textarea
              id={`${uid}-include-hosts`}
              className="input textarea"
              value={draft.includeHosts}
              onChange={(event) => update({ includeHosts: event.target.value })}
              placeholder={'app.example.com\napi.example.com'}
              rows={3}
              disabled={disabled}
              spellCheck={false}
            />
            <p className="field-hint">One host per line. Required.</p>
          </div>
          <div className="field">
            <label htmlFor={`${uid}-exclude-hosts`}>Exclude hosts</label>
            <textarea
              id={`${uid}-exclude-hosts`}
              className="input textarea"
              value={draft.excludeHosts}
              onChange={(event) => update({ excludeHosts: event.target.value })}
              placeholder="host:port or hostname"
              rows={3}
              disabled={disabled}
              spellCheck={false}
            />
            <p className="field-hint">Exclusions prevail over every include rule.</p>
          </div>
        </div>
      </div>

      <div className="form-section">
        <h3 className="form-section-title">Paths</h3>
        <div className="form-grid two">
          <div className="field">
            <label htmlFor={`${uid}-include-paths`}>Include paths</label>
            <textarea
              id={`${uid}-include-paths`}
              className="input textarea"
              value={draft.includePaths}
              onChange={(event) => update({ includePaths: event.target.value })}
              placeholder={'/\n/api'}
              rows={2}
              disabled={disabled}
              spellCheck={false}
            />
            <p className="field-hint">One prefix per line, each starting with /.</p>
          </div>
          <div className="field">
            <label htmlFor={`${uid}-exclude-paths`}>Exclude paths</label>
            <textarea
              id={`${uid}-exclude-paths`}
              className="input textarea"
              value={draft.excludePaths}
              onChange={(event) => update({ excludePaths: event.target.value })}
              placeholder="/admin/forbidden"
              rows={2}
              disabled={disabled}
              spellCheck={false}
            />
            <p className="field-hint">One prefix per line.</p>
          </div>
        </div>
      </div>

      <div className="form-section">
        <h3 className="form-section-title">Transport</h3>
        <div className="form-grid three">
          <div className="field">
            <label htmlFor={`${uid}-schemes`}>Schemes</label>
            <TextInput
              id={`${uid}-schemes`}
              value={draft.schemes}
              onChange={(event) => update({ schemes: event.target.value })}
              placeholder="https, http"
              disabled={disabled}
              spellCheck={false}
            />
          </div>
          <div className="field">
            <label htmlFor={`${uid}-ports`}>Allowed ports</label>
            <TextInput
              id={`${uid}-ports`}
              value={draft.ports}
              onChange={(event) => update({ ports: event.target.value })}
              placeholder="443, 8443"
              inputMode="numeric"
              disabled={disabled}
            />
          </div>
          <div className="field">
            <span className="field-label">Private network</span>
            <Toggle
              label="Allow private addresses"
              hint="Loopback, RFC1918 and local hostnames."
              checked={draft.allowPrivateNetwork}
              onChange={(event) => update({ allowPrivateNetwork: event.target.checked })}
              disabled={disabled}
            />
          </div>
        </div>
      </div>

      <div className="form-section">
        <h3 className="form-section-title">Action classes</h3>
        <div className="check-grid">
          {ACTION_CLASSES.map((actionClass) => (
            <Checkbox
              key={actionClass}
              label={ACTION_CLASS_LABELS[actionClass]}
              checked={draft.actionClasses.includes(actionClass)}
              onChange={() => toggleActionClass(actionClass)}
              disabled={disabled}
            />
          ))}
        </div>
        <p className="field-hint">
          Destructive and external-effect classes are never inferred; they must be selected here.
        </p>
      </div>

      <div className="form-section">
        <h3 className="form-section-title">Limits</h3>
        <div className="form-grid four">
          <div className="field">
            <label htmlFor={`${uid}-duration`}>Duration (seconds)</label>
            <TextInput
              id={`${uid}-duration`}
              type="number"
              min={1}
              value={draft.maxDurationSeconds}
              onChange={(event) => update({ maxDurationSeconds: event.target.value })}
              disabled={disabled}
            />
          </div>
          <div className="field">
            <label htmlFor={`${uid}-requests`}>Requests</label>
            <TextInput
              id={`${uid}-requests`}
              type="number"
              min={1}
              value={draft.maxRequests}
              onChange={(event) => update({ maxRequests: event.target.value })}
              disabled={disabled}
            />
          </div>
          <div className="field">
            <label htmlFor={`${uid}-browser`}>Browser steps</label>
            <TextInput
              id={`${uid}-browser`}
              type="number"
              min={0}
              value={draft.maxBrowserSteps}
              onChange={(event) => update({ maxBrowserSteps: event.target.value })}
              disabled={disabled}
            />
          </div>
          <div className="field">
            <label htmlFor={`${uid}-model`}>Model calls</label>
            <TextInput
              id={`${uid}-model`}
              type="number"
              min={0}
              value={draft.maxModelCalls}
              onChange={(event) => update({ maxModelCalls: event.target.value })}
              disabled={disabled}
            />
          </div>
          <div className="field">
            <label htmlFor={`${uid}-cost`}>Cost (USD)</label>
            <TextInput
              id={`${uid}-cost`}
              type="number"
              min={0}
              step="0.01"
              value={draft.maxCostUsd}
              onChange={(event) => update({ maxCostUsd: event.target.value })}
              disabled={disabled}
            />
          </div>
          <div className="field">
            <label htmlFor={`${uid}-artifacts`}>Artifacts (bytes)</label>
            <TextInput
              id={`${uid}-artifacts`}
              type="number"
              min={0}
              value={draft.maxArtifactBytes}
              onChange={(event) => update({ maxArtifactBytes: event.target.value })}
              disabled={disabled}
            />
          </div>
          <div className="field">
            <label htmlFor={`${uid}-concurrency`}>Concurrency per origin</label>
            <TextInput
              id={`${uid}-concurrency`}
              type="number"
              min={1}
              value={draft.maxConcurrency}
              onChange={(event) => update({ maxConcurrency: event.target.value })}
              disabled={disabled}
            />
          </div>
          <div className="field">
            <span className="field-label">Raw retention</span>
            <Toggle
              label="Retain raw evidence"
              hint="Subject to adapter capability."
              checked={draft.retainRaw}
              onChange={(event) => update({ retainRaw: event.target.checked })}
              disabled={disabled}
            />
          </div>
        </div>
      </div>
    </div>
  );
}
