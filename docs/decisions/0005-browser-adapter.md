# ADR 0005: browser automation behind a process contract

Status: accepted, 2026-10-21.

## Context

Browser automation is the least stable part of the stack. Tying the API to a
Python runtime or a specific browser engine would make the core hard to test and
hard to replace.

## Decision

Run the browser adapter as a separate process. Exchange one versioned JSON
request and JSON Lines responses over stdin/stdout. Resolve secret references
from the adapter environment. Cap output and time; kill the process group on
cancellation. Keep policy decisions for navigation in the adapter and the
authoritative decision in the Rust executor. Report the engine and its limits as
a capability. Ship a deterministic HTTP mode so the core can be exercised
without a browser, and label it honestly.

## Consequences

- Replacing Camoufox with another engine does not change the domain or API.
- A crashing adapter cannot take down the API.
- The mock mode proves session and flow logic, not JavaScript execution.
- A real browser integration still needs contract tests for timeout, invalid
  JSON, oversized output, unexpected exit and cancellation.
