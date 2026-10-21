# Threat model

## Scope

This model covers the self-hosted single-operator deployment in this
repository. It assumes the operator has written authorization for every target
and that the host running DarkBox is trusted by that operator. Multi-tenant
hosting of mutually untrusted organizations is out of scope.

## Assets

1. **Evidence bodies and metadata.** Response captures may contain business
   data. Raw material is more sensitive than the inventory.
2. **Test credentials and session cookies.** Needed to reproduce authenticated
   flows; must not reach logs, model prompts, narrative memory or exports.
3. **Scope and authorization records.** If altered, the platform could act
   outside the authorization.
4. **Assessment state and memory.** Losing it breaks continuity; silently
   changing it turns hypotheses into apparent facts.
5. **Target availability and data.** The platform can cause load or reversible
   changes. Budgets and action classes bound that.
6. **The operator host.** A compromised worker or adapter could pivot to the
   host or the local network.

## Trust boundaries

```text
operator browser  --(loopback, bearer token)-->  API
API               --(SQL)-->                       PostgreSQL
worker            --(policy)-->                    target application
worker            --(JSON Lines, env secrets)-->   adapter process
adapter           --(HTTP)-->                      target application
model provider    --(context package only)-->      external service
```

Anything returned by a target, adapter, tool, model or MCP server is untrusted
input. Untrusted input can be observed, quoted and classified. It cannot alter
policy, credentials, procedure manifests or system instructions.

## Adversaries and failure modes

### Malicious target content

A page can contain text impersonating system instructions, links to excluded
destinations, oversized bodies, invalid encodings or content designed to look
like a finding.

Mitigations: tool output is data; policy is evaluated in Rust before every
connection; HTML is never rendered on the platform origin; evidence is served
as `text/plain` with `nosniff`; body size and character boundaries are capped;
technology detection creates an observation, never a finding; prompt-injection
text is stored inertly and the acceptance harness checks that policy and effect
counters do not change.

Residual risk: a human reviewer or an optional language model can still be
persuaded by plausible text. Model output is labelled derived; confirmation
requires human review and evidence.

### Scope bypass through redirects, DNS or sub-resources

A permitted hostname can resolve to a denied address, redirect to an excluded
host, or load a sub-resource from outside scope.

Mitigations: host/port/scheme/path exclusions; private-address check when
private networking is not allowed; manual redirect loop with a fresh decision
per hop; redirects disabled in the HTTP client; each browser navigation checked
by the adapter against the pinned policy; auxiliary destinations separate from
test targets.

Residual risk: DNS rebinding between the resolution check and the connection.
A production deployment needs a pinned resolver or an egress proxy that
enforces the same policy at connection time. The bundled lab does not model it.

### Cross-workspace access

A caller supplies an identifier from another workspace.

Mitigations: every row carries `workspace_id`; every query filters on it; the
token resolves to exactly one workspace; cross-workspace reads return 404;
evidence access re-checks the owning workspace; exports are built from filtered
queries. AC-11 exercises three read paths with a second workspace token.

Residual risk: a new query that forgets the workspace predicate. The storage API
takes workspace as an argument and integration tests cover the main paths, but
this is a review obligation, not a type-level guarantee.

### Secret leakage

Credentials appear in headers, bodies, cookies, logs or evidence exports.

Mitigations: identities store a reference such as `env:LAB_ALICE_PASSWORD`, not
the value; adapters resolve references from the process environment; sensitive
headers and token-like values are redacted at ingestion; the stored evidence is
a sanitized derivation; SSE payloads contain summaries and identifiers, never
bodies; exports exclude evidence unless requested, and even then only the
sanitized object; logs never print request bodies.

Residual risk: a novel secret format not matched by the redaction patterns.
Raw evidence is disabled by default; enabling it is an explicit configuration
decision with its own retention.

### Worker escape and host compromise

An adapter or tool is compromised, or a browser exploit succeeds.

Mitigations: adapters run as separate processes with a timeout, output cap and
process-group kill; the worker does not receive the Docker socket; paths are
opaque; the adapter has no database access; the API listens on loopback by
default.

Residual risk: a process is not a sandbox. The initial release does not host
mutually untrusted code. Strong isolation requires a VM boundary or a hardened
container profile, which is a later infrastructure step.

### Effect repetition

A worker crashes after sending a state-changing request but before recording
the result.

Mitigations: non-idempotent requests are marked `dispatched` before the call;
recovery turns them into `outcome_unknown`; automatic retry is refused; an
operator must reconcile explicitly; the target's observable state is the
reconciliation source. AC-06 exercises the storage transition and the API
refusal.

Residual risk: reconciliation is a human judgement. The platform cannot know
whether a remote effect happened if the target exposes no observable state.

### Budget and cost abuse

Two workers spend the last unit, or a crawler expands without bound.

Mitigations: a single SQL reservation function with a row lock; request,
browser-step, model-call, cost, artifact and duration limits; crawler depth and
URL caps; cancellation stops new claims. AC-08 runs ten concurrent
reservations against a budget of five and observes exactly five grants.

Residual risk: per-origin concurrency is recorded but not enforced by a
semaphore in this release.

### Evidence tampering

Stored bytes change after capture.

Mitigations: SHA-256 recorded at ingestion; opaque keys; local store writes
through a temporary file and rename; exports carry a manifest hash. The hash
detects change relative to the recorded object; it does not prove authorship or
remote-system authenticity.

### Supply chain

An adapter, procedure or dependency is replaced.

Mitigations: procedure versions are immutable rows and runs pin the version;
adapters are repository files, not downloaded during a run; dependencies are
locked by `Cargo.lock` and `package-lock.json`; images are built from this
repository; no plugin marketplace and no dynamic execution of unreviewed
plugins.

Residual risk: the build toolchain and base images are trusted inputs. Pin
digests before publishing a release.

### Optional adapters

- **Nmap.** The adapter is a tool that sends packets. Scope is enforced by
  passing only approved hosts and ports, checking each pair before the process
  starts, and using a TCP connect scan without raw sockets. A policy that
  allows a wide port range widens what Nmap can touch; the policy is the limit,
  not the crawler.
- **Firecrawl.** The provider fetches from its own egress. DarkBox authorizes
  the URL, refuses private targets and treats returned links as candidates,
  but cannot attest the provider's network path or that its rendering is
  complete. Evidence is labelled as a managed capture.
- **Burp.** Imported traffic is untrusted history. Scope is re-checked per
  exchange, secrets are sanitized, and import never creates new requests. A
  broad Burp history can contain out-of-scope targets; those are skipped and
  recorded, not accepted because Burp saw them.
- **MCP transports.** The MCP client treats server responses as untrusted
  JSON, caps response size and follows only the initialize/tools contract. An
  MCP endpoint is a local or private dependency, never a public listener.

### Denial of service against the platform

Large bodies, many events, or many open SSE connections.

Mitigations: response and adapter output caps, artifact budget, pagination,
SSE event limits, request timeouts. No rate limiter is included in this
release; the API is loopback-only by default.

## Out of scope

- Red Team campaigns, endpoint agents, social engineering and corporate
  infrastructure.
- Public SaaS with mutually untrusted organizations.
- Autonomous exploitation or impact escalation.
- Protecting data from an operator who is authorized to read the workspace.
- Proving that a `passed` test means the target is secure.

## Security-relevant configuration

| Setting | Default | Effect |
| --- | --- | --- |
| API bind | `127.0.0.1:8787` | loopback only |
| `DARKBOX_INLINE_WORKER` | `1` | API processes runs; set `0` with a separate worker in production |
| `DARKBOX_RAW_EVIDENCE` | `0` | raw restricted evidence cannot be read |
| `DARKBOX_MODEL_SYNTHESIS` | `0` | no model call for memory pages |
| `DARKBOX_FIRECRAWL_ALLOW_PRIVATE` | `0` | delegate private targets to Firecrawl |
| Burp MCP URL | unset | optional import; no listener is opened by DarkBox |
| scope `allow_private_network` | `true` only in the lab seed | private targets require explicit permission |
| scope `retain_raw` | `false` | captures are sanitized only |
| workspace retention | 30 days | evidence expiry and purge |

## Reporting

Report a vulnerability in DarkBox itself through the process in `SECURITY.md`.
Do not test the project against systems you do not own or have written
permission to assess.
