import { useEffect, useId, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useImportUrls } from '../api/queries';
import { errorMessage } from '../api/client';
import { splitLines } from '../lib/scope';
import { Dialog } from './Dialog';
import { Field, Textarea, TextInput } from './Form';
import { useToast } from './Toast';

export function ImportUrlsDialog({
  open,
  onClose,
  assessmentId,
}: {
  open: boolean;
  onClose: () => void;
  assessmentId: string;
}) {
  const [urls, setUrls] = useState('');
  const [source, setSource] = useState('');
  const mutation = useImportUrls(assessmentId);
  const navigate = useNavigate();
  const { notify } = useToast();
  const formId = useId();
  const uid = useId();

  useEffect(() => {
    if (open) {
      setUrls('');
      setSource('');
      mutation.reset();
    }
    // Reset only when the dialog opens.
  }, [open]);

  const parsed = splitLines(urls);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (parsed.length === 0) return;
    mutation.mutate(
      { urls: parsed, source: source.trim() || null },
      {
        onSuccess: (run) => {
          notify('Discovery run queued.', 'success');
          onClose();
          navigate(`/assessments/${assessmentId}/runs/${run.id}`);
        },
      },
    );
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Start discovery"
      description="Import URLs as candidate endpoints. Scope filtering is applied before dispatch."
      footer={
        <>
          <button type="button" className="btn" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            form={formId}
            className="btn btn-primary"
            disabled={parsed.length === 0 || mutation.isPending}
          >
            {mutation.isPending ? 'Queueing…' : 'Import and run'}
          </button>
        </>
      }
    >
      <form id={formId} onSubmit={submit} className="form-stack">
        <Field
          label="URLs"
          htmlFor={`${uid}-urls`}
          hint={`One absolute URL per line. ${parsed.length} ${parsed.length === 1 ? 'target' : 'targets'}.`}
        >
          <Textarea
            id={`${uid}-urls`}
            value={urls}
            onChange={(event) => setUrls(event.target.value)}
            rows={8}
            placeholder={'https://app.example.com/\nhttps://app.example.com/api/me'}
            spellCheck={false}
            autoFocus
          />
        </Field>
        <Field label="Source" htmlFor={`${uid}-source`} hint="Optional label for provenance.">
          <TextInput
            id={`${uid}-source`}
            value={source}
            onChange={(event) => setSource(event.target.value)}
            placeholder="operator-seed"
            spellCheck={false}
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
