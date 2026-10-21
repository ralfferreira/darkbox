//! Minimal Model Context Protocol client.
//!
//! Supports both transports DarkBox needs:
//! - streamable HTTP, used by the Firecrawl hosted server;
//! - legacy HTTP+SSE, used by the Burp MCP extension.
//!
//! Only initialize, tools/list and tools/call are implemented. Responses are
//! treated as untrusted JSON and capped before parsing.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::sync::{Mutex, oneshot};

pub const PROTOCOL_VERSION: &str = "2025-03-26";
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("http: {0}")]
    Http(#[from] reqwest::Error),
    #[error("server returned {status}: {body}")]
    Status { status: u16, body: String },
    #[error("protocol: {0}")]
    Protocol(String),
    #[error("tool {0} not found")]
    ToolNotFound(String),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct McpConfig {
    pub url: String,
    pub token: Option<String>,
    pub timeout: Duration,
}

impl McpConfig {
    pub fn new(url: impl Into<String>, token: Option<String>) -> Self {
        Self {
            url: url.into(),
            token,
            timeout: Duration::from_secs(25),
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Burp's extension serves SSE at `/sse` when it is configured with the
    /// host root. The adapter probes both forms.
    fn sse_candidates(&self) -> Vec<String> {
        let mut candidates = vec![self.url.clone()];
        let trimmed = self.url.trim_end_matches('/');
        if !trimmed.ends_with("/sse") {
            candidates.push(format!("{trimmed}/sse"));
        }
        candidates
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "inputSchema")]
    pub input_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerInfo {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
}

type PendingMap = Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Value, String>>>>>;

#[derive(Clone)]
struct SseState {
    endpoint: Arc<Mutex<Option<String>>>,
    pending: PendingMap,
    base_url: String,
}

pub struct McpConnection {
    config: McpConfig,
    http: reqwest::Client,
    session_id: Option<String>,
    next_id: AtomicU64,
    sse: Option<SseState>,
    pub server: McpServerInfo,
    pub protocol_version: String,
    pub tools: Vec<McpTool>,
}

impl McpConnection {
    /// Connects with streamable HTTP first. If that transport is not served,
    /// falls back to the legacy SSE transport.
    pub async fn connect(config: McpConfig) -> Result<Self, McpError> {
        let force_sse = std::env::var("DARKBOX_MCP_TRANSPORT")
            .map(|value| value.eq_ignore_ascii_case("sse"))
            .unwrap_or(false);
        if force_sse || config.url.trim_end_matches('/').ends_with("/sse") {
            return Self::connect_sse(config).await;
        }
        match Self::connect_http(config.clone()).await {
            Ok(connection) => Ok(connection),
            Err(http_error) => Self::connect_sse(config).await.map_err(|sse_error| {
                McpError::Protocol(format!(
                    "streamable HTTP failed ({http_error}); SSE failed ({sse_error})"
                ))
            }),
        }
    }

    async fn connect_http(config: McpConfig) -> Result<Self, McpError> {
        let http = reqwest::Client::builder().timeout(config.timeout).build()?;
        let mut connection = Self {
            config,
            http,
            session_id: None,
            next_id: AtomicU64::new(1),
            sse: None,
            server: McpServerInfo {
                name: "unknown".into(),
                version: "unknown".into(),
            },
            protocol_version: PROTOCOL_VERSION.into(),
            tools: Vec::new(),
        };
        connection.initialize().await?;
        connection.load_tools().await?;
        Ok(connection)
    }

    async fn connect_sse(config: McpConfig) -> Result<Self, McpError> {
        let http = reqwest::Client::builder().timeout(config.timeout).build()?;
        let mut last_error = None;
        for candidate in config.sse_candidates() {
            match Self::open_sse(config.clone(), http.clone(), &candidate).await {
                Ok(connection) => return Ok(connection),
                Err(error) => last_error = Some(error),
            }
        }
        Err(last_error.unwrap_or_else(|| McpError::Protocol("no SSE endpoint".into())))
    }

    async fn open_sse(
        config: McpConfig,
        http: reqwest::Client,
        candidate: &str,
    ) -> Result<Self, McpError> {
        let mut request = http
            .get(candidate)
            .header(reqwest::header::ACCEPT, "text/event-stream");
        if let Some(token) = &config.token {
            request = request.bearer_auth(token);
        }
        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(McpError::Status {
                status: response.status().as_u16(),
                body: String::new(),
            });
        }
        let state = SseState {
            endpoint: Arc::new(Mutex::new(None)),
            pending: Arc::new(Mutex::new(HashMap::new())),
            base_url: candidate.to_string(),
        };
        let reader_state = state.clone();
        tokio::spawn(async move {
            read_sse(response, reader_state).await;
        });
        let mut endpoint = None;
        for _ in 0..100 {
            if let Some(value) = state.endpoint.lock().await.clone() {
                endpoint = Some(value);
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let endpoint = endpoint.ok_or_else(|| {
            McpError::Protocol("SSE server did not announce its message endpoint".into())
        })?;
        *state.endpoint.lock().await = Some(endpoint);

        let mut connection = Self {
            config,
            http,
            session_id: None,
            next_id: AtomicU64::new(1),
            sse: Some(state),
            server: McpServerInfo {
                name: "unknown".into(),
                version: "unknown".into(),
            },
            protocol_version: PROTOCOL_VERSION.into(),
            tools: Vec::new(),
        };
        connection.initialize().await?;
        connection.load_tools().await?;
        Ok(connection)
    }

    async fn initialize(&mut self) -> Result<(), McpError> {
        let id = self.next_id();
        let response = self
            .request(
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": PROTOCOL_VERSION,
                        "capabilities": {},
                        "clientInfo": { "name": "darkbox", "version": env!("CARGO_PKG_VERSION") }
                    }
                }),
                true,
            )
            .await?;
        let result = result_of(&response)?;
        self.server =
            serde_json::from_value(result.get("serverInfo").cloned().unwrap_or(json!({})))
                .unwrap_or(McpServerInfo {
                    name: "unknown".into(),
                    version: "unknown".into(),
                });
        self.protocol_version = result
            .get("protocolVersion")
            .and_then(|value| value.as_str())
            .unwrap_or(PROTOCOL_VERSION)
            .to_string();
        let _ = self
            .notify(json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized",
                "params": {}
            }))
            .await;
        Ok(())
    }

    async fn load_tools(&mut self) -> Result<(), McpError> {
        let id = self.next_id();
        let listed = self
            .request(
                json!({ "jsonrpc": "2.0", "id": id, "method": "tools/list", "params": {} }),
                false,
            )
            .await?;
        let result = result_of(&listed)?;
        self.tools =
            serde_json::from_value(result.get("tools").cloned().unwrap_or_else(|| json!([])))
                .unwrap_or_default();
        Ok(())
    }

    fn next_id(&self) -> u64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    pub fn tool(&self, name: &str) -> Option<&McpTool> {
        self.tools.iter().find(|tool| tool.name == name)
    }

    pub fn find_tool(&self, needles: &[&str]) -> Option<&McpTool> {
        self.tools.iter().find(|tool| {
            let name = tool.name.to_ascii_lowercase();
            needles.iter().all(|needle| name.contains(needle))
        })
    }

    pub fn transport(&self) -> &'static str {
        if self.sse.is_some() { "sse" } else { "http" }
    }

    pub async fn call_tool(&mut self, name: &str, arguments: Value) -> Result<Value, McpError> {
        if self.tool(name).is_none() {
            return Err(McpError::ToolNotFound(name.into()));
        }
        let id = self.next_id();
        let response = self
            .request(
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "method": "tools/call",
                    "params": { "name": name, "arguments": arguments }
                }),
                false,
            )
            .await?;
        let result = result_of(&response)?;
        if let Some(value) = result.get("structuredContent") {
            return Ok(value.clone());
        }
        if let Some(content) = result.get("content").and_then(|value| value.as_array()) {
            for item in content {
                if item.get("type").and_then(|value| value.as_str()) == Some("text")
                    && let Some(text) = item.get("text").and_then(|value| value.as_str())
                {
                    if let Ok(parsed) = serde_json::from_str::<Value>(text) {
                        return Ok(parsed);
                    }
                    return Ok(Value::String(text.to_string()));
                }
            }
        }
        Ok(result.clone())
    }

    async fn notify(&self, payload: Value) -> Result<(), McpError> {
        if self.sse.is_some() {
            return self.sse_post(payload, false).await.map(|_| ());
        }
        let mut request = self.post_request(payload);
        if let Some(session) = &self.session_id {
            request = request.header("Mcp-Session-Id", session);
        }
        let response = request.send().await?;
        if !response.status().is_success() {
            return Err(McpError::Status {
                status: response.status().as_u16(),
                body: response.text().await.unwrap_or_default(),
            });
        }
        Ok(())
    }

    async fn request(&mut self, payload: Value, capture_session: bool) -> Result<Value, McpError> {
        if self.sse.is_some() {
            return self
                .sse_post(payload, true)
                .await?
                .ok_or_else(|| McpError::Protocol("SSE response was not delivered".into()));
        }
        let mut request = self.post_request(payload.clone());
        if let Some(session) = &self.session_id {
            request = request.header("Mcp-Session-Id", session);
        }
        let response = request.send().await?;
        let status = response.status();
        if capture_session
            && let Some(session) = response
                .headers()
                .get("mcp-session-id")
                .and_then(|value| value.to_str().ok())
        {
            self.session_id = Some(session.to_string());
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = response.text().await?;
        if body.len() > MAX_RESPONSE_BYTES {
            return Err(McpError::Protocol("response exceeded size limit".into()));
        }
        if !status.is_success() {
            return Err(McpError::Status {
                status: status.as_u16(),
                body: body.chars().take(1000).collect(),
            });
        }
        let id = payload.get("id").and_then(|value| value.as_u64());
        if content_type.contains("text/event-stream") || body.trim_start().starts_with("event:") {
            return parse_sse(&body, id);
        }
        let value: Value = serde_json::from_str(&body)?;
        Ok(value)
    }

    async fn sse_post(
        &self,
        payload: Value,
        expect_response: bool,
    ) -> Result<Option<Value>, McpError> {
        let state = self
            .sse
            .as_ref()
            .ok_or_else(|| McpError::Protocol("not an SSE connection".into()))?;
        let endpoint = state
            .endpoint
            .lock()
            .await
            .clone()
            .ok_or_else(|| McpError::Protocol("SSE endpoint not ready".into()))?;
        let id = payload.get("id").and_then(|value| value.as_u64());
        let receiver = if expect_response {
            let id = id.ok_or_else(|| McpError::Protocol("request without id".into()))?;
            let (sender, receiver) = oneshot::channel();
            state.pending.lock().await.insert(id, sender);
            Some(receiver)
        } else {
            None
        };
        let mut request = self.http.post(&endpoint).json(&payload);
        if let Some(token) = &self.config.token {
            request = request.bearer_auth(token);
        }
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => {
                if let Some(id) = id {
                    state.pending.lock().await.remove(&id);
                }
                return Err(McpError::Http(error));
            }
        };
        if !response.status().is_success() {
            if let Some(id) = id {
                state.pending.lock().await.remove(&id);
            }
            return Err(McpError::Status {
                status: response.status().as_u16(),
                body: response.text().await.unwrap_or_default(),
            });
        }
        let body = response.text().await.unwrap_or_default();
        if expect_response && !body.trim().is_empty() {
            let value: Value = serde_json::from_str(&body)?;
            if let Some(id) = id {
                state.pending.lock().await.remove(&id);
            }
            return Ok(Some(value));
        }
        match receiver {
            Some(receiver) => match tokio::time::timeout(Duration::from_secs(30), receiver).await {
                Ok(Ok(Ok(value))) => Ok(Some(value)),
                Ok(Ok(Err(message))) => Err(McpError::Protocol(message)),
                Ok(Err(_)) => Err(McpError::Protocol("SSE connection closed".into())),
                Err(_) => Err(McpError::Protocol("SSE response timed out".into())),
            },
            None => Ok(None),
        }
    }

    fn post_request(&self, payload: Value) -> reqwest::RequestBuilder {
        let mut request = self
            .http
            .post(&self.config.url)
            .header(
                reqwest::header::ACCEPT,
                "application/json, text/event-stream",
            )
            .header("MCP-Protocol-Version", &self.protocol_version)
            .json(&payload);
        if let Some(token) = &self.config.token {
            request = request.bearer_auth(token);
        }
        request
    }
}

async fn read_sse(response: reqwest::Response, state: SseState) {
    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    while let Some(chunk) = stream.next().await {
        let Ok(chunk) = chunk else { break };
        buffer.push_str(&String::from_utf8_lossy(&chunk).replace("\r\n", "\n"));
        while let Some(index) = buffer.find("\n\n") {
            let block = buffer[..index].to_string();
            buffer.drain(..index + 2);
            if let Some((event, data)) = parse_block(&block) {
                if event == "endpoint" {
                    let target = resolve_endpoint(&state.base_url, &data);
                    *state.endpoint.lock().await = Some(target);
                    continue;
                }
                if data.is_empty() {
                    continue;
                }
                let Ok(value) = serde_json::from_str::<Value>(&data) else {
                    continue;
                };
                if let Some(id) = value.get("id").and_then(|value| value.as_u64())
                    && let Some(sender) = state.pending.lock().await.remove(&id)
                {
                    let _ = sender.send(Ok(value));
                }
            }
        }
    }
    let pending = std::mem::take(&mut *state.pending.lock().await);
    for (_, sender) in pending {
        let _ = sender.send(Err("SSE stream closed".into()));
    }
}

fn parse_block(block: &str) -> Option<(String, String)> {
    let mut event = "message".to_string();
    let mut data = Vec::new();
    for line in block.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event = value.trim().to_string();
        } else if let Some(value) = line.strip_prefix("data:") {
            data.push(value.trim_start().to_string());
        }
    }
    if data.is_empty() {
        None
    } else {
        Some((event, data.join("\n")))
    }
}

fn resolve_endpoint(base: &str, endpoint: &str) -> String {
    if endpoint.starts_with("http://") || endpoint.starts_with("https://") {
        endpoint.to_string()
    } else if let Ok(base) = url::Url::parse(base) {
        base.join(endpoint)
            .map(|url| url.to_string())
            .unwrap_or_else(|_| endpoint.to_string())
    } else {
        endpoint.to_string()
    }
}

fn result_of(response: &Value) -> Result<Value, McpError> {
    if let Some(error) = response.get("error") {
        return Err(McpError::Protocol(format!("{error}")));
    }
    response
        .get("result")
        .cloned()
        .ok_or_else(|| McpError::Protocol("missing result".into()))
}

fn parse_sse(body: &str, id: Option<u64>) -> Result<Value, McpError> {
    let mut fallback = None;
    for line in body.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let Ok(value) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        if let Some(id) = id
            && value.get("id").and_then(|value| value.as_u64()) == Some(id)
        {
            return Ok(value);
        }
        fallback = Some(value);
    }
    fallback.ok_or_else(|| McpError::Protocol("SSE stream had no JSON-RPC message".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_json_rpc_error() {
        let value = json!({"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"nope"}});
        assert!(result_of(&value).is_err());
    }

    #[test]
    fn parses_sse_matching_id() {
        let body =
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{\"ok\":true}}\n\n";
        let value = parse_sse(body, Some(2)).unwrap();
        assert_eq!(value["result"]["ok"], true);
    }

    #[test]
    fn parses_sse_without_id() {
        let body = "data: {\"jsonrpc\":\"2.0\",\"result\":{\"tools\":[]}}\n\n";
        let value = parse_sse(body, Some(3)).unwrap();
        assert!(value["result"]["tools"].is_array());
    }

    #[test]
    fn parses_event_blocks_and_endpoints() {
        let (event, data) = parse_block("event: endpoint\ndata: /message?sessionId=abc").unwrap();
        assert_eq!(event, "endpoint");
        assert_eq!(
            resolve_endpoint("http://127.0.0.1:9876/sse", &data),
            "http://127.0.0.1:9876/message?sessionId=abc"
        );
    }

    #[test]
    fn finds_a_tool_by_keywords() {
        let connection = McpConnection {
            config: McpConfig::new("http://127.0.0.1:1", None),
            http: reqwest::Client::new(),
            session_id: None,
            next_id: AtomicU64::new(1),
            sse: None,
            server: McpServerInfo {
                name: "mock".into(),
                version: "1".into(),
            },
            protocol_version: PROTOCOL_VERSION.into(),
            tools: vec![McpTool {
                name: "burp_get_proxy_history".into(),
                description: String::new(),
                input_schema: json!({}),
            }],
        };
        assert!(connection.find_tool(&["proxy", "history"]).is_some());
        assert!(connection.find_tool(&["nmap"]).is_none());
        assert_eq!(connection.transport(), "http");
    }
}
