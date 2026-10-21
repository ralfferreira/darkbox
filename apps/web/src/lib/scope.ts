import type { ActionClass, Budget, ScopePolicy } from '../api/types';

export const ACTION_CLASSES: readonly ActionClass[] = [
  'query',
  'session',
  'reversible_change',
  'external_effect',
  'destructive',
];

export const ACTION_CLASS_LABELS: Record<ActionClass, string> = {
  query: 'Query',
  session: 'Session',
  reversible_change: 'Reversible change',
  external_effect: 'External effect',
  destructive: 'Destructive',
};

export function defaultBudget(): Budget {
  return {
    max_duration_seconds: 900,
    max_requests: 400,
    max_browser_steps: 200,
    max_model_calls: 40,
    max_cost_micros: 2_000_000,
    max_artifact_bytes: 64 * 1024 * 1024,
    max_concurrency_per_origin: 2,
  };
}

export function defaultPolicy(): ScopePolicy {
  return {
    include_hosts: [],
    exclude_hosts: [],
    include_paths: ['/'],
    exclude_paths: [],
    allowed_schemes: ['https'],
    allowed_ports: [443],
    allow_private_network: false,
    window: null,
    action_classes: ['query', 'session'],
    navigation_dependencies: [],
    test_targets: [],
    limits: defaultBudget(),
    retain_raw: false,
  };
}

export function splitLines(value: string): string[] {
  return value
    .split('\n')
    .map((line) => line.trim())
    .filter(Boolean);
}

export function joinLines(values: readonly string[]): string {
  return values.join('\n');
}

export function splitCsv(value: string): string[] {
  return value
    .split(',')
    .map((item) => item.trim())
    .filter(Boolean);
}

export function splitPorts(value: string): number[] {
  return splitCsv(value)
    .map((item) => Number(item))
    .filter((item) => Number.isInteger(item) && item > 0 && item <= 65_535);
}

export function joinPorts(values: readonly number[]): string {
  return values.join(', ');
}

export function policyValidationError(policy: ScopePolicy): string | null {
  if (policy.include_hosts.length === 0) return 'At least one included host is required.';
  if (policy.allowed_schemes.length === 0) return 'At least one scheme is required.';
  if (policy.allowed_ports.length === 0) return 'At least one allowed port is required.';
  const badHost = [...policy.include_hosts, ...policy.exclude_hosts].find(
    (host) => !host.trim() || host.includes('/') || host.includes(' '),
  );
  if (badHost !== undefined) return `Invalid host rule: ${badHost || '(empty)'}`;
  const badPath = policy.include_paths.find((path) => !path.startsWith('/'));
  if (badPath) return `Include path must start with /: ${badPath}`;
  const limits = policy.limits;
  if (limits.max_requests < 1) return 'Request budget must be at least 1.';
  if (limits.max_duration_seconds < 1) return 'Duration budget must be at least 1 second.';
  if (limits.max_browser_steps < 0 || limits.max_model_calls < 0) {
    return 'Browser and model budgets cannot be negative.';
  }
  return null;
}

export function scopePolicyFrom(
  input: ScopePolicy,
): ScopePolicy {
  return {
    ...input,
    limits: { ...input.limits },
    include_hosts: [...input.include_hosts],
    exclude_hosts: [...input.exclude_hosts],
    include_paths: [...input.include_paths],
    exclude_paths: [...input.exclude_paths],
    allowed_schemes: [...input.allowed_schemes],
    allowed_ports: [...input.allowed_ports],
    action_classes: [...input.action_classes],
  };
}
