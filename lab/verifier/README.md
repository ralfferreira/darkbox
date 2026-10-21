# Verifier

`darkbox verify` is the reference verifier for the synthetic laboratory. It is
outside the agent's context: the assessment under test sees only the URLs the
harness imports and the identities it creates. Expected labels, effect counters
and excluded paths live here, not in a prompt or in the target's HTML.

The harness checks:

| Criterion | Oracle |
| --- | --- |
| AC-01 | lab `/_lab/state` excluded_hits and per-path hits |
| AC-02 | exact endpoint records read through storage |
| AC-03 | two adapter sessions, session states and captured text |
| AC-04 | finding review API, including a negative confirmation |
| AC-05 | new database connection and a fresh context package |
| AC-06 | lease expiry, dispatch marker, reconciliation refusal |
| AC-07 | policy version and effect counter before/after injection page |
| AC-08 | ten concurrent reservations against a budget of five |
| AC-09 | cancellation plus the lab's request counter |
| AC-10 | sanitized export downloaded through the API |
| AC-11 | second workspace token on three read paths |
| AC-12 | flow reproduction and a fresh context package |
| AC-13 | capability and procedure catalog without optional integrations |
| AC-14 | retention expiry, object deletion and metadata deletion |

The report is written to `artifacts/acceptance/report.json` (ignored by Git).
A copy of the last local run is kept in `docs/acceptance/latest.json`.
