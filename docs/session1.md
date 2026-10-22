# Session 1: real browser exploration

Status: implemented and demonstrated locally with a real browser. The real-model
acceptance criterion is **pending** because the configured provider account
reports insufficient balance. The deterministic planner used for the functional
demonstration is labelled and is not presented as a model.

## What changed

- The Python adapter is now a long-lived browser host. It owns Playwright
  Chromium and one isolated `BrowserContext` per assessment, identity and
  tenant. Cookies, storage and the page survive between actions.
- Observations are semantic: current URL, title, headings, page text, tables,
  forms and interactive elements with stable refs. Actions use refs, not fixed
  coordinates. Screenshots are additional evidence.
- Every page request, including subresources and redirects, is checked against
  the run's pinned scope inside the browser route handler. A denied request is
  aborted and recorded.
- Login is configurable per identity (`login_url`, field hints, success and
  failure markers). The secret is resolved inside the adapter from `env:NAME`.
  Authentication requires a positive page observation; a login page with HTTP
  200 is not success.
- Agent tools act on the live session. They do not create independent runs. All
  actions, sessions, observations and evidence are correlated with one agent
  objective and pay from its aggregate budget.
- Starting an objective returns immediately, and the run page opens before the
  turn ends. It shows the current page, screenshot, action/result trail, short
  decision summaries, completed/blocked work, budget, cancel and checkpoint.
- Cancellation aborts the model, signals the browser host, stops the in-flight
  action and closes the context. No new action is accepted after the objective
  leaves `running`.
- Model-call usage has an idempotent reserve phase and a settle phase. The same
  call id is never booked twice. Usage the provider omits is shown as unknown,
  not zero.
- A flow with an effect whose result was not observed becomes
  `outcome_unknown`; the objective moves to `waiting_operator` and a checkpoint
  is written. The effect is not retried.
- The synthetic lab has a JavaScript report explorer. The table and result line
  are produced by an authenticated `fetch`, the server records the filter and a
  dynamic trace id, and the HTML source contains neither the rows nor the
  filtered result.

## Demonstration

From a clean repository and a running Postgres:

```sh
docker compose -f deploy/docker-compose.yml up -d postgres

python3 -m venv .venv
.venv/bin/pip install -r adapters/browser-python/requirements.txt
.venv/bin/playwright install chromium

export DATABASE_URL=postgres://darkbox:darkbox@127.0.0.1:5433/darkbox
cargo build --workspace
./target/debug/darkbox migrate
./target/debug/darkbox seed

LAB_PORT=8080 EXCLUDED_PORT=9099 ./target/debug/darkbox-lab

DARKBOX_BLOB_ROOT=./data/objects DARKBOX_REPO_ROOT=. \
DARKBOX_LAB_URL=http://127.0.0.1:8080 DARKBOX_INLINE_WORKER=1 \
DARKBOX_ADAPTER_PYTHON=$PWD/.venv/bin/python DARKBOX_BROWSER_MODE=auto \
LAB_ALICE_PASSWORD=alice-lab-pass LAB_BOB_PASSWORD=bob-lab-pass \
./target/debug/darkbox serve --bind 127.0.0.1:8787

cd apps/agent
DARKBOX_API_URL=http://127.0.0.1:8787 DARKBOX_AGENT_PORT=8790 \
DARKBOX_AGENT_HOST=127.0.0.1 DARKBOX_AGENT_PLANNER=deterministic \
DARKBOX_MODEL=deterministic-planner npm start

cd apps/web && npm install && npm run dev
```

Open <http://127.0.0.1:5173>, use the local dev token, open the assessment,
choose **Ask agent**, select `alice`, enter the starting URL
`http://127.0.0.1:8080/`, and use this objective:

> Sign in with the configured identity, find the reports area, apply period
> 2026-08 with minimum margin 19, and confirm that the displayed result matches
> the filter.

The run page opens immediately. The agent discovers the sign-in control from
the rendered page, submits the login with the configured identity secret, finds
**Reports** in the authenticated dashboard, selects `2026-08`, fills `19`, clicks
**Apply filter**, and confirms the rendered line:

```
1 of 4 report rows match period 2026-08 with margin at least 19.0%.
2026-08 · $43900.00 · 19.1%
```

Run the complete session 1 acceptance harness with the API, agent and lab
running:

```sh
python3 scripts/session1_acceptance.py
```

It writes `artifacts/session1/report.json` (also committed as `docs/acceptance/session1.json`) and a new assessment. To attempt the
real-provider criterion:

```sh
DARKBOX_MODEL_API_KEY=... DARKBOX_MODEL_BASE_URL=https://api.deepseek.com/v1 \
DARKBOX_MODEL=deepseek-flash npm start   # in apps/agent

python3 scripts/session1_acceptance.py --real-model
```

### Identity configuration in the interface

The assessment sidebar lists identities. **Configure login** edits the login
URL, username, optional field hints, success/failure markers and the environment
secret reference. **Start session** creates one isolated session run for the
chosen identity and tenant. The secret value is never entered in the interface.

## Acceptance evidence

The local run of `scripts/session1_acceptance.py` produced:

| Criterion | Result |
| --- | --- |
| AC-01 real browser executes JavaScript | passed: `playwright-chromium 153.0.8010.12`, probe passed |
| AC-02 undisclosed path discovered | passed: **Reports** followed from a visible link |
| AC-03 form/filter and functional result | passed: one matching row for `2026-08`, margin `19.1%` |
| AC-04 session preserved | passed: 10 actions, one browser context |
| AC-05 two identities isolated | passed: alice and bob in two contexts |
| AC-06 configurable login | passed: identity `login_config` plus env secret |
| AC-07 progress before completion | passed: one action visible while `running` |
| AC-08 failed login not authenticated | passed: `login_failed`, blocked action, no authenticated session |
| AC-09 blocked action not executed | passed: excluded path denied before dispatch, zero target hits |
| AC-10 cancellation | passed: cancel returned in 0.04s, in-flight action interrupted, later action rejected |
| AC-11 evidence linked and visual | passed: 9 traces, 7 JPEG screenshots, served as `image/jpeg` |
| AC-12 discovered flow recorded | passed: 7 steps, state `reproduced` |
| AC-13 continuation | passed: partial checkpoint plus prior actions in the new context |
| AC-14 unknown external outcome | passed: effect applied once, `waiting_operator`, no retry accepted |
| AC-15 local connectivity | passed: API, agent and browser answered |
| AC-16 real model | **pending**: provider balance |

The discovered flow was also reproduced from persisted actions: 7 of 7 steps
confirmed in a real browser session, with 14 evidence ids and state
`reproduced`. The generated report is `artifacts/session1/report.json`, also committed as
`docs/acceptance/session1.json`; the provider failure is
`docs/acceptance/session1-real-model.json`. UI captures are
`docs/screenshots/session1-live.png` and `docs/screenshots/session1-result.png`.

The legacy `darkbox verify` harness still covers its original criteria and
passes 14/14. Two of its flow checks were updated to record real browser
actions; they no longer pretend a flow is a list of GET requests.

Compose connectivity was verified separately:

- `api`, `lab`, `agent` and `postgres` came up under
  `deploy/docker-compose.yml`; the API and agent health endpoints answered on
  their published ports.
- The API container reached the lab at `http://lab:8080` (HTTP 200).
- The agent service on port 8790 created and completed a run through the API at
  `http://api:8787`. The run succeeded in 5.4s with 18 evidence items.
- The API container reached an application served on the operator host at
  `host.docker.internal:8091`; that host and port were explicitly in scope.
- The container Playwright probe reported Chromium `148.0.7778.0`, which is
  different from the host build. The UI reports the engine that actually ran.

The lab now binds `0.0.0.0` only when `LAB_BIND=0.0.0.0` is set; Compose sets
it. Local runs keep the loopback default. Evidence is in
`docs/acceptance/compose-session1.json`.

## Versions observed

- Adapter: `adapters/browser-python/adapter.py`, session schema 1, adapter
  version 0.4.0.
- Engine: Playwright Chromium `153.0.8010.12` from Playwright Python `1.60.0`.
- Browser user agent: `HeadlessChrome/153.0.0.0`.
- Camoufox: not available. `pip install camoufox` against the system Python
  3.9.6 fails while building `pyobjc-core` (clang reports 16 errors). The
  probe returns `camoufox_error` and never reports Camoufox as the running
  engine.
- Model: DeepSeek `deepseek-flash` over `https://api.deepseek.com/v1` was
  attempted. The provider returned `Insufficient Balance` on the first call.
  The failed run is `01a160e8-8ec3-701b-aa82-5e6c22b2ede2`; the provider
  request id was `8850bf36-6daa-4283-82fc-00a48d4ff832`.

## Remaining limitations

- The real-model criterion is pending. The AI SDK path is implemented, but the
  configured account cannot pay for a call. The deterministic planner is not a
  substitute for that criterion.
- The browser host is a child of the API process. Restarting the API loses live
  contexts; stored sessions are marked lost/unknown rather than reused.
- Screenshots are sanitized by exclusion rather than image redaction. Password
  fields are masked by the browser, but a page that displays a secret in plain
  text would put it in a screenshot. Retention and access control still apply.
- Flow reproduction replays recorded browser actions. A legacy flow with no
  recorded actions is blocked rather than replayed as guessed GET requests.
  A recorded flow was reproduced 7/7 steps in this session.
- The deterministic planner is a test driver with a fixed objective shape. It
  discovers controls from the page but is not a general planner.
- Compose installs Playwright Chromium at image build time. The image is large;
  the browser download is cached only by Docker layer.
- The Firecrawl, Nmap and Burp adapters are unchanged and optional.

## Passing to session 2

Session 2 can build on:

- `agent_actions` as the canonical action trail, with `session_id`, action
  arguments, state, effect status, observation and evidence ids.
- `agent_model_calls` for reserve/settle accounting.
- `browser_sessions.context_key` as the stable handle for a live context, and
  the `session_lost` / `expired` / `unknown` states.
- `flows` with `action_json` for reproducing discovered paths.
- The checkpoint memory page and `continue_from` context section.
- The live exploration panel as the place to add richer evidence inspection.
- The session 1 acceptance script as the regression gate before adding broader
  vulnerability procedures.

## Commits

- `a3a5e40` `feat(browser): add persistent interactive browser sessions`
- `e152f8d` `feat(agent): explore applications through browser actions`
- `2d6196b` `feat(web): show live exploration progress`
- `e4861da` `docs: record session 1 acceptance and limits`
- `d8636c7` `docs: refresh session 1 result capture`
- `90525a8` `docs: list session 1 commits`
- `208eec0` `docs: refresh session 1 live capture`
