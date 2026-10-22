import { useEffect, useMemo, useState } from 'react';
import { Link, useParams } from 'react-router-dom';
import { Ban, RotateCw } from 'lucide-react';
import { useAgentProgress, useResults, useRun, useRunEvidence } from '../api/queries';
import { isTerminalRun, type EventDto, type RunState } from '../api/types';
import { AgentExploration } from '../components/AgentExploration';
import { AgentTranscript } from '../components/AgentTranscript';
import { AgentTurnDialog } from '../components/AgentTurnDialog';
import { CancelRunDialog, ResumeRunDialog } from '../components/RunControlDialogs';
import { Status, LiveDot } from '../components/Status';
import {
  BlockSkeleton,
  CopyButton,
  EmptyState,
  ErrorState,
  IdChip,
  KeyValue,
  PageHeader,
  Panel,
} from '../components/ui';
import { useCurrentSession } from '../hooks/useCurrentSession';
import { useEventStream, type StreamStatus } from '../hooks/useEventStream';
import {
  formatBytes,
  formatCostMicros,
  formatDateTime,
  formatDuration,
  formatDurationBetween,
  formatNumber,
  formatRelative,
  humanize,
  ratioPercent,
  shortId,
} from '../lib/format';

const CANCELLABLE: RunState[] = ['queued', 'running', 'waiting_operator', 'interrupted'];
const RESUMABLE: RunState[] = ['waiting_operator', 'interrupted'];

const STREAM_LABELS: Record<
  StreamStatus,
  { label: string; tone: 'lime' | 'amber' | 'red' | 'dim' | 'neutral' }
> = {
  idle: { label: 'stream idle', tone: 'dim' },
  connecting: { label: 'connecting', tone: 'neutral' },
  open: { label: 'stream live', tone: 'lime' },
  reconnecting: { label: 'reconnecting', tone: 'amber' },
  closed: { label: 'stream closed', tone: 'dim' },
};

function eventsSorted(fetched: EventDto[], streamed: EventDto[]): EventDto[] {
  const byId = new Map<string, EventDto>();
  for (const event of fetched) byId.set(event.event_id, event);
  for (const event of streamed) byId.set(event.event_id, event);
  return [...byId.values()].sort((a, b) => a.seq - b.seq);
}

export function RunDetailPage() {
  const { id, runId } = useParams<{ id: string; runId: string }>();
  const runQuery = useRun(runId);
  const evidence = useRunEvidence(id, runId);
  const results = useResults(id, 50);
  const session = useCurrentSession();
  const [cancelOpen, setCancelOpen] = useState(false);
  const [resumeOpen, setResumeOpen] = useState(false);
  const [continueOpen, setContinueOpen] = useState(false);

  const run = runQuery.data?.run;
  const progress = useAgentProgress(
    runId,
    Boolean(run && run.kind === 'agent' && ['queued', 'running', 'cancelling'].includes(run.state)),
    Boolean(run && run.kind === 'agent'),
  );
  useEffect(() => {
    if (!run || run.kind !== 'agent') return;
    if (!['queued', 'running', 'cancelling'].includes(run.state)) {
      void progress.refetch();
      void evidence.refetch();
    }
  }, [run?.state, run?.kind]);
  const runIsTerminal = run ? isTerminalRun(run.state) : false;
  const afterSeq = (runQuery.data?.events ?? []).reduce(
    (max, event) => Math.max(max, event.seq),
    0,
  );
  const streamEnabled = Boolean(id && run) && (!runIsTerminal || Boolean(run?.effect_unknown));
  const stream = useEventStream({
    assessmentId: id,
    enabled: streamEnabled,
    runId,
    afterSeq,
  });

  const mergedEvents = useMemo(
    () => eventsSorted(runQuery.data?.events ?? [], stream.events),
    [runQuery.data?.events, stream.events],
  );

  if (runQuery.isError) {
    return (
      <ErrorState
        error={runQuery.error}
        label="Run could not be loaded."
        onRetry={() => void runQuery.refetch()}
      />
    );
  }

  if (runQuery.isLoading || !run || !id) {
    return (
      <>
        <PageHeader meta="run" title="Loading run" />
        <BlockSkeleton lines={8} />
      </>
    );
  }

  const canWrite = session.data?.can_write ?? false;
  const steps = [...(runQuery.data?.steps ?? [])].sort((a, b) => a.ordinal - b.ordinal);
  const stepsDone = steps.filter((step) =>
    ['succeeded', 'failed', 'skipped', 'blocked'].includes(step.state),
  ).length;
  const runEvidence = evidence.data ?? [];
  const evidenceIds = new Set(runEvidence.map((item) => item.id));
  const runResults = (results.data ?? []).filter((result) =>
    result.evidence_ids.some((evidenceId) => evidenceIds.has(evidenceId)),
  );
  const elapsedSeconds = run.started_at
    ? Math.max(
        0,
        ((run.finished_at ? new Date(run.finished_at).getTime() : Date.now()) -
          new Date(run.started_at).getTime()) /
          1000,
      )
    : 0;
  const streamState = STREAM_LABELS[stream.status];

  const budgets = [
    {
      label: 'Requests',
      spent: formatNumber(run.spent.requests),
      max: formatNumber(run.budget.max_requests),
      percent: ratioPercent(run.spent.requests, run.budget.max_requests),
    },
    {
      label: 'Browser steps',
      spent: formatNumber(run.spent.browser_steps),
      max: formatNumber(run.budget.max_browser_steps),
      percent: ratioPercent(run.spent.browser_steps, run.budget.max_browser_steps),
    },
    {
      label: 'Model calls',
      spent: formatNumber(run.spent.model_calls),
      max: formatNumber(run.budget.max_model_calls),
      percent: ratioPercent(run.spent.model_calls, run.budget.max_model_calls),
    },
    {
      label: 'Cost',
      spent: formatCostMicros(run.spent.cost_micros),
      max: formatCostMicros(run.budget.max_cost_micros),
      percent: ratioPercent(run.spent.cost_micros, run.budget.max_cost_micros),
    },
    {
      label: 'Artifacts',
      spent: formatBytes(run.spent.artifact_bytes),
      max: formatBytes(run.budget.max_artifact_bytes),
      percent: ratioPercent(run.spent.artifact_bytes, run.budget.max_artifact_bytes),
    },
    {
      label: 'Elapsed',
      spent: formatDuration(elapsedSeconds),
      max: formatDuration(run.budget.max_duration_seconds),
      percent: ratioPercent(elapsedSeconds, run.budget.max_duration_seconds),
    },
  ];

  return (
    <>
      <PageHeader
        meta={`run · ${shortId(run.id)} · policy v${run.policy_version}`}
        title={humanize(run.kind).replace(/^./, (letter) => letter.toUpperCase())}
        description={
          <span className="run-head-meta">
            <Status value={run.state} />
            <span className="mono">{humanize(run.kind)}</span>
            {run.procedure_key ? <span className="mono">{run.procedure_key}</span> : null}
            <span className="mono quiet">created {formatRelative(run.created_at)}</span>
          </span>
        }
        actions={
          <>
            {RESUMABLE.includes(run.state) ? (
              <button
                type="button"
                className="btn"
                onClick={() => setResumeOpen(true)}
                disabled={!canWrite}
                title={canWrite ? undefined : 'Your role cannot resume runs.'}
              >
                <RotateCw size={14} strokeWidth={1.5} aria-hidden="true" />
                Resume
              </button>
            ) : null}
            {run.kind === 'agent' &&
            (run.state === 'failed' ||
              run.state === 'cancelled' ||
              run.state === 'waiting_operator' ||
              run.state === 'interrupted') ? (
              <button
                type="button"
                className="btn"
                onClick={() => setContinueOpen(true)}
                disabled={!canWrite}
                title="Continue from the persisted state in a new session."
              >
                Continue objective
              </button>
            ) : null}
            {CANCELLABLE.includes(run.state) ? (
              <button
                type="button"
                className="btn btn-danger"
                onClick={() => setCancelOpen(true)}
                disabled={!canWrite}
                title={canWrite ? undefined : 'Your role cannot cancel runs.'}
              >
                <Ban size={14} strokeWidth={1.5} aria-hidden="true" />
                Cancel run
              </button>
            ) : null}
          </>
        }
      />

      {run.state === 'cancelling' ? (
        <div className="callout" data-tone="amber">
          <strong>Cancellation requested — not cancelled</strong>
          <p>
            The server has accepted the cancel request. The run is not cancelled until the
            current step finishes and the state transition is recorded.
          </p>
        </div>
      ) : null}

      {run.effect_unknown ? (
        <div className="callout" data-tone="red">
          <strong>External effect outcome unknown</strong>
          <p>
            An external effect may have been applied but its result was not confirmed. The
            platform will not repeat that effect automatically.
            {run.state === 'waiting_operator'
              ? ' The run is waiting on an operator: reconcile the target state, then resume with the explicit acknowledgement.'
              : ' Treat the target state as unverified until it is reconciled.'}
          </p>
          {run.state === 'waiting_operator' && canWrite ? (
            <button type="button" className="btn" onClick={() => setResumeOpen(true)}>
              Reconcile and resume
            </button>
          ) : null}
        </div>
      ) : null}

      {run.kind === 'agent' ? (
        <>
          <AgentExploration
            run={run}
            events={mergedEvents}
            progress={progress.data}
            evidence={runEvidence}
          />
          <AgentTranscript events={mergedEvents} assessmentId={id} state={run.state} />
          {!runIsTerminal && run.state !== 'cancelling' ? (
            <div className="notice">
              <strong>The objective is not complete.</strong>
              <p>
                Cancelling stops new actions, aborts the model call and closes the browser context.
                Work already captured stays available, and the checkpoint can seed a new session.
              </p>
            </div>
          ) : null}
        </>
      ) : null}

      <Panel title="Budget">
        <div className="budget-grid">
          {budgets.map((budget) => {
            const percent = budget.percent;
            const tone =
              percent === null ? 'idle' : percent > 100 ? 'over' : percent > 80 ? 'warn' : 'ok';
            return (
              <div className="budget-item" key={budget.label}>
                <div className="budget-head">
                  <span className="budget-label mono">{budget.label}</span>
                  <span className="budget-values mono">
                    {budget.spent} <span className="quiet">/ {budget.max}</span>
                  </span>
                </div>
                <span className="budget-track" aria-hidden="true">
                  <span
                    className="budget-fill"
                    data-tone={tone}
                    style={{ width: `${Math.min(100, percent ?? 0)}%` }}
                  />
                </span>
                <span className="budget-percent mono">
                  {percent === null ? '—' : `${percent.toFixed(0)}%`}
                </span>
              </div>
            );
          })}
        </div>
      </Panel>

      <div className="split">
        <div className="split-main">
          <Panel
            title="Steps"
            subtitle={`${stepsDone} of ${run.step_count} complete`}
            flush
          >
            {steps.length === 0 ? (
              <EmptyState
                action={
                  <button
                    type="button"
                    className="btn"
                    onClick={() => void runQuery.refetch()}
                  >
                    Refresh
                  </button>
                }
              >
                No steps recorded yet.
              </EmptyState>
            ) : (
              <ol className="timeline">
                {steps.map((step) => (
                  <li className="timeline-item" key={step.id}>
                    <span className="timeline-ordinal mono">
                      {String(step.ordinal).padStart(2, '0')}
                    </span>
                    <div className="timeline-body">
                      <div className="timeline-head">
                        <Status value={step.state} />
                        <Status value={step.effect_status} />
                        <span className="mono quiet">attempt {step.attempt}</span>
                        <span className="mono quiet">{humanize(step.kind)}</span>
                      </div>
                      <p className="timeline-summary">{step.summary || 'No summary recorded.'}</p>
                      <div className="timeline-meta mono">
                        <span>{step.started_at ? formatRelative(step.started_at) : 'not started'}</span>
                        <span>{formatDurationBetween(step.started_at, step.finished_at)}</span>
                        <span>{shortId(step.id)}</span>
                      </div>
                    </div>
                  </li>
                ))}
              </ol>
            )}
          </Panel>

          <Panel
            title="Event stream"
            actions={
              <span className="stream-state mono" title={stream.error ?? undefined}>
                <LiveDot tone={streamState.tone === 'neutral' ? 'neutral' : streamState.tone} />
                {streamState.label}
              </span>
            }
            flush
          >
            {mergedEvents.length === 0 ? (
              <EmptyState>No events recorded for this run.</EmptyState>
            ) : (
              <ul className="event-list">
                {mergedEvents.map((event) => (
                  <li key={event.event_id} className="event-item">
                    <span className="event-seq mono">#{event.seq}</span>
                    <div className="event-body">
                      <div className="event-head">
                        <span className="event-type mono">{event.event_type}</span>
                        <span className="mono quiet">{formatRelative(event.occurred_at)}</span>
                        {event.step_id ? (
                          <span className="mono quiet">step {shortId(event.step_id)}</span>
                        ) : null}
                      </div>
                      <details className="event-payload">
                        <summary className="mono">payload</summary>
                        <pre>{JSON.stringify(event.payload, null, 2)}</pre>
                      </details>
                    </div>
                  </li>
                ))}
              </ul>
            )}
            {stream.error ? (
              <p className="stream-error mono" role="status">
                {stream.error}
              </p>
            ) : null}
          </Panel>
        </div>

        <div className="split-side">
          <Panel title="Run">
            <KeyValue
              items={[
                { label: 'State', value: <Status value={run.state} /> },
                { label: 'Kind', value: <span className="mono">{humanize(run.kind)}</span> },
                {
                  label: 'Procedure',
                  value: run.procedure_key ? (
                    <span className="mono">{run.procedure_key}</span>
                  ) : (
                    <span className="muted">none</span>
                  ),
                },
                {
                  label: 'Policy version',
                  value: <span className="mono">v{run.policy_version}</span>,
                },
                { label: 'Created', value: formatDateTime(run.created_at) },
                {
                  label: 'Started',
                  value: run.started_at ? (
                    formatDateTime(run.started_at)
                  ) : (
                    <span className="muted">not started</span>
                  ),
                },
                {
                  label: 'Finished',
                  value: run.finished_at ? (
                    formatDateTime(run.finished_at)
                  ) : (
                    <span className="muted">in progress</span>
                  ),
                },
                {
                  label: 'Duration',
                  value: formatDurationBetween(run.started_at, run.finished_at),
                },
                {
                  label: 'Reason',
                  value: run.reason ? (
                    <span>{run.reason}</span>
                  ) : (
                    <span className="muted">none</span>
                  ),
                },
                {
                  label: 'Idempotency key',
                  value: (
                    <span className="hash-row">
                      <span className="mono break-any">{run.idempotency_key}</span>
                      <CopyButton value={run.idempotency_key} />
                    </span>
                  ),
                },
                { label: 'Run ID', value: <IdChip id={run.id} length={12} /> },
                {
                  label: 'Assessment',
                  value: (
                    <Link to={`/assessments/${id}`} className="mono">
                      {shortId(run.assessment_id)}
                    </Link>
                  ),
                },
              ]}
            />
          </Panel>

          <Panel
            title="Partial results"
            subtitle="Results linked by evidence captured in this run."
            actions={
              <button
                type="button"
                className="btn btn-ghost"
                onClick={() => {
                  void runQuery.refetch();
                  void evidence.refetch();
                  void results.refetch();
                }}
              >
                Refresh
              </button>
            }
            flush
          >
            {evidence.isError || results.isError ? (
              <div className="panel-pad">
                <ErrorState
                  error={evidence.error ?? results.error}
                  label="Partial results could not be loaded."
                  onRetry={() => {
                    void evidence.refetch();
                    void results.refetch();
                  }}
                />
              </div>
            ) : evidence.isLoading || results.isLoading ? (
              <div className="panel-pad">
                <BlockSkeleton lines={4} />
              </div>
            ) : runEvidence.length === 0 && runResults.length === 0 ? (
              <EmptyState>No results recorded for this run yet.</EmptyState>
            ) : (
              <div className="partial-results">
                {runEvidence.length > 0 ? (
                  <div className="partial-group">
                    <h3 className="drawer-section-title mono">
                      Evidence
                      <span className="section-count mono">{runEvidence.length}</span>
                    </h3>
                    <ul className="link-list">
                      {runEvidence.map((item) => (
                        <li key={item.id}>
                          <Link
                            to={`/assessments/${id}/evidence?focus=${item.id}`}
                            className="mono"
                          >
                            {item.kind.replace(/_/g, ' ')}
                          </Link>
                          <span className="mono quiet">{formatBytes(item.size_bytes)}</span>
                        </li>
                      ))}
                    </ul>
                  </div>
                ) : null}
                {runResults.length > 0 ? (
                  <div className="partial-group">
                    <h3 className="drawer-section-title mono">
                      Test results
                      <span className="section-count mono">{runResults.length}</span>
                    </h3>
                    <ul className="result-list">
                      {runResults.map((result) => (
                        <li key={result.id}>
                          <div className="result-head">
                            <span className="result-procedure mono">{result.procedure_key}</span>
                            <Status value={result.state} />
                          </div>
                          <p className="result-conclusion">
                            {result.conclusion || 'No conclusion recorded.'}
                          </p>
                        </li>
                      ))}
                    </ul>
                  </div>
                ) : null}
              </div>
            )}
          </Panel>
        </div>
      </div>

      <CancelRunDialog open={cancelOpen} onClose={() => setCancelOpen(false)} run={run} />
      <ResumeRunDialog open={resumeOpen} onClose={() => setResumeOpen(false)} run={run} />
      {id ? (
        <AgentTurnDialog
          open={continueOpen}
          onClose={() => setContinueOpen(false)}
          assessmentId={id}
          continueFrom={run.id}
          initialObjective={
            (() => {
              const started = mergedEvents.find((event) => event.event_type === 'agent.started');
              const value =
                started && typeof started.payload === 'object' && started.payload !== null
                  ? (started.payload as Record<string, unknown>).objective
                  : '';
              return typeof value === 'string'
                ? `${value} Continue from the checkpoint; do not repeat an action whose outcome is unknown.`
                : 'Continue from the persisted checkpoint.';
            })()
          }
        />
      ) : null}
    </>
  );
}
