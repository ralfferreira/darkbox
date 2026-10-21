import { useEffect, useId, useState } from 'react';
import { Link } from 'react-router-dom';
import type { FindingDto, FindingState } from '../api/types';
import { useResults, useReviewFinding } from '../api/queries';
import { errorMessage } from '../api/client';
import { formatDateTime, formatRelative, humanize, shortId } from '../lib/format';
import { Drawer } from './Drawer';
import { Status } from './Status';
import { Field, SelectInput, Textarea } from './Form';
import { IdChip, KeyValue } from './ui';
import { useToast } from './Toast';

const FINDING_TRANSITIONS: Record<FindingState, FindingState[]> = {
  hypothesis: ['needs_review', 'rejected'],
  needs_review: ['confirmed', 'rejected'],
  confirmed: ['remediated', 'reopened'],
  rejected: ['reopened'],
  remediated: ['reopened'],
  reopened: ['needs_review', 'confirmed', 'rejected'],
};

export function FindingDrawer({
  finding,
  assessmentId,
  canReview,
  onClose,
}: {
  finding: FindingDto | null;
  assessmentId: string;
  canReview: boolean;
  onClose: () => void;
}) {
  const results = useResults(assessmentId, 100);
  const mutation = useReviewFinding(assessmentId);
  const { notify } = useToast();
  const uid = useId();
  const [nextState, setNextState] = useState<FindingState>('needs_review');
  const [note, setNote] = useState('');

  useEffect(() => {
    if (finding) {
      const transitions = FINDING_TRANSITIONS[finding.state];
      setNextState(transitions[0] ?? 'needs_review');
      setNote('');
      mutation.reset();
    }
    // Reset only when the selected finding changes.
  }, [finding?.id, finding?.state]);

  if (!finding) return null;

  const transitions = FINDING_TRANSITIONS[finding.state];
  const matchedResults = (results.data ?? []).filter((result) =>
    finding.test_result_ids.includes(result.id),
  );
  const confirmWithoutEvidence = nextState === 'confirmed' && finding.evidence_ids.length === 0;

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    if (!note.trim()) return;
    mutation.mutate(
      { findingId: finding.id, body: { state: nextState, note: note.trim() } },
      {
        onSuccess: () => {
          notify('Finding review recorded.', 'success');
          setNote('');
        },
      },
    );
  };

  return (
    <Drawer
      open
      onClose={onClose}
      title={finding.title}
      subtitle={
        <span className="drawer-sub-line">
          <Status value={finding.state} />
          <span className="sev mono">{finding.severity}</span>
          <span className="mono muted">
            {finding.evidence_ids.length} evidence · {formatRelative(finding.updated_at)}
          </span>
        </span>
      }
      footer={
        <button type="button" className="btn" onClick={onClose}>
          Close
        </button>
      }
    >
      <section className="drawer-section">
        <h3 className="drawer-section-title mono">Claim</h3>
        <p className="prose">{finding.claim}</p>
      </section>

      <section className="drawer-section">
        <h3 className="drawer-section-title mono">Impact</h3>
        <p className="prose">{finding.impact}</p>
      </section>

      <section className="drawer-section">
        <KeyValue
          items={[
            { label: 'State', value: <Status value={finding.state} /> },
            { label: 'Severity', value: <span className="mono">{finding.severity}</span> },
            {
              label: 'Endpoint',
              value: finding.endpoint_id ? (
                <Link
                  to={`/assessments/${assessmentId}/inventory?focus=${finding.endpoint_id}`}
                  className="mono"
                >
                  {shortId(finding.endpoint_id)}
                </Link>
              ) : (
                <span className="muted">none</span>
              ),
            },
            { label: 'Created', value: formatDateTime(finding.created_at) },
            { label: 'Updated', value: formatDateTime(finding.updated_at) },
            { label: 'ID', value: <IdChip id={finding.id} length={12} /> },
          ]}
        />
      </section>

      <section className="drawer-section">
        <h3 className="drawer-section-title mono">
          Evidence
          <span className="section-count mono">{finding.evidence_ids.length}</span>
        </h3>
        {finding.evidence_ids.length === 0 ? (
          <p className="drawer-empty">No evidence attached. Confirmation should not proceed.</p>
        ) : (
          <ul className="link-list">
            {finding.evidence_ids.map((id) => (
              <li key={id}>
                <Link to={`/assessments/${assessmentId}/evidence?focus=${id}`} className="mono">
                  {shortId(id)}
                </Link>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="drawer-section">
        <h3 className="drawer-section-title mono">
          Test results
          <span className="section-count mono">{finding.test_result_ids.length}</span>
        </h3>
        {finding.test_result_ids.length === 0 ? (
          <p className="drawer-empty">No test results linked.</p>
        ) : matchedResults.length === 0 ? (
          <p className="drawer-empty">Linked results are not in the current result window.</p>
        ) : (
          <ul className="result-list">
            {matchedResults.map((result) => (
              <li key={result.id}>
                <div className="result-head">
                  <span className="result-procedure mono">{result.procedure_key}</span>
                  <Status value={result.state} />
                </div>
                <p className="result-conclusion">{result.conclusion || 'No conclusion recorded.'}</p>
                <span className="mono result-meta">
                  {result.target_kind} {shortId(result.target_id)} · {formatRelative(result.created_at)}
                </span>
              </li>
            ))}
          </ul>
        )}
      </section>

      <section className="drawer-section">
        <h3 className="drawer-section-title mono">
          Revisions
          <span className="section-count mono">{finding.revisions.length}</span>
        </h3>
        {finding.revisions.length === 0 ? (
          <p className="drawer-empty">No revisions recorded.</p>
        ) : (
          <ol className="revision-list">
            {[...finding.revisions]
              .sort((a, b) => b.revision - a.revision)
              .map((revision) => (
                <li key={revision.revision}>
                  <div className="revision-head">
                    <span className="mono">r{revision.revision}</span>
                    <Status value={revision.state} />
                    <span className="mono muted">{revision.actor}</span>
                    <span className="mono muted">{formatDateTime(revision.at)}</span>
                  </div>
                  {revision.note ? <p className="revision-note">{revision.note}</p> : null}
                </li>
              ))}
          </ol>
        )}
      </section>

      {canReview && transitions.length > 0 ? (
        <section className="drawer-section review-section">
          <h3 className="drawer-section-title mono">Review</h3>
          <form onSubmit={submit} className="form-stack">
            <Field label="Next state" htmlFor={`${uid}-state`}>
              <SelectInput
                id={`${uid}-state`}
                value={nextState}
                onChange={(event) => setNextState(event.target.value as FindingState)}
              >
                {transitions.map((state) => (
                  <option key={state} value={state}>
                    {humanize(state)}
                  </option>
                ))}
              </SelectInput>
            </Field>
            <Field
              label="Note"
              htmlFor={`${uid}-note`}
              hint="Required. Recorded in the revision history."
            >
              <Textarea
                id={`${uid}-note`}
                value={note}
                onChange={(event) => setNote(event.target.value)}
                rows={3}
                required
              />
            </Field>
            {nextState === 'confirmed' ? (
              <p className="notice" data-tone={confirmWithoutEvidence ? 'amber' : undefined}>
                {confirmWithoutEvidence
                  ? 'Confirmation requires evidence. This finding has no evidence attached; attach evidence before confirming.'
                  : 'Confirmation requires evidence. Verify the attached evidence supports the claim.'}
              </p>
            ) : null}
            {mutation.isError ? (
              <p className="form-error" role="alert">
                {errorMessage(mutation.error)}
              </p>
            ) : null}
            <div className="form-actions">
              <button
                type="submit"
                className="btn btn-primary"
                disabled={!note.trim() || mutation.isPending}
              >
                {mutation.isPending ? 'Recording…' : 'Record review'}
              </button>
            </div>
          </form>
        </section>
      ) : null}
    </Drawer>
  );
}
