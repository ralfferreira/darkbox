# Acceptance report

Generated 2026-10-21T22:21:32.043687Z against lab version `lab-1`.

Command: `darkbox verify` with the API and synthetic lab running locally. The
harness creates a fresh assessment and does not reuse demo records.

| Result | Count |
| --- | --- |
| Passed | 14 |
| Failed | 0 |
| Skipped | 0 |

| ID | Criterion | Status | Observed |
| --- | --- | --- | --- |
| AC-01 | Excluded destinations blocked before access | passed | lab oracle excluded_hits=0, forbidden path hits=0; 3 destinations recorded as excluded; run 01a1608e-19d3-7500-9514-ad3002d090db spent 1 requests |
| AC-02 | Methods and semantic paths stay distinct | passed | GET /about, POST /about and GET /about/ are distinct records: ["GET /about/", "POST /about", "GET /about"] |
| AC-03 | Identity contexts are isolated | passed | alice and bob authenticated in separate adapter contexts; evidence contains no session cookie or password |
| AC-04 | Claims point at evidence or stay hypotheses | passed | confirmed finding requires evidence (409 without it); hypotheses remain labelled; every confirmed finding lists evidence |
| AC-05 | Restart preserves evidence and resume context | passed | 11 evidence records read back through a new connection; context package references 8 records and 925 tokens |
| AC-06 | Unknown external effect is not repeated | passed | run 01a1608e-1cc7-77bb-86fe-ab4703954e24 moved to waiting_operator with outcome_unknown; automatic resume refused (409); explicit reconciliation accepted |
| AC-07 | External content cannot change policy | passed | authenticated page text captured as inert evidence; policy stayed at v1; effect counter stayed at 0 |
| AC-08 | Aggregate budget holds with concurrency | passed | 10 concurrent reservations against a budget of 5: exactly 5 granted, spent=5 |
| AC-09 | Cancellation prevents new steps | passed | cancellation before dispatch left the target untouched; run ended cancelled |
| AC-10 | Sanitized export omits secrets | passed | export 37107 bytes; manifest "0f174a20597315a650547aa5711719dfbbc99289af4ccedd3bcf3ba214b656d6"; seeded secret, passwords and cookies absent |
| AC-11 | Workspace isolation on every read | passed | 3/3 cross-workspace reads rejected with 403/404 |
| AC-12 | New session resumes without repeating work | passed | flow 01a1608e-27b1-7266-86e6-0ea31643b65e reproduced with 1 evidence items; a fresh context package for alice includes the flow and a persisted checkpoint |
| AC-13 | Core works without optional integrations | passed | 6 core adapters available; 3 procedure versions loaded; no Burp, Firecrawl or remote runtime used |
| AC-14 | Retention purge removes content and metadata | passed | retention expiry selected the object, removed the stored content and deleted metadata |

The machine-readable report is `latest.json`. It records identifiers from the
synthetic assessment only. No external system was contacted by the harness.

Optional adapters are checked separately. Nmap ran against the lab and scanned
only the policy port. Firecrawl scraped `example.com` through the hosted MCP
server with a refreshed OAuth token and stored the Markdown as evidence. The
Burp path is covered by two MCP contract tests, one streamable HTTP and one
legacy SSE. None of them is required by AC-01 to AC-14.
