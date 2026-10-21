# DarkBox browser adapter

One request per process. The Rust supervisor writes one JSON line to stdin and
reads versioned JSON lines from stdout. Diagnostics go to stderr.

```sh
printf '%s\n' '{"schema_version":1,"request_id":"r1","kind":"browser.session","payload":{"identity":{"label":"alice","secret_ref":"env:LAB_ALICE_PASSWORD"},"tenant":"north","base_url":"http://127.0.0.1:8080","login_path":"/login"}}' \
  | LAB_ALICE_PASSWORD=alice-lab-pass python3 adapter.py --mode mock
```

Modes:

- `auto` uses Camoufox when importable and falls back to the deterministic HTTP client.
- `mock` uses `urllib` with one cookie jar per identity. It performs real HTTP to
  the synthetic lab, keeps identities isolated, and never returns cookie values.
- `camoufox` is reserved for the pinned Camoufox integration. This build reports
  a structured error instead of pretending a browser ran.

Secrets are resolved from environment variables named by `secret_ref`
(`env:LAB_ALICE_PASSWORD`). For the bundled synthetic lab only, the adapter
falls back to the documented `alice-lab-pass` / `bob-lab-pass` values and logs
that fallback to stderr.

The adapter never receives database access, the scope policy owner, or a shell.
The supervisor enforces the timeout, output cap, cancellation and process-group
kill.
