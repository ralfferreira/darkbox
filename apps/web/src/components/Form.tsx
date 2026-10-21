import type {
  InputHTMLAttributes,
  ReactNode,
  SelectHTMLAttributes,
  TextareaHTMLAttributes,
} from 'react';

export function Field({
  label,
  htmlFor,
  hint,
  children,
  className,
}: {
  label: string;
  htmlFor?: string;
  hint?: ReactNode;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className={`field${className ? ` ${className}` : ''}`}>
      {htmlFor ? (
        <label htmlFor={htmlFor}>{label}</label>
      ) : (
        <span className="field-label">{label}</span>
      )}
      {children}
      {hint ? <p className="field-hint">{hint}</p> : null}
    </div>
  );
}

export function TextInput(props: InputHTMLAttributes<HTMLInputElement>) {
  const { className, ...rest } = props;
  return <input {...rest} className={`input${className ? ` ${className}` : ''}`} />;
}

export function SelectInput(props: SelectHTMLAttributes<HTMLSelectElement>) {
  const { className, ...rest } = props;
  return <select {...rest} className={`input select${className ? ` ${className}` : ''}`} />;
}

export function Textarea(props: TextareaHTMLAttributes<HTMLTextAreaElement>) {
  const { className, ...rest } = props;
  return <textarea {...rest} className={`input textarea${className ? ` ${className}` : ''}`} />;
}

export function Checkbox({
  label,
  hint,
  ...props
}: InputHTMLAttributes<HTMLInputElement> & { label: string; hint?: string }) {
  return (
    <label className="check">
      <input type="checkbox" {...props} />
      <span>
        <span className="check-label">{label}</span>
        {hint ? <span className="check-hint">{hint}</span> : null}
      </span>
    </label>
  );
}

export function Toggle({
  label,
  hint,
  ...props
}: InputHTMLAttributes<HTMLInputElement> & { label: string; hint?: string }) {
  return (
    <label className="toggle">
      <input type="checkbox" role="switch" {...props} />
      <span className="toggle-track" aria-hidden="true">
        <span className="toggle-thumb" />
      </span>
      <span>
        <span className="check-label">{label}</span>
        {hint ? <span className="check-hint">{hint}</span> : null}
      </span>
    </label>
  );
}
