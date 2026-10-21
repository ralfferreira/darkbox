import { useEffect, useId, useState } from 'react';
import { Link, useParams } from 'react-router-dom';
import { Plus } from 'lucide-react';
import { useAssessment, useCreateScopeVersion, usePreviewScope, useScopeVersions } from '../api/queries';
import { errorMessage } from '../api/client';
import type { ScopePolicy } from '../api/types';
import { PolicyView } from '../components/PolicyView';
import { ScopeForm } from '../components/ScopeForm';
import { Status } from '../components/Status';
import { Dialog } from '../components/Dialog';
import { Field, Textarea } from '../components/Form';
import {
  BlockSkeleton,
  EmptyState,
  ErrorState,
  PageHeader,
  Panel,
} from '../components/ui';
import { useCurrentSession } from '../hooks/useCurrentSession';
import { formatDateTime, formatRelative, plural } from '../lib/format';
import { policyValidationError, scopePolicyFrom, splitLines } from '../lib/scope';
import { useToast } from '../components/Toast';

export function ScopePage() {
  const { id } = useParams<{ id: string }>();
  const assessment = useAssessment(id);
  const versions = useScopeVersions(id);
  const preview = usePreviewScope(id ?? '');
  const session = useCurrentSession();
  const { notify } = useToast();
  const [targets, setTargets] = useState('');
  const [editorOpen, setEditorOpen] = useState(false);
  const [note, setNote] = useState('');
  const [policy, setPolicy] = useState<ScopePolicy | null>(null);
  const [scopeKey, setScopeKey] = useState(0);
  const [validation, setValidation] = useState<string | null>(null);
  const createVersion = useCreateScopeVersion(id ?? '');
  const formId = useId();
  const uid = useId();

  const effective = assessment.data?.scope;
  const canWrite = session.data?.can_write ?? false;

  useEffect(() => {
    if (editorOpen && effective) {
      setPolicy(scopePolicyFrom(effective.policy));
      setNote('');
      setValidation(null);
      setScopeKey((current) => current + 1);
      createVersion.reset();
    }
    // Reset only when the editor opens.
  }, [editorOpen]);

  if (assessment.isError) {
    return (
      <ErrorState
        error={assessment.error}
        label="Scope could not be loaded."
        onRetry={() => void assessment.refetch()}
      />
    );
  }

  if (assessment.isLoading || !assessment.data || !effective) {
    return (
      <>
        <PageHeader meta="scope" title="Scope" />
        <BlockSkeleton lines={8} />
      </>
    );
  }

  const orderedVersions = [...(versions.data ?? [])].sort((a, b) => b.version - a.version);

  const submitPreview = (event: React.FormEvent) => {
    event.preventDefault();
    const parsed = splitLines(targets);
    if (parsed.length > 0) preview.mutate(parsed);
  };

  const submitVersion = (event: React.FormEvent) => {
    event.preventDefault();
    if (!policy) return;
    const policyError = policyValidationError(policy);
    if (policyError) {
      setValidation(policyError);
      return;
    }
    if (!note.trim()) {
      setValidation('A note is required for a new scope version.');
      return;
    }
    setValidation(null);
    createVersion.mutate(
      { policy, note: note.trim() },
      {
        onSuccess: (version) => {
          notify(`Scope version v${version.version} created.`, 'success');
          setEditorOpen(false);
          preview.reset();
        },
      },
    );
  };

  const allowed = (preview.data ?? []).filter((decision) => decision.allowed).length;
  const denied = (preview.data ?? []).length - allowed;

  return (
    <>
      <PageHeader
        meta={`scope · policy v${effective.version}`}
        title="Scope"
        description="The effective policy, its version history and a preflight preview."
        actions={
          <button
            type="button"
            className="btn btn-primary"
            onClick={() => setEditorOpen(true)}
            disabled={!canWrite}
            title={canWrite ? undefined : 'Your role cannot create scope versions.'}
          >
            <Plus size={14} strokeWidth={1.5} aria-hidden="true" />
            New version
          </button>
        }
      />

      <div className="notice" data-tone="amber">
        Exclusions prevail over every include rule. A new scope version applies to future runs
        and does not expand a run that is already executing; each run keeps the policy version it
        started with.
      </div>

      <Panel
        title={`Effective policy · v${effective.version}`}
        subtitle={`${effective.created_by} · ${formatDateTime(effective.created_at)}`}
        actions={<span className="mono quiet">{effective.note || 'No note recorded.'}</span>}
      >
        <PolicyView policy={effective.policy} />
      </Panel>

      <div className="split">
        <div className="split-main">
          <Panel
            title="Preview"
            subtitle="Evaluate candidate targets against the effective policy."
            flush
          >
            <form className="preview-form" onSubmit={submitPreview}>
              <Field
                label="Targets"
                htmlFor={`${uid}-targets`}
                hint="One absolute URL or host per line."
              >
                <Textarea
                  id={`${uid}-targets`}
                  value={targets}
                  onChange={(event) => setTargets(event.target.value)}
                  rows={4}
                  placeholder={'https://app.example.com/admin\nhttps://excluded.example.com/'}
                  spellCheck={false}
                />
              </Field>
              <div className="form-actions">
                <button
                  type="submit"
                  className="btn"
                  disabled={splitLines(targets).length === 0 || preview.isPending}
                >
                  {preview.isPending ? 'Evaluating…' : 'Evaluate scope'}
                </button>
              </div>
            </form>

            {preview.isError ? (
              <div className="panel-pad">
                <ErrorState
                  error={preview.error}
                  label="Scope preview failed."
                  onRetry={() => void preview.mutate(splitLines(targets))}
                />
              </div>
            ) : preview.data ? (
              <>
                <div className="preview-summary mono">
                  <span>{allowed} allowed</span>
                  <span>{denied} denied</span>
                  <span>policy v{preview.data[0]?.policy_version ?? effective.version}</span>
                </div>
                {preview.data.length === 0 ? (
                  <EmptyState>No targets were evaluated.</EmptyState>
                ) : (
                  <div className="table-wrap">
                    <table className="data-table preview-table">
                      <thead>
                        <tr>
                          <th scope="col">Target</th>
                          <th scope="col">Decision</th>
                          <th scope="col">Rule</th>
                          <th scope="col">Code</th>
                          <th scope="col">Action class</th>
                          <th scope="col" className="num">
                            Policy
                          </th>
                        </tr>
                      </thead>
                      <tbody>
                        {preview.data.map((decision, index) => (
                          <tr key={`${decision.target}-${index}`}>
                            <td className="mono break-any">{decision.target}</td>
                            <td>
                              <Status
                                value={decision.allowed ? 'allowed' : 'denied'}
                                label={decision.allowed ? 'allowed' : 'denied'}
                                tone={decision.allowed ? 'lime' : 'red'}
                              />
                            </td>
                            <td className="mono">{decision.rule}</td>
                            <td className="mono quiet">{decision.code ?? '—'}</td>
                            <td>
                              <span className="mono">{decision.action_class}</span>
                            </td>
                            <td className="num mono">v{decision.policy_version}</td>
                          </tr>
                        ))}
                      </tbody>
                    </table>
                  </div>
                )}
              </>
            ) : (
              <EmptyState>Enter targets to preview scope decisions.</EmptyState>
            )}
          </Panel>
        </div>

        <div className="split-side">
          <Panel
            title="Version history"
            subtitle={
              versions.data
                ? `${versions.data.length} ${plural(versions.data.length, 'version')}`
                : undefined
            }
            flush
          >
            {versions.isLoading ? (
              <div className="panel-pad">
                <BlockSkeleton lines={6} />
              </div>
            ) : versions.isError ? (
              <div className="panel-pad">
                <ErrorState
                  error={versions.error}
                  label="Scope versions could not be loaded."
                  onRetry={() => void versions.refetch()}
                />
              </div>
            ) : orderedVersions.length === 0 ? (
              <EmptyState>No scope versions recorded.</EmptyState>
            ) : (
              <div className="version-list">
                {orderedVersions.map((version) => (
                  <details key={version.id} className="version-item">
                    <summary>
                      <span className="mono version-number">v{version.version}</span>
                      <span className="version-note">{version.note || 'No note.'}</span>
                      <span className="mono quiet version-meta">
                        {version.created_by} · {formatRelative(version.created_at)}
                      </span>
                      {version.version === effective.version ? (
                        <span className="badge" data-tone="lime">
                          effective
                        </span>
                      ) : null}
                    </summary>
                    <div className="version-body">
                      <PolicyView policy={version.policy} />
                    </div>
                  </details>
                ))}
              </div>
            )}
          </Panel>
        </div>
      </div>

      <p className="page-footnote">
        Runs pin the policy version they started with. Inspect a run header to see its version.{' '}
        <Link to={`/assessments/${id}/runs`}>Open runs</Link>
      </p>

      <Dialog
        open={editorOpen}
        onClose={() => setEditorOpen(false)}
        title="New scope version"
        description="Stored immutably. Future runs will use this policy."
        size="lg"
        footer={
          <>
            <button type="button" className="btn" onClick={() => setEditorOpen(false)}>
              Cancel
            </button>
            <button
              type="submit"
              form={formId}
              className="btn btn-primary"
              disabled={createVersion.isPending}
            >
              {createVersion.isPending ? 'Saving…' : 'Save version'}
            </button>
          </>
        }
      >
        <form id={formId} onSubmit={submitVersion} className="form-stack">
          <div className="notice" data-tone="amber">
            Exclusions prevail. A new version does not expand a run that is already executing.
          </div>
          <Field
            label="Note"
            htmlFor={`${uid}-version-note`}
            hint="Required. Record why the scope changed."
          >
            <Textarea
              id={`${uid}-version-note`}
              value={note}
              onChange={(event) => setNote(event.target.value)}
              rows={2}
              required
            />
          </Field>
          {policy ? (
            <ScopeForm
              key={scopeKey}
              initialPolicy={policy}
              onChange={setPolicy}
              disabled={createVersion.isPending}
            />
          ) : null}
          {validation || createVersion.isError ? (
            <p className="form-error" role="alert">
              {validation ?? errorMessage(createVersion.error)}
            </p>
          ) : null}
        </form>
      </Dialog>
    </>
  );
}
