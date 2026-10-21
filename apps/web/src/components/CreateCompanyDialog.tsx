import { useEffect, useId, useState } from 'react';
import { useCreateCompany } from '../api/queries';
import { errorMessage } from '../api/client';
import { Dialog } from './Dialog';
import { Field, Textarea, TextInput } from './Form';
import { useToast } from './Toast';

export function CreateCompanyDialog({
  open,
  onClose,
}: {
  open: boolean;
  onClose: () => void;
}) {
  const [name, setName] = useState('');
  const [description, setDescription] = useState('');
  const mutation = useCreateCompany();
  const { notify } = useToast();
  const formId = useId();

  useEffect(() => {
    if (open) {
      setName('');
      setDescription('');
      mutation.reset();
    }
    // Reset only when the dialog opens.
  }, [open]);

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    mutation.mutate(
      { name: name.trim(), description: description.trim() },
      {
        onSuccess: (company) => {
          notify(`Company ${company.name} created.`, 'success');
          onClose();
        },
      },
    );
  };

  const valid = name.trim().length > 0;

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title="Create company"
      description="A company groups assessments for one target organisation."
      size="sm"
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
            {mutation.isPending ? 'Creating…' : 'Create company'}
          </button>
        </>
      }
    >
      <form id={formId} onSubmit={submit} className="form-stack">
        <Field label="Name" htmlFor="company-name">
          <TextInput
            id="company-name"
            value={name}
            onChange={(event) => setName(event.target.value)}
            placeholder="Acme"
            autoComplete="off"
            required
          />
        </Field>
        <Field label="Description" htmlFor="company-description" hint="Optional.">
          <Textarea
            id="company-description"
            value={description}
            onChange={(event) => setDescription(event.target.value)}
            rows={3}
            placeholder="What this company covers."
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
