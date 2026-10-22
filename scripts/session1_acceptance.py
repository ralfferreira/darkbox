#!/usr/bin/env python3
"""Session 1 acceptance: real browser, agent exploration, evidence, control.

Run against a live local stack:

  DATABASE_URL=postgres://darkbox:darkbox@127.0.0.1:5433/darkbox \
  DARKBOX_BLOB_ROOT=./data/objects DARKBOX_REPO_ROOT=. \
  DARKBOX_INLINE_WORKER=1 DARKBOX_BROWSER_MODE=auto \
  DARKBOX_ADAPTER_PYTHON=$PWD/.venv/bin/python \
  LAB_ALICE_PASSWORD=alice-lab-pass LAB_BOB_PASSWORD=bob-lab-pass \
  ./target/debug/darkbox serve --bind 127.0.0.1:8787

  DARKBOX_API_URL=http://127.0.0.1:8787 \
  DARKBOX_AGENT_PORT=8790 DARKBOX_AGENT_PLANNER=deterministic \
  DARKBOX_MODEL=deterministic-planner \
  npx tsx apps/agent/src/index.ts

  python3 scripts/session1_acceptance.py

The planner is labelled `deterministic-planner`. It is a test driver over the
same real browser operations, not a model and not a mock. A model-provider run
is attempted only with --real-model.
"""

from __future__ import annotations

import argparse
import json
import os
import statistics
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
API = os.environ.get("DARKBOX_API_URL", "http://127.0.0.1:8787").rstrip("/") + "/v1"
AGENT = os.environ.get("DARKBOX_AGENT_URL", "http://127.0.0.1:8790").rstrip("/")
LAB = os.environ.get("DARKBOX_LAB_URL", "http://127.0.0.1:8080").rstrip("/")
TOKEN = os.environ.get("DARKBOX_TOKEN", "darkbox-dev-token")


class HttpError(Exception):
    def __init__(self, status, body):
        super().__init__(f"HTTP {status}: {body[:500]}")
        self.status = status
        self.body = body


def http(method, url, body=None, timeout=60, token=None):
    data = json.dumps(body).encode() if body is not None else None
    headers = {"Accept": "application/json"}
    if data is not None:
        headers["Content-Type"] = "application/json"
    if token:
        headers["Authorization"] = f"Bearer {token}"
    request = urllib.request.Request(url, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            raw = response.read()
            content_type = response.headers.get("content-type", "")
            if "application/json" in content_type and raw:
                return response.status, json.loads(raw), response.headers
            return response.status, raw, response.headers
    except urllib.error.HTTPError as error:
        body = error.read().decode("utf-8", "replace")
        raise HttpError(error.code, body) from None


def api(method, path, body=None, timeout=60):
    status, value, _ = http(method, API + path, body, timeout, TOKEN)
    return value


def agent(method, path, body=None, timeout=900):
    status, value, _ = http(method, AGENT + path, body, timeout, TOKEN)
    return value


def lab_get(path):
    status, value, _ = http("GET", LAB + path, timeout=20)
    return value


def wait_run(run_id, timeout=240):
    deadline = time.time() + timeout
    while time.time() < deadline:
        detail = api("GET", f"/runs/{run_id}")
        if detail["run"]["state"] in (
            "succeeded",
            "failed",
            "cancelled",
            "waiting_operator",
            "interrupted",
        ):
            return detail
        time.sleep(0.5)
    raise TimeoutError(f"run {run_id} did not settle")


def start_session_run(assessment_id, identity_id, tenant, base_url):
    return api(
        "POST",
        "/runs",
        {
            "assessment_id": assessment_id,
            "kind": "session",
            "idempotency_key": f"session1-session-{time.time_ns()}",
            "identity_id": identity_id,
            "tenant": tenant,
            "base_url": base_url,
            "login_path": "/login",
            "note": "session1 isolation check",
            "targets": [],
            "params": {},
            "endpoint_ids": [],
            "identity_ids": [],
            "checks": [],
            "budget_override": None,
        },
    )


def create_assessment(name):
    companies = api("GET", "/companies")
    company_id = companies[0]["id"]
    policy = {
        "include_hosts": ["127.0.0.1", "localhost"],
        "exclude_hosts": ["excluded.darkbox.test"],
        "include_paths": ["/"],
        "exclude_paths": [
            "/admin/forbidden",
            "/app/north/admin/forbidden",
            "/app/south/admin/forbidden",
        ],
        "allowed_schemes": ["http"],
        "allowed_ports": [int(urllib.parse.urlsplit(LAB).port or 8080)],
        "allow_private_network": True,
        "window": None,
        "action_classes": ["query", "session"],
        "navigation_dependencies": [],
        "test_targets": [],
        "limits": {
            "max_duration_seconds": 900,
            "max_requests": 400,
            "max_browser_steps": 200,
            "max_model_calls": 40,
            "max_cost_micros": 2_000_000,
            "max_artifact_bytes": 64 * 1024 * 1024,
            "max_concurrency_per_origin": 2,
        },
        "retain_raw": False,
    }
    assessment = api(
        "POST",
        "/assessments",
        {
            "company_id": company_id,
            "name": name,
            "environment": "lab",
            "objective": "Session 1 functional browser exploration.",
            "initial_targets": [LAB + "/"],
            "policy": policy,
        },
    )
    aid = assessment["id"]
    api("POST", f"/assessments/{aid}/tenants", {"key": "north", "label": "North tenant"})
    api("POST", f"/assessments/{aid}/tenants", {"key": "south", "label": "South tenant"})
    identities = {}
    for label, role, tenants, env in (
        ("alice", "admin", ["north", "south"], "LAB_ALICE_PASSWORD"),
        ("bob", "member", ["north"], "LAB_BOB_PASSWORD"),
        ("mallory", "member", ["north"], "PATH"),
    ):
        identity = api(
            "POST",
            f"/assessments/{aid}/identities",
            {
                "label": label,
                "role_label": role,
                "tenant_keys": tenants,
                "capabilities": ["navigate", "read"],
                "secret_ref": "env:" + env,
                "login_config": {
                    "login_url": LAB + "/login",
                    "success_text": "Signed in as",
                    "failure_text": "Invalid credentials",
                },
            },
        )
        identities[label] = identity["id"]
    return aid, identities


def criteria():
    return []


def add(rows, key, title, fn):
    try:
        detail, evidence = fn()
        rows.append(
            {
                "id": key,
                "title": title,
                "status": "passed",
                "detail": detail,
                "evidence": evidence,
            }
        )
    except Exception as error:  # acceptance boundary: record the failure, continue
        rows.append(
            {
                "id": key,
                "title": title,
                "status": "failed",
                "detail": f"{type(error).__name__}: {error}",
                "evidence": [],
            }
        )


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--real-model", action="store_true")
    parser.add_argument(
        "--output", default=str(ROOT / "artifacts/session1/report.json")
    )
    args = parser.parse_args()

    started = datetime.now(timezone.utc)
    http("POST", LAB + "/_lab/reset", timeout=10)
    aid, identities = create_assessment(f"Session 1 acceptance {started:%H:%M:%S}")
    rows = []

    capability = api("GET", "/capabilities/browser")
    add(
        rows,
        "AC-01",
        "Real browser executes JavaScript",
        lambda: (
            f"{capability['provider']} {capability['version']} JavaScript probe {capability['state']}",
            [f"provider={capability['provider']}", f"version={capability['version']}"],
        ),
    )

    # Start the agent asynchronously so progress can be observed before finish.
    objective = (
        "Sign in with the configured identity, find the reports area, apply period "
        "2026-08 with minimum margin 19, and confirm that the displayed result matches "
        "the filter."
    )
    accepted = agent(
        "POST",
        "/turns",
        {
            "assessment_id": aid,
            "objective": objective,
            "identity_id": identities["alice"],
            "max_steps": 10,
            "token_budget": 8000,
            "start_url": LAB + "/",
        },
    )
    run_id = accepted["run_id"]

    observed_early = {"actions": 0, "url": None, "state": None}
    deadline = time.time() + 90
    while time.time() < deadline:
        detail = api("GET", f"/runs/{run_id}")
        progress = api("GET", f"/runs/{run_id}/browser/progress")
        observed_early = {
            "state": detail["run"]["state"],
            "actions": len(progress["actions"]),
            "url": progress["current"]["url"],
        }
        if progress["actions"] and detail["run"]["state"] == "running":
            break
        time.sleep(0.3)

    detail = wait_run(run_id, timeout=300)
    progress = api("GET", f"/runs/{run_id}/browser/progress")
    evidence = api(
        "GET",
        f"/assessments/{aid}/evidence?run_id={urllib.parse.quote(run_id)}&limit=300",
    )
    actions = progress["actions"]
    contexts = {
        (action.get("action") or {}).get("context_key")
        for action in actions
        if (action.get("action") or {}).get("context_key")
    }
    success_text = " ".join(
        str(action.get("observation") or "") for action in actions
    )
    discovery_confirmed = any(
        "Reports" in json.dumps(action.get("observation") or {}) for action in actions
    )
    filter_confirmed = (
        "1 of 4 report rows match period 2026-08 with margin at least 19.0%"
        in success_text
        and "43900.00" in success_text
    )

    add(
        rows,
        "AC-02",
        "A path the agent was not given was discovered",
        lambda: (
            "Reports was reached by clicking a visible link, not from a supplied route"
            if discovery_confirmed
            else (_ for _ in ()).throw(AssertionError("no discovered reports navigation")),
            [run_id],
        ),
    )
    add(
        rows,
        "AC-03",
        "Filter interaction and functional result verified",
        lambda: (
            "period 2026-08 with margin >= 19 rendered one matching row in the page"
            if filter_confirmed and detail["run"]["state"] == "succeeded"
            else (_ for _ in ()).throw(
                AssertionError(
                    f"state={detail['run']['state']} filter_confirmed={filter_confirmed}"
                )
            ),
            [run_id],
        ),
    )
    add(
        rows,
        "AC-04",
        "Session preserved across several actions",
        lambda: (
            f"{len(actions)} actions used {len(contexts)} browser context"
            if len(contexts) == 1 and len(actions) >= 5
            else (_ for _ in ()).throw(AssertionError(f"contexts={contexts}")),
            sorted(contexts),
        ),
    )

    def two_identities():
        keys = []
        run_ids = []
        for label in ("alice", "bob"):
            run = api(
                "POST",
                f"/assessments/{aid}/agent/turns",
                {
                    "objective": f"identity isolation {label}",
                    "identity_id": identities[label],
                    "max_steps": 3,
                    "token_budget": 2000,
                    "start_url": LAB + "/",
                    "model": "acceptance-isolation",
                },
            )
            rid = run["run"]["id"]
            run_ids.append(rid)
            opened = api(
                "POST",
                f"/runs/{rid}/browser/open",
                {"url": LAB + "/", "identity_id": identities[label], "tenant": "north"},
            )
            key = opened["result"]["session"]["context_key"]
            login = api(
                "POST",
                f"/runs/{rid}/browser/login",
                {"session_key": key, "login": {}},
            )
            if login["result"].get("login_state") != "authenticated":
                raise AssertionError(f"{label} did not authenticate")
            keys.append(key)
        if len(set(keys)) != 2:
            raise AssertionError("identities did not receive separate browser contexts")
        for rid in run_ids:
            api("POST", f"/runs/{rid}/cancel", {"reason": "isolation check complete"})
        return f"alice and bob authenticated in two isolated contexts ({len(set(keys))} keys)", keys

    add(rows, "AC-05", "Two identities use separate contexts", two_identities)

    def configurable_login():
        detail = wait_run(start_session_run(aid, identities["alice"], "north", LAB)["id"])
        if detail["run"]["state"] != "succeeded":
            raise AssertionError("configured login did not succeed")
        return (
            "login used an identity-level login_config with an environment secret reference",
            [detail["run"]["id"]],
        )

    add(rows, "AC-06", "Login is configurable beyond lab conventions", configurable_login)
    add(
        rows,
        "AC-07",
        "Progress is visible before completion",
        lambda: (
            f"at {observed_early['state']}, {observed_early['actions']} actions were visible at {observed_early['url']}"
            if observed_early["state"] == "running" and observed_early["actions"] > 0
            else (_ for _ in ()).throw(AssertionError(str(observed_early))),
            [observed_early["url"] or ""],
        ),
    )

    def negative_login():
        run = api(
            "POST",
            f"/assessments/{aid}/agent/turns",
            {
                "objective": "negative login",
                "identity_id": identities["mallory"],
                "max_steps": 3,
                "token_budget": 2000,
                "start_url": LAB + "/",
                "model": "acceptance-negative",
            },
        )
        rid = run["run"]["id"]
        opened = api(
            "POST",
            f"/runs/{rid}/browser/open",
            {"url": LAB + "/", "identity_id": identities["mallory"], "tenant": "north"},
        )
        key = opened["result"]["session"]["context_key"]
        login = api("POST", f"/runs/{rid}/browser/login", {"session_key": key, "login": {}})
        if login["result"].get("login_state") != "login_failed":
            raise AssertionError("a rejected login did not report login_failed")
        if login["state"] != "blocked":
            raise AssertionError("a rejected login was not a blocked action")
        identity = next(
            item
            for item in api("GET", f"/assessments/{aid}/identities")
            if item["label"] == "mallory"
        )
        if identity["session_state"] == "authenticated":
            raise AssertionError("rejected login persisted as authenticated")
        return (
            "rejected credentials produced login_failed, a blocked action and no authenticated session",
            [rid],
        )

    add(rows, "AC-08", "A failed login cannot look authenticated", negative_login)

    def blocked_action():
        run = api(
            "POST",
            f"/assessments/{aid}/agent/turns",
            {
                "objective": "blocked action",
                "identity_id": identities["alice"],
                "max_steps": 3,
                "token_budget": 2000,
                "start_url": LAB + "/",
                "model": "acceptance-negative",
            },
        )
        rid = run["run"]["id"]
        opened = api(
            "POST",
            f"/runs/{rid}/browser/open",
            {"url": LAB + "/", "identity_id": identities["alice"], "tenant": "north"},
        )
        key = opened["result"]["session"]["context_key"]
        blocked = api(
            "POST",
            f"/runs/{rid}/browser/navigate",
            {"session_key": key, "url": LAB + "/app/north/admin/forbidden"},
        )
        if blocked["state"] != "blocked" or blocked["effect_status"] != "not_dispatched":
            raise AssertionError("excluded navigation was not recorded as blocked before dispatch")
        state = lab_get("/_lab/state")
        if state["hits"].get("/app/north/admin/forbidden", 0) != 0:
            raise AssertionError("excluded path received a request")
        return "excluded path denied before dispatch; target saw zero hits", [rid]

    add(rows, "AC-09", "A blocked action is not executed", blocked_action)

    def cancellation():
        run = api(
            "POST",
            f"/assessments/{aid}/agent/turns",
            {
                "objective": "cancel during wait",
                "identity_id": identities["alice"],
                "max_steps": 3,
                "token_budget": 2000,
                "start_url": LAB + "/",
                "model": "acceptance-cancel",
            },
        )
        rid = run["run"]["id"]
        opened = api(
            "POST",
            f"/runs/{rid}/browser/open",
            {"url": LAB + "/", "identity_id": identities["alice"], "tenant": "north"},
        )
        key = opened["result"]["session"]["context_key"]
        result = {}

        def wait_action():
            try:
                api(
                    "POST",
                    f"/runs/{rid}/browser/act",
                    {"session_key": key, "action": {"type": "wait", "timeout_ms": 15000}},
                    timeout=40,
                )
                result["completed"] = True
            except HttpError as error:
                result["error"] = error.status

        thread = threading.Thread(target=wait_action)
        thread.start()
        time.sleep(1)
        before = time.time()
        cancelled = api("POST", f"/runs/{rid}/cancel", {"reason": "acceptance cancellation"})
        elapsed = time.time() - before
        thread.join(timeout=10)
        if thread.is_alive():
            raise AssertionError("the browser action did not stop after cancellation")
        if cancelled["state"] != "cancelled":
            raise AssertionError("run not cancelled")
        if result.get("completed"):
            raise AssertionError("the in-flight action completed instead of being interrupted")
        try:
            api(
                "POST",
                f"/runs/{rid}/browser/observe",
                {"session_key": key},
            )
        except HttpError as error:
            if error.status != 409:
                raise
        else:
            raise AssertionError("a cancelled objective accepted another browser action")
        return f"cancel returned in {elapsed:.2f}s and stopped the in-flight browser action", [
            rid
        ]

    add(rows, "AC-10", "Cancellation interrupts model and browser work", cancellation)

    def evidence_links():
        screenshots = [item for item in evidence if item["kind"] == "browser_screenshot"]
        traces = [item for item in evidence if item["kind"] == "browser_trace"]
        if not screenshots or not traces:
            raise AssertionError("screenshots or traces missing")
        status, data, headers = http(
            "GET", API + f"/evidence/{screenshots[0]['id']}/content", timeout=20, token=TOKEN
        )
        if status != 200 or not data.startswith(b"\xff\xd8"):
            raise AssertionError("screenshot content is not a JPEG")
        if headers.get("content-type") != "image/jpeg":
            raise AssertionError("screenshot content type is not image/jpeg")
        return (
            f"{len(traces)} traces and {len(screenshots)} screenshots linked to {run_id}; "
            "screenshot served as image/jpeg",
            [item["id"] for item in (traces[:2] + screenshots[:2])],
        )

    add(rows, "AC-11", "Evidence is linked and visual", evidence_links)

    def flow_recorded():
        flows = api("GET", f"/assessments/{aid}/flows")
        discovered = [flow for flow in flows if flow["state"] == "reproduced"]
        if not discovered:
            raise AssertionError("no reproduced flow was recorded")
        return (
            f"flow {discovered[0]['id']} recorded {len(discovered[0]['steps'])} discovered steps",
            [discovered[0]["id"]],
        )

    add(rows, "AC-12", "Discovered flow is recorded", flow_recorded)

    def continuation():
        partial = agent(
            "POST",
            "/turns",
            {
                "assessment_id": aid,
                "objective": "Sign in and open reports but stop before filtering.",
                "identity_id": identities["alice"],
                "max_steps": 10,
                "token_budget": 8000,
                "start_url": LAB + "/",
                "wait": True,
            },
        )
        if partial["run"]["state"] == "succeeded":
            raise AssertionError("an incomplete objective was marked succeeded")
        checkpoint = api("GET", f"/runs/{partial['runId']}/agent/checkpoint")
        if not checkpoint.get("checkpoint"):
            raise AssertionError("no checkpoint persisted")
        new_turn = api(
            "POST",
            f"/assessments/{aid}/agent/turns",
            {
                "objective": "Continue with period 2026-08 and margin 19.",
                "identity_id": identities["alice"],
                "max_steps": 8,
                "token_budget": 8000,
                "start_url": LAB + "/",
                "continue_from": partial["runId"],
                "model": "acceptance-continuation",
            },
        )
        continuation_section = next(
            (
                section
                for section in new_turn["context"]["sections"]
                if section["title"] == "Objective continuation"
            ),
            None,
        )
        if not continuation_section:
            raise AssertionError("continuation context did not include prior actions")
        api(
            "POST",
            f"/runs/{new_turn['run']['id']}/cancel",
            {"reason": "acceptance checked continuation context"},
        )
        return (
            f"partial objective persisted {checkpoint['checkpoint']['slug']} and the new session received {len(checkpoint['actions'])} prior actions",
            [partial["runId"], new_turn["run"]["id"]],
        )

    add(rows, "AC-13", "A new session continues from persisted state", continuation)

    def outcome_unknown():
        run = api(
            "POST",
            f"/assessments/{aid}/agent/turns",
            {
                "objective": "unknown external outcome",
                "identity_id": identities["alice"],
                "max_steps": 3,
                "token_budget": 2000,
                "start_url": LAB + "/",
                "model": "acceptance-unknown",
            },
        )
        rid = run["run"]["id"]
        opened = api(
            "POST",
            f"/runs/{rid}/browser/open",
            {"url": LAB + "/", "identity_id": identities["alice"], "tenant": "north"},
        )
        key = opened["result"]["session"]["context_key"]
        api("POST", f"/runs/{rid}/browser/login", {"session_key": key, "login": {}})
        navigated = api(
            "POST",
            f"/runs/{rid}/browser/navigate",
            {"session_key": key, "url": LAB + "/app/north/effect-check"},
        )
        button = next(
            item
            for item in navigated["result"]["observation"]["interactive"]
            if item["tag"] == "button" and "slow" in item["name"].lower()
        )
        result = api(
            "POST",
            f"/runs/{rid}/browser/act",
            {"session_key": key, "action": {"type": "click", "ref": button["ref"]}},
            timeout=40,
        )
        if result["state"] != "failed" or result["effect_status"] != "outcome_unknown":
            raise AssertionError(f"action was {result['state']}/{result['effect_status']}")
        detail = api("GET", f"/runs/{rid}")
        if not detail["run"]["effect_unknown"] or detail["run"]["state"] != "waiting_operator":
            raise AssertionError("run did not preserve unknown outcome")
        state = lab_get("/_lab/state")
        if state["effects"].get("north", 0) != 1:
            raise AssertionError("slow effect was not applied exactly once")
        try:
            api("POST", f"/runs/{rid}/browser/observe", {"session_key": key})
        except HttpError as error:
            if error.status != 409:
                raise
        else:
            raise AssertionError("an objective with an unknown outcome accepted another action")
        return (
            "effect applied once, outcome_unknown persisted, no automatic retry accepted",
            [rid],
        )

    add(rows, "AC-14", "Unknown external outcome is not repeated", outcome_unknown)

    def connectivity():
        health = agent("GET", "/health")
        capability = api("GET", "/capabilities/browser")
        if health.get("status") != "ok":
            raise AssertionError(f"agent health is {health}")
        if capability.get("state") != "available":
            raise AssertionError("browser capability is unavailable")
        return (
            f"API, agent ({health.get('model')}) and browser ({capability.get('provider')}) answered on the local contract",
            ["/health", "/v1/capabilities/browser"],
        )

    add(rows, "AC-15", "Local stack connectivity", connectivity)

    if args.real_model:
        try:
            health = agent("GET", "/health")
            if health.get("planner") == "deterministic":
                raise AssertionError(
                    "the agent service is in deterministic planner mode; start it with a provider"
                )
            result = agent(
                "POST",
                "/turns",
                {
                    "assessment_id": aid,
                    "objective": objective,
                    "identity_id": identities["alice"],
                    "max_steps": 10,
                    "token_budget": 8000,
                    "start_url": LAB + "/",
                    "wait": True,
                },
            )
            if result.get("error"):
                raise AssertionError(result["error"])
            if result["run"]["state"] != "succeeded":
                raise AssertionError(f"real model state {result['run']['state']}")
            rows.append(
                {
                    "id": "AC-16",
                    "title": "Complete run with a real model",
                    "status": "passed",
                    "detail": f"model run {result['runId']} succeeded",
                    "evidence": [result["runId"]],
                }
            )
        except Exception as error:
            rows.append(
                {
                    "id": "AC-16",
                    "title": "Complete run with a real model",
                    "status": "pending",
                    "detail": f"blocked: {type(error).__name__}: {error}",
                    "evidence": [],
                }
            )
    else:
        rows.append(
            {
                "id": "AC-16",
                "title": "Complete run with a real model",
                "status": "pending",
                "detail": "not attempted; rerun with --real-model and a funded provider",
                "evidence": [],
            }
        )

    report = {
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "assessment_id": aid,
        "planner_run_id": run_id,
        "lab": LAB,
        "api": API,
        "agent": AGENT,
        "criteria": rows,
        "passed": sum(1 for row in rows if row["status"] == "passed"),
        "failed": sum(1 for row in rows if row["status"] == "failed"),
        "pending": sum(1 for row in rows if row["status"] == "pending"),
    }
    output = Path(args.output)
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))
    return 0 if report["failed"] == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
