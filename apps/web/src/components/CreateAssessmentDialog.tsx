import { useEffect, useId, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useCompanies, useCreateAssessment } from '../api/queries';
import { errorMessage } from '../api/client';
import type { ScopePolicy } from '../api/types';
import {
  defaultPolicy,
  policyFromTargets,
  policyValidationError,
  splitLines,
} from '../lib/scope';
import { Dialog } from './Dialog';
import { Field, SelectInput, Textarea, TextInput } from './Form';
import { ScopeForm } from './ScopeForm';
import { useToast } from './Toast';

export function CreateAssessmentDialog({
  open,
  onClose,
  companyId,
}: {
  open: boolean;
  onClose: () => void;
  companyId?: string;
}) {
  const companies = useCompanies();
  const mutation = useCreateAssessment();
  const navigate = useNavigate();
  const { notify } = useToast();
  const formId = useId();
  const uid = useId();

  const [selectedCompany, setSelectedCompany] = useState(companyId ?? '');
  const [name, setName] = useState('');
  const [environment, setEnvironment] = useState('');
  const [objective, setObjective] = useState('');
  const [targets, setTargets] = useState('');
  const [policy, setPolicy] = useState<ScopePolicy>(() => defaultPolicy());
  const [scopeKey, setScopeKey] = useState(0);
  const [validation, setValidation] = useState<string | null>(null);

  useEffect(() => {
    if (open) {
      setSelectedCompany(companyId ?? '');
      setName('');
      setEnvironment('');
      setObjective('');
      setTargets('');
      setPolicy(defaultPolicy());
      setScopeKey((current) => current + 1);
      setValidation(null);
      mutation.reset();
    }
    // Reset only when the dialog opens.
  }, [open, companyId]);

  useEffect(() => {
    if (!open || companyId || selectedCompany) return;
    const first = companies.data?.[0];
    if (first) setSelectedCompany(first.id);
  }, [open, companyId, selectedCompany, companies.data]);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    const resolvedCompany = companyId ?? selectedCompany;
    if (!resolvedCompany) {
      setValidation('Select a company.');
      return;
    }
    if (!name.trim() || !environment.trim() || !objective.trim()) {
      setValidation('Name, environment and objective are required.');
      return;
    }
    const targetsList = splitLines(targets);
    const effectivePolicy =
      policy.include_hosts.length === 0
        ? policyFromTargets(policy, targetsList)
        : policy;
    if (effectivePolicy.include_hosts.length === 0) {
      setValidation('Add at least one initial target or include host.');
      return;
    }
    const policyError = policyValidationError(effectivePolicy);
    if (policyError) {
      setValidation(policyError);
      return;
    }
    setValidation(null);
    mutation.mutate(
      {
        company_id: resolvedCompany,
        name: name.trim(),
        environment: environment.trim(),
        objective: objective.trim(),
        initial_targets: targetsList,
        policy: effectivePolicy,
      },
      {
        onSuccess: (assessment) => {
          notify(`Assessment ${assessment.name} created.`, 'success');
          onClose();
          navigate(`/assessments/${assessment.id}`);
        },
      },
    );
  };

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Create assessment"
      description="The initial scope is immutable once stored; later changes create a new version."
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
            disabled={mutation.isPending}
          >
            {mutation.isPending ? 'Creating…' : 'Create assessment'}
          </button>
        </>
      }
    >
      <form id={formId} onSubmit={submit} className="form-stack">
        {validation || mutation.isError ? (
          <p className="form-error" role="alert">
            {validation ?? errorMessage(mutation.error)}
          </p>
        ) : null}
        <div className="form-section">
          <h3 className="form-section-title">Assessment</h3>
          <div className="form-grid two">
            {!companyId ? (
              <Field label="Company" htmlFor={`${uid}-company`}>
                <SelectInput
                  id={`${uid}-company`}
                  value={selectedCompany}
                  onChange={(event) => setSelectedCompany(event.target.value)}
                  required
                >
                  <option value="">Select a company</option>
                  {(companies.data ?? []).map((company) => (
                    <option key={company.id} value={company.id}>
                      {company.name}
                    </option>
                  ))}
                </SelectInput>
              </Field>
            ) : null}
            <Field label="Name" htmlFor={`${uid}-name`}>
              <TextInput
                id={`${uid}-name`}
                value={name}
                onChange={(event) => setName(event.target.value)}
                placeholder="Customer portal"
                required
              />
            </Field>
            <Field label="Environment" htmlFor={`${uid}-environment`}>
              <TextInput
                id={`${uid}-environment`}
                value={environment}
                onChange={(event) => setEnvironment(event.target.value)}
                placeholder="production"
                required
              />
            </Field>
          </div>
          <Field label="Objective" htmlFor={`${uid}-objective`} hint="What this assessment must establish.">
            <Textarea
              id={`${uid}-objective`}
              value={objective}
              onChange={(event) => setObjective(event.target.value)}
              rows={3}
              placeholder="Establish whether a tenant user can reach another tenant's invoices."
              required
            />
          </Field>
          <Field
            label="Initial targets"
            htmlFor={`${uid}-targets`}
            hint="One absolute URL per line. Imported as candidate endpoints after creation."
          >
            <Textarea
              id={`${uid}-targets`}
              value={targets}
              onChange={(event) => setTargets(event.target.value)}
              rows={4}
              placeholder={'https://app.example.com/\nhttps://app.example.com/api/session'}
              spellCheck={false}
            />
          </Field>
        </div>

        <div className="form-section">
          <h3 className="form-section-title">Scope policy</h3>
          <ScopeForm
            key={scopeKey}
            initialPolicy={policy}
            onChange={setPolicy}
            disabled={mutation.isPending}
          />
        </div>

      </form>
    </Dialog>
  );
}
