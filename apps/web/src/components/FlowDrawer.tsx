import { Link } from 'react-router-dom';
import type { FlowDto, IdentityDto } from '../api/types';
import { formatDateTime, formatRelative, shortId } from '../lib/format';
import { Drawer } from './Drawer';
import { Status } from './Status';
import { IdChip, KeyValue } from './ui';

export function FlowDrawer({
  flow,
  assessmentId,
  identities,
  canWrite,
  onClose,
  onReproduce,
}: {
  flow: FlowDto | null;
  assessmentId: string;
  identities: IdentityDto[];
  canWrite: boolean;
  onClose: () => void;
  onReproduce: (flow: FlowDto) => void;
}) {
  if (!flow) return null;

  const identity = identities.find((item) => item.id === flow.identity_id);
  const steps = [...flow.steps].sort((a, b) => a.ordinal - b.ordinal);

  return (
    <Drawer
      open
      onClose={onClose}
      title={flow.name}
      subtitle={
        <span className="drawer-sub-line">
          <Status value={flow.state} />
          <span className="mono">v{flow.version}</span>
          <span className="mono muted">{steps.length} steps</span>
        </span>
      }
      footer={
        <>
          <button type="button" className="btn" onClick={onClose}>
            Close
          </button>
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => onReproduce(flow)}
            disabled={!canWrite}
            title={canWrite ? undefined : 'Your role cannot start runs.'}
          >
            Reproduce flow
          </button>
        </>
      }
    >
      <section className="drawer-section">
        <h3 className="drawer-section-title mono">Flow</h3>
        <KeyValue
          items={[
            { label: 'State', value: <Status value={flow.state} /> },
            { label: 'Version', value: <span className="mono">v{flow.version}</span> },
            { label: 'Identity', value: identity?.label ?? <span className="muted">none</span> },
            {
              label: 'Tenant',
              value: flow.tenant_key ? (
                <span className="mono">{flow.tenant_key}</span>
              ) : (
                <span className="muted">none</span>
              ),
            },
            {
              label: 'Observed',
              value: flow.observed_at ? (
                <span title={formatDateTime(flow.observed_at)}>
                  {formatRelative(flow.observed_at)}
                </span>
              ) : (
                <span className="muted">never</span>
              ),
            },
            { label: 'Recorded', value: formatDateTime(flow.recorded_at) },
            { label: 'ID', value: <IdChip id={flow.id} length={12} /> },
          ]}
        />
      </section>

      <section className="drawer-section">
        <h3 className="drawer-section-title mono">Preconditions</h3>
        {flow.preconditions.length === 0 ? (
          <p className="drawer-empty">No preconditions recorded.</p>
        ) : (
          <ul className="plain-list">
            {flow.preconditions.map((precondition) => (
              <li key={precondition}>{precondition}</li>
            ))}
          </ul>
        )}
      </section>

      <section className="drawer-section">
        <h3 className="drawer-section-title mono">
          Steps
          <span className="section-count mono">{steps.length}</span>
        </h3>
        {steps.length === 0 ? (
          <p className="drawer-empty">This flow has no recorded steps.</p>
        ) : (
          <ol className="step-list">
            {steps.map((step) => (
              <li key={`${flow.id}-${step.ordinal}`} className="step-item">
                <span className="step-ordinal mono">{String(step.ordinal).padStart(2, '0')}</span>
                <div className="step-content">
                  <div className="step-intent">{step.intent}</div>
                  <div className="step-expected">
                    <span className="mono step-expected-label">expected</span>
                    {step.expected_result}
                  </div>
                  <div className="step-meta mono">
                    {step.endpoint_id ? (
                      <Link
                        to={`/assessments/${assessmentId}/inventory?focus=${step.endpoint_id}`}
                      >
                        endpoint {shortId(step.endpoint_id)}
                      </Link>
                    ) : (
                      <span className="muted">no endpoint</span>
                    )}
                    <span className={step.confirmed ? 'is-confirmed' : 'muted'}>
                      {step.confirmed ? 'confirmed' : 'unconfirmed'}
                    </span>
                  </div>
                </div>
              </li>
            ))}
          </ol>
        )}
      </section>

      <section className="drawer-section">
        <h3 className="drawer-section-title mono">
          Evidence
          <span className="section-count mono">{flow.evidence_ids.length}</span>
        </h3>
        {flow.evidence_ids.length === 0 ? (
          <p className="drawer-empty">No evidence attached.</p>
        ) : (
          <ul className="link-list">
            {flow.evidence_ids.map((id) => (
              <li key={id}>
                <Link to={`/assessments/${assessmentId}/evidence?focus=${id}`} className="mono">
                  {shortId(id)}
                </Link>
              </li>
            ))}
          </ul>
        )}
      </section>
    </Drawer>
  );
}
