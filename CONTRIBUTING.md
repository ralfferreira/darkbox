# Contributing

DarkBox is a young codebase. Small, complete changes with evidence are more
valuable than broad, partially wired features.

## Before you start

- Read `docs/architecture.md` and the decision records relevant to your change.
- Use only synthetic targets. The bundled lab is the default. Do not test
  external systems.
- Do not add an optional integration before the core flow still passes
  `darkbox verify`.

## Development loop

```sh
docker compose -f deploy/docker-compose.yml up -d postgres
export DATABASE_URL=postgres://darkbox:darkbox@127.0.0.1:5433/darkbox
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

For the interface:

```sh
cd apps/web
npm install
npm run build
npx tsc --noEmit
```

For behaviour that touches policy, execution or evidence, run the acceptance
harness against a live lab and API:

```sh
export DARKBOX_BLOB_ROOT=./data/objects
./target/debug/darkbox verify
```

Attach the report, not a claim that it "looks fine".

## Rules for changes

- Keep `domain` free of SQL, HTTP and adapter SDKs.
- Pass `workspace_id` through every storage call. A query without it is a bug.
- Never let target content, model output or adapter output alter policy.
- A new external operation goes through the executor and the policy engine.
- Non-idempotent operations must mark `dispatched` before the call and record
  `confirmed` or `outcome_unknown` after it.
- Evidence is sanitized at ingestion and served as inert text.
- A finding confirmation needs evidence and a review revision. Do not bypass it.
- Add a test that fails when the rule is removed. A test that passes with every
  implementation is not a test.
- Update the API contract and the TypeScript types in the same change.
- Write comments only for a non-obvious why.

## Adapter contributions

Include the capability declaration, version pins, input and output schemas,
limits, cancellation behaviour, evidence format and contract tests for invalid
JSON, timeout, oversized output, unexpected exit and cancellation. Report the
engine honestly; do not label an HTTP client as a browser.

## Procedure contributions

A procedure is an immutable manifest with a version. Include purpose, target
types, effect class, required capabilities, preconditions, inputs, limits,
steps, required evidence, result schema, evaluator and cleanup. Do not add
payload catalogues before evidence, eligibility and review work.

## Commit and review

Keep commits focused. The message states the behaviour change and the proof.
Review looks for: a bypassed policy check, a missing workspace filter, a secret
in a log or prompt, a repeated external effect, a conclusion without evidence,
and a UI that hides a limitation.

By contributing you agree that your contribution is licensed under Apache-2.0.
