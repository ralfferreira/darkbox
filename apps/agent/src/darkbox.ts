import type { ContextPackage, RunSummary } from './types.js';

export class DarkBoxApi {
  constructor(private readonly baseUrl: string, private readonly token: string) {}

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

  async text(path: string): Promise<string> {
    const response = await fetch(`${this.baseUrl}${path}`, {
      headers: { Authorization: `Bearer ${this.token}` },
    });
    if (!response.ok) {
      throw new Error(`DarkBox ${response.status} ${path}`);
    }
    return response.text();
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
  ) {
    return this.post<{ run: RunSummary; page: { slug: string; title: string; version: number } }>(
      `/v1/runs/${runId}/agent/finish`,
      { summary, next_steps: nextSteps, evidence_ids: evidenceIds },
    );
  }

  async usage(
    runId: string,
    inputTokens: number,
    outputTokens: number,
    totalTokens: number,
    model: string,
  ) {
    return this.post<RunSummary>(`/v1/runs/${runId}/agent/usage`, {
      input_tokens: inputTokens,
      output_tokens: outputTokens,
      total_tokens: totalTokens,
      model,
    });
  }

  async abort(runId: string, reason: string) {
    return this.post(`/v1/runs/${runId}/agent/abort`, { reason });
  }

  async waitRun(runId: string, timeoutMs = 90_000) {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      const detail = await this.get<{ run: RunSummary }>(`/v1/runs/${runId}`);
      const state = detail.run.state;
      if (['succeeded', 'failed', 'cancelled'].includes(state)) {
        return detail.run;
      }
      if (state === 'waiting_operator' || state === 'interrupted') {
        return detail.run;
      }
      await new Promise((resolve) => setTimeout(resolve, 700));
    }
    throw new Error(`run ${runId} did not finish in time`);
  }

  async runEvidence(assessmentId: string, runId: string) {
    const items = await this.get<Array<{ id: string; summary: string; size_bytes: number }>>(
      `/v1/assessments/${assessmentId}/evidence?limit=200`,
    );
    return items.filter((item) => (item as { run_id?: string }).run_id === runId);
  }
}
