export type Tone = 'lime' | 'amber' | 'red' | 'blue' | 'neutral' | 'dim' | 'outline';

const TONES: Record<string, Tone> = {
  observed: 'lime',
  reproduced: 'lime',
  succeeded: 'lime',
  confirmed: 'lime',
  available: 'lime',
  sanitized: 'lime',
  running: 'lime',
  active: 'lime',
  valid: 'lime',
  current: 'lime',
  authenticated: 'lime',
  passed: 'lime',
  exported: 'lime',
  candidate: 'outline',
  proposed: 'outline',
  hypothesis: 'neutral',
  queued: 'neutral',
  not_dispatched: 'neutral',
  metadata_only: 'neutral',
  unreachable: 'amber',
  waiting_operator: 'amber',
  needs_review: 'amber',
  inconclusive: 'amber',
  degraded: 'amber',
  cancelling: 'amber',
  interrupted: 'amber',
  reopened: 'amber',
  raw_restricted: 'amber',
  failed: 'red',
  blocked: 'red',
  error: 'red',
  outcome_unknown: 'red',
  unavailable: 'red',
  expired: 'red',
  invalid: 'red',
  excluded: 'dim',
  stale: 'dim',
  rejected: 'dim',
  cancelled: 'dim',
  not_applicable: 'dim',
  remediated: 'blue',
  dispatched: 'blue',
  connected: 'lime',
  disconnected: 'red',
  reconnecting: 'amber',
};

export function toneFor(value: string): Tone {
  return TONES[value] ?? 'neutral';
}

export function labelFor(value: string): string {
  return value.replace(/_/g, ' ');
}

export function isStruck(value: string): boolean {
  return value === 'excluded';
}
