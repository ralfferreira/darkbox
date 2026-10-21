# ADR 0001: modular monolith with a Postgres queue

Status: accepted, 2026-10-21.

## Context

The initial release needs a complete flow: create an assessment, map it,
exercise two identities, preserve evidence, and resume. Splitting into services
per capability would add network boundaries before any measured need.

## Decision

Use one Rust workspace with logical crates (domain, policy, storage, evidence,
execution, memory, api, cli). Run the API and worker as separate processes that
share the database and types. Use PostgreSQL for structured records, events,
leases and the queue.

## Consequences

- One transaction can change state and insert the corresponding event.
- The worker can be scaled horizontally; claims use `FOR UPDATE SKIP LOCKED`.
- A module can become a crate or a service later without changing the model.
- The database is a single operational dependency. Backups and migrations must
  be taken seriously.
- Long-running browser work still runs out of process, so the API stays
  responsive.
