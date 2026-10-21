# Firecrawl adapter

## Role

Firecrawl is the managed browser and URL-discovery path. It is the tool the
agent uses when a page depends on JavaScript, when direct HTTP returns a shell,
or when the site's URL inventory needs a second opinion. It is not a replacement
for the evidence model: Firecrawl returns a derived capture, and the executor
stores it as sanitized evidence with a hash and retention.

Two operations:

| Operation | What it returns | What it does not do |
| --- | --- | --- |
| `firecrawl_map` | candidate URLs under a site | does not fetch their content |
| `firecrawl_scrape` | rendered Markdown, links, title, status | does not make a second direct request |

`firecrawl_search` is exposed as a typed agent operation for public research,
but its results never enter the assessment inventory automatically.

## Transports

1. **REST** with `DARKBOX_FIRECRAWL_API_KEY` (or `FIRECRAWL_API_KEY`). Base URL
   defaults to `https://api.firecrawl.dev/v2` and can be overridden with
   `DARKBOX_FIRECRAWL_BASE_URL` for a compatible gateway or a test server.
2. **MCP** with `DARKBOX_FIRECRAWL_MCP_URL` and
   `DARKBOX_FIRECRAWL_MCP_TOKEN`. The hosted endpoint is
   `https://mcp.firecrawl.dev/v2/mcp-oauth`. Use
   `DARKBOX_FIRECRAWL_MODE=mcp` to prefer MCP when both transports are set.

The access token from the hosted MCP server expires after one hour. A
long-running worker should use an API key, or refresh the OAuth token in its
launch environment. A failed token is surfaced as a blocked run; it is never
retried silently.

For the coding agent, the hosted server is authenticated once:

```sh
opencode mcp add firecrawl --url https://mcp.firecrawl.dev/v2/mcp-oauth --global
opencode mcp auth firecrawl
```

DarkBox is a separate runtime. Give it an API key or an OAuth access token.
Tokens stay in the environment and never reach evidence, prompts or the
browser.

## Scope and safety

- The target URL is checked against the pinned policy and the current policy
  before the provider sees it.
- Private, loopback and link-local targets are refused unless
  `DARKBOX_FIRECRAWL_ALLOW_PRIVATE=1`. Firecrawl fetches from its own network;
  DarkBox cannot attest that path, so private targets are not delegated.
- Returned links are candidates. Each one is authorized before it can become a
  retrievable target, and out-of-scope links are marked `excluded`.
- A scrape marks an endpoint `observed` because a real fetch happened. It does
  not imply that the response came from the platform's IP address, and the
  evidence records the provider mode.
- Response bodies are capped, sanitized and stored as `http_response_body`
  evidence. The raw provider JSON is not stored unless it is small enough to be
  part of the evidence summary.

## Capability

`GET /v1/capabilities` reports `web.firecrawl` with provider
`firecrawl-mcp` or `firecrawl-api`. When neither transport is configured the
state is `unavailable` and the detail says how to enable it. The UI disables
the integration dialog in that case.

## Failure behaviour

| Failure | Result |
| --- | --- |
| No key or token | capability `unavailable`, run blocked before queueing |
| Provider rate limit | step blocked with the provider message |
| Invalid URL | request rejected before the call |
| Private target | blocked with an explicit reason |
| Empty Markdown | HTML fallback, then evidence may be metadata only |
| MCP schema change | capability probe reports the tools actually present |
