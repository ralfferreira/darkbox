const numberFormatter = new Intl.NumberFormat('en-US');
const dateFormatter = new Intl.DateTimeFormat('en-US', {
  year: 'numeric',
  month: 'short',
  day: '2-digit',
  hour: '2-digit',
  minute: '2-digit',
  hour12: false,
});
const shortDateFormatter = new Intl.DateTimeFormat('en-US', {
  month: 'short',
  day: 'numeric',
});

export function formatNumber(value: number | null | undefined): string {
  if (value === null || value === undefined || Number.isNaN(value)) return '—';
  return numberFormatter.format(value);
}

export function formatBytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined || Number.isNaN(bytes)) return '—';
  if (bytes < 1024) return `${bytes} B`;
  const units = ['KB', 'MB', 'GB', 'TB'];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = value >= 100 ? 0 : value >= 10 ? 1 : 1;
  return `${value.toFixed(digits)} ${units[unit]}`;
}

export function formatRelative(iso: string | null | undefined, now = Date.now()): string {
  if (!iso) return '—';
  const time = new Date(iso).getTime();
  if (Number.isNaN(time)) return '—';
  const diff = Math.round((now - time) / 1000);
  if (diff < 0) return 'just now';
  if (diff < 45) return 'just now';
  if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
  if (diff < 86_400) return `${Math.floor(diff / 3600)}h ago`;
  if (diff < 7 * 86_400) return `${Math.floor(diff / 86_400)}d ago`;
  return shortDateFormatter.format(new Date(time));
}

export function formatDateTime(iso: string | null | undefined): string {
  if (!iso) return '—';
  const time = new Date(iso).getTime();
  if (Number.isNaN(time)) return '—';
  return dateFormatter.format(new Date(time));
}

export function formatDuration(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || Number.isNaN(seconds)) return '—';
  if (seconds < 0) return '—';
  if (seconds < 1) return '<1s';
  if (seconds < 60) return `${Math.floor(seconds)}s`;
  if (seconds < 3600) {
    const minutes = Math.floor(seconds / 60);
    const rest = Math.floor(seconds % 60);
    return `${minutes}m ${String(rest).padStart(2, '0')}s`;
  }
  if (seconds < 86_400) {
    const hours = Math.floor(seconds / 3600);
    const minutes = Math.floor((seconds % 3600) / 60);
    return `${hours}h ${String(minutes).padStart(2, '0')}m`;
  }
  const days = Math.floor(seconds / 86_400);
  const hours = Math.floor((seconds % 86_400) / 3600);
  return `${days}d ${hours}h`;
}

export function formatDurationBetween(
  start: string | null | undefined,
  end: string | null | undefined,
  now = Date.now(),
): string {
  if (!start) return '—';
  const startTime = new Date(start).getTime();
  if (Number.isNaN(startTime)) return '—';
  const endTime = end ? new Date(end).getTime() : now;
  if (Number.isNaN(endTime)) return '—';
  return formatDuration((endTime - startTime) / 1000);
}

/** Returns an em dash when there is no denominator, never a misleading 100%. */
export function formatPercent(
  value: number | null | undefined,
  denominator: number | null | undefined,
  digits = 0,
): string {
  if (value === null || value === undefined || denominator === null || denominator === undefined) {
    return '—';
  }
  if (denominator <= 0) return '—';
  return `${((value / denominator) * 100).toFixed(digits)}%`;
}

export function ratioPercent(
  value: number | null | undefined,
  denominator: number | null | undefined,
): number | null {
  if (value === null || value === undefined || denominator === null || denominator === undefined) {
    return null;
  }
  if (denominator <= 0) return null;
  return (value / denominator) * 100;
}

export function formatCostMicros(micros: number | null | undefined): string {
  if (micros === null || micros === undefined || Number.isNaN(micros)) return '—';
  const usd = micros / 1_000_000;
  if (usd > 0 && usd < 0.01) {
    return `$${usd.toFixed(4)}`;
  }
  return `$${usd.toFixed(2)}`;
}

export function formatTokens(tokens: number | null | undefined): string {
  return formatNumber(tokens);
}

export function shortHash(hash: string | null | undefined, length = 12): string {
  if (!hash) return '—';
  return hash.slice(0, length);
}

/** UUIDv7 identifiers share a time prefix, so the tail is the distinguishing part. */
export function shortId(id: string | null | undefined, length = 8): string {
  if (!id) return '—';
  return id.length > length ? `…${id.slice(-length)}` : id;
}

export function plural(count: number, singular: string, pluralForm?: string): string {
  return count === 1 ? singular : (pluralForm ?? `${singular}s`);
}

export function humanize(value: string): string {
  return value.replace(/_/g, ' ');
}

export function truncate(value: string, max: number): string {
  if (value.length <= max) return value;
  return `${value.slice(0, Math.max(0, max - 1))}…`;
}

export function formatJsonValue(value: unknown): string {
  if (typeof value === 'string') return value;
  if (value === null) return 'null';
  try {
    return JSON.stringify(value);
  } catch {
    return String(value);
  }
}
