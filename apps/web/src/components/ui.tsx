import { useEffect, useRef, useState, type ReactNode } from 'react';
import { AlertCircle, Check, Copy, RefreshCw } from 'lucide-react';
import { copyText } from '../lib/clipboard';
import { errorMessage } from '../api/client';

export function PageHeader({
  title,
  description,
  meta,
  actions,
}: {
  title: string;
  description?: ReactNode;
  meta?: ReactNode;
  actions?: ReactNode;
}) {
  return (
    <header className="page-head">
      <div className="page-head-text">
        {meta ? <div className="page-meta mono">{meta}</div> : null}
        <h1>{title}</h1>
        {description ? <p className="page-desc">{description}</p> : null}
      </div>
      {actions ? <div className="page-actions">{actions}</div> : null}
    </header>
  );
}

export function Metric({
  label,
  value,
  hint,
}: {
  label: string;
  value: ReactNode;
  hint?: ReactNode;
}) {
  return (
    <div className="metric">
      <div className="metric-label mono">{label}</div>
      <div className="metric-value">{value}</div>
      {hint ? <div className="metric-hint">{hint}</div> : null}
    </div>
  );
}

export function Panel({
  title,
  subtitle,
  actions,
  children,
  flush = false,
  className,
}: {
  title?: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  flush?: boolean;
  className?: string;
}) {
  return (
    <section className={`panel${className ? ` ${className}` : ''}`}>
      {title || actions ? (
        <header className="panel-head">
          <div>
            <h2 className="panel-title">{title}</h2>
            {subtitle ? <p className="panel-sub">{subtitle}</p> : null}
          </div>
          {actions ? <div className="panel-actions">{actions}</div> : null}
        </header>
      ) : null}
      <div className={flush ? 'panel-flush' : 'panel-body'}>{children}</div>
    </section>
  );
}

export function EmptyState({ children, action }: { children: ReactNode; action?: ReactNode }) {
  return (
    <div className="empty">
      <p>{children}</p>
      {action ? <div className="empty-action">{action}</div> : null}
    </div>
  );
}

export function ErrorState({
  error,
  onRetry,
  label = 'Could not load this view.',
}: {
  error: unknown;
  onRetry?: () => void;
  label?: string;
}) {
  return (
    <div className="error-state" role="alert">
      <AlertCircle size={16} strokeWidth={1.5} aria-hidden="true" />
      <div>
        <p className="error-label">{label}</p>
        <p className="error-detail">{errorMessage(error)}</p>
      </div>
      {onRetry ? (
        <button type="button" className="btn btn-ghost" onClick={onRetry}>
          <RefreshCw size={14} strokeWidth={1.5} />
          Retry
        </button>
      ) : null}
    </div>
  );
}

export function TableLoading({ columns, rows = 6 }: { columns: number; rows?: number }) {
  return (
    <tbody className="skeleton-body" aria-hidden="true">
      {Array.from({ length: rows }, (_, rowIndex) => (
        <tr key={rowIndex}>
          {Array.from({ length: columns }, (_, columnIndex) => (
            <td key={columnIndex}>
              <span
                className="skeleton"
                style={{ width: `${45 + ((rowIndex + columnIndex) % 4) * 14}%` }}
              />
            </td>
          ))}
        </tr>
      ))}
    </tbody>
  );
}

export function BlockSkeleton({ lines = 3 }: { lines?: number }) {
  return (
    <div className="block-skeleton" aria-hidden="true">
      {Array.from({ length: lines }, (_, index) => (
        <span key={index} className="skeleton" style={{ width: `${92 - index * 14}%` }} />
      ))}
    </div>
  );
}

export function CopyButton({
  value,
  label = 'Copy',
  title,
  className,
}: {
  value: string;
  label?: string;
  title?: string;
  className?: string;
}) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<number | null>(null);

  useEffect(
    () => () => {
      if (timer.current !== null) window.clearTimeout(timer.current);
    },
    [],
  );

  const copy = async () => {
    const ok = await copyText(value);
    if (!ok) return;
    setCopied(true);
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = window.setTimeout(() => setCopied(false), 1600);
  };

  return (
    <button
      type="button"
      className={`btn btn-ghost copy-btn${className ? ` ${className}` : ''}`}
      onClick={() => void copy()}
      title={title ?? value}
      aria-label={copied ? 'Copied' : `${label} to clipboard`}
    >
      {copied ? <Check size={14} strokeWidth={1.5} /> : <Copy size={14} strokeWidth={1.5} />}
      {label !== 'Copy' || copied ? <span>{copied ? 'Copied' : label}</span> : null}
    </button>
  );
}

export function IdChip({
  id,
  length = 8,
  className,
}: {
  id: string | null | undefined;
  length?: number;
  className?: string;
}) {
  if (!id) return <span className="muted">—</span>;
  const shown = id.length > length ? `…${id.slice(-length)}` : id;
  return (
    <span className={`id-chip mono${className ? ` ${className}` : ''}`} title={id}>
      {shown}
    </span>
  );
}

export function KeyValue({ items }: { items: Array<{ label: string; value: ReactNode }> }) {
  return (
    <dl className="kv">
      {items.map((item) => (
        <div key={item.label}>
          <dt className="mono">{item.label}</dt>
          <dd>{item.value}</dd>
        </div>
      ))}
    </dl>
  );
}
