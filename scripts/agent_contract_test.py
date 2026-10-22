#!/usr/bin/env python3
"""Contract tests for the agent accounting and cancellation protocol.

Starts a controlled OpenAI-compatible provider and a second agent service on
port 8796. It does not need provider credits and does not replace the real-model
demonstration.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
API_PORT = 8789
API = f"http://127.0.0.1:{API_PORT}/v1"
TOKEN = os.environ.get("DARKBOX_TOKEN", "darkbox-dev-token")
AGENT = "http://127.0.0.1:8796"
PROVIDER_PORT = 9912
LAB = os.environ.get("DARKBOX_LAB_URL", "http://127.0.0.1:8080").rstrip("/")
# Non-zero monetary rates so the accounting protocol is exercised with a real
# cost, not only with token counts.
INPUT_RATE_MICROS_PER_MTOK = 1000
OUTPUT_RATE_MICROS_PER_MTOK = 1000


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
    env["DARKBOX_MODEL_INPUT_MICROS_PER_MTOK"] = str(INPUT_RATE_MICROS_PER_MTOK)
    env["DARKBOX_MODEL_OUTPUT_MICROS_PER_MTOK"] = str(OUTPUT_RATE_MICROS_PER_MTOK)
    return env


ENV = load_env()


def start_api():
    process = subprocess.Popen(
        [
            str(ROOT / "target/debug/darkbox"),
            "serve",
            "--bind",
            f"127.0.0.1:{API_PORT}",
        ],
        cwd=ROOT,
        env=ENV,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    deadline = time.time() + 30
    while time.time() < deadline:
        try:
            status, _ = http("GET", f"http://127.0.0.1:{API_PORT}/health", timeout=2)
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
    raise RuntimeError(
        f"contract API did not start: {output.decode('utf-8', 'replace')[-1200:]}"
    )


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
            return response.status, json.loads(raw) if raw else {}
    except urllib.error.HTTPError as error:
        return error.code, json.loads(error.read() or b"{}")


def call(method, path, body=None, timeout=120):
    return http(method, API + path, body, timeout, TOKEN)


def agent_call(method, path, body=None, timeout=180):
    return http(method, AGENT + path, body, timeout, TOKEN)


class ProviderState:
    counts: dict[str, int] = {}
    lock = threading.Lock()


class Provider(BaseHTTPRequestHandler):
    def log_message(self, *_args):
        return

    def _payload(self):
        length = int(self.headers.get("Content-Length") or 0)
        return json.loads(self.rfile.read(length) or b"{}")

    def _send(self, status, payload):
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _tool(self, name, arguments, usage=None):
        usage = usage or {
            "prompt_tokens": 80,
            "completion_tokens": 20,
            "total_tokens": 100,
        }
        return {
            "id": f"chatcmpl-{time.time_ns()}",
            "object": "chat.completion",
            "model": "contract-model",
            "choices": [
                {
                    "index": 0,
                    "message": {
                        "role": "assistant",
                        "content": None,
                        "tool_calls": [
                            {
                                "id": f"call-{time.time_ns()}",
                                "type": "function",
                                "function": {
                                    "name": name,
                                    "arguments": json.dumps(arguments),
                                },
                            }
                        ],
                    },
                    "finish_reason": "tool_calls",
                }
            ],
            "usage": usage,
        }

    def do_POST(self):
        if self.path != "/v1/chat/completions":
            self._send(404, {"error": "not found"})
            return
        request = self._payload()
        messages = request.get("messages") or []
        objective = " ".join(
            str(message.get("content", ""))
            for message in messages
            if message.get("role") == "user"
        )
        with ProviderState.lock:
            count = ProviderState.counts.get(objective, 0) + 1
            ProviderState.counts[objective] = count
        if "provider failure" in objective:
            self._send(500, {"error": {"message": "controlled provider failure"}})
            return
        if "cancel during model" in objective:
            time.sleep(60)
            self._send(200, self._tool("finish", {"outcome": "inconclusive", "summary": "late"}))
            return
        if "multi-step" in objective and count < 3:
            self._send(200, self._tool("identity_list", {}))
            return
        if "budget exhaustion" in objective:
            self._send(200, self._tool("identity_list", {}))
            return
        if "cost overrun" in objective:
            self._send(
                200,
                self._tool(
                    "identity_list",
                    {},
                    usage={
                        "prompt_tokens": 80,
                        "completion_tokens": 200000,
                        "total_tokens": 200080,
                    },
                ),
            )
            return
        self._send(
            200,
            self._tool(
                "finish",
                {
                    "outcome": "inconclusive",
                    "summary": "contract provider finished without a functional claim",
                    "next_steps": [],
                    "evidence_ids": [],
                },
            ),
        )


def start_provider():
    server = ThreadingHTTPServer(("127.0.0.1", PROVIDER_PORT), Provider)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server


def start_agent():
    env = dict(ENV)
    env.update(
        {
            "DARKBOX_API_URL": f"http://127.0.0.1:{API_PORT}",
            "DARKBOX_AGENT_PORT": "8796",
            "DARKBOX_AGENT_HOST": "127.0.0.1",
            "DARKBOX_AGENT_PLANNER": "",
            "DARKBOX_MODEL_API_KEY": "contract-only",
            "DARKBOX_MODEL_BASE_URL": f"http://127.0.0.1:{PROVIDER_PORT}/v1",
            "DARKBOX_MODEL": "contract-model",
            "DARKBOX_AGENT_MAX_STEPS": "6",
        }
    )
    process = subprocess.Popen(
        ["npx", "tsx", "src/index.ts"],
        cwd=ROOT / "apps/agent",
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    )
    deadline = time.time() + 30
    while time.time() < deadline:
        try:
            status, _ = http("GET", AGENT + "/health", timeout=2)
            if status == 200:
                return process
        except Exception:
            pass
        time.sleep(0.3)
    process.kill()
    raise RuntimeError("contract agent did not start")


def create_assessment(
    name, max_model_calls=20, action_classes=None, max_cost_micros=1_000_000
):
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
        "action_classes": action_classes or ["query", "session"],
        "navigation_dependencies": [],
        "test_targets": [],
        "limits": {
            "max_duration_seconds": 900,
            "max_requests": 100,
            "max_browser_steps": 100,
            "max_model_calls": max_model_calls,
            "max_cost_micros": max_cost_micros,
            "max_artifact_bytes": 10_000_000,
            "max_concurrency_per_origin": 2,
        },
        "retain_raw": False,
    }
    status, assessment = call(
        "POST",
        "/assessments",
        {
            "company_id": company_id,
            "name": name,
            "environment": "lab",
            "objective": "contract accounting",
            "initial_targets": [LAB + "/"],
            "policy": policy,
        },
    )
    aid = assessment["id"]
    call("POST", f"/assessments/{aid}/tenants", {"key": "north", "label": "north"})
    status, identity = call(
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
    )
    return aid, identity["id"]


def turn(aid, identity, objective, wait=True, max_steps=4):
    return agent_call(
        "POST",
        "/turns",
        {
            "assessment_id": aid,
            "objective": objective,
            "identity_id": identity,
            "max_steps": max_steps,
            "token_budget": 3000,
            "start_url": LAB + "/",
            "wait": wait,
        },
    )


def wait_run(run_id, timeout=120):
    deadline = time.time() + timeout
    while time.time() < deadline:
        _, detail = call("GET", f"/runs/{run_id}")
        if detail["run"]["state"] in (
            "succeeded",
            "failed",
            "cancelled",
            "waiting_operator",
            "interrupted",
        ):
            return detail
        time.sleep(0.4)
    raise TimeoutError(run_id)


def event_types(detail):
    return [event["event_type"] for event in detail["events"]]


def main():
    api_process = start_api()
    provider = start_provider()
    agent = start_agent()
    rows = []
    try:
        aid, identity = create_assessment("Contract accounting")

        def finish_settles():
            _, result = turn(aid, identity, "contract finish")
            run_id = result.get("run_id") or result.get("runId")
            detail = wait_run(run_id)
            events = event_types(detail)
            settles = [
                event
                for event in detail["events"]
                if event["event_type"] == "agent.usage"
                and event["payload"].get("phase") == "settle"
            ]
            if not settles:
                raise AssertionError("no settle event")
            settle_event_id = settles[0]["event_id"]
            settle_index = next(
                index
                for index, event in enumerate(detail["events"])
                if event["event_id"] == settle_event_id
            )
            terminal = next(
                (
                    index
                    for index, event in enumerate(events)
                    if event in ("agent.finished", "agent.failed", "agent.cancelled")
                ),
                -1,
            )
            if terminal < 0 or settle_index > terminal:
                raise AssertionError("settle was not recorded before finish")
            if detail["run"]["spent"]["model_calls"] != 1:
                raise AssertionError("model calls not exactly one")
            return (
                "finish settled the completed call before closing the run",
                [run_id],
            )

        rows.append(("ACCOUNT-01", "Usage settles before finish", finish_settles()))

        def multi_step():
            _, result = turn(aid, identity, "contract multi-step", max_steps=5)
            run_id = result.get("run_id") or result.get("runId")
            detail = wait_run(run_id)
            settles = [
                event
                for event in detail["events"]
                if event["event_type"] == "agent.usage"
                and event["payload"].get("phase") == "settle"
            ]
            if len(settles) < 3:
                raise AssertionError(f"expected three settled calls, got {len(settles)}")
            if detail["run"]["spent"]["model_calls"] != len(settles):
                raise AssertionError("model calls differ from settled calls")
            return (
                f"three model steps produced three settled usage records before completion",
                [run_id],
            )

        rows.append(("ACCOUNT-02", "Each completed call settles before the next", multi_step()))

        def duplicate_settle():
            _, created = call(
                "POST",
                f"/assessments/{aid}/agent/turns",
                {
                    "objective": "duplicate settle",
                    "identity_id": identity,
                    "max_steps": 2,
                    "token_budget": 2000,
                    "start_url": LAB + "/",
                    "model": "contract-model",
                },
            )
            rid = created["run"]["id"]
            call_id = f"{rid}-manual"
            call(
                "POST",
                f"/runs/{rid}/agent/usage",
                {
                    "phase": "reserve",
                    "call_id": call_id,
                    "model": "contract-model",
                    "estimated_input_tokens": 50,
                    "estimated_output_tokens": 20,
                },
            )
            body = {
                "phase": "settle",
                "call_id": call_id,
                "model": "contract-model",
                "input_tokens": 80,
                "output_tokens": 20,
                "total_tokens": 100,
                "usage_known": True,
            }
            call("POST", f"/runs/{rid}/agent/usage", body)
            call("POST", f"/runs/{rid}/agent/usage", body)
            detail = api_detail = call("GET", f"/runs/{rid}")[1]
            settles = [
                event
                for event in detail["events"]
                if event["event_type"] == "agent.usage"
                and event["payload"].get("phase") == "settle"
            ]
            if detail["run"]["spent"]["model_calls"] != 1 or len(settles) != 1:
                raise AssertionError("duplicate settlement changed accounting")
            call("POST", f"/runs/{rid}/cancel", {"reason": "contract test"})
            return "duplicate settle was idempotent", [rid]

        rows.append(("ACCOUNT-03", "Duplicate settlement is idempotent", duplicate_settle()))

        def provider_failure():
            _, result = turn(aid, identity, "contract provider failure", max_steps=2)
            run_id = result.get("run_id") or result.get("runId")
            detail = wait_run(run_id)
            if detail["run"]["state"] != "failed":
                raise AssertionError(f"provider failure run is {detail['run']['state']}")
            settles = [
                event
                for event in detail["events"]
                if event["event_type"] == "agent.usage"
                and event["payload"].get("phase") == "settle"
            ]
            if not settles:
                raise AssertionError("provider failure left no accounting record")
            return "provider failure is visible and the reserved call is settled unknown", [
                run_id
            ]

        rows.append(("ACCOUNT-04", "Provider failure remains visible", provider_failure()))

        def cancel_model():
            _, accepted = turn(aid, identity, "contract cancel during model", wait=False, max_steps=2)
            rid = accepted.get("run_id") or accepted.get("runId")
            time.sleep(1)
            call("POST", f"/runs/{rid}/cancel", {"reason": "contract cancellation"})
            detail = wait_run(rid)
            if detail["run"]["state"] != "cancelled":
                raise AssertionError(f"run is {detail['run']['state']}")
            deadline = time.time() + 10
            settle_seen = False
            while time.time() < deadline:
                detail = call("GET", f"/runs/{rid}")[1]
                settle_seen = any(
                    event["event_type"] == "agent.usage"
                    and event["payload"].get("phase") == "settle"
                    for event in detail["events"]
                )
                if settle_seen:
                    break
                time.sleep(0.25)
            if not settle_seen:
                raise AssertionError("cancelled model call was not settled")
            return "cancellation interrupted the model call and settled it", [rid]

        rows.append(("ACCOUNT-05", "Cancellation settles the model call", cancel_model()))

        def budget():
            budget_aid, budget_identity = create_assessment(
                "Contract model budget", max_model_calls=1
            )
            _, result = turn(
                budget_aid, budget_identity, "contract budget exhaustion", max_steps=4
            )
            run_id = result.get("run_id") or result.get("runId")
            detail = wait_run(run_id)
            if detail["run"]["state"] not in ("failed", "cancelled"):
                raise AssertionError(f"budget run is {detail['run']['state']}")
            if detail["run"]["spent"]["model_calls"] > 1:
                raise AssertionError("model-call budget was exceeded")
            return "the second model call was refused by the aggregate budget", [
                run_id
            ]

        rows.append(("ACCOUNT-06", "Model budget blocks another call", budget()))

        def cost_overrun():
            over_aid, over_identity = create_assessment(
                "Contract cost overrun", max_model_calls=10, max_cost_micros=10
            )
            _, result = turn(
                over_aid, over_identity, "contract cost overrun", max_steps=4
            )
            run_id = result.get("run_id") or result.get("runId")
            detail = wait_run(run_id)
            spent = detail["run"]["spent"]
            if detail["run"]["state"] not in ("failed", "cancelled"):
                raise AssertionError(f"overrun run is {detail['run']['state']}")
            if not detail["run"]["budget_blocked"]:
                raise AssertionError("run was not blocked after the overrun")
            if spent["overage_micros"] <= 0:
                raise AssertionError("the overage was not recorded")
            if spent["cost_micros"] != 10:
                raise AssertionError(
                    f"spent cost is {spent['cost_micros']}, expected the budget cap"
                )
            progress = call("GET", f"/runs/{run_id}/browser/progress")[1]
            calls = progress.get("model_calls", [])
            if not calls:
                raise AssertionError("no model call was persisted")
            recorded = calls[0]
            if (recorded.get("output_tokens") or 0) < 200000:
                raise AssertionError("the reported token usage was not persisted")
            if (recorded.get("cost_micros") or 0) < 200:
                raise AssertionError("the reported cost was not persisted")
            if (recorded.get("overage_micros") or 0) <= 0:
                raise AssertionError("the per-call overage was not persisted")
            return (
                "a real cost above the budget was persisted, the excess recorded and new calls blocked",
                [run_id],
            )

        rows.append(
            ("ACCOUNT-07", "Cost overrun is persisted and blocks calls", cost_overrun())
        )

        report = {
            "generated_at": datetime.now(timezone.utc).isoformat(),
            "criteria": [
                {
                    "id": key,
                    "title": title,
                    "status": "passed",
                    "detail": detail,
                    "evidence": evidence,
                }
                for key, title, (detail, evidence) in rows
            ],
        }
        output = ROOT / "artifacts/session1/agent-contract.json"
        output.parent.mkdir(parents=True, exist_ok=True)
        output.write_text(json.dumps(report, indent=2))
        print(json.dumps(report, indent=2))
        return 0
    except Exception as error:
        print(f"FAILED: {type(error).__name__}: {error}", file=sys.stderr)
        return 1
    finally:
        provider.shutdown()
        agent.terminate()
        try:
            agent.wait(timeout=5)
        except subprocess.TimeoutExpired:
            agent.kill()
        api_process.terminate()
        try:
            api_process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            api_process.kill()


if __name__ == "__main__":
    sys.exit(main())
