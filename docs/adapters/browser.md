# Browser adapter protocol

## Process contract

The Rust supervisor spawns one adapter process per step. A request is one JSON
line on stdin. Responses are JSON lines on stdout. Logs go to stderr.

```json
{
  "schema_version": 1,
  "request_id": "<step id>",
  "kind": "browser.session",
  "workspace_id": "...",
  "assessment_id": "...",
  "run_id": "...",
  "step_id": "...",
  "scope_version": 3,
  "policy": { "...": "ScopePolicy" },
  "payload": { "...": "operation specific" },
  "limits": { "timeout_seconds": 120, "max_output_bytes": 8388608, "max_steps": 50 }
}
```

Output messages:

- `{"type":"progress","request_id":"...","payload":{...}}`
- `{"type":"result","request_id":"...","payload":{...}}`
- `{"type":"error","request_id":"...","payload":{"message":"..."}}`

The supervisor enforces a wall-clock timeout, a total output cap and an 8 MiB
line cap. On timeout, protocol failure or cancellation it kills the process
group, so a browser child does not survive. stderr is capped and attached to a
failure, never to a successful result.

## Kinds

`browser.session` establishes one context for one identity and tenant.

Payload: identity label, role, tenant keys, secret reference, tenant, base URL,
login path, username. Result: session state, context key, expiry, evidence and
observations. The supervisor records the session row. Cookie values are never
returned.

`browser.flow` logs in and replays ordered steps. Each step returns ordinal,
status, final URL, title and `confirmed`. The supervisor updates the flow to
`reproduced`, `observed` or `blocked` and attaches evidence ids.

`browser.visibility` logs in as each identity independently and compares the
same checks. A difference produces an `inconclusive` test result: a visible
difference does not prove missing server-side authorization.

## Policy inside the adapter

The adapter receives the pinned policy and re-checks every URL it navigates to:
scheme, port, host exclusion, path exclusion, host inclusion, and auxiliary
destinations. Redirects are followed manually with a check per hop and a limit
of five. The adapter does not re-resolve hostnames; the supervisor does that for
the initial URL. A production deployment should pin resolution at connection
time.

## Secrets

`secret_ref` is `env:NAME`. The adapter resolves the value from its environment.
The value is not written to the request, the command line, evidence, stdout or
stderr. For the bundled synthetic lab only, the adapter falls back to the
documented lab passwords and logs the fallback.

## Modes

- `mock` (default in the local guide): a real HTTP client with one cookie jar
  per identity. It proves tenant and identity isolation, login and flow replay
  without downloading a browser. It does not execute page JavaScript.
- `auto`: uses Camoufox when the package is importable, otherwise `mock`.
- `camoufox`: reserved for the pinned integration. In this build it returns a
  structured error rather than claiming a browser ran.

## Adding another engine

Implement the same JSON Lines contract and record the engine name and version
in the capability list. Keep policy decisions in the adapter for navigation and
return evidence as text or an object key. Do not return cookies, raw
credentials, or a shell command. Add a contract test for invalid JSON,
oversized output, timeout, unexpected exit and cancellation.
