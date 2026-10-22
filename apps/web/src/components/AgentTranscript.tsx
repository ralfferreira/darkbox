import { Link } from 'react-router-dom';
import type { EventDto } from '../api/types';
import { formatNumber, formatRelative } from '../lib/format';
import { Panel } from './ui';

function record(value: unknown): Record<string, unknown> | null {
  return typeof value === 'object' && value !== null && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : null;
}

function text(value: unknown): string {
  return typeof value === 'string' ? value : '';
}

export function AgentTranscript({
  events,
  assessmentId,
  state,
}: {
  events: EventDto[];
  assessmentId: string;
  state: string;
}) {
  const agentEvents = events.filter((event) => event.event_type.startsWith('agent.'));
  const started = agentEvents.find((event) => event.event_type === 'agent.started');
  const startPayload = record(started?.payload);
  const finished = [...agentEvents].reverse().find((event) => event.event_type === 'agent.finished');
  const usageEvents = agentEvents.filter((event) => event.event_type === 'agent.usage');
  const settledUsage = usageEvents.filter(
    (event) => record(event.payload)?.phase === 'settle',
  );
  const unknownUsage = settledUsage.some(
    (event) => record(event.payload)?.usage_known === false,
  );
  const usage = settledUsage.reduce(
    (total, event) => {
      const payload = record(event.payload);
      return {
        input: total.input + Number(payload?.input_tokens ?? 0),
        output: total.output + Number(payload?.output_tokens ?? 0),
      };
    },
    { input: 0, output: 0 },
  );

  return (
    <Panel
      title="Agent turn"
      subtitle={
        startPayload
          ? text(startPayload.model) === 'deterministic-planner'
            ? 'deterministic planner · no model tokens; browser actions are real'
            : usage.input + usage.output > 0
              ? `${text(startPayload.model) || 'model'} · ${formatNumber(usage.input)} in / ${formatNumber(usage.output)} out tokens${unknownUsage ? ' · partial, provider omitted some usage' : ''}`
              : `${text(startPayload.model) || 'model'} · usage not reported by provider`
          : 'The transcript is recorded as events; evidence and tool calls stay inspectable.'
      }
    >
      <div className="agent-transcript">
        {startPayload ? (
          <div className="agent-objective">
            <span className="agent-role mono">objective</span>
            <p>{text(startPayload.objective)}</p>
          </div>
        ) : null}

        {agentEvents
          .filter((event) =>
            ['agent.message', 'agent.tool_call', 'agent.tool_result', 'agent.finished'].includes(
              event.event_type,
            ),
          )
          .map((event) => {
            const payload = record(event.payload) ?? {};
            if (event.event_type === 'agent.message') {
              return (
                <div className="agent-step" key={event.event_id}>
                  <div className="agent-step-head">
                    <span className="agent-role mono">agent</span>
                    <span className="agent-time mono">{formatRelative(event.occurred_at)}</span>
                  </div>
                  <p className="agent-message">{text(payload.content)}</p>
                </div>
              );
            }
            if (event.event_type === 'agent.tool_call') {
              const evidence = Array.isArray(payload.evidence_ids)
                ? payload.evidence_ids.filter((value): value is string => typeof value === 'string')
                : [];
              return (
                <div className="agent-step agent-step-tool" key={event.event_id}>
                  <div className="agent-step-head">
                    <span className="agent-role mono">tool · {text(payload.name)}</span>
                    <span className="agent-time mono">{formatRelative(event.occurred_at)}</span>
                  </div>
                  <p className="agent-tool-summary">{text(payload.summary)}</p>
                  <details className="agent-tool-args">
                    <summary className="mono">arguments</summary>
                    <pre>{JSON.stringify(payload.arguments ?? {}, null, 2)}</pre>
                  </details>
                  {evidence.length > 0 ? (
                    <div className="agent-evidence">
                      {evidence.map((id) => (
                        <Link
                          key={id}
                          className="chip mono"
                          to={`/assessments/${assessmentId}/evidence?focus=${id}`}
                        >
                          evidence {id.slice(-8)}
                        </Link>
                      ))}
                    </div>
                  ) : null}
                </div>
              );
            }
            if (event.event_type === 'agent.tool_result') {
              return (
                <div className="agent-step agent-step-error" key={event.event_id}>
                  <div className="agent-step-head">
                    <span className="agent-role mono">tool error · {text(payload.name)}</span>
                    <span className="agent-time mono">{formatRelative(event.occurred_at)}</span>
                  </div>
                  <p className="agent-tool-summary">{text(payload.error)}</p>
                </div>
              );
            }
            return (
              <div className="agent-final" key={event.event_id}>
                <div className="agent-step-head">
                  <span className="agent-role mono">checkpoint</span>
                  <span className="agent-time mono">{formatRelative(event.occurred_at)}</span>
                </div>
                <p className="agent-message">{text(payload.summary) || text(payload.reason)}</p>
                {Array.isArray(payload.next_steps) && payload.next_steps.length > 0 ? (
                  <ul className="agent-next">
                    {payload.next_steps.map((step) => (
                      <li key={String(step)}>{String(step)}</li>
                    ))}
                  </ul>
                ) : null}
                {text(payload.memory_slug) ? (
                  <Link
                    className="mono"
                    to={`/assessments/${assessmentId}/memory?page=${encodeURIComponent(
                      text(payload.memory_slug),
                    )}`}
                  >
                    open memory page
                  </Link>
                ) : null}
              </div>
            );
          })}

        {!finished && ['queued', 'running'].includes(state) ? (
          <div className="agent-working">
            <span className="agent-pulse" aria-hidden="true" />
            <span className="mono">agent is working; this transcript updates as events arrive</span>
          </div>
        ) : null}

      </div>
    </Panel>
  );
}
