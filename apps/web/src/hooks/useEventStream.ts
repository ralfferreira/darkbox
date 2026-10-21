import { useEffect, useRef, useState } from 'react';
import { clearToken, getToken, UNAUTHORIZED_EVENT } from '../api/client';
import type { EventDto } from '../api/types';

export type StreamStatus = 'idle' | 'connecting' | 'open' | 'reconnecting' | 'closed';

interface UseEventStreamOptions {
  assessmentId: string | undefined;
  enabled: boolean;
  /** Events for other runs are dropped from state but still advance the cursor. */
  runId?: string;
  /** Start cursor (typically the highest seq already fetched). */
  afterSeq?: number;
  onEvents?: (events: EventDto[]) => void;
}

interface UseEventStreamResult {
  events: EventDto[];
  status: StreamStatus;
  error: string | null;
  lastSeq: number;
}

interface ParsedEvent {
  eventId: string;
  seq: number;
  data: EventDto;
}

function parseFrame(frame: string): ParsedEvent | null {
  let data = '';
  for (const line of frame.split('\n')) {
    if (line.startsWith('data:')) {
      data += `${line.slice(5).trimStart()}\n`;
    }
  }
  if (!data.trim()) return null;
  try {
    const parsed: unknown = JSON.parse(data);
    if (typeof parsed !== 'object' || parsed === null) return null;
    const record = parsed as Record<string, unknown>;
    if (typeof record.event_id !== 'string' || typeof record.seq !== 'number') return null;
    return { eventId: record.event_id, seq: record.seq, data: parsed as EventDto };
  } catch {
    return null;
  }
}

export function useEventStream({
  assessmentId,
  enabled,
  runId,
  afterSeq,
  onEvents,
}: UseEventStreamOptions): UseEventStreamResult {
  const [events, setEvents] = useState<EventDto[]>([]);
  const [status, setStatus] = useState<StreamStatus>('idle');
  const [error, setError] = useState<string | null>(null);
  const [lastSeq, setLastSeq] = useState(afterSeq ?? 0);

  const cursorRef = useRef(afterSeq ?? 0);
  const seenRef = useRef(new Set<string>());
  const callbackRef = useRef(onEvents);
  const runRef = useRef(runId);

  useEffect(() => {
    callbackRef.current = onEvents;
    runRef.current = runId;
  }, [onEvents, runId]);

  useEffect(() => {
    if (afterSeq !== undefined && afterSeq > cursorRef.current) {
      cursorRef.current = afterSeq;
      setLastSeq(afterSeq);
    }
  }, [afterSeq]);

  useEffect(() => {
    if (!enabled || !assessmentId) {
      setStatus('idle');
      return;
    }

    setEvents([]);
    seenRef.current = new Set();
    setStatus('connecting');
    setError(null);

    let disposed = false;
    let attempt = 0;
    let controller: AbortController | null = null;
    let retryTimer: number | null = null;

    const schedule = () => {
      if (disposed) return;
      const delay = Math.min(1000 * 2 ** attempt, 15_000);
      attempt += 1;
      setStatus('reconnecting');
      retryTimer = window.setTimeout(() => {
        void connect();
      }, delay);
    };

    const connect = async (): Promise<void> => {
      if (disposed) return;
      controller = new AbortController();
      setStatus(attempt === 0 ? 'connecting' : 'reconnecting');

      const token = getToken();
      const headers: Record<string, string> = { Accept: 'text/event-stream' };
      if (token) headers.Authorization = `Bearer ${token}`;

      try {
        const response = await fetch(
          `/v1/assessments/${assessmentId}/events?after=${cursorRef.current}`,
          { headers, signal: controller.signal },
        );

        if (response.status === 401) {
          clearToken();
          window.dispatchEvent(new CustomEvent(UNAUTHORIZED_EVENT));
          throw new Error('Session expired.');
        }
        if (!response.ok) {
          throw new Error(`Event stream returned ${response.status}.`);
        }
        if (!response.body) throw new Error('Event stream is unavailable.');

        attempt = 0;
        setStatus('open');
        setError(null);

        const reader = response.body.getReader();
        const decoder = new TextDecoder();
        let buffer = '';

        for (;;) {
          const { value, done } = await reader.read();
          if (done || disposed) break;
          buffer += decoder.decode(value, { stream: true });

          let boundary = buffer.search(/\r?\n\r?\n/);
          while (boundary !== -1) {
            const match = /\r?\n\r?\n/.exec(buffer);
            const frame = buffer.slice(0, boundary);
            buffer = buffer.slice(boundary + (match?.[0].length ?? 2));
            boundary = buffer.search(/\r?\n\r?\n/);

            const parsed = parseFrame(frame);
            if (!parsed) continue;
            if (seenRef.current.has(parsed.eventId)) continue;
            seenRef.current.add(parsed.eventId);
            if (parsed.seq > cursorRef.current) cursorRef.current = parsed.seq;
            setLastSeq(cursorRef.current);

            if (runRef.current && parsed.data.run_id !== runRef.current) continue;

            setEvents((previous) => [...previous, parsed.data].slice(-400));
            callbackRef.current?.([parsed.data]);
          }
        }

        if (!disposed) schedule();
      } catch (caught) {
        if (caught instanceof DOMException && caught.name === 'AbortError') return;
        if (disposed) return;
        setError(caught instanceof Error ? caught.message : 'Event stream disconnected.');
        schedule();
      }
    };

    void connect();

    return () => {
      disposed = true;
      controller?.abort();
      if (retryTimer !== null) window.clearTimeout(retryTimer);
      setStatus('closed');
    };
  }, [assessmentId, enabled, runId]);

  return { events, status, error, lastSeq };
}
