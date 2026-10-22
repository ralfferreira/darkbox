import { useEffect, useId, useState } from 'react';
import { useCreateIdentity, useUpdateIdentity } from '../api/queries';
import type { IdentityDto, JsonValue } from '../api/types';
import { errorMessage } from '../api/client';
import { Dialog } from './Dialog';
import { Field, TextInput } from './Form';

function configRecord(value: JsonValue | undefined): Record<string, JsonValue> {
  return value !== null && typeof value === 'object' && !Array.isArray(value)
    ? (value as Record<string, JsonValue>)
    : {};
}

function configText(config: Record<string, JsonValue>, key: string): string {
  const value = config[key];
  return typeof value === 'string' ? value : '';
}

export function IdentityDialog({
  open,
  onClose,
  assessmentId,
  identity,
}: {
  open: boolean;
  onClose: () => void;
  assessmentId: string;
  identity?: IdentityDto | null;
}) {
  const create = useCreateIdentity(assessmentId);
  const update = useUpdateIdentity(assessmentId);
  const uid = useId();
  const [label, setLabel] = useState('');
  const [role, setRole] = useState('member');
  const [tenants, setTenants] = useState('north');
  const [secretRef, setSecretRef] = useState('');
  const [loginUrl, setLoginUrl] = useState('');
  const [username, setUsername] = useState('');
  const [usernameSelector, setUsernameSelector] = useState('');
  const [passwordSelector, setPasswordSelector] = useState('');
  const [submitSelector, setSubmitSelector] = useState('');
  const [successText, setSuccessText] = useState('');
  const [successSelector, setSuccessSelector] = useState('');
  const [failureText, setFailureText] = useState('');
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    const config = configRecord(identity?.login_config);
    setLabel(identity?.label ?? '');
    setRole(identity?.role_label ?? 'member');
    setTenants((identity?.tenant_keys ?? ['north']).join(', '));
    setSecretRef('');
    setLoginUrl(configText(config, 'login_url'));
    setUsername(configText(config, 'username'));
    setUsernameSelector(configText(config, 'username_selector'));
    setPasswordSelector(configText(config, 'password_selector'));
    setSubmitSelector(configText(config, 'submit_selector'));
    setSuccessText(configText(config, 'success_text'));
    setSuccessSelector(configText(config, 'success_selector'));
    setFailureText(configText(config, 'failure_text'));
    setError(null);
  }, [open, identity]);

  const submit = async (event: React.FormEvent) => {
    event.preventDefault();
    setError(null);
    const login_config: Record<string, string> = {};
    for (const [key, value] of Object.entries({
      login_url: loginUrl.trim(),
      username: username.trim(),
      username_selector: usernameSelector.trim(),
      password_selector: passwordSelector.trim(),
      submit_selector: submitSelector.trim(),
      success_text: successText.trim(),
      success_selector: successSelector.trim(),
      failure_text: failureText.trim(),
    })) {
      if (value) login_config[key] = value;
    }
    try {
      if (identity) {
        await update.mutateAsync({
          identityId: identity.id,
          body: {
            role_label: role.trim(),
            tenant_keys: tenants
              .split(',')
              .map((item) => item.trim())
              .filter(Boolean),
            ...(secretRef.trim() ? { secret_ref: secretRef.trim() } : {}),
            login_config,
          },
        });
      } else {
        await create.mutateAsync({
          label: label.trim(),
          role_label: role.trim(),
          tenant_keys: tenants
            .split(',')
            .map((item) => item.trim())
            .filter(Boolean),
          capabilities: ['navigate', 'read'],
          secret_ref: secretRef.trim() || null,
          login_config,
        });
      }
      onClose();
    } catch (cause) {
      setError(errorMessage(cause));
    }
  };

  const busy = create.isPending || update.isPending;
  const valid = Boolean(label.trim() || identity) && Boolean(role.trim());

  return (
    <Dialog
      open={open}
      onClose={onClose}
      title={identity ? `Identity · ${identity.label}` : 'Add identity'}
      description="The secret is resolved from an environment reference at login time. It is never shown to the model or written to evidence."
      size="md"
      footer={
        <>
          <button type="button" className="btn" onClick={onClose}>
            Cancel
          </button>
          <button type="submit" form={`${uid}-form`} className="btn btn-primary" disabled={busy || !valid}>
            {busy ? 'Saving…' : identity ? 'Save identity' : 'Add identity'}
          </button>
        </>
      }
    >
      <form id={`${uid}-form`} onSubmit={submit} className="form-stack">
        <div className="form-grid two">
          <Field label="Label" htmlFor={`${uid}-label`} hint="Used as the login username unless overridden.">
            <TextInput
              id={`${uid}-label`}
              value={label}
              onChange={(event) => setLabel(event.target.value)}
              disabled={Boolean(identity)}
              required
            />
          </Field>
          <Field label="Role label" htmlFor={`${uid}-role`}>
            <TextInput id={`${uid}-role`} value={role} onChange={(event) => setRole(event.target.value)} />
          </Field>
        </div>
        <div className="form-grid two">
          <Field label="Tenants" htmlFor={`${uid}-tenants`} hint="Comma separated. The browser context is fixed to one tenant.">
            <TextInput
              id={`${uid}-tenants`}
              value={tenants}
              onChange={(event) => setTenants(event.target.value)}
            />
          </Field>
          <Field
            label="Secret reference"
            htmlFor={`${uid}-secret`}
            hint="env:NAME only. Leave blank to keep the current reference."
          >
            <TextInput
              id={`${uid}-secret`}
              value={secretRef}
              onChange={(event) => setSecretRef(event.target.value)}
              placeholder="env:LAB_ALICE_PASSWORD"
              autoComplete="off"
            />
          </Field>
        </div>
        <div className="notice">
          <strong>Login configuration (optional)</strong>
          <p>
            Written as login hints, not as a selector script. When omitted, the agent reads the
            rendered login form and identifies its fields semantically.
          </p>
        </div>
        <Field label="Login URL" htmlFor={`${uid}-login-url`} hint="The page containing the form, if it is not the starting URL.">
          <TextInput
            id={`${uid}-login-url`}
            value={loginUrl}
            onChange={(event) => setLoginUrl(event.target.value)}
            placeholder="http://127.0.0.1:8080/login"
          />
        </Field>
        <div className="form-grid two">
          <Field label="Username value" htmlFor={`${uid}-username`} hint="Defaults to the identity label.">
            <TextInput
              id={`${uid}-username`}
              value={username}
              onChange={(event) => setUsername(event.target.value)}
            />
          </Field>
          <Field label="Username selector" htmlFor={`${uid}-user-sel`} hint="Optional fallback hint.">
            <TextInput
              id={`${uid}-user-sel`}
              value={usernameSelector}
              onChange={(event) => setUsernameSelector(event.target.value)}
              placeholder="input[autocomplete=username]"
            />
          </Field>
          <Field label="Password selector" htmlFor={`${uid}-pass-sel`} hint="Optional fallback hint.">
            <TextInput
              id={`${uid}-pass-sel`}
              value={passwordSelector}
              onChange={(event) => setPasswordSelector(event.target.value)}
              placeholder="input[type=password]"
            />
          </Field>
          <Field label="Submit selector" htmlFor={`${uid}-submit-sel`} hint="Optional fallback hint.">
            <TextInput
              id={`${uid}-submit-sel`}
              value={submitSelector}
              onChange={(event) => setSubmitSelector(event.target.value)}
              placeholder="button[type=submit]"
            />
          </Field>
        </div>
        <div className="form-grid two">
          <Field label="Success text" htmlFor={`${uid}-success`} hint="Visible text that confirms authentication.">
            <TextInput
              id={`${uid}-success`}
              value={successText}
              onChange={(event) => setSuccessText(event.target.value)}
              placeholder="Signed in as"
            />
          </Field>
          <Field label="Success selector" htmlFor={`${uid}-success-sel`} hint="Optional element visible only after login.">
            <TextInput
              id={`${uid}-success-sel`}
              value={successSelector}
              onChange={(event) => setSuccessSelector(event.target.value)}
            />
          </Field>
        </div>
        <Field label="Failure text" htmlFor={`${uid}-failure`} hint="Visible text that distinguishes a rejected login.">
          <TextInput
            id={`${uid}-failure`}
            value={failureText}
            onChange={(event) => setFailureText(event.target.value)}
            placeholder="Invalid credentials"
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
