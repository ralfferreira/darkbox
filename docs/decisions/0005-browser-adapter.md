# ADR 0005: browser automation behind a process contract

Status: accepted, 2026-10-21. Updated for interactive sessions, 2026-10-22.

## Context

Browser automation is the least stable part of the stack. Tying the API to a
Python runtime or a specific browser engine would make the core hard to test and
hard to replace. The original one-shot adapter proved session isolation but
could not observe a rendered page, click a discovered control or preserve state
between actions.

## Decision

Run a long-lived browser host as a separate process. The API starts it on a
loopback port and exchanges versioned JSON over HTTP. The host owns Playwright
and one `BrowserContext` per assessment, identity and tenant. Every command
carries the run's pinned scope policy. The adapter enforces that policy on every
request the page makes, including subresources and redirects.

The observation is semantic: URL, title, text, headings, tables, forms and
interactive elements with stable refs. Actions reference those refs. Visual
capture is additional evidence, not the primary control surface.

The adapter reports the engine it actually started, its version and the result
of a JavaScript probe. Camoufox is used only when its package imports and its
browser starts. Otherwise the adapter reports the exact failure and uses
Playwright Chromium in `auto` mode. It never labels Chromium as Camoufox.

Secrets are resolved inside the adapter from `env:NAME`. Login performs a
configurable form submission and requires positive evidence of authentication;
a 200 login page is not acceptance.

The one-shot JSON Lines mode remains for deterministic contract tests and
legacy visibility checks. It is explicitly an HTTP client, not a browser.

## Consequences

- Replacing Camoufox with another engine does not change the domain or API.
- A crashing adapter cannot take down the API.
- A browser host restart loses live contexts. Stored sessions become `unknown`
  or `lost`; they are never silently treated as authenticated.
- Interactive sessions need the host process. The Docker image installs
  Playwright Chromium at build time; local development installs the pinned
  package and browser once.
- Cancellation must signal the host and close the context. The core also stops
  starting new actions when the run leaves `running`.
