import { useState } from 'react';
import { Link, useNavigate, useParams } from 'react-router-dom';
import { Download, Play, Plug, Plus, RotateCcw } from 'lucide-react';
import { useAssessment, useFlows, useObservations } from '../api/queries';
import { isTerminalRun, type AssessmentDetail } from '../api/types';
import { ExportDialog } from '../components/ExportDialog';
import { FlowRunDialog, ProcedureRunDialog } from '../components/RunStartDialogs';
import { ImportUrlsDialog } from '../components/ImportUrlsDialog';
import { IntegrationRunDialog } from '../components/IntegrationRunDialog';
import { Status } from '../components/Status';
import {
  BlockSkeleton,
  EmptyState,
  ErrorState,
  Metric,
  PageHeader,
  Panel,
} from '../components/ui';
import { useCurrentSession } from '../hooks/useCurrentSession';
import {
  formatBytes,
  formatDateTime,
  formatDurationBetween,
  formatJsonValue,
  formatNumber,
  formatPercent,
  formatRelative,
  plural,
  shortId,
  truncate,
} from '../lib/format';
import { ACTION_CLASS_LABELS } from '../lib/scope';

function ScopeSummary({ detail }: { detail: AssessmentDetail }) {
  const policy = detail.scope.policy;
  return (
    <div className="scope-brief">
      <div className="scope-brief-head">
        <span className="mono">v{detail.scope.version}</span>
        <span className="mono quiet">{formatRelative(detail.scope.created_at)}</span>
      </div>
      <p className="scope-note">{detail.scope.note || 'No note recorded.'}</p>
      <div className="policy-row">
        <span className="policy-key mono">Include</span>
        <span className="chip-list">
          {policy.include_hosts.map((host) => (
            <span key={host} className="chip mono">
              {host}
            </span>
          ))}
        </span>
      </div>
      <div className="policy-row">
        <span className="policy-key mono">Exclude</span>
        <span className="chip-list">
          {policy.exclude_hosts.length === 0 ? (
            <span className="muted">none</span>
          ) : (
            policy.exclude_hosts.map((host) => (
              <span key={host} className="chip mono is-exclude">
                <span aria-hidden="true">− </span>
                {host}
              </span>
            ))
          )}
        </span>
      </div>
      <div className="policy-row">
        <span className="policy-key mono">Transport</span>
        <span className="mono quiet">
          {policy.allowed_schemes.join(', ')} · {policy.allowed_ports.join(', ')}
        </span>
      </div>
      <div className="policy-row">
        <span className="policy-key mono">Actions</span>
        <span className="chip-list">
          {policy.action_classes.map((actionClass) => (
            <span key={actionClass} className="chip">
              {ACTION_CLASS_LABELS[actionClass]}
            </span>
          ))}
        </span>
      </div>
      <div className="policy-row">
        <span className="policy-key mono">Private net</span>
        <span>{policy.allow_private_network ? 'allowed' : 'denied'}</span>
      </div>
    </div>
  );
}

export function AssessmentOverviewPage() {
  const { id } = useParams<{ id: string }>();
  const assessment = useAssessment(id);
  const observations = useObservations(id, 6);
  const flows = useFlows(id);
  const session = useCurrentSession();
  const navigate = useNavigate();
  const [importOpen, setImportOpen] = useState(false);
  const [reproduceOpen, setReproduceOpen] = useState(false);
  const [procedureOpen, setProcedureOpen] = useState(false);
  const [exportOpen, setExportOpen] = useState(false);
  const [toolsOpen, setToolsOpen] = useState(false);

  if (assessment.isError) {
    return (
      <ErrorState
        error={assessment.error}
        label="Assessment could not be loaded."
        onRetry={() => void assessment.refetch()}
      />
    );
  }

  if (assessment.isLoading || !assessment.data) {
    return (
      <>
        <PageHeader meta="assessment" title="Loading assessment" />
        <BlockSkeleton lines={8} />
      </>
    );
  }

  const detail = assessment.data;
  const coverage = detail.coverage;
  const canWrite = session.data?.can_write ?? false;
  const candidateGap = Math.max(0, coverage.known_candidates - coverage.observed_endpoints);
  const staleFlows = (flows.data ?? []).filter((flow) => flow.state === 'stale').length;
  const waitingRuns = detail.recent_runs.filter(
    (run) => run.state === 'waiting_operator',
  ).length;

  return (
    <>
      <PageHeader
        meta={`assessment · policy v${detail.scope.version}`}
        title={detail.assessment.name}
        description={detail.assessment.objective}
        actions={
          <>
            <button
              type="button"
              className="btn"
              onClick={() => setReproduceOpen(true)}
              disabled={!canWrite}
              title={canWrite ? undefined : 'Your role cannot start runs.'}
            >
              <RotateCcw size={14} strokeWidth={1.5} aria-hidden="true" />
              Reproduce flow
            </button>
            <button
              type="button"
              className="btn"
              onClick={() => setProcedureOpen(true)}
              disabled={!canWrite}
              title={canWrite ? undefined : 'Your role cannot start runs.'}
            >
              <Play size={14} strokeWidth={1.5} aria-hidden="true" />
              Run procedure
            </button>
            <button
              type="button"
              className="btn"
              onClick={() => setToolsOpen(true)}
              disabled={!canWrite}
              title={canWrite ? undefined : 'Your role cannot start runs.'}
            >
              <Plug size={14} strokeWidth={1.5} aria-hidden="true" />
              Integrations
            </button>
            <button type="button" className="btn" onClick={() => setExportOpen(true)}>
              <Download size={14} strokeWidth={1.5} aria-hidden="true" />
              Export
            </button>
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
          </>
        }
      />

      <div className="metrics">
        <Metric label="Known candidates" value={formatNumber(coverage.known_candidates)} />
        <Metric
          label="Observed"
          value={formatNumber(coverage.observed_endpoints)}
          hint={formatPercent(coverage.observed_endpoints, coverage.known_candidates)}
        />
        <Metric label="Excluded" value={formatNumber(coverage.excluded_endpoints)} />
        <Metric label="Unreachable" value={formatNumber(coverage.unreachable_endpoints)} />
        <Metric
          label="Flows"
          value={
            <span className="mono">
              {formatNumber(coverage.flows_reproduced)}
              <span className="metric-fraction"> / {formatNumber(coverage.flows_known)}</span>
            </span>
          }
          hint="reproduced"
        />
        <Metric
          label="Findings"
          value={formatNumber(coverage.findings_confirmed)}
          hint={`${formatNumber(coverage.findings_needs_review)} needs review`}
        />
        <Metric
          label="Evidence"
          value={formatNumber(coverage.evidence_count)}
          hint={formatBytes(coverage.evidence_bytes)}
        />
      </div>

      <div className="split">
        <div className="split-main">
          <Panel
            title="Recent runs"
            actions={<Link to={`/assessments/${id}/runs`}>All runs</Link>}
            flush
          >
            {detail.recent_runs.length === 0 ? (
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
            ) : (
              <div className="table-wrap">
                <table className="data-table">
                  <thead>
                    <tr>
                      <th scope="col">Kind</th>
                      <th scope="col">State</th>
                      <th scope="col" className="num">
                        Steps
                      </th>
                      <th scope="col">Started</th>
                      <th scope="col">Duration</th>
                    </tr>
                  </thead>
                  <tbody>
                    {detail.recent_runs.map((run) => (
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
                          <span className="cell-sub mono">{shortId(run.id)}</span>
                        </td>
                        <td>
                          <Status value={run.state} />
                          {isTerminalRun(run.state) ? null : (
                            <span className="cell-sub mono">live</span>
                          )}
                        </td>
                        <td className="num mono">
                          {run.steps_done}/{run.step_count}
                        </td>
                        <td>
                          {run.started_at ? (
                            <span title={formatDateTime(run.started_at)}>
                              {formatRelative(run.started_at)}
                            </span>
                          ) : (
                            <span className="muted">queued</span>
                          )}
                        </td>
                        <td className="mono quiet">
                          {formatDurationBetween(run.started_at, run.finished_at)}
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </Panel>

          <Panel
            title="Recent observations"
            actions={<Link to={`/assessments/${id}/inventory`}>Open inventory</Link>}
            flush
          >
            {observations.isLoading ? (
              <div className="panel-pad">
                <BlockSkeleton lines={4} />
              </div>
            ) : observations.isError ? (
              <div className="panel-pad">
                <ErrorState
                  error={observations.error}
                  label="Observations could not be loaded."
                  onRetry={() => void observations.refetch()}
                />
              </div>
            ) : (observations.data ?? []).length === 0 ? (
              <EmptyState action={<Link to={`/assessments/${id}/runs`}>View runs</Link>}>
                No observations recorded yet.
              </EmptyState>
            ) : (
              <ul className="obs-list obs-list-page">
                {(observations.data ?? []).map((observation) => (
                  <li key={observation.id} className="obs-item">
                    <div className="obs-subject mono">
                      {observation.subject_kind}
                      <span className="quiet"> {shortId(observation.subject_id)}</span>
                    </div>
                    <div className="obs-property mono">{observation.property}</div>
                    <div className="obs-value">
                      {truncate(formatJsonValue(observation.value), 280)}
                    </div>
                    <div className="obs-meta mono">
                      <span>{observation.source}</span>
                      <span>{formatRelative(observation.observed_at)}</span>
                      {observation.evidence_id ? (
                        <Link
                          to={`/assessments/${id}/evidence?focus=${observation.evidence_id}`}
                        >
                          evidence
                        </Link>
                      ) : null}
                    </div>
                  </li>
                ))}
              </ul>
            )}
          </Panel>
        </div>

        <div className="split-side">
          <Panel title="Next actions">
            <div className="next-actions">
              {candidateGap > 0 ? (
                <Link className="next-action" to={`/assessments/${id}/inventory?state=candidate`}>
                  <span>
                    {formatNumber(candidateGap)} {plural(candidateGap, 'candidate')} not observed
                  </span>
                  <span className="next-action-go mono">review</span>
                </Link>
              ) : null}
              {coverage.unreachable_endpoints > 0 ? (
                <Link
                  className="next-action"
                  to={`/assessments/${id}/inventory?state=unreachable`}
                >
                  <span>
                    {formatNumber(coverage.unreachable_endpoints)}{' '}
                    {plural(coverage.unreachable_endpoints, 'endpoint')} unreachable
                  </span>
                  <span className="next-action-go mono">retry</span>
                </Link>
              ) : null}
              {staleFlows > 0 ? (
                <Link className="next-action" to={`/assessments/${id}/flows`}>
                  <span>
                    {formatNumber(staleFlows)} stale {plural(staleFlows, 'flow')}
                  </span>
                  <span className="next-action-go mono">reproduce</span>
                </Link>
              ) : null}
              {coverage.findings_needs_review > 0 ? (
                <Link className="next-action" to={`/assessments/${id}/findings`}>
                  <span>
                    {formatNumber(coverage.findings_needs_review)}{' '}
                    {plural(coverage.findings_needs_review, 'finding')}{' '}
                    {coverage.findings_needs_review === 1 ? 'needs' : 'need'} review
                  </span>
                  <span className="next-action-go mono">review</span>
                </Link>
              ) : null}
              {waitingRuns > 0 ? (
                <Link className="next-action" to={`/assessments/${id}/runs`}>
                  <span>
                    {formatNumber(waitingRuns)} {plural(waitingRuns, 'run')} waiting on an
                    operator
                  </span>
                  <span className="next-action-go mono">open</span>
                </Link>
              ) : null}
              {candidateGap === 0 &&
              coverage.unreachable_endpoints === 0 &&
              staleFlows === 0 &&
              coverage.findings_needs_review === 0 &&
              waitingRuns === 0 ? (
                <p className="drawer-empty">No gaps detected in the current coverage counts.</p>
              ) : null}
            </div>
          </Panel>

          <Panel
            title="Effective scope"
            actions={<Link to={`/assessments/${id}/scope`}>Open scope</Link>}
          >
            <ScopeSummary detail={detail} />
          </Panel>

          <Panel title="Identities" flush>
            {detail.identities.length === 0 ? (
              <EmptyState action={<Link to={`/assessments/${id}/runs`}>View runs</Link>}>
                No identities registered for this assessment.
              </EmptyState>
            ) : (
              <ul className="identity-list">
                {detail.identities.map((identity) => (
                  <li key={identity.id} className="identity-item">
                    <div className="identity-head">
                      <span className="identity-label">{identity.label}</span>
                      <Status value={identity.session_state} />
                    </div>
                    <div className="identity-meta mono">
                      <span>{identity.role_label}</span>
                      <span>{shortId(identity.id)}</span>
                      {identity.session_expires_at ? (
                        <span title={formatDateTime(identity.session_expires_at)}>
                          expires {formatRelative(identity.session_expires_at)}
                        </span>
                      ) : null}
                    </div>
                    {identity.tenant_keys.length > 0 ? (
                      <div className="chip-list">
                        {identity.tenant_keys.map((key) => (
                          <span key={key} className="chip mono">
                            {key}
                          </span>
                        ))}
                      </div>
                    ) : null}
                  </li>
                ))}
              </ul>
            )}
          </Panel>
        </div>
      </div>

      {id ? (
        <>
          <ImportUrlsDialog
            open={importOpen}
            onClose={() => setImportOpen(false)}
            assessmentId={id}
          />
          <FlowRunDialog
            open={reproduceOpen}
            onClose={() => setReproduceOpen(false)}
            assessmentId={id}
          />
          <ProcedureRunDialog
            open={procedureOpen}
            onClose={() => setProcedureOpen(false)}
            assessmentId={id}
          />
          <IntegrationRunDialog
            open={toolsOpen}
            onClose={() => setToolsOpen(false)}
            assessmentId={id}
          />
          <ExportDialog
            open={exportOpen}
            onClose={() => setExportOpen(false)}
            assessmentId={id}
          />
        </>
      ) : null}
    </>
  );
}
