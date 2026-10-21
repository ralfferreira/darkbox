import { useEffect, useId, useState } from 'react';
import { useCreateExport } from '../api/queries';
import { errorMessage, requestBlob } from '../api/client';
import { formatBytes, formatDateTime, shortHash } from '../lib/format';
import { Dialog } from './Dialog';
import { Field, Textarea, Toggle } from './Form';
import { CopyButton } from './ui';
import { useToast } from './Toast';

export function ExportDialog({
  open,
  onClose,
  assessmentId,
}: {
  open: boolean;
  onClose: () => void;
  assessmentId: string;
}) {
  const [includeEvidence, setIncludeEvidence] = useState(false);
  const [note, setNote] = useState('');
  const [downloading, setDownloading] = useState(false);
  const mutation = useCreateExport(assessmentId);
  const { notify } = useToast();
  const formId = useId();

  useEffect(() => {
    if (open) {
      setIncludeEvidence(false);
      setNote('');
      setDownloading(false);
      mutation.reset();
    }
    // Reset only when the dialog opens.
  }, [open]);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    mutation.mutate({ include_evidence: includeEvidence, note: note.trim() });
  };

  const result = mutation.data;

  const download = async () => {
    if (!result) return;
    setDownloading(true);
    try {
      const blob = await requestBlob(`/exports/${result.id}/download`);
      const url = URL.createObjectURL(blob);
      const link = document.createElement('a');
      link.href = url;
      link.download = `darkbox-${result.assessment_id.slice(0, 8)}-${shortHash(result.manifest_sha256, 8)}.json`;
      document.body.appendChild(link);
      link.click();
      link.remove();
      URL.revokeObjectURL(url);
    } catch (error) {
      notify(errorMessage(error), 'error');
    } finally {
      setDownloading(false);
    }
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Export assessment"
      description="A signed JSON manifest with the selected assessment data."
      footer={
        result ? (
          <>
            <button type="button" className="btn" onClick={onClose}>
              Close
            </button>
            <button
              type="button"
              className="btn btn-primary"
              onClick={() => void download()}
              disabled={downloading}
            >
              {downloading ? 'Preparing…' : 'Download export'}
            </button>
          </>
        ) : (
          <>
            <button type="button" className="btn" onClick={onClose}>
              Cancel
            </button>
            <button
              type="submit"
              form={formId}
              className="btn btn-primary"
              disabled={mutation.isPending}
            >
              {mutation.isPending ? 'Building…' : 'Build export'}
            </button>
          </>
        )
      }
    >
      {result ? (
        <div className="export-result">
          <div className="result-mark mono">export ready</div>
          <dl className="kv">
            <div>
              <dt className="mono">Manifest sha256</dt>
              <dd className="hash-row">
                <span className="mono" title={result.manifest_sha256}>
                  {shortHash(result.manifest_sha256, 24)}
                </span>
                <CopyButton value={result.manifest_sha256} />
              </dd>
            </div>
            <div>
              <dt className="mono">Items</dt>
              <dd>{result.item_count}</dd>
            </div>
            <div>
              <dt className="mono">Size</dt>
              <dd>{formatBytes(result.size_bytes)}</dd>
            </div>
            <div>
              <dt className="mono">Evidence included</dt>
              <dd>{result.include_evidence ? 'yes' : 'no'}</dd>
            </div>
            <div>
              <dt className="mono">Created</dt>
              <dd>{formatDateTime(result.created_at)}</dd>
            </div>
            <div>
              <dt className="mono">Download path</dt>
              <dd className="mono break-any">{result.download_url}</dd>
            </div>
          </dl>
        </div>
      ) : (
        <form id={formId} onSubmit={submit} className="form-stack">
          <Toggle
            label="Include evidence"
            hint="Adds evidence content and hashes to the package."
            checked={includeEvidence}
            onChange={(event) => setIncludeEvidence(event.target.checked)}
          />
          <Field label="Note" htmlFor="export-note" hint="Optional. Stored with the export manifest.">
            <Textarea
              id="export-note"
              value={note}
              onChange={(event) => setNote(event.target.value)}
              rows={3}
              placeholder="Reason for this export."
            />
          </Field>
          {mutation.isError ? (
            <p className="form-error" role="alert">
              {errorMessage(mutation.error)}
            </p>
          ) : null}
        </form>
      )}
    </Dialog>
  );
}
