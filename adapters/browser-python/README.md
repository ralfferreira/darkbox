# DarkBox browser adapter

The adapter has two contracts.

## Interactive host

```sh
python3 -m venv .venv
.venv/bin/pip install -r requirements.txt
.venv/bin/playwright install chromium
.venv/bin/python adapter.py --probe
.venv/bin/python adapter.py --serve --host 127.0.0.1 --port 8791 --mode auto
```

The Rust core normally starts this process itself. It owns Playwright and all
browser contexts. Commands are versioned JSON sent to `/v1/command`:

```json
{
  "schema_version": 1,
  "kind": "session.open",
  "session_key": "ctx-demo",
  "policy": {
    "include_hosts": ["127.0.0.1"],
    "allowed_ports": [8080],
    "allowed_schemes": ["http"]
  },
  "payload": {
    "identity": {"label": "alice", "secret_ref": "env:LAB_ALICE_PASSWORD"},
    "tenant": "north",
    "url": "http://127.0.0.1:8080/"
  }
}
```

`/v1/capability` runs a JavaScript probe and reports the engine that actually
started, its version, the executable and any Camoufox error. `/v1/abort` and
`/v1/close` interrupt and close a context.

The `mock` mode is the legacy one-shot HTTP client. It is not a browser. The
one-shot stdin/stdout contract remains for contract tests and legacy
visibility comparison.

Secrets are resolved from `env:NAME`; the value never appears in a request,
stdout, stderr or evidence. Login success is confirmed from the rendered page.
A 200 login page is not authentication.

See `docs/adapters/browser.md` for the full protocol and policy enforcement.
