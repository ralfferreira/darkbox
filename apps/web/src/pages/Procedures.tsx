import { useState } from 'react';
import { Link, useParams } from 'react-router-dom';
import { Play } from 'lucide-react';
import { useEligibility, useProcedures } from '../api/queries';
import { ProcedureRunDialog } from '../components/RunStartDialogs';
import { Status } from '../components/Status';
import {
  EmptyState,
  ErrorState,
  PageHeader,
  TableLoading,
} from '../components/ui';
import { useCurrentSession } from '../hooks/useCurrentSession';
import { formatNumber } from '../lib/format';
import { ACTION_CLASS_LABELS } from '../lib/scope';

export function ProceduresPage() {
  const { id } = useParams<{ id: string }>();
  const procedures = useProcedures();
  const eligibility = useEligibility(id);
  const session = useCurrentSession();
  const [runKey, setRunKey] = useState<string | null>(null);
  const [runOpen, setRunOpen] = useState(false);

  const canWrite = session.data?.can_write ?? false;
  const rows = procedures.data ?? [];
  const eligibilityByKey = new Map(
    (eligibility.data ?? []).map((item) => [item.procedure_key, item]),
  );
  const eligibleCount = rows.filter(
    (procedure) => eligibilityByKey.get(procedure.key)?.eligible,
  ).length;

  return (
    <>
      <PageHeader
        meta={
          procedures.data
            ? `procedures · ${eligibleCount} of ${procedures.data.length} eligible`
            : 'procedures'
        }
        title="Procedures"
        description="Registered test procedures and their eligibility against this assessment."
      />

      <div className="panel">
        {procedures.isError ? (
          <div className="panel-pad">
            <ErrorState
              error={procedures.error}
              label="Procedures could not be loaded."
              onRetry={() => void procedures.refetch()}
            />
          </div>
        ) : procedures.isLoading ? (
          <div className="table-wrap">
            <table className="data-table">
              <thead>
                <tr>
                  <th scope="col">Procedure</th>
                  <th scope="col">Key</th>
                  <th scope="col">Effect</th>
                  <th scope="col">Targets</th>
                  <th scope="col">Eligibility</th>
                  <th scope="col">Blockers</th>
                  <th scope="col" />
                </tr>
              </thead>
              <TableLoading columns={7} rows={6} />
            </table>
          </div>
        ) : rows.length === 0 ? (
          <EmptyState action={<Link to="/capabilities">Open capabilities</Link>}>
            No procedures are registered.
          </EmptyState>
        ) : (
          <div className="table-wrap">
            <table className="data-table procedure-table">
              <thead>
                <tr>
                  <th scope="col">Procedure</th>
                  <th scope="col">Key</th>
                  <th scope="col">Effect</th>
                  <th scope="col">Targets</th>
                  <th scope="col">Eligibility</th>
                  <th scope="col">Blockers</th>
                  <th scope="col" className="action-col">
                    <span className="visually-hidden">Action</span>
                  </th>
                </tr>
              </thead>
              <tbody>
                {rows.map((procedure) => {
                  const state = eligibilityByKey.get(procedure.key);
                  const eligible = state?.eligible ?? false;
                  const known = Boolean(state);
                  return (
                    <tr key={procedure.key}>
                      <td>
                        <span className="cell-primary">{procedure.name}</span>
                        <span className="cell-sub">{procedure.purpose}</span>
                      </td>
                      <td>
                        <span className="mono">{procedure.id}</span>
                        <span className="cell-sub mono">v{procedure.version}</span>
                      </td>
                      <td>
                        <span className="mono">{ACTION_CLASS_LABELS[procedure.effect_class]}</span>
                      </td>
                      <td>
                        <span className="chip-list">
                          {procedure.target_types.map((type) => (
                            <span key={type} className="chip mono">
                              {type}
                            </span>
                          ))}
                        </span>
                      </td>
                      <td>
                        {known ? (
                          <div className="eligibility-cell">
                            <Status
                              value={eligible ? 'available' : 'blocked'}
                              label={eligible ? 'eligible' : 'blocked'}
                            />
                            <span className="mono quiet">
                              {formatNumber(state?.target_count ?? 0)} targets
                            </span>
                          </div>
                        ) : (
                          <span className="mono quiet">not evaluated</span>
                        )}
                        {eligibility.isError ? (
                          <span className="cell-sub">Eligibility unavailable.</span>
                        ) : null}
                      </td>
                      <td>
                        {!known ? (
                          <span className="muted">—</span>
                        ) : (state?.blockers.length ?? 0) === 0 ? (
                          <span className="muted">—</span>
                        ) : (
                          <ul className="blocker-list">
                            {state?.blockers.map((blocker) => (
                              <li key={`${blocker.code}-${blocker.detail}`} className="blocker">
                                <span className="mono">{blocker.code}</span>
                                {blocker.detail}
                              </li>
                            ))}
                          </ul>
                        )}
                      </td>
                      <td className="action-col">
                        <button
                          type="button"
                          className="btn"
                          disabled={!eligible || !canWrite}
                          title={
                            !canWrite
                              ? 'Your role cannot start runs.'
                              : !eligible
                                ? 'Procedure is blocked for this assessment.'
                                : undefined
                          }
                          onClick={() => {
                            setRunKey(procedure.key);
                            setRunOpen(true);
                          }}
                        >
                          <Play size={13} strokeWidth={1.5} aria-hidden="true" />
                          Run
                        </button>
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </div>

      <p className="page-footnote">
        Eligibility reflects the effective scope, identity sessions and available capabilities.
        Required capabilities are reported in the capability matrix.{' '}
        <Link to="/capabilities">Open capabilities</Link>
      </p>

      {id ? (
        <ProcedureRunDialog
          open={runOpen}
          onClose={() => {
            setRunOpen(false);
            setRunKey(null);
          }}
          assessmentId={id}
          procedureKey={runKey ?? undefined}
        />
      ) : null}
    </>
  );
}
