import { useState } from 'react';
import { Link, useNavigate, useParams } from 'react-router-dom';
import { Plus } from 'lucide-react';
import { useRuns } from '../api/queries';
import { ImportUrlsDialog } from '../components/ImportUrlsDialog';
import { Status } from '../components/Status';
import {
  EmptyState,
  ErrorState,
  PageHeader,
  TableLoading,
} from '../components/ui';
import { useCurrentSession } from '../hooks/useCurrentSession';
import {
  formatCostMicros,
  formatDateTime,
  formatDurationBetween,
  formatNumber,
  formatRelative,
  shortId,
  truncate,
} from '../lib/format';

export function RunsPage() {
  const { id } = useParams<{ id: string }>();
  const runs = useRuns(id);
  const session = useCurrentSession();
  const navigate = useNavigate();
  const [importOpen, setImportOpen] = useState(false);
  const canWrite = session.data?.can_write ?? false;

  return (
    <>
      <PageHeader
        meta={`runs${runs.data ? ` · ${runs.data.length}` : ''}`}
        title="Runs"
        description="Execution history, budgets and external-effect state."
        actions={
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => setImportOpen(true)}
            disabled={!canWrite}
            title={canWrite ? undefined : 'Your role cannot start runs.'}
          >
            <Plus size={14} strokeWidth={1.5} aria-hidden="true" />
            Start discovery
          </button>
        }
      />

      <div className="panel">
        {runs.isError ? (
          <div className="panel-pad">
            <ErrorState
              error={runs.error}
              label="Runs could not be loaded."
              onRetry={() => void runs.refetch()}
            />
          </div>
        ) : (
          <>
            <div className="table-wrap">
              <table className="data-table runs-table">
                <thead>
                  <tr>
                    <th scope="col">Kind</th>
                    <th scope="col">State</th>
                    <th scope="col">Procedure</th>
                    <th scope="col" className="num">
                      Policy
                    </th>
                    <th scope="col" className="num">
                      Steps
                    </th>
                    <th scope="col" className="num">
                      Spend / budget
                    </th>
                    <th scope="col">Duration</th>
                    <th scope="col">Effect</th>
                  </tr>
                </thead>
                {runs.isLoading ? (
                  <TableLoading columns={8} rows={8} />
                ) : (
                  <tbody>
                    {(runs.data ?? []).map((run) => (
                      <tr
                        key={run.id}
                        className="row-link"
                        onClick={() => navigate(`/assessments/${id}/runs/${run.id}`)}
                      >
                        <td>
                          <Link
                            to={`/assessments/${id}/runs/${run.id}`}
                            className="cell-primary mono"
                            onClick={(event) => event.stopPropagation()}
                          >
                            {run.kind.replace(/_/g, ' ')}
                          </Link>
                          <span className="cell-sub mono">
                            {shortId(run.id)} · {formatRelative(run.created_at)}
                          </span>
                        </td>
                        <td>
                          <Status value={run.state} />
                          {run.reason ? (
                            <span className="cell-sub" title={run.reason}>
                              {truncate(run.reason, 48)}
                            </span>
                          ) : null}
                        </td>
                        <td className="mono quiet">{run.procedure_key ?? '—'}</td>
                        <td className="num mono">v{run.policy_version}</td>
                        <td className="num mono">
                          {formatNumber(run.steps_done)}/{formatNumber(run.step_count)}
                        </td>
                        <td className="num">
                          <span className="cell-stack mono">
                            <span>
                              {formatNumber(run.spent.requests)}/
                              {formatNumber(run.budget.max_requests)} req
                            </span>
                            <span className="quiet">
                              {formatCostMicros(run.spent.cost_micros)} /{' '}
                              {formatCostMicros(run.budget.max_cost_micros)}
                            </span>
                          </span>
                        </td>
                        <td
                          className="mono quiet"
                          title={
                            run.started_at
                              ? `${formatDateTime(run.started_at)} → ${
                                  run.finished_at ? formatDateTime(run.finished_at) : 'now'
                                }`
                              : undefined
                          }
                        >
                          {formatDurationBetween(run.started_at, run.finished_at)}
                        </td>
                        <td>
                          {run.effect_unknown ? (
                            <span className="badge" data-tone="red">
                              effect unknown
                            </span>
                          ) : (
                            <span className="muted">—</span>
                          )}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                )}
              </table>
            </div>
            {!runs.isLoading && (runs.data ?? []).length === 0 ? (
              <EmptyState
                action={
                  <button
                    type="button"
                    className="btn"
                    onClick={() => setImportOpen(true)}
                    disabled={!canWrite}
                  >
                    Start discovery
                  </button>
                }
              >
                No runs recorded for this assessment yet.
              </EmptyState>
            ) : null}
          </>
        )}
      </div>

      {id ? (
        <ImportUrlsDialog
          open={importOpen}
          onClose={() => setImportOpen(false)}
          assessmentId={id}
        />
      ) : null}
    </>
  );
}
