# DarkBox

DarkBox is a self-hosted platform for authorized web assessments. It maps an
application, keeps evidence, and preserves enough context that a later session
can continue the work without repeating finished steps or repeating an effect
whose outcome is unknown.

The core is Rust. The operator interface is React and TypeScript. The synthetic
laboratory in this repository is a controlled target used to verify the
promise; it is not a set of real customer systems.

Version 0.3.0. The implementation follows `SPEC.md` (kept outside this
repository by the maintainer) and the milestone plan. The original acceptance
harness covers AC-01 through AC-14; the session 1 harness adds real browser
exploration, cancellation, continuation and outcome-unknown checks. The last
session 1 local run passed 15 of 15 independent criteria; the report is in
`artifacts/session1/report.json`. The real-provider criterion remains pending
because the configured provider account reports insufficient balance.

## What it does

![Inventory](docs/screenshots/inventory.png)

![Optional integrations](docs/screenshots/integrations.png)

![Agent turn](docs/screenshots/agent.png)

- Separates workspaces, companies, assessments, target tenants, test identities
  and browser sessions. An identifier from another workspace is rejected before
  any operation.
- Applies a versioned scope policy to every external connection, including
  redirects and browser navigation. Exclusions prevail over inclusions.
- Distinguishes a discovered candidate from an observed resource, and records
  the source, run and time of every observation.
- Keeps execution durable: a Postgres queue, leases, attempt counters,
  cancellation, atomic budget reservations and an SSE event stream.
- Captures sanitized evidence with hashes, sizes and retention, and serves it
  as inert text. Target HTML is never rendered on the platform origin.
- Stores versioned narrative memory, builds a filtered context package with an
  explicit token budget, and exports Markdown and a sanitized JSON manifest.
- Runs three versioned procedures: HTTP configuration inventory, role
  visibility comparison and flow reproduction. Test results stay separate from
  findings. A finding cannot be confirmed without accessible evidence and a
  recorded review.
- Optional adapters for Nmap, Firecrawl and Burp. Nmap scans only the hosts and
  ports allowed by the scope. Firecrawl provides URL discovery and a managed
  browser scrape for JavaScript-heavy pages, which is the main web tool for the
  agent. Burp imports proxy history over MCP without creating new traffic.
- A bounded AI agent built on the Vercel AI SDK. It receives a context package,
  chooses typed tools over a real, persistent browser context, records every
  action and observation, and writes a checkpoint. It has no shell and cannot
  exceed the run policy.
- A live exploration view that opens before the turn finishes. It shows the
  current URL and screenshot, the semantic action it took, the observed result,
  short decision summaries, completed and blocked work, budget and cancellation.

## What it deliberately does not do

- No arbitrary command execution suggested by a model.
- No required external scanner. Burp, Nmap and Firecrawl are optional adapters
  with capability detection; the core assessment flow runs without them.
- No SaaS tenancy, graph database, vector search or remote runtime.
- No autonomous attempt to confirm every form. Candidate actions remain useful
  as a map of gaps.
- No claim that observed coverage equals all routes of an unknown system.

## Quick start (local, macOS or Linux)

Requirements: Rust stable, Node 20+, Docker, Python 3.9+.

```sh
# 1. Database
docker compose -f deploy/docker-compose.yml up -d postgres

# 2. Browser runtime (Playwright Chromium, one time)
python3 -m venv .venv
.venv/bin/pip install -r adapters/browser-python/requirements.txt
.venv/bin/playwright install chromium

# 3. Core and laboratory
export DATABASE_URL=postgres://darkbox:darkbox@127.0.0.1:5433/darkbox
cargo build --workspace
./target/debug/darkbox migrate
./target/debug/darkbox seed
LAB_PORT=8080 EXCLUDED_PORT=9099 ./target/debug/darkbox-lab   # terminal 2

DATABASE_URL=$DATABASE_URL \
DARKBOX_BLOB_ROOT=./data/objects \
DARKBOX_REPO_ROOT=. \
DARKBOX_LAB_URL=http://127.0.0.1:8080 \
DARKBOX_INLINE_WORKER=1 \
DARKBOX_ADAPTER_PYTHON=$PWD/.venv/bin/python \
DARKBOX_BROWSER_MODE=auto \
LAB_ALICE_PASSWORD=alice-lab-pass \
LAB_BOB_PASSWORD=bob-lab-pass \
./target/debug/darkbox serve --bind 127.0.0.1:8787         # terminal 3

# 4. Interface
cd apps/web && npm install && npm run dev                    # terminal 4
```

The browser host is a separate process managed by the API. It is started on
demand on a loopback port and reports its actual engine and browser version at
`GET /v1/capabilities/browser`. `DARKBOX_BROWSER_MODE=mock` is an HTTP session
client, not a browser, and interactive sessions are disabled in that mode.

Open <http://127.0.0.1:5173> and use the local token `darkbox-dev-token`.
The seed prints the workspace and assessment identifiers.

For a single-origin deployment, build the interface and point the API at it:

```sh
cd apps/web && npm run build && cd ../..
DARKBOX_WEB_DIST=./apps/web/dist ./target/debug/darkbox serve --bind 127.0.0.1:8787
```

The UI is then served at <http://127.0.0.1:8787>. The Docker image does this
by default.

The lab identities are `alice` / `alice-lab-pass` (admin, north and south) and
`bob` / `bob-lab-pass` (member, north). They are synthetic and only exist in the
bundled laboratory.

### Docker Compose

```sh
docker compose -f deploy/docker-compose.yml up --build postgres lab api
```

The API listens on `127.0.0.1:8787`, the lab on `8080` and its excluded
destination on `9099`. Run `docker compose -f deploy/docker-compose.yml run --rm api darkbox seed`
once to create the workspace and token. Images are built locally; no remote
runtime is involved.

## Optional integrations

### Firecrawl (managed browser and URL discovery)

Firecrawl is the managed browser path for the agent. `firecrawl_map` discovers
URLs, and `firecrawl_scrape` renders a JavaScript-heavy page and returns
Markdown plus links, stored as sanitized evidence against the endpoint. Targets
still pass the scope policy, and private or loopback addresses are refused
because Firecrawl's egress cannot be attested.

The hosted MCP server uses OAuth. Configure it once for OpenCode:

```sh
opencode mcp add firecrawl --url https://mcp.firecrawl.dev/v2/mcp-oauth --global
opencode mcp auth firecrawl
```

For the DarkBox runtime, either set an API key:

```sh
export DARKBOX_FIRECRAWL_API_KEY=fc-...
```

or point the adapter at the hosted MCP server with an OAuth access token:

```sh
export DARKBOX_FIRECRAWL_MCP_URL=https://mcp.firecrawl.dev/v2/mcp-oauth
export DARKBOX_FIRECRAWL_MCP_TOKEN=...
```

The adapter reports `web.firecrawl` in `/v1/capabilities` and in the UI. A run
of kind `firecrawl_scrape` is the typed browser operation available to the
agent. API keys and tokens stay in the environment.

### Nmap

```sh
brew install nmap   # or the package manager for your platform
```

The adapter uses a TCP connect scan (`-sT`), never a raw-socket scan. It passes
only hosts in scope and only ports present in `allowed_ports`; a requested port
outside the policy is dropped rather than scanned. XML is parsed, stored as
evidence, and turned into `network.open_port` observations on a host asset.
Without the binary, the capability is `unavailable` and the rest of the product
still works.

### Burp over MCP

Start the PortSwigger MCP server from your Burp installation, then point the
worker at it:

```sh
export DARKBOX_BURP_MCP_URL=http://127.0.0.1:9876
# export DARKBOX_BURP_MCP_TOKEN=...
# export DARKBOX_BURP_MCP_TOOL=burp_get_proxy_history
```

The adapter performs the MCP initialize and tool discovery handshake, reports
the actual server name, version and tools, and imports proxy history through
the tool the edition exposes. Importing does not create new requests. Every
imported exchange is re-checked against the assessment scope and sanitized
before storage. Burp remains an external installation; the core never requires
it.

## AI agent

The agent is a separate TypeScript service in `apps/agent`, built with the
[Vercel AI SDK](https://vercel.com/docs/ai-sdk). It is not a chat bolted onto the
UI. Each turn is a durable DarkBox run with a lease, a model-step budget, a tool
budget and a transcript.

```sh
cd apps/agent
npm install
export DARKBOX_MODEL_API_KEY=...
export DARKBOX_MODEL_BASE_URL=https://api.openai.com/v1
export DARKBOX_MODEL=your-tool-calling-model
npm start
```

The service listens on `127.0.0.1:8790`; the web UI uses `VITE_AGENT_URL`.
Click **Ask agent** on an assessment, give an objective, and follow the run.

Tools available to the model:

- `browser_open`, `browser_observe`, `browser_navigate`, `browser_act`
- `browser_login`, `browser_screenshot`, `browser_traffic`
- `identity_list`, `inventory_list`, `evidence_read`, `memory_search`
- `finish`

Every browser action is one operation exposed by the Rust API and one row in
the objective's action trail. The model never gets a terminal, a database
connection or a secret. Policy, budgets, evidence ingestion, session state and
the success condition stay in the core. The agent service only chooses the next
action. The context carries the starting URL and the objective; it never carries
the route or click sequence as a solution.

To test without a provider, run the bundled mock:

```sh
cd apps/agent
npm run mock-llm   # 127.0.0.1:9911
DARKBOX_MODEL_BASE_URL=http://127.0.0.1:9911/v1 \
DARKBOX_MODEL_API_KEY=mock DARKBOX_MODEL=mock-agent npm start
```

See `apps/agent/README.md` for the full contract and failure behaviour.
For the session 1 demonstration and the exact real-provider blocker, see
`docs/session1.md`.

## Verify the promise

With the lab and API running:

```sh
export DATABASE_URL=postgres://darkbox:darkbox@127.0.0.1:5433/darkbox
export DARKBOX_BLOB_ROOT=./data/objects
./target/debug/darkbox verify
```

The harness creates a fresh assessment, imports targets, authenticates two
identities, reproduces a flow, and checks each acceptance criterion against
the lab's out-of-band oracle. It prints a report and writes
`artifacts/acceptance/report.json`. A failing criterion exits non-zero.

## Repository map

```text
crates/contracts      ids, states, event envelope, API DTOs, scope policy
crates/domain         endpoint identity, normalization, entities, finding rules
crates/policy         scope decisions, exclusions, auxiliary destinations
crates/storage        PostgreSQL schema, queries, leases, budgets, finding review
crates/evidence       sanitization, hashes, opaque local object store
crates/execution      worker, bounded HTTP, built-in crawler, adapter bridge
crates/integrations   Nmap, Firecrawl and Burp MCP adapters with capability probes
crates/memory         context packages, deterministic synthesis, Markdown export
crates/api            Axum routes, auth, SSE, export manifest
crates/cli            migrate, seed, serve, worker, procedures, purge, verify
apps/agent            Vercel AI SDK agent service with typed tools
lab/application       synthetic target with two tenants and two identities
adapters/browser-python  versioned JSON Lines browser adapter
apps/web              operator interface
procedures            immutable procedure manifests
deploy                Dockerfiles and Compose
docs                  API, architecture, threat model, operations, decisions
```

## Design decisions in one paragraph

The core is a modular monolith with a Postgres-backed queue and separate worker
processes. Policy is evaluated in the executor, not in the model or adapter.
Adapters are replaceable processes that exchange versioned JSON Lines. Evidence
is stored by opaque key and sanitized at ingestion. Memory is a derived,
versioned layer over structured records; it never replaces them. A run fixes
its policy, procedure and prompt versions at creation, and a policy change
revokes future operations rather than expanding an existing run.

## Limitations (measured, not hidden)

- The browser path uses Playwright Chromium in this environment. Camoufox could
  not be installed against the system Python 3.9: `pyobjc-core` failed to build.
  The adapter reports that exact error and never labels Chromium as Camoufox.
  The engine, version and JavaScript probe are shown in **Capabilities**.
- A browser host lives in the API process. Restarting the API closes live
  contexts; the stored session becomes `unknown` or `lost`, never silently
  authenticated.
- The Katana adapter is detected and reported as `degraded`, never silently
  used: Katana cannot guarantee a per-request scope decision, so scoped runs
  use the built-in bounded crawler. See `docs/adapters/katana.md`.
- Firecrawl fetches pages from its own egress. DarkBox can authorize the URL and
  refuse private targets, but cannot attest the provider's network path.
- Burp support is implemented and contract-tested against an MCP mock. It is
  not tested against every Burp edition; the capability probe reports what the
  installed edition exposes, and an absent tool disables the import with a
  reason.
- Nmap scans are TCP connect scans on the policy's port list. Service detection
  (`-sV`) is not enabled in this release; product and version fields only appear
  when Nmap reports them from its default probes.
- Per-origin concurrency is stored in the policy but not enforced by a
  semaphore in this release. Request budgets are enforced atomically.
- DNS resolution is checked immediately before a request, but a hostile
  resolver can rebind between check and connect. The local lab does not model
  that race; a production deployment needs a pinned resolver.
- SSE uses short-interval database polling. It reconnects with a cursor and
  de-duplicates by event id, but it is not a push bus.
- The agent requires a tool-calling OpenAI-compatible provider. Without a key
  the service reports `missing_model_key` and the UI disables the turn. The
  bundled mock exercises the tool loop without a provider.
- Model calls are reserved before the provider request and settled against the
  same call id. A retry never books the call twice. A provider that does not
  report usage is shown as `usage not reported`, not as zero consumed tokens.
  Monetary cost is enforced when per-million-token rates are configured.
- The agent proposes; it cannot confirm a finding, bypass scope or execute a
  shell. A hypothesis created by the agent still needs evidence and human
  review.

## Documentation

- `docs/API.md`: HTTP contract and error envelope.
- `docs/architecture.md`: components, execution and trust boundaries.
- `docs/threat-model.md`: assets, attackers, accepted risks.
- `docs/operations.md`: install, backup, retention, purge, upgrade.
- `docs/adapters/browser.md`: adapter protocol and cancellation.
- `docs/adapters/firecrawl.md`: managed browser and URL discovery.
- `docs/adapters/nmap.md`: network scan scope and parsing.
- `docs/adapters/burp.md`: MCP capability probe and history import.
- `docs/decisions/`: architecture decision records.
- `docs/acceptance/latest.json`: the last recorded acceptance report.
- `docs/session1.md`: real browser exploration, the demonstration, versions and
  the remaining real-provider blocker.

## Security and contribution

Use DarkBox only against systems you are authorized to test. The scope policy
is a guardrail, not a substitute for written authorization. Report issues in
the project itself according to `SECURITY.md`. Contribution guidance is in
`CONTRIBUTING.md`.

The code in this repository is licensed under Apache-2.0. External tools keep
their own licenses; nothing here redistributes Burp, Katana, Camoufox or
Firecrawl.
