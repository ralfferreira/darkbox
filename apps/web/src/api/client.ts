import type { AgentTurnResponse, JsonValue } from './types';

export const TOKEN_KEY = 'darkbox.token';
export const DEV_TOKEN = 'darkbox-dev-token';
export const UNAUTHORIZED_EVENT = 'darkbox:unauthorized';
export const AGENT_URL = (import.meta.env.VITE_AGENT_URL ?? 'http://127.0.0.1:8790').replace(/\/$/, '');

export interface ApiErrorBody {
  error: {
    code: string;
    message: string;
    detail: JsonValue;
  };
}

export class ApiError extends Error {
  readonly status: number;
  readonly code: string;
  readonly detail: JsonValue;

  constructor(status: number, code: string, message: string, detail: JsonValue = null) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
    this.code = code;
    this.detail = detail;
  }
}

export function getToken(): string | null {
  try {
    return window.localStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

export function setToken(token: string): void {
  window.localStorage.setItem(TOKEN_KEY, token);
}

export function clearToken(): void {
  try {
    window.localStorage.removeItem(TOKEN_KEY);
  } catch {
    // Storage can be unavailable in private browsing; the token simply stays in memory.
  }
}

export type QueryValue = string | number | boolean | null | undefined;

export interface RequestOptions {
  method?: 'GET' | 'POST' | 'PATCH' | 'DELETE';
  body?: unknown;
  query?: Record<string, QueryValue>;
  signal?: AbortSignal;
  accept?: string;
}

function buildUrl(path: string, query?: Record<string, QueryValue>): string {
  const params = new URLSearchParams();
  if (query) {
    for (const [key, value] of Object.entries(query)) {
      if (value === undefined || value === null || value === '') continue;
      params.set(key, String(value));
    }
  }
  const search = params.toString();
  return `/v1${path}${search ? `?${search}` : ''}`;
}

async function parseError(response: Response): Promise<ApiError> {
  let code = 'http_error';
  let message = `Request failed with status ${response.status}`;
  let detail: JsonValue = null;
  try {
    const data: unknown = await response.json();
    if (typeof data === 'object' && data !== null && 'error' in data) {
      const envelope = (data as { error?: unknown }).error;
      if (typeof envelope === 'object' && envelope !== null) {
        const record = envelope as Record<string, unknown>;
        if (typeof record.code === 'string') code = record.code;
        if (typeof record.message === 'string') message = record.message;
        if ('detail' in record) detail = record.detail as JsonValue;
      }
    }
  } catch {
    // Non-JSON error bodies keep the status-derived message.
  }
  return new ApiError(response.status, code, message, detail);
}

function handleUnauthorized(): void {
  clearToken();
  window.dispatchEvent(new CustomEvent(UNAUTHORIZED_EVENT));
}

async function send(path: string, options: RequestOptions): Promise<Response> {
  const headers: Record<string, string> = {
    Accept: options.accept ?? 'application/json',
  };
  const token = getToken();
  if (token) headers.Authorization = `Bearer ${token}`;
  if (options.body !== undefined) headers['Content-Type'] = 'application/json';

  let response: Response;
  try {
    response = await fetch(buildUrl(path, options.query), {
      method: options.method ?? 'GET',
      headers,
      body: options.body === undefined ? undefined : JSON.stringify(options.body),
      signal: options.signal,
    });
  } catch (error) {
    if (error instanceof DOMException && error.name === 'AbortError') throw error;
    throw new ApiError(
      0,
      'network_error',
      'The API did not respond. Check that the local service is running.',
    );
  }

  if (response.status === 401) {
    handleUnauthorized();
    throw await parseError(response);
  }
  if (!response.ok) throw await parseError(response);
  return response;
}

export async function request<T>(path: string, options: RequestOptions = {}): Promise<T> {
  const response = await send(path, options);
  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

export async function requestText(path: string, options: RequestOptions = {}): Promise<string> {
  const response = await send(path, { ...options, accept: 'text/plain, text/*' });
  return response.text();
}

export async function requestBlob(path: string, options: RequestOptions = {}): Promise<Blob> {
  const response = await send(path, { ...options, accept: '*/*' });
  return response.blob();
}

export function errorMessage(error: unknown): string {
  if (error instanceof ApiError) return error.message;
  if (error instanceof Error) return error.message;
  return 'Something went wrong.';
}

export interface AgentHealth {
  status: string;
  model: string;
  base_url: string;
  api_url: string;
  max_steps: number;
}

export async function agentHealth(): Promise<AgentHealth> {
  const response = await fetch(`${AGENT_URL}/health`);
  if (!response.ok) throw new Error(`Agent service returned ${response.status}`);
  return (await response.json()) as AgentHealth;
}

export interface AgentTurnInput {
  assessment_id: string;
  objective: string;
  identity_id: string | null;
  max_steps: number;
  token_budget: number;
}

export async function agentTurn(input: AgentTurnInput): Promise<AgentTurnResponse> {
  const token = getToken();
  if (!token) throw new ApiError(401, 'unauthorized', 'Sign in before starting an agent turn.');
  const response = await fetch(`${AGENT_URL}/turns`, {
    method: 'POST',
    headers: {
      Authorization: `Bearer ${token}`,
      'Content-Type': 'application/json',
    },
    body: JSON.stringify(input),
  });
  const body = await response.text();
  if (!response.ok) {
    let message = `Agent service returned ${response.status}`;
    try {
      const parsed = JSON.parse(body) as { error?: string };
      if (parsed.error) message = parsed.error;
    } catch {
      // Keep the status message.
    }
    throw new ApiError(response.status, 'agent_error', message);
  }
  return JSON.parse(body) as AgentTurnResponse;
}
