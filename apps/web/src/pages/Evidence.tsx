import { useEffect, useState } from 'react';
import { Link, useParams, useSearchParams } from 'react-router-dom';
import { useEvidence, useEvidenceItem } from '../api/queries';
import type { EvidenceDto } from '../api/types';
import { EvidenceDrawer } from '../components/EvidenceDrawer';
import { Status } from '../components/Status';
import {
  EmptyState,
  ErrorState,
  PageHeader,
  TableLoading,
} from '../components/ui';
import { formatBytes, formatRelative, shortHash, shortId } from '../lib/format';

const KINDS = [
  'http_exchange',
  'http_response_body',
  'browser_screenshot',
  'browser_trace',
  'tool_output',
  'export_manifest',
] as const;

const PAGE_SIZE = 100;

export function EvidencePage() {
  const { id } = useParams<{ id: string }>();
  const [searchParams, setSearchParams] = useSearchParams();
  const [kind, setKind] = useState('');
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const evidence = useEvidence(id, kind, PAGE_SIZE);
  const focus = searchParams.get('focus');

  const rows = evidence.data ?? [];
  const selected = selectedId
    ? (rows.find((item) => item.id === selectedId) ?? null)
    : focus
      ? (rows.find((item) => item.id === focus) ?? null)
      : null;

  useEffect(() => {
    if (!focus) return;
    if (rows.some((item) => item.id === focus)) {
      setSelectedId((current) => current ?? focus);
    }
  }, [focus, rows]);

  const closeDrawer = () => {
    setSelectedId(null);
    if (focus) {
      const next = new URLSearchParams(searchParams);
      next.delete('focus');
      setSearchParams(next, { replace: true });
    }
  };

  return (
    <>
      <PageHeader
        meta={`evidence${evidence.data ? ` · ${evidence.data.length}` : ''}`}
        title="Evidence"
        description="Sanitized captures and artifacts, with hashes and run provenance."
      />

      <div className="filter-row filter-row-single" role="group" aria-label="Evidence filters">
        <div className="filter">
          <label htmlFor="evidence-kind" className="filter-label mono">
            Kind
          </label>
          <select
            id="evidence-kind"
            className="input select"
            value={kind}
            onChange={(event) => {
              setKind(event.target.value);
              setSelectedId(null);
            }}
          >
            <option value="">All kinds</option>
            {KINDS.map((option) => (
              <option key={option} value={option}>
                {option.replace(/_/g, ' ')}
              </option>
            ))}
          </select>
        </div>
        {evidence.data && evidence.data.length === PAGE_SIZE ? (
          <span className="filter-note mono">showing first {PAGE_SIZE}</span>
        ) : null}
      </div>

      <div className="panel">
        {evidence.isError ? (
          <div className="panel-pad">
            <ErrorState
              error={evidence.error}
              label="Evidence could not be loaded."
              onRetry={() => void evidence.refetch()}
            />
          </div>
        ) : (
          <>
            <div className="table-wrap">
              <table className="data-table">
                <thead>
                  <tr>
                    <th scope="col">Kind</th>
                    <th scope="col">Classification</th>
                    <th scope="col" className="num">
                      Size
                    </th>
                    <th scope="col">SHA-256</th>
                    <th scope="col">Run</th>
                    <th scope="col">Created</th>
                  </tr>
                </thead>
                {evidence.isLoading ? (
                  <TableLoading columns={6} rows={8} />
                ) : (
                  <tbody>
                    {rows.map((item) => (
                      <tr
                        key={item.id}
                        className="row-link"
                        onClick={() => setSelectedId(item.id)}
                      >
                        <td>
                          <button
                            type="button"
                            className="row-open"
                            onClick={() => setSelectedId(item.id)}
                            aria-label={`Open ${item.kind.replace(/_/g, ' ')} evidence`}
                          >
                            {item.kind.replace(/_/g, ' ')}
                          </button>
                          <span className="cell-sub" title={item.summary}>
                            {item.summary || item.content_type}
                          </span>
                        </td>
                        <td>
                          <Status value={item.classification} />
                        </td>
                        <td className="num mono">
                          {formatBytes(item.size_bytes)}
                          {item.truncated ? (
                            <span className="cell-sub mono">truncated</span>
                          ) : null}
                        </td>
                        <td>
                          <span className="mono quiet" title={item.sha256}>
                            {shortHash(item.sha256)}
                          </span>
                        </td>
                        <td>
                          {item.run_id ? (
                            <Link
                              to={`/assessments/${id}/runs/${item.run_id}`}
                              className="mono"
                            >
                              {shortId(item.run_id)}
                            </Link>
                          ) : (
                            <span className="muted">—</span>
                          )}
                        </td>
                        <td>{formatRelative(item.created_at)}</td>
                      </tr>
                    ))}
                  </tbody>
                )}
              </table>
            </div>
            {!evidence.isLoading && rows.length === 0 ? (
              <EmptyState
                action={<Link to={`/assessments/${id}/runs`}>View runs</Link>}
              >
                No evidence captured yet.
              </EmptyState>
            ) : null}
          </>
        )}
      </div>

      {id ? (
        <>
          <EvidenceDrawer evidence={selected} assessmentId={id} onClose={closeDrawer} />
          {focus && selectedId === null && !selected ? (
            <FocusedEvidence assessmentId={id} evidenceId={focus} onClose={closeDrawer} />
          ) : null}
        </>
      ) : null}
    </>
  );
}

function FocusedEvidence({
  assessmentId,
  evidenceId,
  onClose,
}: {
  assessmentId: string;
  evidenceId: string;
  onClose: () => void;
}) {
  const evidence = useEvidenceItem(evidenceId);
  const item: EvidenceDto | null = evidence.data ?? null;

  if (!item) return null;

  return <EvidenceDrawer evidence={item} assessmentId={assessmentId} onClose={onClose} />;
}
