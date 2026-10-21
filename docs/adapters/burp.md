# Burp adapter (MCP)

## Role

Burp is an optional external proxy. DarkBox does not ship it, does not require
it, and does not assume a CLI. The adapter speaks MCP to a running Burp MCP
server and imports proxy history that the operator already captured.

Importing does not create new target traffic. This is deliberate: the Burp
integration cannot become a side channel that bypasses the executor's scope
checks.

## Configuration

```sh
export DARKBOX_BURP_MCP_URL=http://127.0.0.1:9876/mcp
# export DARKBOX_BURP_MCP_TOKEN=...
# export DARKBOX_BURP_MCP_TOOL=burp_get_proxy_history
```

Set the URL to the Burp MCP server root, for example
`http://127.0.0.1:9876`. The adapter tries streamable HTTP first, then the
legacy HTTP+SSE transport used by the PortSwigger extension, probing `/sse`
when the root does not serve it. Responses arrive over the SSE stream and are
matched by JSON-RPC id. No stdio proxy is required.

## Capability probe

On every run and on `GET /v1/capabilities`, the adapter:

1. connects over streamable HTTP or SSE and sends `initialize`, recording the
   server name and version;
2. sends `notifications/initialized`;
3. calls `tools/list`;
4. selects the configured tool, or the first tool whose name contains
   `proxy` and `history`, then `proxy`, then `http history`.

The UI reports the real state:

- `unavailable`: no endpoint configured;
- `degraded`: endpoint configured, tools verified at call time;
- a run is blocked if the installed edition exposes no import tool.

## Import

The selected tool is called with `{ "limit": n }`. The adapter accepts the
shapes MCP bridges commonly return:

- a bare array;
- an object with `data`, `items`, `history`, `entries`, `exchanges` or
  `messages`;
- structured request/response objects;
- raw HTTP request and response strings.

Request lines that contain only a path are made absolute with the `Host` header.
The scheme defaults to `https`, except for loopback hosts, which default to
`http`. `X-Forwarded-Proto` is honored when present.

Every imported exchange is then:

1. checked against the run's pinned policy and the current policy;
2. skipped with a reason if out of scope;
3. sanitized (cookies, authorization, tokens, password fields);
4. stored as `http_exchange` evidence with a hash and retention;
5. linked to an endpoint marked `observed` with source `burp`.

## Limits

- No request generation, no repeater automation, no scanner API.
- No claim that an imported response came from the platform.
- The MCP endpoint is never exposed publicly; it is a local or private network
  dependency.
- A Burp tool that cannot respect scope is not used for import. If the only
  available tool captures traffic from many targets, the operator must filter
  it before import or configure a narrower tool.
