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

## Event envelope

Every event includes `event_id`, `schema_version`, `workspace_id`,
`assessment_id`, `run_id`, `step_id`, `event_type`, `occurred_at`,
`recorded_at`, `correlation_id`, `seq`. SSE frames use the event type as the
SSE `event` name and the JSON envelope as `data`. Reconnect with `after=<seq>`.

## Errors

Non-2xx responses use `{"error":{"code":"...","message":"...","detail":{}}}`.
Codes include `unauthorized`, `forbidden`, `not_found`, `invalid`, `conflict`,
`blocked`, `capability_unavailable`.
