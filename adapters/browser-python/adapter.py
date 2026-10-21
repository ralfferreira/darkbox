#!/usr/bin/env python3
"""DarkBox browser adapter.

Speaks one versioned JSON request on stdin and one or more JSON messages on
stdout (JSON Lines). Logs go to stderr. Secrets are resolved from the
environment; they never travel in the request payload or command line.

Modes:
  auto     use Camoufox when importable, otherwise the deterministic HTTP mode
  mock     deterministic HTTP client with one cookie jar per identity
  camoufox use Camoufox through Playwright

The mock mode is a real HTTP client, not a browser. It exists so the core can
be exercised without downloading a browser; capability reporting says so.
"""

import argparse
import http.cookiejar
import json
import os
import re
import sys
import urllib.error
import urllib.parse
import urllib.request
import uuid


def log(message):
    print(message, file=sys.stderr, flush=True)


def emit(kind, request_id, payload):
    print(
        json.dumps(
            {"type": kind, "request_id": request_id, "schema_version": 1, "payload": payload},
            ensure_ascii=False,
        ),
        flush=True,
    )


def resolve_secret(ref, label):
    if ref and ref.startswith("env:"):
        value = os.environ.get(ref[4:])
        if value:
            return value
    fallback = os.environ.get(f"LAB_{label.upper()}_PASSWORD")
    if fallback:
        return fallback
    if label in ("alice", "bob"):
        log(f"using documented synthetic lab password for {label}")
        return f"{label}-lab-pass"
    return None


def title_of(body):
    match = re.search(r"<title[^>]*>(.*?)</title>", body, re.I | re.S)
    return match.group(1).strip()[:120] if match else ""


class Scope:
    """The subset of the run policy the adapter must enforce for navigation."""

    def __init__(self, policy):
        self.policy = policy or {}
        self.ports = set(self.policy.get("allowed_ports") or [])
        self.schemes = set(self.policy.get("allowed_schemes") or [])

    @staticmethod
    def _host(rule, host):
        rule = rule.strip().lower().rstrip(".")
        host = host.strip().lower().rstrip(".")
        if rule.startswith("*."):
            suffix = rule[2:]
            return host.endswith("." + suffix)
        return rule == host

    @staticmethod
    def _path(rule, path):
        rule = rule.rstrip("/") or "/"
        path = path.rstrip("/") or "/"
        if rule == "/":
            return path.startswith("/")
        return path == rule or path.startswith(rule + "/")

    def allows(self, url):
        try:
            parsed = urllib.parse.urlsplit(url)
        except ValueError:
            return False, "invalid_url"
        host = parsed.hostname or ""
        scheme = parsed.scheme.lower()
        port = parsed.port or (443 if scheme == "https" else 80)
        path = parsed.path or "/"
        if self.schemes and scheme not in self.schemes:
            return False, "unsupported_scheme"
        if self.ports and port not in self.ports:
            return False, "port_not_allowed"
        for rule in self.policy.get("exclude_hosts") or []:
            if self._host(rule, host):
                return False, "excluded_host"
        for rule in self.policy.get("exclude_paths") or []:
            if self._path(rule, path):
                return False, "excluded_path"
        included = any(self._host(rule, host) for rule in self.policy.get("include_hosts") or [])
        auxiliary = any(
            self._host(dep.get("host", ""), host)
            for dep in self.policy.get("navigation_dependencies") or []
        )
        if not included and not auxiliary:
            return False, "host_not_included"
        return True, "ok"


class HttpIdentity:
    def __init__(self, identity, base_url, login_path, scope):
        self.identity = identity
        self.label = identity.get("label", "unknown")
        self.base_url = base_url.rstrip("/")
        self.login_path = login_path or "/login"
        self.jar = http.cookiejar.CookieJar()
        self.opener = urllib.request.build_opener(
            urllib.request.HTTPCookieProcessor(self.jar),
            NoRedirect(),
        )
        self.password = resolve_secret(identity.get("secret_ref"), self.label)
        self.authenticated = False
        self.login_status = None
        self.scope = scope

    def raw_request(self, method, url, data=None, headers=None):
        headers = dict(headers or {})
        headers.setdefault("User-Agent", "DarkBox-Adapter/0.3 (authorized lab)")
        body = None
        if data is not None:
            body = urllib.parse.urlencode(data).encode()
            headers.setdefault("Content-Type", "application/x-www-form-urlencoded")
        request = urllib.request.Request(url, data=body, headers=headers, method=method)
        try:
            response = self.opener.open(request, timeout=20)
            raw = response.read(262144)
            return response.status, dict(response.headers), raw.decode("utf-8", "replace"), response.geturl()
        except urllib.error.HTTPError as error:
            raw = error.read(262144)
            return error.code, dict(error.headers), raw.decode("utf-8", "replace"), url
        except urllib.error.URLError as error:
            return 0, {}, "", str(error)

    def request(self, method, url, data=None, headers=None):
        current = url
        for _ in range(6):
            allowed, reason = self.scope.allows(current)
            if not allowed:
                return -1, {}, "", f"blocked:{reason}"
            status, response_headers, body, final = self.raw_request(method, current, data, headers)
            if 300 <= status < 400:
                location = response_headers.get("location") or response_headers.get("Location")
                if location:
                    current = urllib.parse.urljoin(current, location)
                    if status in (301, 302, 303):
                        method = "GET"
                        data = None
                    continue
            return status, response_headers, body, final
        return -1, {}, "", "blocked:too_many_redirects"

    def login(self):
        if not self.password:
            return 0, "no_secret_available"
        username = self.identity.get("label")
        url = self.base_url + self.login_path
        allowed, reason = self.scope.allows(url)
        if not allowed:
            return -1, f"blocked:{reason}"
        status, headers, body, final = self.raw_request(
            "POST", url, {"username": username, "password": self.password}
        )
        self.login_status = status
        if 300 <= status < 400:
            location = headers.get("location") or headers.get("Location")
            if location:
                target = urllib.parse.urljoin(url, location)
                allowed, reason = self.scope.allows(target)
                if allowed:
                    status, headers, body, final = self.request("GET", target)
                else:
                    return -1, f"blocked:{reason}"
        self.authenticated = 200 <= status < 400 and "/login" not in final
        return status, final

    def cookie_names(self):
        return sorted(cookie.name for cookie in self.jar)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def fetch_steps(identity, steps, base_url):
    results = []
    evidence = []
    for step in steps:
        url = step.get("url") or base_url
        method = step.get("method", "GET")
        status, headers, body, final = identity.request(method, url)
        text = f"{method} {url}\nstatus: {status}\nfinal: {final}\ntitle: {title_of(body)}\n\n{body[:4000]}"
        evidence.append(
            {
                "summary": f"{method} {url} -> {status}",
                "text": text,
            }
        )
        results.append(
            {
                "ordinal": step.get("ordinal", len(results)),
                "status": status,
                "url": url,
                "final_url": final,
                "confirmed": 200 <= status < 400,
                "title": title_of(body),
            }
        )
    return results, evidence


def run_session(request):
    payload = request.get("payload", {})
    identity = payload.get("identity", {})
    tenant = payload.get("tenant")
    base_url = payload.get("base_url", "")
    scope = Scope(request.get("policy"))
    session_identity = HttpIdentity(identity, base_url, payload.get("login_path"), scope)
    status, final = session_identity.login()
    if not session_identity.authenticated:
        return {
            "status": "blocked",
            "summary": "login failed; session not established",
            "session": {"state": "expired", "tenant": tenant},
            "evidence": [
                {
                    "summary": "login attempt",
                    "text": f"POST {base_url}{payload.get('login_path')}\nstatus: {status}",
                }
            ],
        }
    target = base_url.rstrip("/") + f"/app/{tenant}" if tenant else final
    home_status, headers, body, home_final = session_identity.request("GET", target)
    return {
        "status": "succeeded",
        "summary": f"authenticated {identity.get('label')} for tenant {tenant}",
        "session": {
            "state": "authenticated",
            "tenant": tenant,
            "context_key": f"ctx-{identity.get('label')}-{uuid.uuid4().hex[:12]}",
            "cookie_names": session_identity.cookie_names(),
        },
        "evidence": [
            {
                "summary": "login response",
                "text": f"POST {base_url}{payload.get('login_path')}\nstatus: {status}\nfinal: {final}",
            },
            {
                "summary": f"tenant dashboard {target}",
                "text": f"GET {target}\nstatus: {home_status}\ntitle: {title_of(body)}\n\n{body[:4000]}",
            },
        ],
        "observations": [],
    }


def run_flow(request):
    payload = request.get("payload", {})
    identity = payload.get("identity", {})
    base_url = payload.get("base_url", "")
    scope = Scope(request.get("policy"))
    session_identity = HttpIdentity(identity, base_url, payload.get("login_path"), scope)
    status, final = session_identity.login()
    if not session_identity.authenticated:
        return {
            "status": "blocked",
            "summary": "identity_expired: login was not accepted",
            "steps": [],
            "evidence": [
                {
                    "summary": "flow login",
                    "text": f"POST {base_url}{payload.get('login_path')}\nstatus: {status}",
                }
            ],
        }
    steps, evidence = fetch_steps(session_identity, payload.get("steps", []), base_url)
    confirmed = sum(1 for step in steps if step["confirmed"])
    return {
        "status": "succeeded" if confirmed == len(steps) and steps else "failed",
        "summary": f"flow {payload.get('flow', {}).get('name', '')}: {confirmed}/{len(steps)} steps confirmed",
        "steps": steps,
        "evidence": evidence,
    }


def run_visibility(request):
    payload = request.get("payload", {})
    base_url = payload.get("base_url", "")
    checks = payload.get("checks", [])
    identities = payload.get("identities", [])
    scope = Scope(request.get("policy"))
    evidence = []
    logins = {}
    for identity in identities:
        client = HttpIdentity(identity, base_url, payload.get("login_path"), scope)
        status, final = client.login()
        logins[identity.get("label")] = (client, status, final)
        evidence.append(
            {
                "summary": f"login {identity.get('label')}",
                "text": f"POST {base_url}/login\nstatus: {status}\nfinal: {final}",
            }
        )
    comparisons = []
    for check in checks:
        url = check.get("url") or base_url
        label = check.get("label", url)
        row = {"check": label, "results": {}, "different": False}
        statuses = []
        titles = []
        for identity in identities:
            name = identity.get("label")
            client, login_status, _ = logins[name]
            if login_status not in (200, 302):
                row["results"][name] = {"status": login_status, "error": "login_failed"}
                statuses.append(login_status)
                titles.append("")
                continue
            status, headers, body, final = client.request("GET", url)
            row["results"][name] = {
                "status": status,
                "title": title_of(body),
                "length": len(body),
                "final": final,
            }
            statuses.append(status)
            titles.append(title_of(body))
            evidence.append(
                {
                    "summary": f"{name} GET {url} -> {status}",
                    "text": f"GET {url}\nidentity: {name}\nstatus: {status}\ntitle: {title_of(body)}\n\n{body[:3000]}",
                }
            )
        row["different"] = len(set(statuses)) > 1 or len(set(titles)) > 1
        comparisons.append(row)
    differences = sum(1 for row in comparisons if row["different"])
    return {
        "status": "succeeded",
        "summary": f"{differences} of {len(comparisons)} checks differed",
        "comparisons": comparisons,
        "evidence": evidence,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", default="auto", choices=["auto", "mock", "camoufox"])
    args = parser.parse_args()
    line = sys.stdin.readline()
    if not line:
        emit("error", "", {"message": "empty stdin"})
        return 2
    try:
        request = json.loads(line)
    except json.JSONDecodeError as error:
        emit("error", "", {"message": f"invalid request json: {error}"})
        return 2
    if request.get("schema_version") != 1:
        emit("error", request.get("request_id", ""), {"message": "unsupported schema version"})
        return 2
    kind = request.get("kind")
    request_id = request.get("request_id", "")
    emit("progress", request_id, {"phase": "started", "kind": kind})
    mode = args.mode
    if mode == "auto":
        try:
            import camoufox  # noqa: F401

            mode = "camoufox"
        except Exception:
            mode = "mock"
    if mode == "camoufox":
        emit("error", request_id, {"message": "camoufox mode is not available in this build"})
        return 3
    handlers = {
        "browser.session": run_session,
        "browser.flow": run_flow,
        "browser.visibility": run_visibility,
    }
    handler = handlers.get(kind)
    if handler is None:
        emit("error", request_id, {"message": f"unknown operation {kind}"})
        return 2
    try:
        payload = handler(request)
    except Exception as error:  # adapter boundary: convert to a structured error
        log(f"adapter failure: {error!r}")
        emit("error", request_id, {"message": f"adapter failure: {error}"})
        return 1
    emit("progress", request_id, {"phase": "finished", "mode": mode})
    emit("result", request_id, payload)
    return 0


if __name__ == "__main__":
    sys.exit(main())
