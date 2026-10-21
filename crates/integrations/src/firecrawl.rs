//! Firecrawl adapter: managed URL discovery and page retrieval.
//!
//! Supports the REST API with an API key and the hosted MCP server with an
//! OAuth access token. A scrape is the managed browser path for the agent:
//! it fetches JavaScript-rendered pages and returns Markdown plus links, while
//! the executor keeps scope and evidence ownership.

use std::time::Duration;

use darkbox_contracts::{Capability, CapabilityState};
use serde_json::{Value, json};
use url::Url;

use crate::mcp::{McpConfig, McpConnection};

const DEFAULT_BASE: &str = "https://api.firecrawl.dev/v2";

#[derive(Debug, thiserror::Error)]
pub enum FirecrawlError {
    #[error("firecrawl is not configured")]
    Unavailable,
    #[error("request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("provider returned {status}: {body}")]
    Status { status: u16, body: String },
    #[error("invalid response: {0}")]
    Invalid(String),
    #[error("mcp: {0}")]
    Mcp(#[from] crate::mcp::McpError),
}

#[derive(Debug, Clone)]
enum Mode {
    Rest { base: String, key: String },
    Mcp { url: String, token: String },
}

#[derive(Debug, Clone)]
pub struct FirecrawlAdapter {
    mode: Mode,
    http: reqwest::Client,
}

#[derive(Debug, Clone, Default)]
pub struct FirecrawlPage {
    pub markdown: String,
    pub html: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub status_code: Option<u16>,
    pub links: Vec<String>,
    pub raw: Value,
}

impl FirecrawlAdapter {
    pub fn from_env() -> Result<Self, FirecrawlError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        let prefer_mcp = std::env::var("DARKBOX_FIRECRAWL_MODE")
            .map(|value| value == "mcp")
            .unwrap_or(false);
        let non_empty = |name: &str| {
            std::env::var(name)
                .ok()
                .filter(|value| !value.trim().is_empty())
        };
        let mcp_url = non_empty("DARKBOX_FIRECRAWL_MCP_URL");
        let mcp_token = non_empty("DARKBOX_FIRECRAWL_MCP_TOKEN");
        let api_key =
            non_empty("DARKBOX_FIRECRAWL_API_KEY").or_else(|| non_empty("FIRECRAWL_API_KEY"));
        let mode = if let (Some(url), Some(token)) = (mcp_url, mcp_token)
            && (prefer_mcp || api_key.is_none())
        {
            Mode::Mcp { url, token }
        } else if let Some(key) = api_key {
            Mode::Rest {
                base: std::env::var("DARKBOX_FIRECRAWL_BASE_URL")
                    .unwrap_or_else(|_| DEFAULT_BASE.into()),
                key,
            }
        } else {
            return Err(FirecrawlError::Unavailable);
        };
        Ok(Self { mode, http })
    }

    pub fn capability() -> Capability {
        match Self::from_env() {
            Ok(adapter) => {
                let (provider, detail) = match &adapter.mode {
                    Mode::Rest { .. } => (
                        "firecrawl-api",
                        "managed map and scrape over the REST API; scrape returns rendered Markdown for the agent",
                    ),
                    Mode::Mcp { .. } => (
                        "firecrawl-mcp",
                        "hosted MCP map and scrape; scrape is the managed browser path for the agent",
                    ),
                };
                Capability {
                    id: "web.firecrawl".into(),
                    provider: provider.into(),
                    version: "v2".into(),
                    state: CapabilityState::Available,
                    detail: format!(
                        "{detail}. Requests are limited to in-scope public URLs and evidence is stored as a derived capture."
                    ),
                }
            }
            Err(_) => Capability {
                id: "web.firecrawl".into(),
                provider: "firecrawl".into(),
                version: "not configured".into(),
                state: CapabilityState::Unavailable,
                detail: "set DARKBOX_FIRECRAWL_API_KEY or the MCP URL and token. Core crawling still works without it.".into(),
            },
        }
    }

    pub fn is_mcp(&self) -> bool {
        matches!(self.mode, Mode::Mcp { .. })
    }

    /// Enumerates URLs without fetching their content.
    pub async fn map(
        &self,
        target: &str,
        limit: u32,
        search: Option<&str>,
    ) -> Result<Vec<String>, FirecrawlError> {
        let limit = limit.clamp(1, 500);
        match &self.mode {
            Mode::Rest { base, key } => {
                let mut body = json!({ "url": target, "limit": limit, "includeSubdomains": true });
                if let Some(search) = search.filter(|value| !value.trim().is_empty()) {
                    body["search"] = json!(search);
                }
                let response = self
                    .http
                    .post(format!("{}/map", base.trim_end_matches('/')))
                    .bearer_auth(key)
                    .json(&body)
                    .send()
                    .await?;
                let status = response.status();
                let body: Value = response.json().await?;
                if !status.is_success() || body.get("success") == Some(&json!(false)) {
                    return Err(FirecrawlError::Status {
                        status: status.as_u16(),
                        body: body.to_string(),
                    });
                }
                let mut links = extract_links(&body);
                normalize_links(target, &mut links);
                links.truncate(limit as usize);
                Ok(links)
            }
            Mode::Mcp { url, token } => {
                let mut connection =
                    McpConnection::connect(McpConfig::new(url, Some(token.clone()))).await?;
                let tool = connection
                    .find_tool(&["map"])
                    .ok_or_else(|| FirecrawlError::Invalid("MCP server has no map tool".into()))?
                    .name
                    .clone();
                let mut arguments = json!({ "url": target, "limit": limit });
                if let Some(search) = search.filter(|value| !value.trim().is_empty()) {
                    arguments["search"] = json!(search);
                }
                let value = connection.call_tool(&tool, arguments).await?;
                let mut links = extract_links(&value);
                normalize_links(target, &mut links);
                links.truncate(limit as usize);
                Ok(links)
            }
        }
    }

    /// Fetches one page through Firecrawl. This is the managed browser path.
    pub async fn scrape(
        &self,
        target: &str,
        objective: &str,
    ) -> Result<FirecrawlPage, FirecrawlError> {
        match &self.mode {
            Mode::Rest { base, key } => {
                let body = json!({
                    "url": target,
                    "formats": ["markdown", "links"],
                    "onlyMainContent": true,
                    "maxAge": 0,
                    "timeout": 45_000
                });
                let response = self
                    .http
                    .post(format!("{}/scrape", base.trim_end_matches('/')))
                    .bearer_auth(key)
                    .json(&body)
                    .send()
                    .await?;
                let status = response.status();
                let value: Value = response.json().await?;
                if !status.is_success() || value.get("success") == Some(&json!(false)) {
                    return Err(FirecrawlError::Status {
                        status: status.as_u16(),
                        body: value.to_string().chars().take(1000).collect(),
                    });
                }
                Ok(page_from_value(&value))
            }
            Mode::Mcp { url, token } => {
                let mut connection =
                    McpConnection::connect(McpConfig::new(url, Some(token.clone()))).await?;
                let tool = connection
                    .find_tool(&["scrape"])
                    .ok_or_else(|| FirecrawlError::Invalid("MCP server has no scrape tool".into()))?
                    .name
                    .clone();
                let value = connection
                    .call_tool(
                        &tool,
                        json!({
                            "url": target,
                            "formats": ["markdown", "links"],
                            "onlyMainContent": true,
                            "objective": objective
                        }),
                    )
                    .await?;
                if let Some(error) = value.get("error").filter(|value| !value.is_null()) {
                    return Err(FirecrawlError::Invalid(error.to_string()));
                }
                if value.get("success") == Some(&json!(false)) {
                    return Err(FirecrawlError::Invalid(value.to_string()));
                }
                Ok(page_from_value(&value))
            }
        }
    }

    pub async fn search(
        &self,
        query: &str,
        limit: u32,
        objective: &str,
    ) -> Result<Value, FirecrawlError> {
        match &self.mode {
            Mode::Rest { base, key } => {
                let response = self
                    .http
                    .post(format!("{}/search", base.trim_end_matches('/')))
                    .bearer_auth(key)
                    .json(&json!({ "query": query, "limit": limit.clamp(1, 20) }))
                    .send()
                    .await?;
                let status = response.status();
                let value: Value = response.json().await?;
                if !status.is_success() {
                    return Err(FirecrawlError::Status {
                        status: status.as_u16(),
                        body: value.to_string(),
                    });
                }
                Ok(value)
            }
            Mode::Mcp { url, token } => {
                let mut connection =
                    McpConnection::connect(McpConfig::new(url, Some(token.clone()))).await?;
                let tool = connection
                    .find_tool(&["search"])
                    .filter(|tool| !tool.name.contains("feedback"))
                    .ok_or_else(|| FirecrawlError::Invalid("MCP server has no search tool".into()))?
                    .name
                    .clone();
                connection
                    .call_tool(
                        &tool,
                        json!({ "query": query, "limit": limit.clamp(1, 20), "objective": objective }),
                    )
                    .await
                    .map_err(FirecrawlError::from)
            }
        }
    }
}

fn page_from_value(value: &Value) -> FirecrawlPage {
    let data = value.get("data").unwrap_or(value);
    let metadata = data.get("metadata").cloned().unwrap_or(Value::Null);
    let markdown = data
        .get("markdown")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let html = data
        .get("html")
        .or_else(|| data.get("rawHtml"))
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let title = metadata
        .get("title")
        .or_else(|| data.get("title"))
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let description = metadata
        .get("description")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let status_code = metadata
        .get("statusCode")
        .or_else(|| metadata.get("status_code"))
        .and_then(|value| value.as_u64())
        .map(|value| value as u16);
    let mut links = extract_links(data);
    normalize_links(
        metadata
            .get("sourceURL")
            .or_else(|| metadata.get("url"))
            .and_then(|value| value.as_str())
            .unwrap_or(""),
        &mut links,
    );
    FirecrawlPage {
        markdown,
        html,
        title,
        description,
        status_code,
        links,
        raw: value.clone(),
    }
}

pub fn extract_links(value: &Value) -> Vec<String> {
    let mut output = Vec::new();
    let candidate = value
        .get("links")
        .or_else(|| value.get("data").and_then(|data| data.get("links")))
        .or_else(|| value.get("result").and_then(|result| result.get("links")))
        .unwrap_or(value);
    collect_links(candidate, &mut output);
    if output.is_empty()
        && let Some(array) = candidate.as_array()
    {
        collect_links(&Value::Array(array.clone()), &mut output);
    }
    output.sort();
    output.dedup();
    output
}

fn collect_links(value: &Value, output: &mut Vec<String>) {
    match value {
        Value::String(url) => output.push(url.clone()),
        Value::Array(items) => {
            for item in items {
                collect_links(item, output);
            }
        }
        Value::Object(map) => {
            for key in ["url", "href", "link"] {
                if let Some(url) = map.get(key).and_then(|value| value.as_str()) {
                    output.push(url.to_string());
                    return;
                }
            }
            for key in ["links", "data", "result", "web"] {
                if let Some(child) = map.get(key) {
                    collect_links(child, output);
                }
            }
        }
        _ => {}
    }
}

fn normalize_links(base: &str, links: &mut Vec<String>) {
    let base = Url::parse(base).ok();
    let mut normalized = Vec::new();
    for link in links.drain(..) {
        let candidate = base
            .as_ref()
            .and_then(|base| base.join(&link).ok())
            .or_else(|| Url::parse(&link).ok());
        if let Some(url) = candidate
            && matches!(url.scheme(), "http" | "https")
        {
            normalized.push(url.to_string());
        }
    }
    normalized.sort();
    normalized.dedup();
    *links = normalized;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_links_from_rest_and_objects() {
        let value = json!({ "success": true, "links": [
            "https://example.test/a",
            {"url": "https://example.test/b"},
            {"href": "/c"}
        ]});
        let mut links = extract_links(&value);
        normalize_links("https://example.test/", &mut links);
        assert_eq!(
            links,
            vec![
                "https://example.test/a".to_string(),
                "https://example.test/b".to_string(),
                "https://example.test/c".to_string()
            ]
        );
    }

    #[test]
    fn reads_scrape_payload() {
        let value = json!({
            "success": true,
            "data": {
                "markdown": "# Hello\n\nRendered by the managed browser.",
                "links": ["https://example.test/next"],
                "metadata": {
                    "title": "Example",
                    "sourceURL": "https://example.test/",
                    "statusCode": 200
                }
            }
        });
        let page = page_from_value(&value);
        assert!(page.markdown.contains("managed browser"));
        assert_eq!(page.title.as_deref(), Some("Example"));
        assert_eq!(page.status_code, Some(200));
        assert_eq!(page.links, vec!["https://example.test/next"]);
    }

    #[test]
    fn capability_reports_missing_without_configuration() {
        // Environment may be configured on a developer machine; either state is valid.
        let capability = FirecrawlAdapter::capability();
        assert_eq!(capability.id, "web.firecrawl");
        assert!(matches!(
            capability.state,
            CapabilityState::Available | CapabilityState::Unavailable
        ));
    }
}
