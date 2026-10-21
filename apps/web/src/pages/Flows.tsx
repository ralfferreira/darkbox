import { useEffect, useState } from 'react';
import { Link, useParams, useSearchParams } from 'react-router-dom';
import { useFlows, useIdentities } from '../api/queries';
import type { FlowDto } from '../api/types';
import { FlowDrawer } from '../components/FlowDrawer';
import { FlowRunDialog } from '../components/RunStartDialogs';
import { Status } from '../components/Status';
import {
  EmptyState,
  ErrorState,
  PageHeader,
  TableLoading,
} from '../components/ui';
import { useCurrentSession } from '../hooks/useCurrentSession';
import { formatRelative, truncate } from '../lib/format';

export function FlowsPage() {
  const { id } = useParams<{ id: string }>();
  const [searchParams, setSearchParams] = useSearchParams();
  const flows = useFlows(id);
  const identities = useIdentities(id);
  const session = useCurrentSession();
  const [selectedFlow, setSelectedFlow] = useState<FlowDto | null>(null);
  const [reproduceFlow, setReproduceFlow] = useState<FlowDto | null>(null);
  const [reproduceOpen, setReproduceOpen] = useState(false);

  const focus = searchParams.get('focus');
  const identityList = identities.data ?? [];

  useEffect(() => {
    if (!focus) return;
    const match = (flows.data ?? []).find((flow) => flow.id === focus);
    if (match) {
      setSelectedFlow((current) => current ?? match);
    }
  }, [focus, flows.data]);

  const closeDrawer = () => {
    setSelectedFlow(null);
    if (focus) {
      const next = new URLSearchParams(searchParams);
      next.delete('focus');
      setSearchParams(next, { replace: true });
    }
  };

  const canWrite = session.data?.can_write ?? false;

  return (
    <>
      <PageHeader
        meta={`flows${flows.data ? ` · ${flows.data.length}` : ''}`}
        title="Flows"
        description="Observed application flows and their recorded step sequences."
      />

      <div className="panel">
        {flows.isError ? (
          <div className="panel-pad">
            <ErrorState
              error={flows.error}
              label="Flows could not be loaded."
              onRetry={() => void flows.refetch()}
            />
          </div>
        ) : (
          <>
            <div className="table-wrap">
              <table className="data-table flows-table">
                <thead>
                  <tr>
                    <th scope="col">Name</th>
                    <th scope="col">State</th>
                    <th scope="col">Identity</th>
                    <th scope="col">Tenant</th>
                    <th scope="col" className="num">
                      Version
                    </th>
                    <th scope="col">Steps</th>
                    <th scope="col">Recorded</th>
                  </tr>
                </thead>
                {flows.isLoading ? (
                  <TableLoading columns={7} rows={6} />
                ) : (
                  <tbody>
                    {(flows.data ?? []).map((flow) => {
                      const steps = [...flow.steps].sort((a, b) => a.ordinal - b.ordinal);
                      const preview = truncate(steps.map((step) => step.intent).join(' → '), 110);
                      return (
                        <tr
                          key={flow.id}
                          className="row-link"
                          onClick={() => setSelectedFlow(flow)}
                        >
                          <td>
                            <button
                              type="button"
                              className="row-open"
                              onClick={() => setSelectedFlow(flow)}
                              aria-label={`Open flow ${flow.name}`}
                            >
                              {flow.name}
                            </button>
                          </td>
                          <td>
                            <Status value={flow.state} />
                          </td>
                          <td>
                            {flow.identity_id ? (
                              <span className="quiet">
                                {identityList.find((item) => item.id === flow.identity_id)?.label ??
                                  'unknown'}
                              </span>
                            ) : (
                              <span className="muted">—</span>
                            )}
                          </td>
                          <td className="mono quiet">{flow.tenant_key ?? '—'}</td>
                          <td className="num mono">v{flow.version}</td>
                          <td>
                            <span className="flow-preview">{preview || 'No steps recorded.'}</span>
                            <span className="cell-sub mono">
                              {steps.length} {steps.length === 1 ? 'step' : 'steps'}
                            </span>
                          </td>
                          <td>{formatRelative(flow.recorded_at)}</td>
                        </tr>
                      );
                    })}
                  </tbody>
                )}
              </table>
            </div>
            {!flows.isLoading && (flows.data ?? []).length === 0 ? (
              <EmptyState
                action={<Link to={`/assessments/${id}/inventory`}>Open inventory</Link>}
              >
                No flows recorded yet.
              </EmptyState>
            ) : null}
          </>
        )}
      </div>

      {id ? (
        <>
          <FlowDrawer
            flow={selectedFlow}
            assessmentId={id}
            identities={identityList}
            canWrite={canWrite}
            onClose={closeDrawer}
            onReproduce={(flow) => {
              closeDrawer();
              setReproduceFlow(flow);
              setReproduceOpen(true);
            }}
          />
          <FlowRunDialog
            open={reproduceOpen}
            onClose={() => {
              setReproduceOpen(false);
              setReproduceFlow(null);
            }}
            assessmentId={id}
            flow={reproduceFlow ?? undefined}
          />
        </>
      ) : null}
    </>
  );
}
