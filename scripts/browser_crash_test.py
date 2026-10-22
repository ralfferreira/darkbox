#!/usr/bin/env python3
"""Controlled process crash after an external effect, before confirmation.

Starts a second API process on port 8788 with the same database, applies the
slow lab effect, kills the process, restarts it, and verifies recovery marks the
action outcome_unknown without repeating the effect.
"""

from __future__ import annotations

import json
import os
import subprocess
import threading
import time
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
TOKEN = "darkbox-dev-token"


def load_env():
    env = dict(os.environ)
    path = ROOT / ".env"
    if path.exists():
        for line in path.read_text().splitlines():
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            key, value = line.split("=", 1)
            env[key.strip()] = value
    return env


ENV = load_env()
API = "http://127.0.0.1:8788/v1"
FALLBACK = "http://127.0.0.1:8787/v1"
LAB = ENV.get("DARKBOX_LAB_URL", "http://127.0.0.1:8080").rstrip("/")


def http(method, url, body=None, timeout=60):
    data = json.dumps(body).encode() if body is not None else None
    headers = {"Accept": "application/json", "Authorization": f"Bearer {TOKEN}"}
    if data is not None:
        headers["Content-Type"] = "application/json"
    request = urllib.request.Request(url, data=data, method=method, headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            raw = response.read()
            return response.status, json.loads(raw) if raw else {}
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read() or b"{}")


def call(method, path, body=None, timeout=60, base=API):
    return http(method, base + path, body, timeout)


def start_api():
    process = subprocess.Popen(
        [str(ROOT / "target/debug/darkbox"), "serve", "--bind", "127.0.0.1:8788"],
        cwd=ROOT,
        env=ENV,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    deadline = time.time() + 30
    while time.time() < deadline:
        try:
            status, _ = http("GET", "http://127.0.0.1:8788/health", timeout=2)
            if status == 200:
                return process
        except Exception:
            pass
        time.sleep(0.2)
    process.kill()
    output = b""
    try:
        output = process.stdout.read() or b""
    except Exception:
        pass
    raise RuntimeError(f"second API did not start: {output.decode('utf-8', 'replace')[-1200:]}")


def create_assessment():
    companies = call("GET", "/companies")[1]
    company_id = companies[0]["id"]
    policy = {
        "include_hosts": ["127.0.0.1"],
        "exclude_hosts": [],
        "include_paths": ["/"],
        "exclude_paths": [],
        "allowed_schemes": ["http"],
        "allowed_ports": [8080],
        "allow_private_network": True,
        "window": None,
        "action_classes": ["query", "session", "external_effect"],
        "navigation_dependencies": [],
        "test_targets": [],
        "limits": {
            "max_duration_seconds": 900,
            "max_requests": 100,
            "max_browser_steps": 100,
            "max_model_calls": 10,
            "max_cost_micros": 1_000_000,
            "max_artifact_bytes": 10_000_000,
            "max_concurrency_per_origin": 2,
        },
        "retain_raw": False,
    }
    assessment = call(
        "POST",
        "/assessments",
        {
            "company_id": company_id,
            "name": f"Crash recovery {int(time.time())}",
            "environment": "lab",
            "objective": "prove crash recovery",
            "initial_targets": [LAB + "/"],
            "policy": policy,
        },
    )[1]
    aid = assessment["id"]
    call("POST", f"/assessments/{aid}/tenants", {"key": "north", "label": "north"})
    identity = call(
        "POST",
        f"/assessments/{aid}/identities",
        {
            "label": "alice",
            "role_label": "admin",
            "tenant_keys": ["north"],
            "capabilities": ["navigate"],
            "secret_ref": "env:LAB_ALICE_PASSWORD",
            "login_config": {"login_url": LAB + "/login"},
        },
    )[1]
    return aid, identity["id"]


def main():
    http("POST", LAB + "/_lab/reset", timeout=10)
    process = start_api()
    try:
        aid, identity = create_assessment()
        run = call(
            "POST",
            f"/assessments/{aid}/agent/turns",
            {
                "objective": "crash recovery",
                "identity_id": identity,
                "max_steps": 3,
                "token_budget": 2000,
                "start_url": LAB + "/",
                "model": "crash-test",
            },
        )[1]
        rid = run["run"]["id"]
        opened = call(
            "POST",
            f"/runs/{rid}/browser/open",
            {"url": LAB + "/", "identity_id": identity, "tenant": "north"},
        )[1]
        key = opened["result"]["session"]["context_key"]
        call("POST", f"/runs/{rid}/browser/login", {"session_key": key, "login": {}})
        navigated = call(
            "POST",
            f"/runs/{rid}/browser/navigate",
            {"session_key": key, "url": LAB + "/app/north/effect-check"},
        )[1]
        button = next(
            item
            for item in navigated["result"]["observation"]["interactive"]
            if item["tag"] == "button" and "slow" in item["name"].lower()
        )

        def click():
            try:
                call(
                    "POST",
                    f"/runs/{rid}/browser/act",
                    {"session_key": key, "action": {"type": "click", "ref": button["ref"]}},
                    timeout=60,
                )
            except Exception:
                pass

        thread = threading.Thread(target=click, daemon=True)
        thread.start()
        effect = 0
        deadline = time.time() + 10
        while time.time() < deadline:
            _, state = http("GET", LAB + "/_lab/state", timeout=5)
            effect = state["effects"].get("north", 0)
            if effect == 1:
                break
            time.sleep(0.15)
        if effect != 1:
            raise AssertionError(f"effect was {effect}, expected one before crash")

        process.kill()
        process.wait(timeout=5)
        time.sleep(1)
        restarted = start_api()
        try:
            deadline = time.time() + 40
            detail = None
            while time.time() < deadline:
                status, detail = call("GET", f"/runs/{rid}", base=API)
                if status == 200 and detail["run"]["state"] == "waiting_operator":
                    break
                time.sleep(0.5)
            if not detail or detail["run"]["state"] != "waiting_operator":
                raise AssertionError(f"recovery state is {detail and detail['run']['state']}")
            if not detail["run"]["effect_unknown"]:
                raise AssertionError("recovered run did not preserve effect_unknown")
            progress = call("GET", f"/runs/{rid}/browser/progress", base=API)[1]
            unknown = [
                action
                for action in progress["actions"]
                if action.get("effect_status") == "outcome_unknown"
            ]
            if not unknown:
                raise AssertionError("no action was recovered as outcome_unknown")
            _, state = http("GET", LAB + "/_lab/state", timeout=5)
            if state["effects"].get("north", 0) != 1:
                raise AssertionError("recovery repeated the external effect")
            reconciled = call(
                "POST",
                f"/runs/{rid}/agent/reconcile",
                {"resolution": "applied", "note": "crash recovery acceptance"},
                base=API,
            )[1]
            if reconciled["effect_unknown"]:
                raise AssertionError("reconciliation did not clear effect_unknown")
            _, state = http("GET", LAB + "/_lab/state", timeout=5)
            if state["effects"].get("north", 0) != 1:
                raise AssertionError("reconciliation repeated the effect")
            result = {
                "status": "passed",
                "run_id": rid,
                "effect_count": state["effects"].get("north", 0),
                "actions": [item["id"] for item in unknown],
                "detail": "process killed after the lab applied the effect; restart recovered outcome_unknown without repeating it",
            }
            output = ROOT / "artifacts/session1/crash-recovery.json"
            output.parent.mkdir(parents=True, exist_ok=True)
            output.write_text(json.dumps(result, indent=2))
            print(json.dumps(result, indent=2))
            return 0
        finally:
            restarted.kill()
            restarted.wait(timeout=5)
    finally:
        if process.poll() is None:
            process.kill()
            process.wait(timeout=5)


if __name__ == "__main__":
    raise SystemExit(main())
