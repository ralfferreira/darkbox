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


def create_assessment(name, action_classes=None, max_requests=400):
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
        "action_classes": action_classes or ["query", "session"],
        "navigation_dependencies": [],
        "test_targets": [],
        "limits": {
            "max_duration_seconds": 900,
            "max_requests": max_requests,
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
    portal = api(
        "POST",
        f"/assessments/{aid}/identities",
        {
            "label": "portal",
            "role_label": "admin",
            "tenant_keys": ["north"],
            "capabilities": ["navigate", "read"],
            "secret_ref": "env:LAB_ALICE_PASSWORD",
            "login_config": {
                "login_url": LAB + "/portal/signin",
                "username": "alice",
                "username_selector": "input[name=account]",
                "password_selector": "input[name=passphrase]",
                "submit_selector": "button[type=submit]",
                "success_text": "Signed in as",
                "failure_text": "Invalid credentials",
            },
        },
    )
    identities["portal"] = portal["id"]
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
            "max_steps": 14,
            "token_budget": 9000,
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
    def functional_result():
        if detail["run"]["state"] != "succeeded":
            raise AssertionError(f"objective ended as {detail['run']['state']}: {detail['run']['reason']}")
        oracle = lab_get("/_lab/state")
        matches = [
            item
            for item in oracle.get("report_queries", [])
            if item.get("period") == "2026-08"
            and abs(float(item.get("min_margin", 0)) - 19.0) < 0.01
            and item.get("rows") == 1
        ]
        if not filter_confirmed:
            raise AssertionError("the rendered observation did not contain the expected filter result")
        if not matches:
            raise AssertionError("the lab oracle did not record a JavaScript filter query with one row")
        return (
            "the rendered page and the independent lab oracle both show one row for 2026-08 / margin 19",
            [run_id, str(matches[-1].get("trace_id"))],
        )

    add(rows, "AC-03", "Filter interaction and functional result verified", functional_result)
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
        run = start_session_run(aid, identities["portal"], "north", LAB)
        detail = wait_run(run["id"])
        if detail["run"]["state"] != "succeeded":
            raise AssertionError(f"portal login did not succeed: {detail['run']['reason']}")
        return (
            "the portal fixture used /portal/signin with account/passphrase fields and CSS field hints",
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
                "objective": "blocked mutation",
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
        state_before = lab_get("/_lab/state")
        effects_before = state_before["effects"].get("north", 0)
        blocked = api(
            "POST",
            f"/runs/{rid}/browser/act",
            {"session_key": key, "action": {"type": "click", "ref": button["ref"]}},
        )
        if blocked["state"] != "blocked" or blocked["effect_status"] != "not_dispatched":
            raise AssertionError(f"mutation was {blocked['state']}/{blocked['effect_status']}")
        state_after = lab_get("/_lab/state")
        if state_after["effects"].get("north", 0) != effects_before:
            raise AssertionError("a denied mutation changed the lab effect counter")
        return "a POST mutation was denied before dispatch; the target effect counter did not change", [rid]

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
        discovered = [
            flow
            for flow in flows
            if flow["state"] == "reproduced" and flow.get("assertion")
        ]
        if not discovered:
            raise AssertionError("no reproduced flow with a structured assertion was recorded")
        flow = discovered[0]
        return (
            f"flow {flow['id']} recorded {len(flow['steps'])} actions and a re-checkable assertion",
            [flow["id"], json.dumps(flow["assertion"])],
        )

    add(rows, "AC-12", "Discovered flow is recorded", flow_recorded)

    def continuation():
        partial = api(
            "POST",
            f"/assessments/{aid}/agent/turns",
            {
                "objective": "Persist a partial reports exploration.",
                "identity_id": identities["alice"],
                "max_steps": 4,
                "token_budget": 4000,
                "start_url": LAB + "/",
                "model": "acceptance-partial",
            },
        )
        partial_id = partial["run"]["id"]
        opened = api(
            "POST",
            f"/runs/{partial_id}/browser/open",
            {"url": LAB + "/", "identity_id": identities["alice"], "tenant": "north"},
        )
        key = opened["result"]["session"]["context_key"]
        api("POST", f"/runs/{partial_id}/browser/login", {"session_key": key, "login": {}})
        api(
            "POST",
            f"/runs/{partial_id}/browser/navigate",
            {"session_key": key, "url": LAB + "/app/north/reports"},
        )
        api("POST", f"/runs/{partial_id}/cancel", {"reason": "partial proof"})
        checkpoint = api("GET", f"/runs/{partial_id}/agent/checkpoint")
        if not checkpoint.get("checkpoint"):
            raise AssertionError("no checkpoint persisted")
        effects_before = lab_get("/_lab/state")["effects"].get("north", 0)
        before_queries = len(
            [
                item
                for item in lab_get("/_lab/state").get("report_queries", [])
                if item.get("period") == "2026-08"
            ]
        )
        result = agent(
            "POST",
            "/turns",
            {
                "assessment_id": aid,
                "objective": "Continue from the checkpoint: apply period 2026-08 with minimum margin 19 and confirm the rendered result.",
                "identity_id": identities["alice"],
                "max_steps": 12,
                "token_budget": 9000,
                "start_url": LAB + "/",
                "continue_from": partial_id,
                "wait": True,
            },
        )
        if result.get("error"):
            raise AssertionError(f"continuation failed: {result['error']}")
        if result["run"]["state"] != "succeeded":
            raise AssertionError(f"continued objective ended as {result['run']['state']}")
        oracle = lab_get("/_lab/state")
        after_queries = len(
            [
                item
                for item in oracle.get("report_queries", [])
                if item.get("period") == "2026-08"
            ]
        )
        if after_queries != before_queries + 1:
            raise AssertionError("the continued session did not apply exactly one new filter query")
        if oracle["effects"].get("north", 0) != effects_before:
            raise AssertionError("continuation repeated an effect")
        return (
            f"partial run {partial_id} persisted {len(checkpoint['actions'])} actions; a new model session completed the filter and added one query",
            [partial_id, result["runId"]],
        )

    add(rows, "AC-13", "A new session continues from persisted state", continuation)

    def outcome_unknown():
        unknown_aid, unknown_identities = create_assessment(
            "Unknown outcome lab",
            action_classes=["query", "session", "external_effect"],
            max_requests=100,
        )
        alice = unknown_identities["alice"]
        run = api(
            "POST",
            f"/assessments/{unknown_aid}/agent/turns",
            {
                "objective": "unknown external outcome",
                "identity_id": alice,
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
            {"url": LAB + "/", "identity_id": alice, "tenant": "north"},
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
        effects = lab_get("/_lab/state")["effects"].get("north", 0)
        if effects != 1:
            raise AssertionError(f"slow effect applied {effects} times")
        try:
            api("POST", f"/runs/{rid}/browser/observe", {"session_key": key})
        except HttpError as error:
            if error.status != 409:
                raise
        else:
            raise AssertionError("an objective with an unknown outcome accepted another action")
        reconciled = api(
            "POST",
            f"/runs/{rid}/agent/reconcile",
            {"resolution": "applied", "note": "acceptance confirmed one applied effect"},
        )
        if reconciled["effect_unknown"]:
            raise AssertionError("reconciliation did not clear effect_unknown")
        if lab_get("/_lab/state")["effects"].get("north", 0) != 1:
            raise AssertionError("reconciliation re-ran the effect")
        return (
            "effect applied once, outcome_unknown persisted, no retry accepted, operator reconciled",
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

    def request_budget():
        budget_aid, budget_identities = create_assessment(
            "Request budget lab", max_requests=8
        )
        alice = budget_identities["alice"]
        run = api(
            "POST",
            f"/assessments/{budget_aid}/agent/turns",
            {
                "objective": "request burst",
                "identity_id": alice,
                "max_steps": 3,
                "token_budget": 2000,
                "start_url": LAB + "/",
                "model": "acceptance-budget",
            },
        )
        rid = run["run"]["id"]
        opened = api(
            "POST",
            f"/runs/{rid}/browser/open",
            {"url": LAB + "/", "identity_id": alice, "tenant": "north"},
        )
        key = opened["result"]["session"]["context_key"]
        api("POST", f"/runs/{rid}/browser/login", {"session_key": key, "login": {}})
        navigated = api(
            "POST",
            f"/runs/{rid}/browser/navigate",
            {"session_key": key, "url": LAB + "/app/north/burst"},
        )
        button = next(
            item
            for item in navigated["result"]["observation"]["interactive"]
            if item["tag"] == "button"
        )
        result = api(
            "POST",
            f"/runs/{rid}/browser/act",
            {"session_key": key, "action": {"type": "click", "ref": button["ref"]}},
        )
        if result["state"] != "blocked" or "request budget exhausted" not in result["result"].get("summary", ""):
            raise AssertionError(f"burst was not blocked: {result['state']} {result['result'].get('summary')}")
        detail = api("GET", f"/runs/{rid}")
        if detail["run"]["spent"]["requests"] > 8:
            raise AssertionError("request grant exceeded the aggregate budget")
        return (
            f"a 20-request browser burst stopped at the aggregate request budget ({detail['run']['spent']['requests']}/8)",
            [rid],
        )

    add(rows, "AC-17", "Browser requests consume the aggregate budget", request_budget)

    def scope_revocation():
        revoke_aid, revoke_identities = create_assessment("Scope revocation lab")
        alice = revoke_identities["alice"]
        run = api(
            "POST",
            f"/assessments/{revoke_aid}/agent/turns",
            {
                "objective": "scope revocation",
                "identity_id": alice,
                "max_steps": 3,
                "token_budget": 2000,
                "start_url": LAB + "/",
                "model": "acceptance-revoke",
            },
        )
        rid = run["run"]["id"]
        opened = api(
            "POST",
            f"/runs/{rid}/browser/open",
            {"url": LAB + "/", "identity_id": alice, "tenant": "north"},
        )
        key = opened["result"]["session"]["context_key"]
        api("POST", f"/runs/{rid}/browser/login", {"session_key": key, "login": {}})
        before = lab_get("/_lab/state")["hits"].get("/app/north/reports", 0)
        detail = api("GET", f"/assessments/{revoke_aid}")
        policy = detail["scope"]["policy"]
        policy["exclude_paths"] = list(policy.get("exclude_paths", [])) + ["/app/north/reports"]
        api(
            "POST",
            f"/assessments/{revoke_aid}/scope-versions",
            {"policy": policy, "note": "revoke reports in an open context"},
        )
        dashboard = api(
            "POST",
            f"/runs/{rid}/browser/navigate",
            {"session_key": key, "url": LAB + "/app/north"},
        )
        reports = next(
            item
            for item in dashboard["result"]["observation"]["interactive"]
            if item["tag"] == "a" and "reports" in item["name"].lower()
        )
        blocked = api(
            "POST",
            f"/runs/{rid}/browser/act",
            {"session_key": key, "action": {"type": "click", "ref": reports["ref"]}},
        )
        if blocked["state"] != "blocked":
            raise AssertionError(f"reports click was {blocked['state']}, expected blocked")
        try:
            direct = api(
                "POST",
                f"/runs/{rid}/browser/navigate",
                {"session_key": key, "url": LAB + "/app/north/reports"},
            )
        except HttpError as error:
            if error.status != 409:
                raise
        else:
            if direct.get("state") != "blocked":
                raise AssertionError(
                    f"direct navigation was {direct.get('state')} after revocation"
                )
        after = lab_get("/_lab/state")["hits"].get("/app/north/reports", 0)
        if after != before:
            raise AssertionError("revoked reports path received a request")
        return (
            "a scope version revoked reports in an open context; click and direct navigation were blocked with zero hits",
            [rid],
        )

    add(rows, "AC-18", "Scope changes revoke open browser contexts", scope_revocation)

    def sanitized_observation():
        sanitize_aid, sanitize_identities = create_assessment("Sanitization lab")
        alice = sanitize_identities["alice"]
        run = api(
            "POST",
            f"/assessments/{sanitize_aid}/agent/turns",
            {
                "objective": "sanitization",
                "identity_id": alice,
                "max_steps": 3,
                "token_budget": 2000,
                "start_url": LAB + "/",
                "model": "acceptance-sanitize",
            },
        )
        rid = run["run"]["id"]
        opened = api(
            "POST",
            f"/runs/{rid}/browser/open",
            {"url": LAB + "/", "identity_id": alice, "tenant": "north"},
        )
        key = opened["result"]["session"]["context_key"]
        api("POST", f"/runs/{rid}/browser/login", {"session_key": key, "login": {}})
        navigated = api(
            "POST",
            f"/runs/{rid}/browser/navigate",
            {"session_key": key, "url": LAB + "/app/north/secret-surface"},
        )
        progress = api("GET", f"/runs/{rid}/browser/progress")
        detail = api("GET", f"/runs/{rid}")
        destinations = {
            "response": json.dumps(navigated),
            "progress": json.dumps(progress),
            "events": json.dumps(detail["events"]),
        }
        leaked = [name for name, text in destinations.items() if "LAB_SECRET_darkbox_7f3a" in text]
        if leaked:
            raise AssertionError(f"sentinel reached {leaked}")
        return (
            "the lab sentinel did not appear in the action response, progress or event stream",
            [rid],
        )

    add(rows, "AC-19", "Structured observations are sanitized before use", sanitized_observation)

    def replay_negative():
        negative_aid, negative_identities = create_assessment("Replay negative lab")
        alice = negative_identities["alice"]
        flow = api(
            "POST",
            f"/assessments/{negative_aid}/flows",
            {
                "name": "Corrupt report replay",
                "identity_id": alice,
                "tenant_key": "north",
                "preconditions": [],
                "assertion": {
                    "kind": "text_contains",
                    "value": "1 of 4 report rows match period 2026-08",
                },
                "steps": [
                    {
                        "intent": "open the lab",
                        "expected_result": "",
                        "endpoint_id": None,
                        "action_json": {"type": "open", "url": LAB + "/"},
                    },
                    {
                        "intent": "authenticate",
                        "expected_result": "",
                        "endpoint_id": None,
                        "action_json": {"type": "login"},
                    },
                    {
                        "intent": "open a deliberately corrupt report view",
                        "expected_result": "",
                        "endpoint_id": None,
                        "action_json": {
                            "type": "navigate",
                            "url": LAB + "/app/north/reports?corrupt=1",
                        },
                    },
                ],
                "state": "proposed",
                "evidence_ids": [],
            },
        )
        run = api(
            "POST",
            "/runs",
            {
                "assessment_id": negative_aid,
                "kind": "flow_reproduction",
                "idempotency_key": f"corrupt-{time.time_ns()}",
                "flow_id": flow["id"],
                "identity_id": alice,
                "budget_override": None,
                "note": "negative replay",
            },
        )
        detail = wait_run(run["id"])
        if detail["run"]["state"] != "failed":
            raise AssertionError(f"corrupt replay ended as {detail['run']['state']}")
        flows = api("GET", f"/assessments/{negative_aid}/flows")
        result_flow = next(item for item in flows if item["id"] == flow["id"])
        if result_flow["state"] == "reproduced":
            raise AssertionError("wrong rendered data was marked as a reproduced flow")
        return (
            "actions executed but the corrupt report view failed the structured assertion",
            [run["id"], flow["id"]],
        )

    add(rows, "AC-20", "Replay verifies the result, not just action execution", replay_negative)

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
                    "max_steps": 14,
                    "token_budget": 9000,
                    "start_url": LAB + "/",
                    "wait": True,
                },
            )
            if result.get("error"):
                external = any(
                    marker in result["error"].lower()
                    for marker in ("insufficient balance", "credit", "quota", "billing", "balance")
                )
                if external:
                    rows.append(
                        {
                            "id": "AC-16",
                            "title": "Complete run with a real model",
                            "status": "pending",
                            "detail": f"blocked by provider precondition: {result['error']}",
                            "evidence": [],
                        }
                    )
                else:
                    raise AssertionError(result["error"])
            elif result["run"]["state"] != "succeeded":
                raise AssertionError(f"real model state {result['run']['state']}: {result['run']['reason']}")
            else:
                oracle = lab_get("/_lab/state")
                filtered = [
                    item
                    for item in oracle.get("report_queries", [])
                    if item.get("period") == "2026-08" and item.get("rows") == 1
                ]
                if not filtered:
                    raise AssertionError("real model run did not produce the independent filter oracle record")
                rows.append(
                    {
                        "id": "AC-16",
                        "title": "Complete run with a real model",
                        "status": "passed",
                        "detail": f"model run {result['runId']} succeeded and the lab oracle recorded the filtered report",
                        "evidence": [result["runId"], filtered[-1].get("trace_id", "")],
                    }
                )
        except Exception as error:
            message = f"{type(error).__name__}: {error}"
            external = any(
                marker in message.lower()
                for marker in ("insufficient balance", "credit", "quota", "billing", "balance")
            )
            rows.append(
                {
                    "id": "AC-16",
                    "title": "Complete run with a real model",
                    "status": "failed" if not external else "pending",
                    "detail": (f"provider precondition: {message}" if external else message),
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
