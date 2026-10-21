import { labelFor, isStruck, toneFor, type Tone } from '../lib/status';

interface StatusProps {
  value: string;
  label?: string;
  tone?: Tone;
  className?: string;
}

export function Status({ value, label, tone, className }: StatusProps) {
  const resolved = tone ?? toneFor(value);
  return (
    <span
      className={`status${className ? ` ${className}` : ''}`}
      data-tone={resolved}
      title={label ?? labelFor(value)}
    >
      <span className="status-dot" aria-hidden="true" />
      <span className={`status-label${isStruck(value) ? ' is-struck' : ''}`}>
        {label ?? labelFor(value)}
      </span>
    </span>
  );
}

export function LiveDot({ tone = 'lime' }: { tone?: Tone }) {
  return <span className="status-dot live-dot" data-tone={tone} aria-hidden="true" />;
}
