import { useState } from 'react';
import { ArrowRight, Terminal } from 'lucide-react';
import { DEV_TOKEN } from '../api/client';
import { useAuth } from '../components/AuthContext';

export function AuthScreen() {
  const { signIn, rejection } = useAuth();
  const [token, setToken] = useState('');

  const submit = (event: React.FormEvent) => {
    event.preventDefault();
    const trimmed = token.trim();
    if (trimmed) signIn(trimmed);
  };

  const useDevToken = () => {
    setToken(DEV_TOKEN);
    signIn(DEV_TOKEN);
  };

  return (
    <div className="auth-screen">
      <div className="auth-frame">
        <div className="auth-brand">
          <span className="brand-glyph" aria-hidden="true">
            <svg width="17" height="17" viewBox="0 0 16 16" fill="none">
              <rect
                x="2.25"
                y="2.25"
                width="11.5"
                height="11.5"
                rx="0.5"
                stroke="currentColor"
                strokeWidth="1.5"
              />
              <path d="M5.5 8h5M8 5.5v5" stroke="currentColor" strokeWidth="1.5" />
            </svg>
          </span>
          <span className="brand-word mono">DARKBOX</span>
        </div>

        <div className="auth-card">
          <div className="auth-kicker mono">operator access</div>
          <h1>Connect to workspace</h1>
          <p className="auth-copy">
            Enter the bearer token issued for this workspace. The token is held in this
            browser and sent with every request.
          </p>

          <form onSubmit={submit} className="auth-form">
            <div className="field">
              <label htmlFor="auth-token">Bearer token</label>
              <input
                id="auth-token"
                className="input mono"
                type="password"
                value={token}
                onChange={(event) => setToken(event.target.value)}
                placeholder="token"
                autoComplete="off"
                spellCheck={false}
                autoFocus
              />
            </div>
            {rejection ? (
              <p className="form-error" role="alert">
                {rejection}
              </p>
            ) : null}
            <div className="auth-actions">
              <button type="submit" className="btn btn-primary" disabled={!token.trim()}>
                Connect
                <ArrowRight size={14} strokeWidth={1.5} aria-hidden="true" />
              </button>
              <button type="button" className="btn" onClick={useDevToken}>
                <Terminal size={14} strokeWidth={1.5} aria-hidden="true" />
                Use local dev token
              </button>
            </div>
          </form>

          <p className="auth-note">
            The API is local, proxied at <code className="mono">/v1</code> to{' '}
            <code className="mono">127.0.0.1:8787</code>. There is no remote account flow.
          </p>
        </div>
      </div>
    </div>
  );
}
