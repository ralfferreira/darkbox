import type { ReactNode } from 'react';
import type { ScopePolicy } from '../api/types';
import { formatBytes, formatCostMicros, formatDateTime, formatDuration, formatNumber } from '../lib/format';
import { ACTION_CLASS_LABELS } from '../lib/scope';

function Chips({
  values,
  exclude = false,
  empty = 'none',
}: {
  values: string[];
  exclude?: boolean;
  empty?: string;
}) {
  if (values.length === 0) return <span className="muted">{empty}</span>;
  return (
    <span className="chip-list">
      {values.map((value) => (
        <span key={value} className={`chip mono${exclude ? ' is-exclude' : ''}`}>
          {exclude ? <span aria-hidden="true">− </span> : null}
          {value}
        </span>
      ))}
    </span>
  );
}

function Row({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="policy-row">
      <span className="policy-key mono">{label}</span>
      <div className="policy-value">{children}</div>
    </div>
  );
}

export function PolicyView({ policy }: { policy: ScopePolicy }) {
  const limits = policy.limits;
  return (
    <div className="policy-view">
      <Row label="Include hosts">
        <Chips values={policy.include_hosts} />
      </Row>
      <Row label="Exclude hosts">
        <Chips values={policy.exclude_hosts} exclude />
      </Row>
      <Row label="Include paths">
        <Chips values={policy.include_paths} />
      </Row>
      <Row label="Exclude paths">
        <Chips values={policy.exclude_paths} exclude />
      </Row>
      <Row label="Schemes">
        <Chips values={policy.allowed_schemes} />
      </Row>
      <Row label="Ports">
        <Chips values={policy.allowed_ports.map(String)} />
      </Row>
      <Row label="Action classes">
        <Chips values={policy.action_classes.map((value) => ACTION_CLASS_LABELS[value])} />
      </Row>
      <Row label="Private network">
        <span>{policy.allow_private_network ? 'allowed' : 'denied'}</span>
      </Row>
      <Row label="Window">
        <span>
          {policy.window
            ? `${formatDateTime(policy.window.start)} → ${formatDateTime(policy.window.end)}`
            : 'unbounded'}
        </span>
      </Row>
      {policy.navigation_dependencies.length > 0 ? (
        <Row label="Navigation deps">
          <Chips
            values={policy.navigation_dependencies.map(
              (destination) => `${destination.host} : ${destination.ports.join(', ')}`,
            )}
          />
        </Row>
      ) : null}
      {policy.test_targets.length > 0 ? (
        <Row label="Test targets">
          <Chips
            values={policy.test_targets.map(
              (destination) => `${destination.host} : ${destination.ports.join(', ')}`,
            )}
          />
        </Row>
      ) : null}
      <Row label="Raw retention">
        <span>{policy.retain_raw ? 'retain raw evidence' : 'sanitized only'}</span>
      </Row>
      <div className="policy-budgets">
        <div>
          <span className="mono budget-key">Duration</span>
          <span>{formatDuration(limits.max_duration_seconds)}</span>
        </div>
        <div>
          <span className="mono budget-key">Requests</span>
          <span>{formatNumber(limits.max_requests)}</span>
        </div>
        <div>
          <span className="mono budget-key">Browser</span>
          <span>{formatNumber(limits.max_browser_steps)}</span>
        </div>
        <div>
          <span className="mono budget-key">Model</span>
          <span>{formatNumber(limits.max_model_calls)}</span>
        </div>
        <div>
          <span className="mono budget-key">Cost</span>
          <span>{formatCostMicros(limits.max_cost_micros)}</span>
        </div>
        <div>
          <span className="mono budget-key">Artifacts</span>
          <span>{formatBytes(limits.max_artifact_bytes)}</span>
        </div>
        <div>
          <span className="mono budget-key">Concurrency</span>
          <span>{formatNumber(limits.max_concurrency_per_origin)}</span>
        </div>
      </div>
    </div>
  );
}
