import { useEffect, useId, useState } from 'react';
import { useCreateFinding, useEvidence } from '../api/queries';
import { errorMessage } from '../api/client';
import { Checkbox, Field, SelectInput, Textarea, TextInput } from './Form';
import { Dialog } from './Dialog';
import { formatBytes } from '../lib/format';
import { useToast } from './Toast';

const SEVERITIES = ['critical', 'high', 'medium', 'low', 'info'];

export function CreateFindingDialog({
  open,
  onClose,
  assessmentId,
}: {
  open: boolean;
  onClose: () => void;
  assessmentId: string;
}) {
  const evidence = useEvidence(assessmentId, '', 50);
  const mutation = useCreateFinding(assessmentId);
  const { notify } = useToast();
  const formId = useId();
  const uid = useId();
  const [title, setTitle] = useState('');
  const [severity, setSeverity] = useState('medium');
  const [claim, setClaim] = useState('');
  const [impact, setImpact] = useState('');
  const [evidenceIds, setEvidenceIds] = useState<string[]>([]);

  useEffect(() => {
    if (open) {
      setTitle('');
      setSeverity('medium');
      setClaim('');
      setImpact('');
      setEvidenceIds([]);
      mutation.reset();
    }
    // Reset only when the dialog opens.
  }, [open]);

  const toggleEvidence = (id: string) => {
    setEvidenceIds((current) =>
      current.includes(id) ? current.filter((item) => item !== id) : [...current, id],
    );
  };

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    mutation.mutate(
      {
        title: title.trim(),
        severity,
        claim: claim.trim(),
        impact: impact.trim(),
        endpoint_id: null,
        evidence_ids: evidenceIds,
        test_result_ids: [],
        from_hypothesis: true,
      },
      {
        onSuccess: (finding) => {
          notify(`Finding ${finding.title} created as a hypothesis.`, 'success');
          onClose();
        },
      },
    );
  };

  const valid = title.trim().length > 0 && claim.trim().length > 0;

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Create finding"
      description="New findings start as hypotheses. They become confirmed only through review."
      size="lg"
      footer={
        <>
          <button type="button" className="btn" onClick={onClose}>
            Cancel
          </button>
          <button
            type="submit"
            form={formId}
            className="btn btn-primary"
            disabled={!valid || mutation.isPending}
          >
            {mutation.isPending ? 'Creating…' : 'Create hypothesis'}
          </button>
        </>
      }
    >
      <form id={formId} onSubmit={submit} className="form-stack">
        <div className="form-grid two">
          <Field label="Title" htmlFor={`${uid}-title`}>
            <TextInput
              id={`${uid}-title`}
              value={title}
              onChange={(event) => setTitle(event.target.value)}
              placeholder="Cross-tenant invoice read"
              required
            />
          </Field>
          <Field label="Severity" htmlFor={`${uid}-severity`}>
            <SelectInput
              id={`${uid}-severity`}
              value={severity}
              onChange={(event) => setSeverity(event.target.value)}
            >
              {SEVERITIES.map((option) => (
                <option key={option} value={option}>
                  {option}
                </option>
              ))}
            </SelectInput>
          </Field>
        </div>
        <Field label="Claim" htmlFor={`${uid}-claim`} hint="What the finding asserts, plainly.">
          <Textarea
            id={`${uid}-claim`}
            value={claim}
            onChange={(event) => setClaim(event.target.value)}
            rows={4}
            required
          />
        </Field>
        <Field label="Impact" htmlFor={`${uid}-impact`} hint="What follows if the claim is true.">
          <Textarea
            id={`${uid}-impact`}
            value={impact}
            onChange={(event) => setImpact(event.target.value)}
            rows={3}
          />
        </Field>
        <Field label="Evidence">
          {evidence.isLoading ? (
            <p className="field-hint">Loading evidence…</p>
          ) : (evidence.data ?? []).length === 0 ? (
            <p className="field-hint">No evidence available to attach.</p>
          ) : (
            <div className="evidence-pick-list">
              {(evidence.data ?? []).map((item) => (
                <Checkbox
                  key={item.id}
                  label={`${item.kind.replace(/_/g, ' ')} · ${formatBytes(item.size_bytes)}`}
                  hint={item.summary}
                  checked={evidenceIds.includes(item.id)}
                  onChange={() => toggleEvidence(item.id)}
                />
              ))}
            </div>
          )}
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
