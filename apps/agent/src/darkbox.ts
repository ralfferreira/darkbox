import type { ContextPackage, RunSummary } from './types.js';

export class DarkBoxApi {
  constructor(
    private readonly baseUrl: string,
    private readonly token: string,
  ) {}

  private async request<T>(path: string, init?: RequestInit): Promise<T> {
    const response = await fetch(`${this.baseUrl}${path}`, {
      ...init,
      headers: {
        Authorization: `Bearer ${this.token}`,
        'Content-Type': 'application/json',
        ...(init?.headers ?? {}),
      },
    });
    const body = await response.text();
    if (!response.ok) {
      throw new Error(`DarkBox ${response.status} ${path}: ${body.slice(0, 800)}`);
    }
    return (body ? JSON.parse(body) : {}) as T;
  }

  get<T>(path: string): Promise<T> {
    return this.request<T>(path);
  }

  post<T>(path: string, body: unknown): Promise<T> {
    return this.request<T>(path, { method: 'POST', body: JSON.stringify(body) });
  }

  async text(path: string, headers: Record<string, string> = {}): Promise<string> {
    const response = await fetch(`${this.baseUrl}${path}`, {
      headers: { Authorization: `Bearer ${this.token}`, ...headers },
    });
    if (!response.ok) {
      throw new Error(`DarkBox ${response.status} ${path}`);
    }
    return response.text();
  }

  async bytes(path: string): Promise<{ bytes: Uint8Array; contentType: string }> {
    const response = await fetch(`${this.baseUrl}${path}`, {
      headers: { Authorization: `Bearer ${this.token}` },
    });
    if (!response.ok) {
      throw new Error(`DarkBox ${response.status} ${path}`);
    }
    return {
      bytes: new Uint8Array(await response.arrayBuffer()),
      contentType: response.headers.get('content-type') ?? 'application/octet-stream',
    };
  }

  async createAgentTurn(assessmentId: string, body: unknown) {
    return this.post<{ run: RunSummary; context: ContextPackage }>(
      `/v1/assessments/${assessmentId}/agent/turns`,
      body,
    );
  }

  async event(runId: string, eventType: string, payload: unknown) {
    return this.post(`/v1/runs/${runId}/agent/events`, {
      event_type: eventType,
      payload,
    });
  }

  async finish(
    runId: string,
    summary: string,
    nextSteps: string[],
    evidenceIds: string[],
    outcome = 'succeeded',
  ) {
    return this.post<{
      run: RunSummary;
      page: { slug: string; title: string; version: number };
      flow_id: string | null;
    }>(`/v1/runs/${runId}/agent/finish`, {
      summary,
      next_steps: nextSteps,
      evidence_ids: evidenceIds,
      outcome,
    });
  }

  async reserveUsage(runId: string, callId: string, model: string) {
    return this.post<RunSummary>(`/v1/runs/${runId}/agent/usage`, {
      phase: 'reserve',
      call_id: callId,
      model,
      usage_known: false,
    });
  }

  async settleUsage(
    runId: string,
    callId: string,
    model: string,
    usage: {
      inputTokens?: number | null;
      outputTokens?: number | null;
      totalTokens?: number | null;
    },
  ) {
    const known =
      usage.totalTokens !== null &&
      usage.totalTokens !== undefined &&
      usage.inputTokens !== undefined &&
      usage.outputTokens !== undefined;
    return this.post<RunSummary>(`/v1/runs/${runId}/agent/usage`, {
      phase: 'settle',
      call_id: callId,
      model,
      input_tokens: usage.inputTokens ?? 0,
      output_tokens: usage.outputTokens ?? 0,
      total_tokens: usage.totalTokens ?? 0,
      usage_known: known,
    });
  }

  async abort(runId: string, reason: string) {
    return this.post(`/v1/runs/${runId}/agent/abort`, { reason });
  }

  async getRun(runId: string) {
    return this.get<{
      run: RunSummary;
      steps: unknown[];
      events: Array<{ event_type: string; payload: Record<string, unknown> }>;
    }>(`/v1/runs/${runId}`);
  }

  async waitRun(runId: string, timeoutMs = 90_000) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      const detail = await this.getRun(runId);
      const state = detail.run.state;
      if (
        ['succeeded', 'failed', 'cancelled', 'waiting_operator', 'interrupted'].includes(state)
      ) {
        return detail.run;
      }
      await new Promise((resolve) => setTimeout(resolve, 700));
    }
    throw new Error(`run ${runId} did not finish in time`);
  }

  async runEvidence(assessmentId: string, runId: string) {
    return this.get<
      Array<{
        id: string;
        summary: string;
        size_bytes: number;
        kind: string;
        content_type: string;
        run_id: string | null;
      }>
    >(`/v1/assessments/${assessmentId}/evidence?run_id=${encodeURIComponent(runId)}&limit=300`);
  }

  async identities(assessmentId: string) {
    return this.get<
      Array<{
        id: string;
        label: string;
        role_label: string;
        tenant_keys: string[];
        session_state: string;
      }>
    >(`/v1/assessments/${assessmentId}/identities`);
  }

  async browserOpen(
    runId: string,
    body: { url: string; identity_id: string; tenant?: string | null; decision?: string },
  ) {
    return this.post<BrowserResult>(`/v1/runs/${runId}/browser/open`, body);
  }

  async browserObserve(runId: string, sessionKey: string, decision?: string) {
    return this.post<BrowserResult>(`/v1/runs/${runId}/browser/observe`, {
      session_key: sessionKey,
      decision,
    });
  }

  async browserNavigate(runId: string, sessionKey: string, url: string, decision?: string) {
    return this.post<BrowserResult>(`/v1/runs/${runId}/browser/navigate`, {
      session_key: sessionKey,
      url,
      decision,
    });
  }

  async browserAct(
    runId: string,
    sessionKey: string,
    action: Record<string, unknown>,
    decision?: string,
  ) {
    return this.post<BrowserResult>(`/v1/runs/${runId}/browser/act`, {
      session_key: sessionKey,
      action,
      decision,
    });
  }

  async browserLogin(
    runId: string,
    sessionKey: string,
    login: Record<string, unknown>,
    decision?: string,
  ) {
    return this.post<BrowserResult>(`/v1/runs/${runId}/browser/login`, {
      session_key: sessionKey,
      login,
      decision,
    });
  }

  async browserScreenshot(runId: string, sessionKey: string) {
    return this.post<BrowserResult>(`/v1/runs/${runId}/browser/screenshot`, {
      session_key: sessionKey,
    });
  }

  async browserTraffic(runId: string, sessionKey: string) {
    return this.post<BrowserResult>(`/v1/runs/${runId}/browser/traffic`, {
      session_key: sessionKey,
    });
  }

  async checkpoint(runId: string) {
    return this.get<Record<string, unknown>>(`/v1/runs/${runId}/agent/checkpoint`);
  }
}

export interface BrowserObservation {
  url: string;
  title: string;
  ready_state: string;
  headings: string[];
  text: string;
  text_truncated: boolean;
  interactive: Array<{
    ref: string;
    tag: string;
    type: string | null;
    role: string | null;
    name: string;
    href: string | null;
    value?: string;
    disabled: boolean;
    visible: boolean;
    options?: Array<{ value: string; label: string; selected: boolean }>;
  }>;
  forms: Array<{
    index: number;
    id: string | null;
    action: string;
    method: string;
    fields: Array<{ ref: string; tag: string; type: string | null; name: string | null; label: string }>;
  }>;
  tables: Array<{ headers: string[]; rows: string[][] }>;
  auth: {
    password_fields: number;
    login_form: boolean;
    logout_visible: boolean;
    likely_authenticated: boolean;
    error_text: string;
  };
  traffic?: Array<Record<string, unknown>>;
  blocked_requests?: Array<Record<string, unknown>>;
}

export interface BrowserResult {
  action_id: string;
  state: 'succeeded' | 'failed' | 'blocked';
  effect_status: string;
  result: {
    status: string;
    summary: string;
    observation?: BrowserObservation;
    evidence_ids?: string[];
    session?: {
      session_id: string;
      context_key: string;
      identity_id: string;
      identity_label: string;
      tenant: string | null;
      state: string;
    };
    engine?: Record<string, unknown>;
    login_state?: string;
    traffic?: unknown;
  };
}
