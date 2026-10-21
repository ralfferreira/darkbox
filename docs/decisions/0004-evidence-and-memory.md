# ADR 0004: sanitized evidence and derived memory

Status: accepted, 2026-10-21.

## Context

Captures can contain credentials and customer data. Narrative memory is useful
for continuity but is generated text and can turn a hypothesis into an apparent
fact.

## Decision

Sanitize at ingestion, store by opaque key, hash the stored derivation, and note
that it is not a byte-identical copy. Serve evidence as inert text. Keep
narrative pages versioned and derived, with references to records. Never let a
narrative page replace an observation. Build context packages with filters,
a token budget and an explicit omission list. Raw restricted evidence is off by
default and requires its own policy and retention.

## Consequences

- An export can be shared without the seeded secret or session cookies.
- A hash detects change relative to the stored object; it does not prove remote
  authenticity.
- Recovery of a deleted object is impossible; metadata may remain and report
  the gap.
- Markdown export is one-way by design. There is no bidirectional sync with an
  external notes tool.
