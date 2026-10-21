import { Link } from 'react-router-dom';
import type { EndpointDto, IdentityDto } from '../api/types';
import { useFlows, useObservations } from '../api/queries';
import { endpointRawUrl, endpointUrl } from '../lib/endpoint';
import { formatDateTime, formatJsonValue, formatRelative, shortId } from '../lib/format';
import { Drawer } from './Drawer';
import { Status } from './Status';
import { CopyButton, IdChip, KeyValue } from './ui';
import { useId } from 'react';

export function EndpointDrawer({
  endpoint,
  assessmentId,
  identities,
  onClose,
}: {
  endpoint: EndpointDto | null;
  assessmentId: string;
  identities: IdentityDto[];
  onClose: () => void;
}) {
  const observations = useObservations(assessmentId, 100);
  const flows = useFlows(assessmentId);
  const uid = useId();

  if (!endpoint) return null;

  const identity = identities.find((item) => item.id === endpoint.identity_id);
  const endpointObservations = (observations.data ?? []).filter(
    (observation) => observation.subject_id === endpoint.id,
  );
  const relatedFlows = (flows.data ?? []).filter((flow) =>
    flow.steps.some((step) => step.endpoint_id === endpoint.id),
  );
  const rawUrl = endpointRawUrl(endpoint);

  return (
    <Drawer
      open
      onClose={onClose}
      title={endpoint.path}
      subtitle={
        <span className="drawer-sub-line">
          <Status value={endpoint.state} />
          <span className="mono">{endpoint.method}</span>
          <span className="mono muted">{endpoint.host}</span>
        </span>
      }
    >
      <section className="drawer-section">
        <h3 className="drawer-section-title mono">Endpoint</h3>
        <KeyValue
          items={[
            { label: 'Method', value: <span className="mono">{endpoint.method}</span> },
            { label: 'URL', value: <span className="mono break-any">{endpointUrl(endpoint)}</span> },
            {
              label: 'Query names',
              value:
                endpoint.query_names.length > 0 ? (
                  <span className="chip-list">
                    {endpoint.query_names.map((name) => (
                      <span className="chip mono" key={`${uid}-${name}`}>
                        {name}
                      </span>
                    ))}
                  </span>
                ) : (
                  <span className="muted">none</span>
                ),
            },
            { label: 'State', value: <Status value={endpoint.state} /> },
            { label: 'Source', value: <span className="mono">{endpoint.source}</span> },
            { label: 'Identity', value: identity?.label ?? <span className="muted">none</span> },
            {
              label: 'Run',
              value: endpoint.run_id ? (
                <Link to={`/assessments/${assessmentId}/runs/${endpoint.run_id}`} className="mono">
                  {shortId(endpoint.run_id)}
                </Link>
              ) : (
                <span className="muted">none</span>
              ),
            },
            {
              label: 'Group',
              value: endpoint.group_pattern ? (
                <span className="mono">{endpoint.group_pattern}</span>
              ) : (
                <span className="muted">none</span>
              ),
            },
            {
              label: 'Observed',
              value: endpoint.observed_at ? (
                <span title={formatDateTime(endpoint.observed_at)}>
                  {formatRelative(endpoint.observed_at)}
                </span>
              ) : (
                <span className="muted">never</span>
              ),
            },
            { label: 'Recorded', value: formatDateTime(endpoint.recorded_at) },
            { label: 'ID', value: <IdChip id={endpoint.id} length={12} /> },
          ]}
        />
        <div className="raw-url">
          <span className="mono break-any">{rawUrl}</span>
          <CopyButton value={rawUrl} label="Copy URL" />
        </div>
      </section>

      <section className="drawer-section">
        <h3 className="drawer-section-title mono">
          Observations
          <span className="section-count mono">{endpointObservations.length}</span>
        </h3>
        {endpointObservations.length === 0 ? (
          <p className="drawer-empty">No observations recorded for this endpoint.</p>
        ) : (
          <ul className="obs-list">
            {endpointObservations.map((observation) => (
              <li key={observation.id} className="obs-item">
                <div className="obs-property mono">{observation.property}</div>
                <div className="obs-value">{formatJsonValue(observation.value)}</div>
                <div className="obs-meta mono">
                  <span>{observation.source}</span>
                  <span>conf {observation.confidence.toFixed(2)}</span>
                  <span>{formatRelative(observation.observed_at)}</span>
                  {observation.evidence_id ? (
                    <Link
                      to={`/assessments/${assessmentId}/evidence?focus=${observation.evidence_id}`}
                    >
                      evidence
                    </Link>
                  ) : null}
                </div>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="drawer-section">
        <h3 className="drawer-section-title mono">
          Related flows
          <span className="section-count mono">{relatedFlows.length}</span>
        </h3>
        {relatedFlows.length === 0 ? (
          <p className="drawer-empty">No flow references this endpoint.</p>
        ) : (
          <ul className="link-list">
            {relatedFlows.map((flow) => (
              <li key={flow.id}>
                <Link to={`/assessments/${assessmentId}/flows?focus=${flow.id}`}>
                  {flow.name}
                </Link>
                <Status value={flow.state} />
              </li>
            ))}
          </ul>
        )}
      </section>
    </Drawer>
  );
}
