# Operations

## Prerequisites

- Rust stable (workspace pinned in `rust-toolchain.toml`).
- Node 20 or newer for the interface.
- PostgreSQL 17. The Compose file pins `postgres:17.6-alpine` on host port 5433.
- Python 3.9 or newer for the browser adapter.
- Docker only for the database and optional images. The core runs without it.

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `DATABASE_URL` | `postgres://darkbox:darkbox@127.0.0.1:5433/darkbox` | Postgres connection |
| `DARKBOX_BLOB_ROOT` | `./data/objects` | evidence object directory |
| `DARKBOX_REPO_ROOT` | current directory | adapter and procedure resolution |
| `DARKBOX_LAB_URL` | `http://127.0.0.1:8080` | default navigation base |
| `DARKBOX_INLINE_WORKER` | `1` | run the worker inside the API process |
| `DARKBOX_WORKER_ID` | `worker-<pid>` | lease owner label |
| `DARKBOX_LEASE_SECONDS` | `60` | step lease duration |
| `DARKBOX_MAX_RESPONSE_BYTES` | `262144` | capture body cap |
| `DARKBOX_ADAPTER_PYTHON` | `python3` | adapter interpreter |
| `DARKBOX_BROWSER_MODE` | `auto` | `auto`, `mock` or `camoufox` |
| `DARKBOX_ADAPTER_TIMEOUT` | `120` | adapter wall clock limit |
| `DARKBOX_RAW_EVIDENCE` | `0` | allow reading raw restricted objects |
| `DARKBOX_MODEL_API_KEY` | unset | optional OpenAI-compatible key |
| `DARKBOX_MODEL_BASE_URL` | `https://api.openai.com/v1` | provider base |
| `DARKBOX_MODEL` | `gpt-4.1-mini` | model name |
| `DARKBOX_MODEL_SYNTHESIS` | `0` | use the model for memory synthesis |
| `DARKBOX_MODEL_API_KEY` | unset | agent service provider key |
| `DARKBOX_MODEL_BASE_URL` | `https://api.openai.com/v1` | provider base |
| `DARKBOX_MODEL` | `gpt-4.1-mini` | tool-calling model |
| `DARKBOX_AGENT_URL` | unset | agent service advertised to the API capability list |
| `DARKBOX_AGENT_PORT` | `8790` | agent service port |
| `DARKBOX_AGENT_MAX_STEPS` | `4` | default model-step cap |
| `VITE_AGENT_URL` | `http://127.0.0.1:8790` | UI agent endpoint |
| `DARKBOX_MODEL_INPUT_MICROS_PER_MTOK` | `0` | optional input cost rate |
| `DARKBOX_MODEL_OUTPUT_MICROS_PER_MTOK` | `0` | optional output cost rate |
| `DARKBOX_FIRECRAWL_API_KEY` | unset | Firecrawl REST mode |
| `DARKBOX_FIRECRAWL_MCP_URL` | unset | Firecrawl hosted MCP endpoint |
| `DARKBOX_FIRECRAWL_MCP_TOKEN` | unset | Firecrawl OAuth access token |
| `DARKBOX_FIRECRAWL_BASE_URL` | `https://api.firecrawl.dev/v2` | compatible gateway or test server |
| `DARKBOX_FIRECRAWL_ALLOW_PRIVATE` | `0` | allow delegation to private targets |
| `DARKBOX_BURP_MCP_URL` | unset | Burp MCP server root (HTTP or SSE) |
| `DARKBOX_BURP_MCP_TOKEN` | unset | optional bearer token for Burp MCP |
| `DARKBOX_BURP_MCP_TOOL` | unset | explicit proxy history tool name |
| `DARKBOX_WEB_DIST` | unset | serve a built UI from this directory |

Secrets for the lab adapter are `LAB_ALICE_PASSWORD` and
`LAB_BOB_PASSWORD`. In a real assessment, store credentials in the
installation's secret manager and put only the environment reference in the
identity (`env:NAME`).

## First run

```sh
docker compose -f deploy/docker-compose.yml up -d postgres
export DATABASE_URL=postgres://darkbox:darkbox@127.0.0.1:5433/darkbox
cargo build --workspace
./target/debug/darkbox migrate
./target/debug/darkbox seed
```

`seed` is idempotent: it creates the two workspaces, the synthetic company and
assessment, tenants, identities, the procedure catalog and an initial memory
page, and prints the tokens. It never fetches a target.

Start the lab and API as shown in the README. The API serves
`http://127.0.0.1:8787`; the UI dev server proxies `/v1` to it.

## Running a real assessment

1. Create a company.
2. Create an assessment with an environment, objective, initial targets and a
   structured scope. Review the effective policy before creating it.
3. Add target tenants and test identities. Use secret references, not values.
4. Import known URLs or start a bounded crawl. Discovery only records
   candidates; observation is a separate step.
5. Establish a session per identity. The adapter records the state; an expired
   session blocks a flow instead of producing a false negative.
6. Record flows from observed endpoints and reproduce them with a fixed
   identity and tenant.
7. Run procedures only when eligibility has no blockers. Blockers are shown
   with a reason, not hidden.
8. Review test results. A hypothesis becomes a finding for review; confirmation
   requires evidence and a recorded reviewer.
9. Export a sanitized manifest for the report. Keep the assessment for the
   retention window.

## Agent service

The AI agent is a separate TypeScript process. It needs a tool-calling
OpenAI-compatible provider and a bearer token from the operator.

```sh
cd apps/agent && npm install
DARKBOX_API_URL=http://127.0.0.1:8787 \
DARKBOX_MODEL_API_KEY=... \
DARKBOX_MODEL_BASE_URL=https://api.openai.com/v1 \
DARKBOX_MODEL=your-tool-calling-model \
npm start
```

The UI points at `VITE_AGENT_URL`, default `http://127.0.0.1:8790`. Each turn is
a durable agent run. The model can call only the typed tools; it cannot bypass
scope, budgets or finding review. If the service dies, the agent lease expires
and the run is not silently resumed.

## Worker topologies

- **Single process (default).** The API processes its own queued steps until
  the queue is empty. Convenient for a lab.
- **Split.** Set `DARKBOX_INLINE_WORKER=0` and run `darkbox worker`. Multiple
  workers are safe: claims are leased and budget reservations are atomic.
- **One assessment.** `darkbox worker --assessment <id>` restricts the worker
  to that assessment.

A worker started with `--once` drains the queue and exits, which is useful in a
scheduler or a test.

## Restart and recovery

Confirmed rows and evidence objects survive a restart. On startup, a worker
recovers expired leases:

- a step that never dispatched returns to `pending`;
- a step that dispatched but never recorded a result becomes
  `outcome_unknown`, and its run becomes `waiting_operator`.

Resuming an `outcome_unknown` run requires `reconcile_unknown: true`. The step
is marked skipped, not retried. If no eligible steps remain, the run is marked
failed with the reason recorded.

## Backup

Back up three things together:

1. PostgreSQL (`pg_dump --format=custom`).
2. The object directory (`DARKBOX_BLOB_ROOT`).
3. The secret material needed to resolve identity references and, if enabled,
   provider keys. Never put secrets in Git.

```sh
pg_dump "$DATABASE_URL" --format=custom --file darkbox.dump
tar -czf darkbox-objects.tgz data/objects
```

A restore test is part of the release checklist. Restore into a fresh database
and object directory, start the API, and verify that an assessment still lists
its evidence and that a context package cites the restored records.

## Retention and purge

Every evidence row receives `retention_until` from the workspace retention
(default 30 days). The operator runs:

```sh
./target/debug/darkbox purge
```

This deletes expired objects, their metadata, and old memory page versions
(keeping the five most recent per page slug). It does not delete the current
narrative page or the structured inventory. Purge is explicit so that a
scheduled job can be reviewed; it is not a hidden background deletion.

Backups have their own documented window. A purge does not rewrite an existing
backup; expire backups on the same schedule.

## Upgrade

1. Stop the API and workers.
2. Back up the database and objects.
3. Build the new revision and run `darkbox migrate`.
4. Start the API, then workers.
5. Re-run `darkbox verify` against the lab if the change touched policy,
   execution, evidence or export.

Migrations are append-only. A run pins its policy, procedure and prompt
versions; upgrading the catalog does not rewrite history.

## Observability

Structured logs record identifiers, decisions and durations, never request
bodies or secrets. Useful signals:

- queue depth and oldest queued step;
- lease recoveries;
- blocked operations by deny code;
- capture failures and truncated artifacts;
- budget exhaustion;
- inconclusive and blocked test results;
- SSE cursor lag.

The acceptance report is a point-in-time proof, not a production dashboard.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| API returns 401 | token hash is not seeded; run `darkbox seed` |
| Agent dialog says unavailable | start `apps/agent` and set a provider key |
| Agent run fails `budget_exhausted` | raise the model-call budget or lower `max_steps` |
| Agent turn has no tool calls | the provider may not support tool calling; test with the bundled mock |
| run stays queued | no worker; set `DARKBOX_INLINE_WORKER=1` or start `darkbox worker` |
| all targets excluded | exclude list includes the whole host, or the port is missing from `allowed_ports` |
| browser run blocked | adapter script missing, python missing, or secret env var unset |
| evidence download 404 | object was purged while metadata remains; check retention |
| run waits for operator | an effect has `outcome_unknown`; reconcile explicitly |
| SSE reconnects repeat events | deduplicate by `event_id`; reconnect with `after=<seq>` |
