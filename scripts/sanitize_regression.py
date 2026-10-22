#!/usr/bin/env python3
"""Regression tests for the adapter-side secret projection.

These run without a browser. They cover the two things the adapter must do
before an observation reaches the core or the model: drop values of fields it
identified as sensitive, and drop secrets it resolved itself. The URL-parameter
case is shared with the core sanitizer.
"""

from __future__ import annotations

import importlib.util
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ADAPTER = ROOT / "adapters/browser-python/adapter.py"


def load_adapter():
    spec = importlib.util.spec_from_file_location("darkbox_adapter", ADAPTER)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def main() -> int:
    adapter = load_adapter()
    sentinel = "93ad8c22d8b1316813ec"
    checks = []

    # 1. A sensitive field value is dropped even with no recognizable prefix.
    observation = {
        "interactive": [
            {
                "ref": "e5",
                "type": "text",
                "field_name": "token",
                "sensitive": True,
                "value": sentinel,
            }
        ]
    }
    redacted = adapter.redact_observation(observation, set())
    checks.append(
        (
            "sensitive field value",
            redacted["interactive"][0]["value"] == "[redacted]"
            and redacted["interactive"][0]["ref"] == "e5"
            and sentinel not in json.dumps(redacted),
        )
    )

    # 2. A sensitive URL parameter is redacted, the rest of the URL survives.
    url = adapter.redact_url_query(f"http://lab/app?access_token={sentinel}&period=2026-08")
    checks.append(
        (
            "sensitive URL parameter",
            sentinel not in url and "period=2026-08" in url and "access_token=[redacted]" in url,
        )
    )

    # 3. A secret the adapter resolved itself is dropped wherever it appears.
    known = adapter.redact_observation(
        {"text": f"echo {sentinel} here", "title": "Reports"},
        {sentinel},
    )
    checks.append(
        (
            "known secret value",
            sentinel not in known["text"] and known["title"] == "Reports",
        )
    )

    # 4. Regression: the previous projection only replaced known secrets and
    #    left sensitive field values and URL parameters in place.

    def previous_projection(value, known):
        if isinstance(value, str):
            for secret in known:
                value = value.replace(secret, "[redacted]")
            return value
        if isinstance(value, list):
            return [previous_projection(child, known) for child in value]
        if isinstance(value, dict):
            return {key: previous_projection(child, known) for key, child in value.items()}
        return value

    previous = previous_projection(
        {"interactive": observation["interactive"], "url": f"http://lab?access_token={sentinel}"},
        set(),
    )
    checks.append(
        (
            "previous projection reproduced the leak",
            sentinel in json.dumps(previous),
        )
    )

    failed = [name for name, ok in checks if not ok]
    report = {
        "status": "passed" if not failed else "failed",
        "checks": [{"name": name, "ok": ok} for name, ok in checks],
    }
    output = ROOT / "artifacts/session1/sanitize-regression.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2))
    print(json.dumps(report, indent=2))
    return 0 if not failed else 1


if __name__ == "__main__":
    sys.exit(main())
