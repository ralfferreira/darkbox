# Security policy

## Supported versions

The current `0.3.x` line receives security fixes. Older development snapshots
do not.

## Reporting a vulnerability

Do not open a public issue for a vulnerability in DarkBox itself.

Send a private report to the maintainer listed in the repository metadata with:

- a description of the issue and the affected component;
- the version or commit;
- reproduction steps against the bundled lab, if possible;
- the impact you believe it has;
- any suggested fix or mitigation.

You will receive an acknowledgement. Please allow time for a fix and a release
before public disclosure. Do not include real customer data, live credentials
or evidence from systems you are not authorized to test.

## Scope

In scope: the API, policy engine, execution engine, evidence handling, memory
and context builder, browser adapter contract, bundled lab, CLI and the
operator interface.

Out of scope: vulnerabilities in third-party tools that DarkBox can integrate
with (report those upstream), findings in the synthetic lab that are deliberate
by design, and issues that require an already-compromised operator host.

## Safe harbor

We will not pursue action against good-faith research on the project's own code
and bundled lab. That safe harbor does not extend to third-party systems. You
must have written authorization for any external target.

## Handling secrets in a report

Redact tokens, cookies and customer data. If a reproduction requires a secret,
use the synthetic values from `lab/README.md`.
