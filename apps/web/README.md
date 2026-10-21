# DarkBox web

Operator interface for the DarkBox API. React 19, TypeScript, Vite and
TanStack Query. Plain CSS with a token layer; no component library.

## Run

```sh
npm install
npm run dev
```

The dev server listens on `http://127.0.0.1:5173` and proxies `/v1` to
`http://127.0.0.1:8787`. The AI agent is a separate service at
`http://127.0.0.1:8790`; override it with `VITE_AGENT_URL`.

Start the API and, for agent turns, `apps/agent` first.

Open the app and paste a token. The local seed uses `darkbox-dev-token`.
The token is stored in `localStorage` under `darkbox.token`. Signing out removes
it.

```sh
npm run build     # tsc --noEmit and a production build
npm run typecheck
```

## Screens

- Workspace overview: assessments, coverage, capabilities.
- Companies and company detail: create an assessment with a structured scope.
- Assessment overview: coverage with an explicit denominator, recent runs,
  observations, identities and next actions derived from real gaps.
- Inventory: exact endpoints with filters, URL-synced state and a detail drawer.
- Flows: recorded steps, preconditions, evidence and reproduction.
- Runs and run detail: budget, step timeline, SSE events, cancellation and
  reconciliation of unknown effects.
- Evidence: metadata and an inert text viewer; target HTML is never rendered.
- Memory: versioned pages, search, references and a bounded context package.
- Findings: review transitions; confirmation requires evidence.
- Scope: effective policy, version history and a preflight preview.
- Integrations: Nmap scan, Firecrawl map, managed browser scrape and Burp
  history import, with capability availability and reasons.
- Procedures: catalog and eligibility with explicit blockers.
- Capabilities: adapter availability and the reason for each limitation.

## Design tokens

Defined in `src/styles/tokens.css`.

| Token | Value | Use |
| --- | --- | --- |
| `--canvas` | `#08090a` | page background |
| `--surface` | `#0d0f12` | panels and drawers |
| `--inset` | `#111418` | inputs and wells |
| `--line` | `rgba(255,255,255,.07)` | hairline separators |
| `--text` | `#e9e9e4` | primary text |
| `--text-2` | `#9a9ea7` | secondary text |
| `--text-3` | `#6b7079` | tertiary text |
| `--lime` | `#c9f24d` | observed, live, primary action |
| `--amber` | `#e6b566` | needs review, waiting |
| `--red` | `#f0736a` | failed, blocked |
| `--blue` | `#8fb4ff` | links |
| `--ease` | `cubic-bezier(.22,1,.36,1)` | all UI motion |
| `--dur` | `160ms` | default transition |

Instrument Sans for UI, JetBrains Mono for identifiers, paths, hashes and
counts. Numeric columns use tabular figures. Motion is limited to opacity and
transform, stays under 220ms, and is disabled under
`prefers-reduced-motion`.

## Behaviour notes

- There is no mock data. Every value comes from the API; empty states explain
  what is missing.
- Event streams use authenticated `fetch` streaming, reconnect with the last
  `seq`, and de-duplicate by `event_id`.
- A cancellation request is displayed separately from a completed
  cancellation. An unknown external effect is never presented as success.
- Filter state lives in the URL so a view can be shared or reloaded.
