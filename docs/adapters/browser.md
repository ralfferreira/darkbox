# Browser adapter protocol

The adapter is a Python process that owns Playwright (or Camoufox when it is
actually installed). The Rust core owns policy, budgets, evidence and the
objective. There are two modes.

## Interactive host (the session 1 path)

`adapter.py --serve --host 127.0.0.1 --port 0 --mode auto` runs a long-lived
HTTP JSON host. The API starts it on demand on a loopback port and keeps one
process alive for the lifetime of the API. The host is thread-affine: one
worker thread owns Playwright and commands are queued to it.

Requests carry the pinned run policy and a session key:

```json
{
  "schema_version": 1,
  "kind": "act",
  "session_key": "ctx-<run>-<identity>-<uuid>",
  "policy": { "...": "ScopePolicy" },
  "payload": { "action": { "type": "click", "ref": "e12" } },
  "timeout_seconds": 55
}
```

Kinds: `session.open`, `navigate`, `act`, `login`, `observe`, `screenshot`,
`traffic`, `session.state`. `POST /v1/abort` signals cancellation and
`POST /v1/close` closes the context. `GET /v1/capability` starts the engine and
executes a JavaScript probe, then reports the real engine name, version,
executable, user agent and any Camoufox import error.

A session owns one `BrowserContext` per assessment, identity and tenant. Cookies,
local storage and the current page survive between commands. Contexts never
share a cookie jar. A request that is not allowed by `ScopePolicy` is aborted
inside `context.route`, including subresources and redirects. Blocked requests
are returned in the observation and in `browser_traffic`.

The observation is semantic. It contains the current URL, title, headings, page
text, tables, forms and interactive elements with stable `data-darkbox-ref`
references. Password values are never returned. The model receives refs, not
coordinates or a pre-recorded selector script.

`login` resolves `secret_ref` from the adapter environment (`env:NAME`). The
secret is filled into the observed field and never appears in a request payload,
model prompt, transcript, log or evidence. Success is not a 200 status: the
adapter checks that the password form disappeared, that the URL is not a login
URL, and that a logout/success text/selector is visible. A remaining login form
or an error string is `login_failed`. A page that changed without confirmation
is `unknown`.

The API stores the session row with its `context_key`, engine, state and last
observation. A lost context returns `session_lost`; it is never treated as an
authenticated session.

## One-shot contract

The original one-request stdin/stdout JSON Lines mode remains for deterministic
contract tests and legacy `browser.visibility`. It uses a real HTTP cookie jar,
not a browser. It does not execute JavaScript and is not used for interactive
agent work.

## Modes

- `auto`: Camoufox when importable and launchable, otherwise Playwright Chromium.
- `playwright`: Playwright Chromium.
- `camoufox`: requires a working Camoufox install; otherwise returns a
  structured error with the import/launch detail.
- `mock`: one-shot HTTP client only. Interactive sessions are refused.

The engine is reported from the running browser, never from an environment
variable. With Playwright 1.60.0 on this machine the probe reports Chromium
153.0.8010.12. Camoufox is not present: installing it with the system Python
3.9 fails in `pyobjc-core` during wheel construction, and the adapter reports
that error instead of claiming Camoufox.

## Local and container reachability

The API starts the adapter on `127.0.0.1` inside its own process namespace. In
Compose that is the API container, and the image installs Playwright Chromium at
`/opt/browser`. A target running on the operator's host is not `127.0.0.1` from
inside the container. Use `host.docker.internal` (Docker Desktop) or the host
gateway, and include that host in the scope policy and allowed ports. A target
in another Compose service uses its service name, for example `http://lab:8080`.

The same adapter accepts public destinations when the scope includes them. The
scope policy is still evaluated in the core and again on every browser request.
No test in this session uses a real third-party system.

## Process safety

The core enforces the command timeout, output caps and cancellation. The host
process is killed with the API process. The adapter refuses a non-loopback bind
without `DARKBOX_BROWSER_TOKEN`.
