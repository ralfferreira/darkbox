#!/usr/bin/env python3
"""DarkBox browser adapter.

Two entry points share one policy and one observation model.

``--serve`` runs a long-lived browser host. The Rust supervisor creates one
isolated browser context per assessment identity and tenant, then sends
semantic commands to it over a small JSON HTTP API. Cookies, storage and the
page survive between commands. Contexts never share a cookie jar.

One-shot mode (one JSON line on stdin, JSON Lines on stdout) is kept for the
legacy ``browser.session``/``browser.flow`` contract and its deterministic
tests. It is labelled as an HTTP client when the real engine is not used.

Engines are reported, never guessed. ``camoufox`` is used only when its Python
package imports and its patched Firefox build starts. Otherwise the adapter
reports the exact import or launch error and falls back to Playwright Chromium
only in ``auto`` mode.
"""

import argparse
import base64
import glob
import http.cookiejar
import json
import os
import queue
import re
import shutil
import sys
import threading
import time
import traceback
import urllib.error
import urllib.parse
import urllib.request
import uuid
from collections import deque
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

SCHEMA_VERSION = 1
MAX_BODY_BYTES = 2 * 1024 * 1024
MAX_TEXT_CHARS = 18_000
MAX_TRAFFIC = 200
MAX_SCREENSHOT_WIDTH = 1280
DEFAULT_ACTION_TIMEOUT_MS = 12_000


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


def clipped(value, limit=500):
    text = "" if value is None else str(value)
    return text[:limit]


def resolve_secret(ref):
    if not ref:
        return None
    if ref.startswith("env:"):
        return os.environ.get(ref[4:]) or None
    if ref.startswith("file:"):
        try:
            with open(ref[5:], "r", encoding="utf-8") as handle:
                return handle.read().strip() or None
        except OSError:
            return None
    return None


def title_of(body):
    match = re.search(r"<title[^>]*>(.*?)</title>", body, re.I | re.S)
    return match.group(1).strip()[:120] if match else ""


class Scope:
    """The subset of the run policy the browser must enforce for every request."""

    def __init__(self, policy):
        self.policy = policy or {}
        self.ports = set(self.policy.get("allowed_ports") or [])
        self.schemes = set(self.policy.get("allowed_schemes") or [])
        self.include_paths = list(self.policy.get("include_paths") or [])

    @staticmethod
    def _host(rule, host):
        rule = (rule or "").strip().lower().rstrip(".")
        host = (host or "").strip().lower().rstrip(".")
        if rule.startswith("*."):
            return host.endswith("." + rule[2:])
        return rule == host

    @staticmethod
    def _path(rule, path):
        rule = rule.rstrip("/") or "/"
        path = path.rstrip("/") or "/"
        if rule == "/":
            return True
        return path == rule or path.startswith(rule + "/")

    def allows(self, url):
        try:
            parsed = urllib.parse.urlsplit(url)
        except ValueError:
            return False, "invalid_url"
        scheme = (parsed.scheme or "").lower()
        if scheme in ("data", "blob", "about"):
            return True, "ok"
        if scheme not in ("http", "https"):
            return False, "unsupported_scheme"
        host = parsed.hostname or ""
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
        if self.include_paths and not any(self._path(rule, path) for rule in self.include_paths):
            return False, "path_not_included"
        included = any(self._host(rule, host) for rule in self.policy.get("include_hosts") or [])
        auxiliary = any(
            self._host(dep.get("host", ""), host)
            for dep in (self.policy.get("navigation_dependencies") or [])
        )
        if not included and not auxiliary:
            return False, "host_not_included"
        return True, "ok"


OBSERVE_JS = r"""
(maxText) => {
  let next = 1;
  const used = new Set();
  document.querySelectorAll('[data-darkbox-ref]').forEach((el) => {
    const ref = el.getAttribute('data-darkbox-ref');
    if (ref) used.add(ref);
  });
  const assign = (el) => {
    let ref = el.getAttribute('data-darkbox-ref');
    if (!ref) {
      do { ref = 'e' + next++; } while (used.has(ref));
      used.add(ref);
      el.setAttribute('data-darkbox-ref', ref);
    }
    return ref;
  };
  const visible = (el) => {
    const style = window.getComputedStyle(el);
    if (style.display === 'none' || style.visibility === 'hidden' || Number(style.opacity) === 0) {
      return false;
    }
    const rect = el.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  };
  const label = (el) => {
    const bits = [];
    const aria = el.getAttribute('aria-label');
    if (aria) bits.push(aria);
    const labelled = el.getAttribute('aria-labelledby');
    if (labelled) {
      for (const id of labelled.split(/\s+/)) {
        const node = document.getElementById(id);
        if (node && node.textContent) bits.push(node.textContent.trim());
      }
    }
    if (el.id) {
      const forLabel = document.querySelector('label[for="' + CSS.escape(el.id) + '"]');
      if (forLabel && forLabel.textContent) bits.push(forLabel.textContent.trim());
    }
    const wrapping = el.closest('label');
    if (wrapping && wrapping.textContent) bits.push(wrapping.textContent.trim());
    const placeholder = el.getAttribute('placeholder');
    if (placeholder) bits.push(placeholder);
    const name = el.getAttribute('name');
    if (name) bits.push(name);
    const title = el.getAttribute('title');
    if (title) bits.push(title);
    const own = (el.innerText || el.textContent || '').trim();
    if (own) bits.push(own);
    return [...new Set(bits.map((b) => b.replace(/\s+/g, ' ').trim()).filter(Boolean))]
      .join(' · ')
      .slice(0, 220);
  };
  const selector = [
    'a[href]', 'button', 'input:not([type="hidden"])', 'select', 'textarea',
    '[role="button"]', '[role="link"]', '[role="tab"]', '[role="menuitem"]',
    '[contenteditable="true"]', '[onclick]'
  ].join(',');
  const interactive = [];
  for (const el of Array.from(document.querySelectorAll(selector))) {
    if (interactive.length >= 160) break;
    const tag = el.tagName.toLowerCase();
    const type = (el.getAttribute('type') || '').toLowerCase();
    if (tag === 'input' && type === 'hidden') continue;
    const rect = el.getBoundingClientRect();
    const entry = {
      ref: assign(el),
      tag,
      type: type || null,
      role: el.getAttribute('role') || null,
      name: label(el),
      href: el.getAttribute('href') ? el.href : null,
      form_action: el.form ? el.form.action : null,
      form_method: el.form ? (el.form.method || 'get').toLowerCase() : null,
      disabled: Boolean(el.disabled) || el.getAttribute('aria-disabled') === 'true',
      visible: visible(el),
      required: Boolean(el.required),
      checked: typeof el.checked === 'boolean' ? el.checked : null,
      selected: el.selectedIndex !== undefined ? el.selectedIndex : null,
      bounds: [Math.round(rect.x), Math.round(rect.y), Math.round(rect.width), Math.round(rect.height)],
    };
    if (tag === 'select') {
      entry.options = Array.from(el.options).slice(0, 30).map((option) => ({
        value: option.value, label: (option.textContent || '').trim().slice(0, 80), selected: option.selected
      }));
    }
    if (tag === 'input' && type !== 'password') {
      entry.value = el.value ? String(el.value).slice(0, 200) : '';
    }
    if (tag === 'textarea') entry.value = (el.value || '').slice(0, 200);
    if (tag === 'a') entry.text = (el.innerText || el.textContent || '').trim().slice(0, 160);
    interactive.push(entry);
  }
  const forms = Array.from(document.forms).slice(0, 12).map((form, index) => ({
    index,
    id: form.id || null,
    action: form.action,
    method: (form.method || 'get').toLowerCase(),
    fields: Array.from(form.elements)
      .filter((el) => el.tagName && el.getAttribute('type') !== 'hidden')
      .slice(0, 40)
      .map((el) => ({
        ref: assign(el),
        tag: el.tagName.toLowerCase(),
        type: (el.getAttribute('type') || '').toLowerCase() || null,
        name: el.getAttribute('name'),
        label: label(el),
      })),
  }));
  const headings = Array.from(document.querySelectorAll('h1,h2,h3'))
    .slice(0, 30)
    .map((el) => (el.innerText || el.textContent || '').trim().slice(0, 180))
    .filter(Boolean);
  const tables = Array.from(document.querySelectorAll('table')).slice(0, 8).map((table) => ({
    headers: Array.from(table.querySelectorAll('thead th')).map((th) => (th.innerText || '').trim().slice(0, 80)),
    rows: Array.from(table.querySelectorAll('tbody tr')).slice(0, 60).map((tr) =>
      Array.from(tr.querySelectorAll('td')).map((td) => (td.innerText || '').trim().slice(0, 160))
    ),
  }));
  const bodyText = (document.body ? document.body.innerText : '') || '';
  const passwordFields = Array.from(document.querySelectorAll('input[type="password"]'))
    .filter(visible).length;
  const logout = Array.from(document.querySelectorAll('a,button')).some((el) => {
    const text = ((el.innerText || el.textContent || '') + ' ' + (el.getAttribute('aria-label') || '')).toLowerCase();
    return /log ?out|sign ?out|sair/.test(text);
  });
  const errorText = Array.from(document.querySelectorAll(
    '[role="alert"], .error, .form-error, .alert, [data-error]'
  )).map((el) => (el.innerText || '').trim()).filter(Boolean).join(' \n ').slice(0, 800);
  return {
    url: location.href,
    title: document.title,
    ready_state: document.readyState,
    headings,
    interactive,
    forms,
    tables,
    text: bodyText.replace(/\s+\n/g, '\n').slice(0, maxText),
    text_truncated: bodyText.length > maxText,
    auth: {
      password_fields: passwordFields,
      login_form: passwordFields > 0,
      logout_visible: logout,
      likely_authenticated: passwordFields === 0 && logout,
      error_text: errorText,
    },
    viewport: { width: window.innerWidth, height: window.innerHeight },
    collected_at: new Date().toISOString(),
  };
}
"""


class BrowserEngineError(RuntimeError):
    def __init__(self, message, detail=None):
        super().__init__(message)
        self.detail = detail or {}


class PlaywrightEngine:
    name = "playwright-chromium"

    def __init__(self, payload=None):
        self.playwright = None
        self.browser = None
        self.executable = None
        self.mode = "playwright"

    def _candidate_executables(self):
        requested = os.environ.get("DARKBOX_CHROMIUM_PATH")
        if requested:
            return [requested]
        candidates = []
        try:
            from playwright.sync_api import sync_playwright

            with sync_playwright() as probe:
                candidates.append(probe.chromium.executable_path)
        except Exception:
            pass
        patterns = [
            os.path.expanduser("~/Library/Caches/ms-playwright/chromium-*/chrome-mac*/*.app/Contents/MacOS/*"),
            os.path.expanduser("~/.cache/ms-playwright/chromium-*/chrome-linux*/chrome"),
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            "/usr/bin/chromium",
            "/usr/bin/chromium-browser",
            "/usr/bin/google-chrome",
        ]
        for pattern in patterns:
            candidates.extend(sorted(glob.glob(pattern), reverse=True))
        return [path for path in candidates if path and os.path.exists(path)]

    def start(self):
        from playwright.sync_api import sync_playwright

        self.playwright = sync_playwright().start()
        errors = []
        for executable in [None] + self._candidate_executables():
            try:
                kwargs = {"headless": True}
                if executable:
                    kwargs["executable_path"] = executable
                self.browser = self.playwright.chromium.launch(**kwargs)
                self.executable = executable or self.playwright.chromium.executable_path
                break
            except Exception as error:
                errors.append(f"{executable or 'bundled'}: {error}")
        if self.browser is None:
            raise BrowserEngineError("playwright chromium did not start", {"attempts": errors[:5]})
        return self

    def version(self):
        if self.browser is not None:
            return self.browser.version
        return "unknown"

    def close(self):
        try:
            if self.browser is not None:
                self.browser.close()
        finally:
            self.browser = None
            if self.playwright is not None:
                self.playwright.stop()
                self.playwright = None


class CamoufoxEngine(PlaywrightEngine):
    name = "camoufox"

    def __init__(self, payload=None):
        super().__init__(payload)
        self.camoufox = None
        self.mode = "camoufox"

    def start(self):
        try:
            from camoufox.sync_api import Camoufox
        except Exception as error:
            raise BrowserEngineError("camoufox is not importable", {"error": repr(error)})
        try:
            self.camoufox = Camoufox(headless=True)
            self.browser = self.camoufox.__enter__()
        except Exception as error:
            raise BrowserEngineError("camoufox did not start", {"error": repr(error)})
        return self

    def close(self):
        try:
            if self.camoufox is not None:
                self.camoufox.__exit__(None, None, None)
        finally:
            self.camoufox = None
            self.browser = None


class BrowserSession:
    def __init__(self, key, engine, scope, payload):
        self.key = key
        self.engine = engine
        self.scope = scope
        self.identity = payload.get("identity") or {}
        self.tenant = payload.get("tenant")
        self.context = engine.browser.new_context(
            viewport={"width": 1280, "height": 900},
            ignore_https_errors=False,
            locale="en-US",
        )
        self.page = self.context.new_page()
        self.page.set_default_timeout(DEFAULT_ACTION_TIMEOUT_MS)
        self.page.set_default_navigation_timeout(DEFAULT_ACTION_TIMEOUT_MS + 3000)
        self.traffic = deque(maxlen=MAX_TRAFFIC)
        self.blocked = deque(maxlen=50)
        self.aborted = threading.Event()
        self.last_observation = None
        self.login_state = "unknown"
        self.created_at = time.time()
        self._attach()

    def _record(self, kind, **fields):
        entry = {"kind": kind, "at": time.time()}
        entry.update(fields)
        self.traffic.append(entry)

    def _attach(self):
        page = self.page

        def on_request(request):
            self._record(
                "request",
                url=clipped(request.url, 500),
                method=request.method,
                resource_type=request.resource_type,
            )

        def on_response(response):
            self._record(
                "response",
                url=clipped(response.url, 500),
                status=response.status,
                resource_type=response.request.resource_type,
            )

        def on_failed(request):
            self._record(
                "failed",
                url=clipped(request.url, 500),
                method=request.method,
                failure=clipped(request.failure, 300),
            )

        def on_dialog(dialog):
            self._record("dialog", message=clipped(dialog.message, 300), dialog_type=dialog.type)
            try:
                dialog.dismiss()
            except Exception:
                pass

        page.on("request", on_request)
        page.on("response", on_response)
        page.on("requestfailed", on_failed)
        page.on("dialog", on_dialog)
        page.on("console", lambda message: self._record(
            "console", level=message.type, text=clipped(message.text, 400)
        ) if message.type in ("error", "warning") else None)
        page.on("pageerror", lambda error: self._record("pageerror", text=clipped(error, 500)))

        def route_handler(route, request):
            allowed, reason = self.scope.allows(request.url)
            if not allowed:
                self.blocked.append(
                    {"url": clipped(request.url, 500), "reason": reason, "at": time.time()}
                )
                self._record("blocked", url=clipped(request.url, 500), reason=reason)
                try:
                    route.abort("blockedbyclient")
                except Exception:
                    pass
                return
            try:
                route.continue_()
            except Exception:
                try:
                    route.abort()
                except Exception:
                    pass

        self.context.route("**/*", route_handler)

    def _check_abort(self):
        if self.aborted.is_set():
            raise BrowserEngineError("browser action cancelled", {"reason": "cancelled"})

    def _sleep(self, milliseconds):
        deadline = time.time() + milliseconds / 1000.0
        while time.time() < deadline:
            self._check_abort()
            time.sleep(min(0.1, max(0.0, deadline - time.time())))

    def observe(self):
        self._check_abort()
        observation = self.page.evaluate(OBSERVE_JS, MAX_TEXT_CHARS)
        observation["traffic"] = list(self.traffic)[-60:]
        observation["blocked_requests"] = list(self.blocked)[-10:]
        if isinstance(observation.get("auth"), dict):
            observation["auth"]["session_state"] = self.login_state
        self.last_observation = observation
        if observation.get("auth", {}).get("login_form"):
            if self.login_state == "authenticated":
                self.login_state = "expired"
            elif self.login_state == "unknown":
                self.login_state = "login_required"
        elif observation.get("auth", {}).get("likely_authenticated"):
            if self.login_state not in ("expired",):
                self.login_state = "authenticated"
        return observation

    def screenshot(self):
        self._check_abort()
        try:
            return self.page.screenshot(type="jpeg", quality=72, full_page=False)
        except Exception as error:
            raise BrowserEngineError("screenshot failed", {"error": repr(error)})

    def navigate(self, url):
        self._check_abort()
        allowed, reason = self.scope.allows(url)
        if not allowed:
            return {
                "status": "blocked",
                "summary": f"navigation blocked by {reason}",
                "error": reason,
            }
        response = self.page.goto(url, wait_until="domcontentloaded")
        self._settle()
        observation = self.observe()
        return {
            "status": "ok",
            "summary": f"navigated to {observation.get('url')}",
            "http_status": response.status if response is not None else None,
            "observation": observation,
        }

    def _settle(self, milliseconds=450):
        try:
            self.page.wait_for_load_state("domcontentloaded", timeout=4000)
        except Exception:
            pass
        if milliseconds >= 400:
            try:
                self.page.wait_for_load_state("networkidle", timeout=2500)
            except Exception:
                pass
        self._sleep(milliseconds)

    def _find(self, ref):
        if not ref:
            return None
        locator = self.page.locator(f'[data-darkbox-ref="{ref}"]')
        return locator.first if locator.count() else None

    def _locator_by_text(self, role, text):
        if not text:
            return None
        if role in ("link", "a"):
            locator = self.page.get_by_role("link", name=re.compile(re.escape(text), re.I))
        elif role in ("button", "submit"):
            locator = self.page.get_by_role("button", name=re.compile(re.escape(text), re.I))
        else:
            locator = self.page.get_by_text(re.compile(re.escape(text), re.I))
        return locator.first if locator.count() else None

    def _resolve(self, args):
        ref = args.get("ref")
        if ref:
            locator = self._find(ref)
            if locator is None:
                raise BrowserEngineError(
                    "element reference is no longer present; observe the page again",
                    {"reason": "stale_ref", "ref": ref},
                )
            return locator
        locator = self._locator_by_text(args.get("role"), args.get("text"))
        if locator is None:
            raise BrowserEngineError(
                "element not found; observe the page again",
                {"reason": "not_found", "text": args.get("text")},
            )
        return locator

    def act(self, action):
        self._check_abort()
        kind = (action.get("type") or "").strip()
        args = action.get("value") if isinstance(action.get("value"), dict) else action
        before = self.observe()
        before_url = before.get("url")
        effect = "not_applicable"
        detail = ""
        if kind == "click":
            locator = self._resolve(args)
            detail = clipped(locator.inner_text() if locator else args.get("text"), 160)
            try:
                locator.click(timeout=DEFAULT_ACTION_TIMEOUT_MS)
            except Exception as error:
                recent = list(self.traffic)[-12:]
                posted = any(
                    entry.get("kind") == "request"
                    and entry.get("method") == "POST"
                    and "timeout" in str(error).lower()
                    for entry in recent
                )
                if posted:
                    observation = None
                    try:
                        self._sleep(300)
                        observation = self.observe()
                    except Exception:
                        observation = None
                    return {
                        "status": "outcome_unknown",
                        "summary": "the click submitted a request but the page did not confirm the result",
                        "error": "submit_result_unknown",
                        "detail": clipped(error, 400),
                        "login_state": "unknown",
                        "observation": observation,
                    }
                raise
            self._settle()
        elif kind == "fill":
            locator = self._resolve(args)
            value = args.get("value")
            if value is None:
                raise BrowserEngineError("fill needs a value", {"reason": "invalid_action"})
            locator.fill(str(value), timeout=DEFAULT_ACTION_TIMEOUT_MS)
            detail = f"filled {args.get('ref') or args.get('text') or 'field'}"
        elif kind == "select":
            locator = self._resolve(args)
            locator.select_option(str(args.get("value", "")), timeout=DEFAULT_ACTION_TIMEOUT_MS)
            detail = f"selected {args.get('value')}"
        elif kind == "check":
            locator = self._resolve(args)
            if args.get("checked", True):
                locator.check(timeout=DEFAULT_ACTION_TIMEOUT_MS)
            else:
                locator.uncheck(timeout=DEFAULT_ACTION_TIMEOUT_MS)
            detail = "changed a checkbox"
        elif kind == "press":
            key = args.get("key") or "Enter"
            locator = self._resolve(args) if (args.get("ref") or args.get("text")) else None
            if locator is not None:
                locator.press(key, timeout=DEFAULT_ACTION_TIMEOUT_MS)
            else:
                self.page.keyboard.press(key)
            self._settle()
            detail = f"pressed {key}"
        elif kind == "scroll":
            amount = int(args.get("amount") or 700)
            if args.get("direction") == "up":
                amount = -abs(amount)
            if args.get("ref"):
                self._resolve(args).scroll_into_view_if_needed()
            else:
                self.page.mouse.wheel(0, amount)
            self._sleep(200)
            detail = f"scrolled {amount}px"
        elif kind == "back":
            self.page.go_back(wait_until="domcontentloaded")
            self._settle()
            detail = "went back"
        elif kind == "forward":
            self.page.go_forward(wait_until="domcontentloaded")
            self._settle()
            detail = "went forward"
        elif kind == "reload":
            self.page.reload(wait_until="domcontentloaded")
            self._settle()
            detail = "reloaded"
        elif kind == "wait":
            target = args.get("text") or args.get("selector")
            timeout = min(int(args.get("timeout_ms") or 5000), 15_000)
            if args.get("text"):
                self.page.get_by_text(re.compile(re.escape(args["text"]), re.I)).first.wait_for(
                    state="visible", timeout=timeout
                )
            elif target:
                self.page.locator(target).first.wait_for(state="visible", timeout=timeout)
            else:
                self._sleep(timeout)
            detail = f"waited {timeout}ms"
        elif kind == "navigate":
            result = self.navigate(args.get("url") or "")
            if result.get("status") != "ok":
                return result
            detail = f"navigated to {result['observation'].get('url')}"
        else:
            raise BrowserEngineError(f"unsupported action {kind}", {"reason": "invalid_action"})

        after = self.observe()
        changed = after.get("url") != before_url or after.get("text") != before.get("text")
        return {
            "status": "ok",
            "summary": detail or kind,
            "action": {"type": kind, "detail": detail, "effect": effect},
            "changed": changed,
            "observation": after,
        }

    def login(self, payload):
        self._check_abort()
        config = payload.get("login") or {}
        secret_ref = self.identity.get("secret_ref")
        password = resolve_secret(secret_ref)
        if not password:
            return {
                "status": "blocked",
                "summary": "no secret is available for this identity",
                "error": "missing_secret",
                "login_state": "unknown",
            }
        username = config.get("username") or payload.get("username") or self.identity.get("label")
        login_url = config.get("login_url") or payload.get("login_url")
        if login_url:
            started = self.navigate(login_url)
            if started.get("status") != "ok":
                return started
        before = self.observe()
        before_url = before.get("url")
        password_ref = config.get("password_selector") or config.get("password_ref")
        username_ref = config.get("username_selector") or config.get("username_ref")
        submit_ref = config.get("submit_selector") or config.get("submit_ref")
        if not password_ref:
            password_input = self.page.locator('input[type="password"]:visible').first
            if password_input.count() == 0:
                return {
                    "status": "blocked",
                    "summary": "no visible password field was found; authentication cannot be attempted",
                    "error": "password_field_missing",
                    "observation": before,
                }
            password_ref = password_input.get_attribute("data-darkbox-ref")
            if not password_ref:
                password_ref = password_input.evaluate(
                    "el => { if (!el.dataset.darkboxRef) { el.dataset.darkboxRef = 'login-password'; } return el.dataset.darkboxRef; }"
                )
        if not username_ref:
            candidates = [
                'input[autocomplete="username"]:visible',
                'input[type="email"]:visible',
                'input[name*="user" i]:visible',
                'input[name*="email" i]:visible',
                'form input[type="text"]:visible',
            ]
            for selector in candidates:
                field = self.page.locator(selector).first
                if field.count() > 0:
                    username_ref = field.get_attribute("data-darkbox-ref")
                    if not username_ref:
                        username_ref = field.evaluate(
                            "el => { if (!el.dataset.darkboxRef) { el.dataset.darkboxRef = 'login-username'; } return el.dataset.darkboxRef; }"
                        )
                    break
        if not submit_ref:
            buttons = self.page.locator(
                'form button[type="submit"]:visible, form input[type="submit"]:visible, button:has-text("Sign in"):visible, button:has-text("Log in"):visible, button:has-text("Login"):visible'
            )
            if buttons.count() > 0:
                submit_ref = buttons.first.get_attribute("data-darkbox-ref")
                if not submit_ref:
                    submit_ref = buttons.first.evaluate(
                        "el => { if (!el.dataset.darkboxRef) { el.dataset.darkboxRef = 'login-submit'; } return el.dataset.darkboxRef; }"
                    )
        if not username_ref or not submit_ref:
            return {
                "status": "blocked",
                "summary": "login form fields could not be identified without guessing",
                "error": "login_form_incomplete",
                "observation": before,
                "auth": before.get("auth"),
            }
        self._find(username_ref).fill(str(username))
        self._find(password_ref).fill(str(password))
        try:
            self._find(submit_ref).click(timeout=DEFAULT_ACTION_TIMEOUT_MS)
        except Exception as error:
            return {
                "status": "outcome_unknown",
                "summary": "the login submission may have reached the application but the result was not observed",
                "error": "submit_result_unknown",
                "detail": clipped(error, 500),
                "observation": self.observe(),
            }
        self._settle(700)
        after = self.observe()
        auth = after.get("auth") or {}
        error_text = (auth.get("error_text") or "").lower()
        failure_text = str(config.get("failure_text") or "").lower()
        success_text = str(config.get("success_text") or "").lower()
        success_selector = config.get("success_selector")
        has_success_selector = False
        if success_selector:
            try:
                has_success_selector = self.page.locator(success_selector).count() > 0
            except Exception:
                has_success_selector = False
        page_text = (after.get("text") or "").lower()
        after_url = (after.get("url") or "").lower()
        login_url_hint = bool(
            (config.get("login_url") and config.get("login_url").lower().rstrip("/") in after_url)
            or re.search(r"/(login|signin|sign-in|auth)(/|$|\?)", after_url)
        )
        generic_failure = bool(
            re.search(r"invalid credentials|invalid username|invalid password|authentication failed|unauthorized|not accepted|incorrect password|credenciais inv", page_text)
        )
        failed = bool(
            (failure_text and failure_text in (error_text + " " + page_text))
            or generic_failure
            or (auth.get("password_fields", 0) > 0 and after.get("url") == before_url)
            or (auth.get("password_fields", 0) == 0 and login_url_hint and not auth.get("logout_visible"))
        )
        if failed:
            self.login_state = "login_failed"
            return {
                "status": "blocked",
                "summary": "login was not accepted; the page still shows a login form or an error",
                "error": "login_failed",
                "login_state": "login_failed",
                "observation": after,
            }
        authenticated = bool(
            auth.get("password_fields", 0) == 0
            and (
                has_success_selector
                or auth.get("logout_visible")
                or (success_text and success_text in page_text)
                or (after.get("url") != before_url and not login_url_hint)
            )
        )
        if not authenticated:
            self.login_state = "unknown"
            return {
                "status": "inconclusive",
                "summary": "the page changed but authentication could not be confirmed",
                "error": "auth_not_confirmed",
                "login_state": "unknown",
                "observation": after,
            }
        self.login_state = "authenticated"
        cookies = []
        try:
            cookies = [cookie.get("name") for cookie in self.context.cookies()]
        except Exception:
            pass
        return {
            "status": "ok",
            "summary": f"authenticated {self.identity.get('label')} for tenant {self.tenant}",
            "login_state": "authenticated",
            "session": {
                "state": "authenticated",
                "tenant": self.tenant,
                "cookie_names": sorted(cookies),
            },
            "observation": after,
        }

    def close(self):
        try:
            self.context.close()
        except Exception:
            pass


class EngineHost:
    """Owns Playwright on one thread, because sync Playwright is thread-affine."""

    def __init__(self, mode):
        self.mode = mode
        self.requests = queue.Queue()
        self.engine = None
        self.engine_info = {}
        self.sessions = {}
        self.error = None
        self.thread = threading.Thread(target=self._run, name="darkbox-browser", daemon=True)

    def start(self):
        self.thread.start()
        deadline = time.time() + 45
        while time.time() < deadline:
            if self.engine_info or self.error is not None:
                break
            time.sleep(0.05)
        if self.error is not None:
            raise BrowserEngineError("browser host failed to start", self.error)
        if not self.engine_info:
            raise BrowserEngineError("browser host did not start in time")
        return self

    def _run(self):
        try:
            self.engine = self._select_engine()
            self.engine_info = self._probe_engine()
        except Exception as error:
            self.error = {"error": repr(error), "detail": getattr(error, "detail", {})}
            return
        while True:
            item = self.requests.get()
            if item is None:
                break
            command, result = item
            try:
                result["value"] = self._dispatch(command)
            except BrowserEngineError as error:
                result["error"] = {"message": str(error), "detail": error.detail}
            except Exception as error:
                result["error"] = {
                    "message": f"{type(error).__name__}: {error}",
                    "detail": {"traceback": traceback.format_exc()[-2000:]},
                }
            finally:
                result["done"].set()

    def _select_engine(self):
        mode = self.mode
        camoufox_error = None
        if mode in ("auto", "camoufox"):
            try:
                import camoufox  # noqa: F401

                return CamoufoxEngine().start()
            except Exception as error:
                camoufox_error = repr(error)
                if mode == "camoufox":
                    raise BrowserEngineError(
                        "camoufox mode was requested but is not available",
                        {"error": camoufox_error},
                    )
        if mode == "mock":
            raise BrowserEngineError("mock mode is not a browser and cannot serve interactive sessions")
        engine = PlaywrightEngine().start()
        engine.camoufox_error = camoufox_error
        return engine

    def _probe_engine(self):
        page = self.engine.browser.new_page()
        try:
            page.set_content(
                "<title>DarkBox probe</title><div id='probe'>no js</div>"
                "<script>document.querySelector('#probe').textContent='js-ok'</script>"
            )
            js_ok = page.text_content("#probe") == "js-ok"
            user_agent = page.evaluate("navigator.userAgent")
        finally:
            page.close()
        if not js_ok:
            raise BrowserEngineError("browser started but JavaScript execution failed")
        return {
            "engine": self.engine.name,
            "engine_version": self.engine.version(),
            "javascript": True,
            "executable": getattr(self.engine, "executable", None),
            "user_agent": user_agent,
            "camoufox": self.engine.name == "camoufox",
            "camoufox_error": getattr(self.engine, "camoufox_error", None),
            "requested_mode": self.mode,
        }

    def _dispatch(self, command):
        kind = command.get("kind")
        payload = command.get("payload") or {}
        if kind == "capability":
            return {"status": "ok", **self.engine_info}
        if kind == "session.open":
            key = command.get("session_key") or payload.get("session_key")
            if not key:
                raise BrowserEngineError("session_key is required", {"reason": "invalid_request"})
            existing = self.sessions.get(key)
            if existing is not None:
                existing.close()
            scope = Scope(command.get("policy") or {})
            session = BrowserSession(key, self.engine, scope, payload)
            self.sessions[key] = session
            initial_url = payload.get("url")
            if initial_url:
                opened = session.navigate(initial_url)
                if opened.get("status") != "ok":
                    return opened
            else:
                session.page.goto("about:blank")
            observation = session.observe()
            data = session.screenshot()
            return {
                "status": "ok",
                "summary": f"opened {self.engine.name} session",
                "engine": self.engine_info,
                "session": {"context_key": key, "state": "open", "tenant": session.tenant},
                "observation": observation,
                "screenshot_b64": base64.b64encode(data).decode("ascii"),
                "content_type": "image/jpeg",
            }
        key = command.get("session_key") or payload.get("session_key")
        session = self.sessions.get(key)
        if session is None:
            raise BrowserEngineError(
                "browser context is not available; open the session again",
                {"reason": "session_lost", "context_key": key},
            )
        if kind == "session.close":
            session.close()
            self.sessions.pop(key, None)
            return {"status": "ok", "summary": "closed browser context"}
        if kind == "session.abort":
            session.aborted.set()
            return {"status": "ok", "summary": "cancellation signalled"}
        if kind == "observe":
            observation = session.observe()
            return {"status": "ok", "summary": "observed current page", "observation": observation}
        if kind == "navigate":
            result = session.navigate(payload.get("url") or "")
            if result.get("status") == "ok":
                data = session.screenshot()
                result["screenshot_b64"] = base64.b64encode(data).decode("ascii")
                result["content_type"] = "image/jpeg"
            return result
        if kind == "act":
            result = session.act(payload.get("action") or {})
            if result.get("status") == "ok":
                data = session.screenshot()
                result["screenshot_b64"] = base64.b64encode(data).decode("ascii")
                result["content_type"] = "image/jpeg"
            return result
        if kind == "login":
            result = session.login(payload)
            if result.get("status") in ("ok", "blocked", "inconclusive", "outcome_unknown"):
                try:
                    data = session.screenshot()
                    result["screenshot_b64"] = base64.b64encode(data).decode("ascii")
                    result["content_type"] = "image/jpeg"
                except Exception:
                    pass
            return result
        if kind == "screenshot":
            data = session.screenshot()
            return {
                "status": "ok",
                "summary": "captured screenshot",
                "screenshot_b64": base64.b64encode(data).decode("ascii"),
                "content_type": "image/jpeg",
                "observation": session.observe(),
            }
        if kind == "traffic":
            return {
                "status": "ok",
                "summary": f"{len(session.traffic)} recent browser exchanges",
                "traffic": list(session.traffic),
                "blocked_requests": list(session.blocked),
            }
        if kind == "session.state":
            observation = session.observe()
            return {
                "status": "ok",
                "summary": f"session is {session.login_state}",
                "login_state": session.login_state,
                "session": {
                    "context_key": session.key,
                    "tenant": session.tenant,
                    "state": session.login_state,
                    "uptime_seconds": round(time.time() - session.created_at, 1),
                },
                "observation": observation,
            }
        raise BrowserEngineError(f"unsupported command {kind}", {"reason": "invalid_request"})

    def submit(self, command, timeout=60):
        result = {"done": threading.Event(), "value": None, "error": None}
        self.requests.put((command, result))
        if not result["done"].wait(timeout):
            raise BrowserEngineError("browser host command timed out", {"reason": "timeout"})
        if result["error"]:
            raise BrowserEngineError(result["error"]["message"], result["error"].get("detail"))
        return result["value"]

    def abort(self, key):
        session = self.sessions.get(key)
        if session is not None:
            session.aborted.set()
            return True
        return False

    def close(self, key):
        session = self.sessions.get(key)
        if session is not None:
            session.close()
            self.sessions.pop(key, None)
            return True
        return False

    def close_all(self):
        self.requests.put(None)
        for session in list(self.sessions.values()):
            try:
                session.close()
            except Exception:
                pass
        self.sessions.clear()
        try:
            if self.engine is not None:
                self.engine.close()
        except Exception:
            pass


HOST = None
HOST_LOCK = threading.Lock()


def get_host(mode):
    global HOST
    with HOST_LOCK:
        if HOST is None:
            HOST = EngineHost(mode).start()
        return HOST


class DaemonHandler(BaseHTTPRequestHandler):
    server_version = "DarkBoxBrowser/1"

    def log_message(self, fmt, *args):
        log("http " + (fmt % args))

    def _authorized(self):
        token = os.environ.get("DARKBOX_BROWSER_TOKEN")
        if not token:
            return True
        header = self.headers.get("Authorization", "")
        return header == f"Bearer {token}"

    def _send(self, status, payload):
        body = json.dumps(payload, ensure_ascii=False).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        try:
            self.wfile.write(body)
        except BrokenPipeError:
            pass

    def do_GET(self):
        if self.path == "/health":
            host = HOST
            self._send(
                200,
                {
                    "status": "ok" if host else "starting",
                    "engine": host.engine_info if host else None,
                },
            )
            return
        if not self._authorized():
            self._send(401, {"status": "error", "error": "unauthorized"})
            return
        if self.path == "/v1/capability":
            host = get_host(self.server.mode)
            self._send(200, {"status": "ok", **host.engine_info})
            return
        self._send(404, {"status": "error", "error": "not_found"})

    def do_POST(self):
        if not self._authorized():
            self._send(401, {"status": "error", "error": "unauthorized"})
            return
        length = int(self.headers.get("Content-Length") or 0)
        if length <= 0 or length > MAX_BODY_BYTES:
            self._send(413, {"status": "error", "error": "invalid_body_size"})
            return
        raw = self.rfile.read(length)
        try:
            command = json.loads(raw)
        except json.JSONDecodeError as error:
            self._send(400, {"status": "error", "error": f"invalid json: {error}"})
            return
        host = get_host(self.server.mode)
        if self.path == "/v1/abort":
            key = command.get("session_key") or (command.get("payload") or {}).get("session_key")
            stopped = host.abort(key)
            self._send(200, {"status": "ok", "aborted": stopped})
            return
        if self.path == "/v1/close":
            key = command.get("session_key") or (command.get("payload") or {}).get("session_key")
            closed = host.close(key)
            self._send(200, {"status": "ok", "closed": closed})
            return
        if command.get("schema_version") != SCHEMA_VERSION:
            self._send(400, {"status": "error", "error": "unsupported schema version"})
            return
        if self.path != "/v1/command":
            self._send(404, {"status": "error", "error": "not_found"})
            return
        try:
            result = host.submit(command, timeout=min(int(command.get("timeout_seconds") or 55), 120))
        except BrowserEngineError as error:
            self._send(
                409,
                {
                    "status": "error",
                    "error": str(error),
                    "detail": error.detail,
                    "engine": host.engine_info,
                },
            )
            return
        except Exception as error:
            self._send(
                500,
                {
                    "status": "error",
                    "error": f"{type(error).__name__}: {error}",
                    "engine": host.engine_info,
                },
            )
            return
        if result.get("status") == "blocked":
            self._send(200, {**result, "engine": host.engine_info})
            return
        if result.get("status") == "outcome_unknown":
            self._send(200, {**result, "engine": host.engine_info})
            return
        self._send(200, {**result, "engine": host.engine_info})


def serve(host, port, mode):
    get_host(mode)
    server = ThreadingHTTPServer((host, port), DaemonHandler)
    server.mode = mode
    log(f"darkbox browser adapter listening on {host}:{port}")
    try:
        server.serve_forever()
    finally:
        server.server_close()
        if HOST is not None:
            HOST.close_all()


# ---------------------------------------------------------------------------
# Legacy one-shot mode. It uses a real browser only when the legacy request
# asks for browser.session and the engine is available; flow replay remains a
# contract-test path. Real interactive work uses --serve.
# ---------------------------------------------------------------------------


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class HttpIdentity:
    def __init__(self, identity, base_url, login_path, scope):
        self.identity = identity
        self.label = identity.get("label", "unknown")
        self.base_url = base_url.rstrip("/")
        self.login_path = login_path or "/login"
        self.jar = http.cookiejar.CookieJar()
        self.opener = urllib.request.build_opener(
            urllib.request.HTTPCookieProcessor(self.jar), NoRedirect()
        )
        self.password = resolve_secret(identity.get("secret_ref"))
        if not self.password and self.label in ("alice", "bob"):
            fallback = os.environ.get(f"LAB_{self.label.upper()}_PASSWORD")
            if fallback:
                self.password = fallback
        self.authenticated = False
        self.login_status = None
        self.scope = scope

    def raw_request(self, method, url, data=None, headers=None):
        headers = dict(headers or {})
        headers.setdefault("User-Agent", "DarkBox-Adapter/0.4 (authorized lab)")
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
        username = self.identity.get("login_username") or self.identity.get("label")
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


def run_session(request):
    payload = request.get("payload", {})
    identity = payload.get("identity", {})
    tenant = payload.get("tenant")
    base_url = payload.get("base_url", "")
    scope = Scope(request.get("policy"))
    client = HttpIdentity(identity, base_url, payload.get("login_path"), scope)
    status, final = client.login()
    if not client.authenticated:
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
    target = f"{base_url.rstrip('/')}/app/{tenant}" if tenant else final
    home_status, _, body, home_final = client.request("GET", target)
    return {
        "status": "succeeded",
        "summary": f"authenticated {identity.get('label')} for tenant {tenant}",
        "session": {
            "state": "authenticated",
            "tenant": tenant,
            "context_key": f"ctx-{identity.get('label')}-{uuid.uuid4().hex[:12]}",
            "cookie_names": client.cookie_names(),
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


def fetch_steps(identity, steps, base_url):
    results = []
    evidence = []
    for step in steps:
        url = step.get("url") or base_url
        method = step.get("method", "GET")
        status, headers, body, final = identity.request(method, url)
        evidence.append(
            {
                "summary": f"{method} {url} -> {status}",
                "text": f"{method} {url}\nstatus: {status}\nfinal: {final}\ntitle: {title_of(body)}\n\n{body[:4000]}",
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


def run_flow(request):
    payload = request.get("payload", {})
    identity = payload.get("identity", {})
    base_url = payload.get("base_url", "")
    scope = Scope(request.get("policy"))
    client = HttpIdentity(identity, base_url, payload.get("login_path"), scope)
    status, final = client.login()
    if not client.authenticated:
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
    steps, evidence = fetch_steps(client, payload.get("steps", []), base_url)
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
            status, _, body, final = client.request("GET", url)
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


def one_shot(mode):
    line = sys.stdin.readline()
    if not line:
        emit("error", "", {"message": "empty stdin"})
        return 2
    try:
        request = json.loads(line)
    except json.JSONDecodeError as error:
        emit("error", "", {"message": f"invalid request json: {error}"})
        return 2
    if request.get("schema_version") != SCHEMA_VERSION:
        emit("error", request.get("request_id", ""), {"message": "unsupported schema version"})
        return 2
    kind = request.get("kind")
    request_id = request.get("request_id", "")
    emit("progress", request_id, {"phase": "started", "kind": kind})
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
    except Exception as error:
        log(f"adapter failure: {error!r}")
        emit("error", request_id, {"message": f"adapter failure: {error}"})
        return 1
    emit("progress", request_id, {"phase": "finished", "mode": "http" if mode != "camoufox" else mode})
    emit("result", request_id, payload)
    return 0


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--mode", default="auto", choices=["auto", "mock", "playwright", "camoufox"])
    parser.add_argument("--serve", action="store_true")
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=8791)
    parser.add_argument("--probe", action="store_true")
    args = parser.parse_args()
    if args.probe:
        try:
            host = EngineHost(args.mode).start()
            print(json.dumps({"status": "ok", **host.engine_info}, indent=2))
            host.close_all()
            return 0
        except Exception as error:
            print(
                json.dumps(
                    {
                        "status": "error",
                        "error": str(error),
                        "detail": getattr(error, "detail", {}),
                    },
                    indent=2,
                )
            )
            return 1
    if args.serve:
        if args.host not in ("127.0.0.1", "localhost", "::1") and not os.environ.get(
            "DARKBOX_BROWSER_TOKEN"
        ):
            log("refusing to bind a non-loopback address without DARKBOX_BROWSER_TOKEN")
            return 2
        serve(args.host, args.port, args.mode)
        return 0
    return one_shot(args.mode)


if __name__ == "__main__":
    sys.exit(main())
