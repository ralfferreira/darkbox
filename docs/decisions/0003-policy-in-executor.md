# ADR 0003: policy in the executor, pinned per run

Status: accepted, 2026-10-21.

## Context

A model proposes operations. An adapter performs them. Neither should own the
authorization decision. A policy can change while a run is in progress.

## Decision

The executor evaluates a structured policy immediately before every external
operation, including each redirect hop and each browser navigation. It evaluates
against both the policy pinned at run creation and the assessment's current
policy; both must allow. The decision returns a rule identifier and policy
version. Exclusions are checked before inclusions. Auxiliary destinations are
distinguishable from test targets.

## Consequences

- A compromised model or adapter cannot expand scope.
- A policy expansion does not retroactively grant power to an old run.
- Denials are auditable with the exact rule.
- The policy engine is pure and unit-testable; the executor supplies resolved
  addresses.
- Every adapter must enforce the navigation subset it performs, or it cannot be
  enabled for scoped runs.
