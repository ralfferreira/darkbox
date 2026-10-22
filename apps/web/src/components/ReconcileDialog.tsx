import { useState } from 'react';
import { useQueryClient } from '@tanstack/react-query';
import { errorMessage, request } from '../api/client';
import type { RunSummary } from '../api/types';
import { Dialog } from './Dialog';
import { Field, Textarea } from './Form';

export function ReconcileDialog({
  open,
  onClose,
  run,
}: {
  open: boolean;
  onClose: () => void;
  run: RunSummary;
}) {
  const queryClient = useQueryClient();
  const [resolution, setResolution] = useState<'applied' | 'not_applied'>('applied');
  const [note, setNote] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await request(`/runs/${run.id}/agent/reconcile`, {
        method: 'POST',
        body: { resolution, note },
      });
      await queryClient.invalidateQueries({ queryKey: ['run', run.id] });
      await queryClient.invalidateQueries({ queryKey: ['runs', run.assessment_id, 50] });
      await queryClient.invalidateQueries({ queryKey: ['agent-progress', run.id] });
      onClose();
    } catch (cause) {
      setError(errorMessage(cause));
      setBusy(false);
    }
  };

  return (
    <Dialog
      open={open}
      onClose={busy ? () => undefined : onClose}
      title="Reconcile unknown outcome"
      description="The platform will not repeat the action automatically. Record what the operator observed on the target."
      size="sm"
      footer={
        <>
          <button type="button" className="btn" onClick={onClose} disabled={busy}>
            Cancel
          </button>
          <button type="submit" form={`${run.id}-reconcile`} className="btn btn-primary" disabled={busy}>
            {busy ? 'Recording…' : 'Record reconciliation'}
          </button>
        </>
      }
    >
      <form id={`${run.id}-reconcile`} onSubmit={submit} className="form-stack">
        <div className="form-grid two">
          <button
            type="button"
            className={`btn ${resolution === 'applied' ? 'btn-primary' : ''}`}
            onClick={() => setResolution('applied')}
          >
            Effect was applied
          </button>
          <button
            type="button"
            className={`btn ${resolution === 'not_applied' ? 'btn-primary' : ''}`}
            onClick={() => setResolution('not_applied')}
          >
            Effect was not applied
          </button>
        </div>
        <Field
          label="Operator note"
          htmlFor={`${run.id}-reconcile-note`}
          hint="What did you observe on the target? This note is kept with the action."
        >
          <Textarea
            id={`${run.id}-reconcile-note`}
            rows={4}
            value={note}
            onChange={(event) => setNote(event.target.value)}
            placeholder="Observed one new row on the target; not repeating."
          />
        </Field>
        {error ? (
          <p className="form-error" role="alert">
            {error}
          </p>
        ) : null}
      </form>
    </Dialog>
  );
}
