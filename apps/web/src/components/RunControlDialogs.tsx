import { useEffect, useId, useState } from 'react';
import type { RunSummary } from '../api/types';
import { useCancelRun, useResumeRun } from '../api/queries';
import { errorMessage } from '../api/client';
import { Dialog } from './Dialog';
import { Field, Textarea, Toggle } from './Form';
import { Status } from './Status';
import { useToast } from './Toast';

export function CancelRunDialog({
  open,
  onClose,
  run,
}: {
  open: boolean;
  onClose: () => void;
  run: RunSummary | null;
}) {
  const mutation = useCancelRun(run?.id ?? '');
  const { notify } = useToast();
  const [reason, setReason] = useState('');
  const formId = useId();
  const uid = useId();

  useEffect(() => {
    if (open) {
      setReason('');
      mutation.reset();
    }
    // Reset only when the dialog opens.
  }, [open]);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!run) return;
    mutation.mutate(
      { reason: reason.trim() },
      {
        onSuccess: () => {
          notify('Cancellation requested. The run is not cancelled until the server confirms.');
          onClose();
        },
      },
    );
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Cancel run"
      description="Cancellation is a request. The current step may still complete before the run stops."
      size="sm"
      footer={
        <>
          <button type="button" className="btn" onClick={onClose}>
            Back
          </button>
          <button
            type="submit"
            form={formId}
            className="btn btn-danger"
            disabled={mutation.isPending || !run}
          >
            {mutation.isPending ? 'Requesting…' : 'Request cancellation'}
          </button>
        </>
      }
    >
      <form id={formId} onSubmit={submit} className="form-stack">
        {run ? (
          <div className="run-dialog-state">
            <span className="mono quiet">{run.kind.replace(/_/g, ' ')}</span>
            <Status value={run.state} />
          </div>
        ) : null}
        <Field label="Reason" htmlFor={`${uid}-cancel-reason`} hint="Optional. Recorded on the run.">
          <Textarea
            id={`${uid}-cancel-reason`}
            value={reason}
            onChange={(event) => setReason(event.target.value)}
            rows={3}
            placeholder="Why this run should stop."
          />
        </Field>
        {mutation.isError ? (
          <p className="form-error" role="alert">
            {errorMessage(mutation.error)}
          </p>
        ) : null}
      </form>
    </Dialog>
  );
}

export function ResumeRunDialog({
  open,
  onClose,
  run,
}: {
  open: boolean;
  onClose: () => void;
  run: RunSummary | null;
}) {
  const mutation = useResumeRun(run?.id ?? '');
  const { notify } = useToast();
  const [note, setNote] = useState('');
  const [reconciled, setReconciled] = useState(false);
  const formId = useId();
  const uid = useId();

  useEffect(() => {
    if (open) {
      setNote('');
      setReconciled(false);
      mutation.reset();
    }
    // Reset only when the dialog opens.
  }, [open]);

  const requiresReconcile = run?.effect_unknown ?? false;

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!run) return;
    if (requiresReconcile && !reconciled) return;
    mutation.mutate(
      { reconcile_unknown: reconciled, note: note.trim() },
      {
        onSuccess: () => {
          notify('Run resumed.', 'success');
          onClose();
        },
      },
    );
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Resume run"
      description="Resuming re-queues the run from its recorded state."
      size="sm"
      footer={
        <>
          <button type="button" className="btn" onClick={onClose}>
            Back
          </button>
          <button
            type="submit"
            form={formId}
            className="btn btn-primary"
            disabled={
              mutation.isPending || !run || (requiresReconcile && !reconciled)
            }
          >
            {mutation.isPending ? 'Resuming…' : 'Resume run'}
          </button>
        </>
      }
    >
      <form id={formId} onSubmit={submit} className="form-stack">
        {run ? (
          <div className="run-dialog-state">
            <span className="mono quiet">{run.kind.replace(/_/g, ' ')}</span>
            <Status value={run.state} />
          </div>
        ) : null}
        {requiresReconcile ? (
          <div className="callout" data-tone="red">
            <strong>External effect outcome unknown</strong>
            <p>
              The platform does not know whether the last external effect completed. It will not
              repeat that effect automatically. Reconcile the target state before resuming.
            </p>
            <Toggle
              label="I reconciled the unknown external effect"
              hint="Required to resume this run."
              checked={reconciled}
              onChange={(event) => setReconciled(event.target.checked)}
            />
          </div>
        ) : null}
        <Field label="Note" htmlFor={`${uid}-resume-note`} hint="Optional. Recorded with the resume.">
          <Textarea
            id={`${uid}-resume-note`}
            value={note}
            onChange={(event) => setNote(event.target.value)}
            rows={3}
          />
        </Field>
        {mutation.isError ? (
          <p className="form-error" role="alert">
            {errorMessage(mutation.error)}
          </p>
        ) : null}
      </form>
    </Dialog>
  );
}
