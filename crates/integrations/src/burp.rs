//! Optional Burp integration over MCP.
//!
//! The adapter never assumes a CLI. It connects to a configured MCP endpoint,
//! lists the tools the installed edition actually exposes, and imports proxy
//! history through the selected tool. Importing does not create new target
//! connections; the executor still checks scope before retaining an exchange.

use darkbox_contracts::{Capability, CapabilityState};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::mcp::{McpConfig, McpConnection};

#[derive(Debug, thiserror::Error)]
pub enum BurpError {
    #[error("burp MCP is not configured")]
    Unavailable,
    #[error("mcp: {0}")]
    Mcp(#[from] crate::mcp::McpError),
    #[error("the installed Burp/MCP edition exposes no proxy import tool")]
    NoImportTool,
    #[error("invalid exchange: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurpStatus {
    pub available: bool,
    pub server: String,
    pub version: String,
    pub tools: Vec<String>,
    pub import_tool: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BurpExchange {
    pub method: String,
    pub url: String,
    pub request_headers: Vec<(String, String)>,
    pub request_body: Option<String>,
    pub status: Option<u16>,
    pub response_headers: Vec<(String, String)>,
    pub response_body: Option<String>,
    pub captured_at: Option<String>,
    pub note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct BurpAdapter {
    config: McpConfig,
    tool: Option<String>,
}

impl BurpAdapter {
    pub fn from_env() -> Result<Self, BurpError> {
        let url = std::env::var("DARKBOX_BURP_MCP_URL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .ok_or(BurpError::Unavailable)?;
        let token = std::env::var("DARKBOX_BURP_MCP_TOKEN")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let tool = std::env::var("DARKBOX_BURP_MCP_TOOL")
            .ok()
            .filter(|value| !value.trim().is_empty());
        Ok(Self {
            config: McpConfig::new(url, token),
            tool,
        })
    }

    pub fn capability() -> Capability {
        match Self::from_env() {
            Ok(_) => Capability {
                id: "import.burp".into(),
                provider: "burp-mcp".into(),
                version: "configured".into(),
                state: CapabilityState::Degraded,
                detail: "MCP endpoint configured; capabilities are verified at call time because Burp editions expose different tools".into(),
            },
            Err(_) => Capability {
                id: "import.burp".into(),
                provider: "burp-mcp".into(),
                version: "not configured".into(),
                state: CapabilityState::Unavailable,
                detail: "set DARKBOX_BURP_MCP_URL to a running Burp MCP server. Import remains optional; the core does not require Burp.".into(),
            },
        }
    }

    pub async fn probe(&self) -> Result<BurpStatus, BurpError> {
        let connection = McpConnection::connect(self.config.clone()).await?;
        let tools: Vec<String> = connection
            .tools
            .iter()
            .map(|tool| tool.name.clone())
            .collect();
        let import_tool = self
            .tool
            .clone()
            .filter(|name| connection.tool(name).is_some())
            .or_else(|| {
                connection
                    .find_tool(&["proxy", "history"])
                    .map(|tool| tool.name.clone())
            })
            .or_else(|| {
                connection
                    .find_tool(&["proxy"])
                    .map(|tool| tool.name.clone())
            })
            .or_else(|| {
                connection
                    .find_tool(&["http", "history"])
                    .map(|tool| tool.name.clone())
            });
        Ok(BurpStatus {
            available: import_tool.is_some(),
            server: connection.server.name.clone(),
            version: connection.server.version.clone(),
            tools,
            import_tool: import_tool.clone(),
            detail: match import_tool {
                Some(tool) => format!(
                    "connected to {} {} via MCP; importing through {tool}",
                    connection.server.name, connection.server.version
                ),
                None => "connected, but no proxy history tool is exposed".into(),
            },
        })
    }

    pub async fn import(&self, limit: u32) -> Result<Vec<BurpExchange>, BurpError> {
        let mut connection = McpConnection::connect(self.config.clone()).await?;
        let tool = self
            .tool
            .clone()
            .filter(|name| connection.tool(name).is_some())
            .or_else(|| {
                connection
                    .find_tool(&["proxy", "history"])
                    .map(|tool| tool.name.clone())
            })
            .or_else(|| {
                connection
                    .find_tool(&["proxy"])
                    .map(|tool| tool.name.clone())
            })
            .ok_or(BurpError::NoImportTool)?;
        let value = connection
            .call_tool(&tool, json!({ "limit": limit.clamp(1, 500) }))
            .await?;
        Ok(parse_exchanges(&value))
    }
}

/// Accepts the shapes Burp MCP bridges commonly return: a bare array, an object
/// with `data`/`items`/`history`, or one exchange per object.
pub fn parse_exchanges(value: &Value) -> Vec<BurpExchange> {
    let items = if let Some(array) = value.as_array() {
        array.clone()
    } else {
        for key in [
            "items",
            "history",
            "entries",
            "exchanges",
            "data",
            "messages",
        ] {
            if let Some(array) = value.get(key).and_then(|value| value.as_array()) {
                return array
                    .iter()
                    .filter_map(exchange_from_value)
                    .collect::<Vec<_>>();
            }
        }
        vec![value.clone()]
    };
    items
        .iter()
        .filter_map(exchange_from_value)
        .collect::<Vec<_>>()
}

fn exchange_from_value(value: &Value) -> Option<BurpExchange> {
    let object = value.as_object()?;
    let request_value = object
        .get("request")
        .or_else(|| object.get("request_raw"))
        .or_else(|| object.get("raw_request"));
    let response_value = object
        .get("response")
        .or_else(|| object.get("response_raw"))
        .or_else(|| object.get("raw_response"));

    let mut exchange = BurpExchange::default();
    if let Some(request) = request_value {
        match request {
            Value::String(raw) => apply_request_raw(&mut exchange, raw),
            Value::Object(request) => apply_request_object(&mut exchange, request),
            _ => {}
        }
    }
    if let Some(response) = response_value {
        match response {
            Value::String(raw) => apply_response_raw(&mut exchange, raw),
            Value::Object(response) => apply_response_object(&mut exchange, response),
            _ => {}
        }
    }
    if exchange.method.is_empty()
        && let Some(method) = object.get("method").and_then(|value| value.as_str())
    {
        exchange.method = method.to_ascii_uppercase();
    }
    if exchange.url.is_empty()
        && let Some(url) = object.get("url").and_then(|value| value.as_str())
    {
        exchange.url = url.to_string();
    }
    if exchange.url.is_empty() && exchange.method.is_empty() {
        return None;
    }
    if exchange.method.is_empty() {
        exchange.method = "GET".into();
    }
    exchange.captured_at = object
        .get("time")
        .or_else(|| object.get("captured_at"))
        .or_else(|| object.get("timestamp"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    exchange.note = object
        .get("notes")
        .or_else(|| object.get("comment"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    Some(exchange)
}

fn apply_request_object(exchange: &mut BurpExchange, request: &serde_json::Map<String, Value>) {
    if let Some(method) = request.get("method").and_then(|value| value.as_str()) {
        exchange.method = method.to_ascii_uppercase();
    }
    if let Some(url) = request.get("url").and_then(|value| value.as_str()) {
        exchange.url = url.to_string();
    }
    if let Some(headers) = request.get("headers") {
        exchange.request_headers = parse_headers(headers);
    }
    if let Some(body) = request.get("body").or_else(|| request.get("body_text")) {
        exchange.request_body = body.as_str().map(str::to_string);
    }
}

fn apply_response_object(exchange: &mut BurpExchange, response: &serde_json::Map<String, Value>) {
    if let Some(status) = response
        .get("status")
        .or_else(|| response.get("status_code"))
    {
        exchange.status = status
            .as_u64()
            .map(|value| value as u16)
            .or_else(|| status.as_str().and_then(|value| value.parse().ok()));
    }
    if let Some(headers) = response.get("headers") {
        exchange.response_headers = parse_headers(headers);
    }
    if let Some(body) = response.get("body").or_else(|| response.get("body_text")) {
        exchange.response_body = body.as_str().map(str::to_string);
    }
}

fn apply_request_raw(exchange: &mut BurpExchange, raw: &str) {
    let (head, body) = split_message(raw);
    let mut lines = head.lines();
    if let Some(request_line) = lines.next() {
        let mut parts = request_line.split_whitespace();
        exchange.method = parts.next().unwrap_or("GET").to_ascii_uppercase();
        exchange.url = parts.next().unwrap_or("").to_string();
    }
    exchange.request_headers = parse_header_lines(lines);
    if !body.is_empty() {
        exchange.request_body = Some(body.to_string());
    }
}

fn apply_response_raw(exchange: &mut BurpExchange, raw: &str) {
    let (head, body) = split_message(raw);
    let mut lines = head.lines();
    if let Some(status_line) = lines.next() {
        exchange.status = status_line
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse().ok());
    }
    exchange.response_headers = parse_header_lines(lines);
    if !body.is_empty() {
        exchange.response_body = Some(body.to_string());
    }
}

fn split_message(raw: &str) -> (&str, &str) {
    raw.split_once("\r\n\r\n")
        .or_else(|| raw.split_once("\n\n"))
        .unwrap_or((raw, ""))
}

fn parse_header_lines<'a>(lines: impl Iterator<Item = &'a str>) -> Vec<(String, String)> {
    lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.trim().to_string(), value.trim().to_string()))
        })
        .collect()
}

fn parse_headers(value: &Value) -> Vec<(String, String)> {
    match value {
        Value::Object(map) => map
            .iter()
            .filter_map(|(name, value)| {
                value
                    .as_str()
                    .map(|value| (name.clone(), value.to_string()))
            })
            .collect(),
        Value::Array(items) => items
            .iter()
            .filter_map(|item| {
                let name = item
                    .get("name")
                    .or_else(|| item.get("key"))
                    .and_then(|value| value.as_str())?;
                let value = item.get("value").and_then(|value| value.as_str())?;
                Some((name.to_string(), value.to_string()))
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_structured_exchanges() {
        let value = json!({
            "data": [{
                "time": "2026-10-21T10:00:00Z",
                "request": {
                    "method": "GET",
                    "url": "https://example.test/a",
                    "headers": {"Host": "example.test", "Cookie": "sid=secret"},
                    "body": null
                },
                "response": {
                    "status": 200,
                    "headers": [{"name": "Content-Type", "value": "text/html"}],
                    "body": "<html>ok</html>"
                }
            }]
        });
        let exchanges = parse_exchanges(&value);
        assert_eq!(exchanges.len(), 1);
        assert_eq!(exchanges[0].method, "GET");
        assert_eq!(exchanges[0].status, Some(200));
        assert_eq!(exchanges[0].response_headers[0].1, "text/html");
    }

    #[test]
    fn parses_raw_http_messages() {
        let value = json!({
            "request": "POST /login HTTP/1.1\r\nHost: example.test\r\nContent-Type: application/x-www-form-urlencoded\r\n\r\nuser=a",
            "response": "HTTP/1.1 302 Found\r\nLocation: /app\r\n\r\n"
        });
        let exchanges = parse_exchanges(&value);
        assert_eq!(exchanges.len(), 1);
        assert_eq!(exchanges[0].method, "POST");
        assert_eq!(exchanges[0].url, "/login");
        assert_eq!(exchanges[0].status, Some(302));
        assert_eq!(exchanges[0].request_body.as_deref(), Some("user=a"));
    }

    #[test]
    fn ignores_empty_objects() {
        assert!(parse_exchanges(&json!({})).is_empty());
    }
}
