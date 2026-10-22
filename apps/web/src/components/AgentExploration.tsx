import { useEffect, useState } from 'react';
import { useQuery } from '@tanstack/react-query';
import { Link } from 'react-router-dom';
import { requestBlob } from '../api/client';
import type {
  AgentProgress,
  EventDto,
  EvidenceDto,
  RunSummary,
} from '../api/types';
import { formatBytes, formatRelative, shortId } from '../lib/format';
import { Panel } from './ui';

function record(value: unknown): Record<string, unknown> | null {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function text(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

function EvidenceImage({ evidence }: { evidence: EvidenceDto }) {
  const image = useQuery({
    queryKey: ['evidence-image', evidence.id],
    queryFn: () => requestBlob(`/evidence/${evidence.id}/content`),
    staleTime: Infinity,
    retry: false,
  });
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    if (!image.data) {
      setUrl(null);
      return;
    }
    const next = URL.createObjectURL(image.data);
    setUrl(next);
    return () => URL.revokeObjectURL(next);
  }, [image.data]);
  if (image.isLoading) {
    return <div className="browser-shot is-loading mono">loading screenshot…</div>;
  }
  if (image.isError || !url) {
    return <div className="browser-shot is-loading mono">screenshot unavailable</div>;
  }
  return (
    <figure className="browser-shot">
      <img src={url} alt={`Rendered page at ${formatRelative(evidence.created_at)}`} />
      <figcaption className="mono">
        {formatRelative(evidence.created_at)} · {formatBytes(evidence.size_bytes)} ·{' '}
        <Link to={`/assessments/${evidence.assessment_id}/evidence?focus=${evidence.id}`}>
          open evidence
        </Link>
      </figcaption>
    </figure>
  );
}

export function AgentExploration({
  run,
  events,
  progress,
  evidence,
}: {
  run: RunSummary;
  events: EventDto[];
  progress: AgentProgress | undefined;
  evidence: EvidenceDto[];
}) {
  const started = events.find((event) => event.event_type === 'agent.started');
  const startPayload = record(started?.payload);
  const objective = text(startPayload?.objective) || text(progress?.objective);
  const actions = progress?.actions ?? [];
  const latest = actions[actions.length - 1];
  const currentObservation =
    record(progress?.current?.observation) ?? record(latest?.observation);
  const currentUrl = text(progress?.current?.url) || text(currentObservation?.url);
  const currentTitle = text(progress?.current?.title) || text(currentObservation?.title);
  const screenshot = evidence.find(
    (item) => item.kind === 'browser_screenshot' && item.content_type.startsWith('image/'),
  );
  const decisions = events
    .filter((event) => event.event_type === 'agent.decision')
    .map((event) => record(event.payload))
    .filter((payload): payload is Record<string, unknown> => Boolean(payload));
  const checkpoint = [...events]
    .reverse()
    .find((event) => event.event_type === 'agent.checkpoint');
  const checkpointPayload = record(checkpoint?.payload);
  const actionEvents = events.filter((event) => event.event_type === 'agent.action');
  const session = record(currentObservation?.session);
  const engineInfo = record(currentObservation?.engine);
  const engine = text(engineInfo?.engine);
  const identityLabel = text(session?.identity_label);
  const openSessions = new Set(
    actionEvents.map((event) => text(record(event.payload)?.session_key)).filter(Boolean),
  );
  const blocked = actionEvents.filter(
    (event) => text(record(event.payload)?.state) === 'blocked',
  );
  const pending = actions.filter((action) => action.state === 'running');
  const observable = record(currentObservation?.auth);
  const authState = observable
    ? observable.session_state === 'authenticated' || observable.likely_authenticated
      ? 'authenticated'
      : observable.login_form
        ? 'login form visible'
        : observable.session_state === 'expired'
          ? 'expired'
          : 'not confirmed'
    : 'not observed';

  return (
    <Panel
      title="Live exploration"
      subtitle={
        objective
          ? objective
          : 'The browser, action trail and evidence appear here while the objective runs.'
      }
      actions={
        <span className="stream-state mono">
          <span className={`agent-pulse${run.state === 'running' ? '' : ' is-idle'}`} aria-hidden="true" />
          {run.state}
        </span>
      }
    >
      <div className="explore-shell">
        <div className="explore-view">
          <div className="explore-chrome">
            <span className="explore-dots" aria-hidden="true">
              <i />
              <i />
              <i />
            </span>
            <span className="explore-url mono" title={currentUrl || 'no page yet'}>
              {currentUrl || 'waiting for the first page'}
            </span>
            <span className="mono quiet">{engine || 'browser engine pending'}</span>
          </div>
          <div className="explore-page">
            <div className="explore-page-head">
              <div>
                <span className="explore-eyebrow mono">current page</span>
                <h3>{currentTitle || 'No page observed yet'}</h3>
              </div>
              <div className="explore-facts mono">
                <span>identity · {identityLabel || text(progress?.identity_id) || 'none'}</span>
                <span>session · {session ? shortId(text(session.context_key)) : 'not open'}</span>
                <span>auth · {authState}</span>
              </div>
            </div>
            {screenshot ? (
              <EvidenceImage evidence={screenshot} />
            ) : (
              <div className="browser-shot is-loading mono">
                The first screenshot is captured when the agent opens a page.
              </div>
            )}
            <p className="explore-excerpt">
              {text(currentObservation?.text).slice(0, 480) ||
                'The rendered page text will appear here after the first observation.'}
            </p>
          </div>
        </div>

        <div className="explore-rail">
          <section className="explore-block">
            <h4 className="mono">Objective state</h4>
            <dl className="explore-stats">
              <div>
                <dt className="mono">actions</dt>
                <dd className="mono">{actions.length}</dd>
              </div>
              <div>
                <dt className="mono">evidence</dt>
                <dd className="mono">{evidence.length}</dd>
              </div>
              <div>
                <dt className="mono">sessions</dt>
                <dd className="mono">{openSessions.size}</dd>
              </div>
              <div>
                <dt className="mono">blocked</dt>
                <dd className="mono">{blocked.length}</dd>
              </div>
            </dl>
            <p className="explore-note mono">
              {pending.length > 0
                ? `${pending.length} action in progress`
                : run.state === 'running'
                  ? 'agent is choosing the next action'
                  : 'no action is running'}
            </p>
          </section>

          <section className="explore-block">
            <h4 className="mono">Decision trail</h4>
            {decisions.length === 0 ? (
              <p className="explore-note">Short decision summaries appear after each step.</p>
            ) : (
              <ol className="explore-decisions">
                {decisions.slice(-6).map((decision, index) => (
                  <li key={`${text(decision.summary)}-${index}`}>
                    <span className="mono quiet">why</span>
                    <p>{text(decision.summary)}</p>
                  </li>
                ))}
              </ol>
            )}
          </section>

          {checkpointPayload ? (
            <section className="explore-block">
              <h4 className="mono">Checkpoint</h4>
              <p className="explore-note">
                Saved as <span className="mono">{text(checkpointPayload.slug)}</span>. A new
                session can continue from this state.
              </p>
              <Link
                className="mono"
                to={`/assessments/${run.assessment_id}/memory?page=${encodeURIComponent(
                  text(checkpointPayload.slug),
                )}`}
              >
                open checkpoint
              </Link>
            </section>
          ) : null}
        </div>
      </div>

      <div className="action-trail">
        {actions.length === 0 ? (
          <p className="drawer-empty">
            No browser action yet. Opening the session is the first action.
          </p>
        ) : (
          <ol>
            {actions.map((action) => {
              const observation = record(action.observation);
              return (
                <li key={action.id} data-state={action.state}>
                  <span className="action-ordinal mono">
                    {String(action.ordinal).padStart(2, '0')}
                  </span>
                  <div className="action-body">
                    <div className="action-head">
                      <span className="mono">{action.kind}</span>
                      <span className="mono" data-state={action.state}>
                        {action.state}
                      </span>
                      <span className="mono quiet">{formatRelative(action.started_at)}</span>
                    </div>
                    <p>{action.summary}</p>
                    {text(observation?.url) ? (
                      <p className="action-url mono">{text(observation?.url)}</p>
                    ) : null}
                    {action.evidence_ids.length > 0 ? (
                      <div className="agent-evidence">
                        {action.evidence_ids.map((id) => (
                          <Link
                            key={id}
                            className="chip mono"
                            to={`/assessments/${run.assessment_id}/evidence?focus=${id}`}
                          >
                            evidence {id.slice(-8)}
                          </Link>
                        ))}
                      </div>
                    ) : null}
                  </div>
                </li>
              );
            })}
          </ol>
        )}
      </div>
    </Panel>
  );
}
