# Changelog

## Unreleased

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
