import { useEffect, useId, useState } from 'react';
import type { IdentityDto } from '../api/types';
import { useContextPackage } from '../api/queries';
import { formatDateTime, formatNumber, formatTokens, ratioPercent } from '../lib/format';
import { Drawer } from './Drawer';
import { Field, SelectInput, TextInput } from './Form';
import { ReferenceChip } from './ReferenceChip';
import { BlockSkeleton, ErrorState, Metric } from './ui';

const DEFAULT_BUDGET = 8000;

export function ContextDrawer({
  open,
  onClose,
  assessmentId,
  identities,
}: {
  open: boolean;
  onClose: () => void;
  assessmentId: string;
  identities: IdentityDto[];
}) {
  const uid = useId();
  const [budgetInput, setBudgetInput] = useState(String(DEFAULT_BUDGET));
  const [budget, setBudget] = useState(DEFAULT_BUDGET);
  const [identityId, setIdentityId] = useState('');
  const context = useContextPackage(assessmentId, budget, identityId, open);

  useEffect(() => {
    if (open) {
      setBudgetInput(String(DEFAULT_BUDGET));
      setBudget(DEFAULT_BUDGET);
      setIdentityId('');
    }
  }, [open]);

  const applyBudget = (event: React.FormEvent) => {
    event.preventDefault();
    const parsed = Number(budgetInput);
    if (Number.isFinite(parsed) && parsed > 0) {
      setBudget(Math.round(parsed));
    }
  };

  const data = context.data;
  const used = data ? ratioPercent(data.estimated_tokens, data.token_budget) : null;

  return (
    <Drawer
      open={open}
      onClose={onClose}
      title="Context package"
      subtitle={<span className="mono">token-budgeted memory and references</span>}
      footer={
        <button type="button" className="btn" onClick={onClose}>
          Close
        </button>
      }
    >
      <section className="drawer-section">
        <form className="context-controls" onSubmit={applyBudget}>
          <Field label="Token budget" htmlFor={`${uid}-budget`}>
            <div className="inline-input">
              <TextInput
                id={`${uid}-budget`}
                type="number"
                min={1}
                value={budgetInput}
                onChange={(event) => setBudgetInput(event.target.value)}
              />
              <button type="submit" className="btn">
                Apply
              </button>
            </div>
          </Field>
          <Field label="Identity" htmlFor={`${uid}-identity`}>
            <SelectInput
              id={`${uid}-identity`}
              value={identityId}
              onChange={(event) => setIdentityId(event.target.value)}
            >
              <option value="">Assessment default</option>
              {identities.map((identity) => (
                <option key={identity.id} value={identity.id}>
                  {identity.label}
                </option>
              ))}
            </SelectInput>
          </Field>
        </form>
      </section>

      {context.isLoading ? (
        <section className="drawer-section">
          <BlockSkeleton lines={6} />
        </section>
      ) : context.isError ? (
        <section className="drawer-section">
          <ErrorState
            error={context.error}
            label="Context package could not be built."
            onRetry={() => void context.refetch()}
          />
        </section>
      ) : data ? (
        <>
          <section className="drawer-section">
            <div className="metrics metrics-compact">
              <Metric label="Token budget" value={formatTokens(data.token_budget)} />
              <Metric label="Estimated" value={formatTokens(data.estimated_tokens)} />
              <Metric
                label="Used"
                value={used === null ? '—' : `${used.toFixed(0)}%`}
                hint={
                  <span className="budget-track" aria-hidden="true">
                    <span
                      className="budget-fill"
                      style={{ width: `${Math.min(100, used ?? 0)}%` }}
                    />
                  </span>
                }
              />
            </div>
            <dl className="kv kv-tight">
              <div>
                <dt className="mono">Policy version</dt>
                <dd className="mono">v{data.policy_version}</dd>
              </div>
              <div>
                <dt className="mono">Generated</dt>
                <dd>{formatDateTime(data.generated_at)}</dd>
              </div>
              <div>
                <dt className="mono">Sections</dt>
                <dd>{formatNumber(data.sections.length)}</dd>
              </div>
            </dl>
            <p className="drawer-note">{data.objective}</p>
          </section>

          <section className="drawer-section">
            <h3 className="drawer-section-title mono">
              Sections
              <span className="section-count mono">{data.sections.length}</span>
            </h3>
            {data.sections.length === 0 ? (
              <p className="drawer-empty">No memory pages fit inside this budget.</p>
            ) : (
              <div className="context-sections">
                {data.sections.map((section, index) => (
                  <article key={`${section.title}-${index}`} className="context-section">
                    <header>
                      <h4>{section.title}</h4>
                      {section.reference_kind ? (
                        <span className="mono muted">{section.reference_kind}</span>
                      ) : null}
                    </header>
                    <pre>{section.body}</pre>
                  </article>
                ))}
              </div>
            )}
          </section>

          {data.omitted.length > 0 ? (
            <section className="drawer-section">
              <h3 className="drawer-section-title mono">
                Omitted
                <span className="section-count mono">{data.omitted.length}</span>
              </h3>
              <ul className="plain-list">
                {data.omitted.map((item) => (
                  <li key={item}>{item}</li>
                ))}
              </ul>
            </section>
          ) : null}

          {data.references.length > 0 ? (
            <section className="drawer-section">
              <h3 className="drawer-section-title mono">References</h3>
              <div className="ref-list">
                {data.references.map((reference, index) => (
                  <ReferenceChip
                    key={`${reference.kind}-${reference.id}-${index}`}
                    reference={reference}
                    assessmentId={assessmentId}
                  />
                ))}
              </div>
            </section>
          ) : null}
        </>
      ) : null}
    </Drawer>
  );
}
