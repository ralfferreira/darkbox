# Changelog

## Unreleased — session 1 correction round

- Request-level browser policy: every request, including subresources,
  fetch/XHR and redirects, is checked against the pinned and current policy. A
  scope version is pushed to open contexts and revokes access.
- Request grants reserved before dispatch and settled against requests actually
  fired. A JavaScript burst stops at the aggregate budget.
- Action intent persisted before dispatch with idempotency, effect class,
  authorization and lease. Process-stop recovery marks unknown outcomes and the
  operator reconciles them without a repeated effect.
- Action class and effect authorization outside the model. POST mutations are
  denied before dispatch unless the policy allows them.
- Recursive sanitized projection for structured observations before storage,
  events, progress, memory and model input.
- Usage settlement before the next call and after finish, cancellation or
  provider failure, with idempotent call ids and explicit unknown usage.
- Structured replay assertions and a negative corrupt-data replay.
- New acceptance criteria AC-17 to AC-20 plus a controlled-provider accounting
  contract and a process-crash recovery test.

## Unreleased — session 1

- Real interactive browser host on Playwright Chromium, with one persistent
  context per assessment, identity and tenant, semantic page observations,
  clicks/fills/selects/waits/back, screenshots and request/response traffic.
  Camoufox is reported only when it actually starts; the pinned install failure
  is exposed in the capability probe.
- Configurable identity login (`login_url`, username/field hints, success and
  failure markers) with the secret injected inside the adapter. Login success is
  confirmed from the page, never from a 200 status.
- Agent tools for the live browser. Every action belongs to the objective run,
  inherits its pinned policy, pays from its aggregated budget and emits an
  `agent.action` event with evidence.
- Model-call reservation and settlement with idempotent call ids. Unknown usage
  is shown as unknown.
- Operator view opens immediately on objective creation and shows the current
  URL, live screenshot, action/result trail, short decision summaries, completed
  and blocked work, budget, cancel and checkpoint.
- Exact evidence-by-run lookup, image evidence preview and a run-scoped browser
  progress endpoint.
- Synthetic lab report explorer rendered from an authenticated JavaScript fetch,
  with filter state, a dynamic trace id and an out-of-band query oracle.
- Slow-effect fixture and `outcome_unknown` handling; new-session continuation
  from a persisted checkpoint.
- Session 1 acceptance harness: 15 independent criteria passed locally; the
  real-provider criterion is pending because the configured account reports
  insufficient balance.

- Bounded agent turns built on the Vercel AI SDK (`apps/agent`) with typed
  tools, durable agent runs, transcript events, model-call budgets and memory
  checkpoints.
- Agent panel in the assessment view and transcript in run detail.
- Mock OpenAI-compatible provider for local development without a key.

- Nmap adapter with scoped TCP connect scans, XML parsing and `network.*`
  observations.
- Firecrawl adapter over REST or the hosted MCP server: URL discovery and a
  managed browser scrape for the agent, stored as sanitized evidence.
- Burp MCP adapter: capability probe, tool discovery, proxy history import and
  scope re-check.
- Integrations dialog in the operator UI and live capability reporting.
- Firecrawl MCP OAuth configured for the local OpenCode session.

## 0.3.0 — 2026-10-21

First complete core release.

- Workspace, company, assessment, scope version, asset, endpoint, tenant,
  identity, session, action, flow, run, evidence, memory, procedure, result and
  finding records with cross-workspace isolation.
- Versioned scope policy evaluated in the executor, including manual redirect
  checks, DNS-resolved private address checks and auxiliary destinations.
- Postgres queue with leases, attempt ownership, atomic budgets, cancellation,
  recovery of expired leases and `outcome_unknown` reconciliation.
- Sanitized evidence ingestion, opaque object store, hashes and retention purge.
- Bounded crawler with JS route extraction and URL import.
- Python JSON Lines browser adapter with isolated cookie jars, flow replay and
  role visibility comparison.
- Three versioned procedure manifests and deterministic executors.
- Versioned memory, bounded context packages, deterministic synthesis, optional
  model summarizer and Markdown export.
- React operator interface with inventory, flows, runs and SSE, evidence viewer,
  memory, findings review, scope preview, procedures and capabilities.
- Acceptance harness covering AC-01 to AC-14 against the synthetic lab, plus
  CI, Compose deployment and documentation.

Known limitations are listed in `README.md` and `docs/operations.md`. Katana is
reported as degraded; the built-in crawler enforces policy per request. The
adapter's default mode is a deterministic HTTP client, not a browser.
