import { useEffect, useState } from 'react';
import { Link, useParams, useSearchParams } from 'react-router-dom';
import { Plus } from 'lucide-react';
import { useFindings } from '../api/queries';
import type { FindingDto, FindingState } from '../api/types';
import { CreateFindingDialog } from '../components/CreateFindingDialog';
import { FindingDrawer } from '../components/FindingDrawer';
import { Status } from '../components/Status';
import {
  EmptyState,
  ErrorState,
  PageHeader,
  TableLoading,
} from '../components/ui';
import { useCurrentSession } from '../hooks/useCurrentSession';
import { formatRelative, humanize, truncate } from '../lib/format';

const FINDING_STATES: FindingState[] = [
  'hypothesis',
  'needs_review',
  'confirmed',
  'rejected',
  'remediated',
  'reopened',
];

export function FindingsPage() {
  const { id } = useParams<{ id: string }>();
  const [searchParams, setSearchParams] = useSearchParams();
  const [stateFilter, setStateFilter] = useState('');
  const [selectedFinding, setSelectedFinding] = useState<FindingDto | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const findings = useFindings(id, stateFilter);
  const session = useCurrentSession();

  const focus = searchParams.get('focus');
  const rows = findings.data ?? [];
  const canWrite = session.data?.can_write ?? false;
  const canReview = session.data?.role === 'admin' || session.data?.role === 'reviewer';

  useEffect(() => {
    if (!focus) return;
    const match = rows.find((finding) => finding.id === focus);
    if (match) setSelectedFinding((current) => current ?? match);
  }, [focus, rows]);

  const closeDrawer = () => {
    setSelectedFinding(null);
    if (focus) {
      const next = new URLSearchParams(searchParams);
      next.delete('focus');
      setSearchParams(next, { replace: true });
    }
  };

  return (
    <>
      <PageHeader
        meta={`findings${findings.data ? ` · ${findings.data.length}` : ''}`}
        title="Findings"
        description="Claims under review, with evidence and revision history."
        actions={
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => setCreateOpen(true)}
            disabled={!canWrite}
            title={canWrite ? undefined : 'Your role cannot create findings.'}
          >
            <Plus size={14} strokeWidth={1.5} aria-hidden="true" />
            New finding
          </button>
        }
      />

      <div className="filter-row filter-row-single" role="group" aria-label="Finding filters">
        <div className="filter">
          <label htmlFor="finding-state" className="filter-label mono">
            State
          </label>
          <select
            id="finding-state"
            className="input select"
            value={stateFilter}
            onChange={(event) => setStateFilter(event.target.value)}
          >
            <option value="">All states</option>
            {FINDING_STATES.map((state) => (
              <option key={state} value={state}>
                {humanize(state)}
              </option>
            ))}
          </select>
        </div>
      </div>

      <div className="panel">
        {findings.isError ? (
          <div className="panel-pad">
            <ErrorState
              error={findings.error}
              label="Findings could not be loaded."
              onRetry={() => void findings.refetch()}
            />
          </div>
        ) : (
          <>
            <div className="table-wrap">
              <table className="data-table">
                <thead>
                  <tr>
                    <th scope="col">State</th>
                    <th scope="col">Severity</th>
                    <th scope="col">Title</th>
                    <th scope="col" className="num">
                      Evidence
                    </th>
                    <th scope="col">Updated</th>
                  </tr>
                </thead>
                {findings.isLoading ? (
                  <TableLoading columns={5} rows={6} />
                ) : (
                  <tbody>
                    {rows.map((finding) => (
                      <tr
                        key={finding.id}
                        className="row-link"
                        onClick={() => setSelectedFinding(finding)}
                      >
                        <td>
                          <Status value={finding.state} />
                        </td>
                        <td>
                          <span className="sev mono">{finding.severity}</span>
                        </td>
                        <td>
                          <button
                            type="button"
                            className="row-open"
                            onClick={() => setSelectedFinding(finding)}
                            aria-label={`Open finding ${finding.title}`}
                          >
                            {finding.title}
                          </button>
                          <span className="cell-sub">{truncate(finding.claim, 96)}</span>
                        </td>
                        <td className="num mono">{finding.evidence_ids.length}</td>
                        <td>{formatRelative(finding.updated_at)}</td>
                      </tr>
                    ))}
                  </tbody>
                )}
              </table>
            </div>
            {!findings.isLoading && rows.length === 0 ? (
              <EmptyState
                action={
                  stateFilter ? (
                    <button type="button" className="btn" onClick={() => setStateFilter('')}>
                      Clear filter
                    </button>
                  ) : (
                    <button
                      type="button"
                      className="btn"
                      onClick={() => setCreateOpen(true)}
                      disabled={!canWrite}
                    >
                      New finding
                    </button>
                  )
                }
              >
                {stateFilter
                  ? 'No findings in this state.'
                  : 'No findings recorded yet.'}
              </EmptyState>
            ) : null}
          </>
        )}
      </div>

      <p className="page-footnote">
        Findings are recorded as hypotheses first. Confirmation requires evidence and a review
        note. <Link to={`/assessments/${id}/evidence`}>Open evidence</Link>
      </p>

      {id ? (
        <>
          <FindingDrawer
            finding={selectedFinding}
            assessmentId={id}
            canReview={canReview}
            onClose={closeDrawer}
          />
          <CreateFindingDialog
            open={createOpen}
            onClose={() => setCreateOpen(false)}
            assessmentId={id}
          />
        </>
      ) : null}
    </>
  );
}
