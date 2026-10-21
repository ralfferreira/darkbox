# Katana adapter status

Katana is the reference crawler in the specification and the first candidate
provider in the plan. It is detected by the capability probe, but this release
does not let it perform scoped runs.

## Why

Katana is a powerful crawler with its own scope flags. Those flags cannot express
every decision DarkBox makes per request: the run's pinned policy, the current
assessment policy, path exclusions, auxiliary destinations and the resolved
private-address check. Running Katana and filtering its output afterwards would
mean the excluded destination had already been contacted. That violates AC-01.

The core therefore ships a bounded breadth-first crawler that asks the policy
engine before every request and before following every discovered link. It
handles HTML links, form actions, script sources and quoted JavaScript routes,
which covers the JS-only surface in the lab.

## Capability reporting

`GET /v1/capabilities` reports:

- `discovery.crawler`: `available`, provider `builtin`;
- `discovery.katana`: `degraded` when the binary exists, `unavailable` when it
  does not, with the reason in `detail`.

The UI shows the reason. No run silently switches engines.

## What would change the decision

A Katana integration can be enabled when one of these is true:

1. The worker runs behind an egress proxy that enforces the same policy at
   connection time, so a post-filter is not the only control.
2. Katana exposes a per-request hook that can call the policy decision and abort
   before the socket is opened.
3. The run uses a dedicated network namespace whose firewall is generated from
   the pinned policy.

Until then, importing Katana's structured output remains useful: capture a
JSONL file in a controlled environment and import the URLs through the
`discovery.import` path. Origin and parser expectations stay recorded.
