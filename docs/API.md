# HTTP contract (v1)

All routes are prefixed with `/v1`. Every request needs
`Authorization: Bearer <token>`. The token resolves to one workspace and role.
Identifiers from another workspace are rejected before any operation.

`GET /health` is unauthenticated.

| Method | Path | Body / query | Response |
| --- | --- | --- | --- |
| GET | `/session` | | `SessionDto` |
| GET | `/companies` | | `CompanyDto[]` |
| POST | `/companies` | `CreateCompanyRequest` | `CompanyDto` |
| GET | `/companies/{id}` | | `CompanyDto` |
| GET | `/assessments` | `company_id` | `AssessmentSummary[]` |
| POST | `/assessments` | `CreateAssessmentRequest` | `AssessmentSummary` |
| GET | `/assessments/{id}` | | `AssessmentDetail` |
| GET | `/assessments/{id}/scope-versions` | | `ScopeVersionDto[]` |
| POST | `/assessments/{id}/scope-versions` | `{policy, note}` | `ScopeVersionDto` |
| POST | `/assessments/{id}/scope-preview` | `{targets: string[]}` | `ScopeDecision[]` |
| GET | `/assessments/{id}/inventory` | `state, host, identity_id, source, q, limit, offset` | `InventoryResponse` |
| POST | `/assessments/{id}/import-urls` | `ImportUrlsRequest` | `RunSummary` |
| GET | `/assessments/{id}/observations` | `limit` | `ObservationDto[]` |
| GET | `/assessments/{id}/identities` | | `IdentityDto[]` |
| POST | `/assessments/{id}/identities` | `CreateIdentityRequest` | `IdentityDto` |
| GET | `/assessments/{id}/tenants` | | `TargetTenantDto[]` |
| POST | `/assessments/{id}/tenants` | `{key,label}` | `TargetTenantDto` |
| GET | `/assessments/{id}/flows` | | `FlowDto[]` |
| POST | `/assessments/{id}/flows` | `CreateFlowRequest` | `FlowDto` |
| POST | `/assessments/{id}/actions` | `CreateActionRequest` | `ActionDto` |
| GET | `/assessments/{id}/runs` | `limit` | `RunSummary[]` |
| GET | `/assessments/{id}/evidence` | `kind, limit, offset` | `EvidenceDto[]` |
| GET | `/assessments/{id}/memory` | | `MemoryPageDto[]` |
| POST | `/assessments/{id}/memory/search` | `MemorySearchRequest` | `MemoryPageDto[]` |
| GET | `/assessments/{id}/context` | `token_budget, identity_id` | `ContextPackageDto` |
| GET | `/assessments/{id}/findings` | `state` | `FindingDto[]` |
| POST | `/assessments/{id}/findings` | `CreateFindingRequest` | `FindingDto` |
| GET | `/assessments/{id}/results` | `limit` | `TestResultDto[]` |
| GET | `/assessments/{id}/eligibility` | | `EligibilityDto[]` |
| GET | `/assessments/{id}/events` | `after` | `EventDto[]`, or SSE when `Accept: text/event-stream` |
| POST | `/assessments/{id}/exports` | `ExportRequest` | `ExportDto` |
| GET | `/procedures` | | `ProcedureDto[]` |
| GET | `/runs` | `assessment_id` | `RunSummary[]` |
| POST | `/runs` | `CreateRunRequest` | `RunSummary` |
| GET | `/runs/{id}` | | `RunDetail` |
| POST | `/runs/{id}/cancel` | `CancelRunRequest` | `RunSummary` |
| POST | `/runs/{id}/resume` | `ResumeRunRequest` | `RunSummary` |
| GET | `/runs/{id}/events` | `after` | `EventDto[]` |
| GET | `/evidence/{id}` | | `EvidenceDto` |
| GET | `/evidence/{id}/content` | | sanitized text |
| GET | `/exports/{id}/download` | | JSON export |
| GET | `/capabilities` | | `CapabilitiesDto` |
| GET | `/findings/{id}` | | `FindingDto` |
| POST | `/findings/{id}/review` | `ReviewFindingRequest` | `FindingDto` |

### Integration runs

`POST /v1/runs` accepts additional optional fields for the adapters:

```json
{
  "assessment_id": "...",
  "kind": "network_scan",
  "idempotency_key": "...",
  "targets": ["127.0.0.1"],
  "params": { "ports": [8080] },
  "note": "optional"
}
```

Run kinds: `network_scan`, `firecrawl_map`, `firecrawl_scrape`, `burp_import`.
`network_scan` defaults targets to the distinct hosts already in the inventory
and ignores ports outside `allowed_ports`. `firecrawl_map` takes the first
target as the site URL and `params.limit` / `params.search`. `firecrawl_scrape`
takes up to five targets and uses `note` as the scrape objective.
`burp_import` needs no target and uses `params.limit`.

### Agent turn endpoints

| Method | Path | Body | Response |
| --- | --- | --- | --- |
| POST | `/assessments/{id}/agent/turns` | `AgentTurnRequest` (`start_url`, `continue_from`, optional `model`) | `AgentTurnResponse` (`run` + bounded `context`) |
| POST | `/runs/{id}/agent/events` | `{event_type, payload}` | `EventDto` |
| POST | `/runs/{id}/agent/usage` | `{phase: reserve\|settle, call_id, tokens, model}` | `RunSummary` |
| POST | `/runs/{id}/agent/finish` | `{summary, next_steps, evidence_ids, outcome}` | run, memory page and optional flow |
| POST | `/runs/{id}/agent/abort` | `{reason}` | `RunSummary` |
| GET | `/runs/{id}/agent/checkpoint` | | objective, actions and persisted checkpoint |
| POST | `/runs/{id}/browser/open` | `{url, identity_id, tenant, decision}` | action + observation + evidence |
| GET | `/runs/{id}/browser/progress` | | run, actions, evidence, current page, model calls |
| POST | `/runs/{id}/browser/observe` | `{session_key, decision}` | action + observation |
| POST | `/runs/{id}/browser/navigate` | `{session_key, url, decision}` | action + observation |
| POST | `/runs/{id}/browser/act` | `{session_key, action, decision}` | action + observation |
| POST | `/runs/{id}/browser/login` | `{session_key, login}` | login state + observation |
| POST | `/runs/{id}/browser/screenshot` | `{session_key}` | screenshot evidence |
| POST | `/runs/{id}/browser/traffic` | `{session_key}` | traffic evidence |
| POST | `/runs/{id}/browser/close` | `{session_key}` | close the context |
| GET | `/capabilities/browser` | | functional engine probe |

The agent service owns the model loop. The API owns the durable agent run,
lease, model-call budget, transcript events, memory checkpoint and audit.
Allowed event types include `agent.message`, `agent.tool_call`,
`agent.tool_result`, `agent.usage`, `agent.decision`, `agent.action`,
`agent.browser_session`, `agent.progress`, `agent.checkpoint` and `agent.note`.

`POST /runs/{id}/agent/usage` has two phases. `reserve` books one model call and
one `call_id`; `settle` records the real tokens and cost against that same row.
Repeating the same phase does not double count. Usage the provider does not
report is stored as null with `usage_known=false`.

`browser/*` routes are scoped to one agent objective. They reject a run that is
not `running`. An action with `effect_status=outcome_unknown` moves the objective
to `waiting_operator` and preserves a checkpoint; no further browser action is
accepted until the operator reconciles it.

## Event envelope

Every event includes `event_id`, `schema_version`, `workspace_id`,
`assessment_id`, `run_id`, `step_id`, `event_type`, `occurred_at`,
`recorded_at`, `correlation_id`, `seq`. SSE frames use the event type as the
SSE `event` name and the JSON envelope as `data`. Reconnect with `after=<seq>`.

## Errors

Non-2xx responses use `{"error":{"code":"...","message":"...","detail":{}}}`.
Codes include `unauthorized`, `forbidden`, `not_found`, `invalid`, `conflict`,
`blocked`, `capability_unavailable`.
