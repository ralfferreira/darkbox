use darkbox_contracts::ContextPackageDto;

use crate::{MemoryError, Result, context_to_prompt};

#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
}

impl ModelConfig {
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("DARKBOX_MODEL_API_KEY").ok()?;
        Some(Self {
            base_url: std::env::var("DARKBOX_MODEL_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".into()),
            api_key,
            model: std::env::var("DARKBOX_MODEL").unwrap_or_else(|_| "gpt-4.1-mini".into()),
        })
    }

    pub fn name(&self) -> String {
        format!("{}@{}", self.model, self.base_url)
    }
}

/// Optional summarizer. It may only restate the supplied context; it never
/// receives evidence bodies, cookies or tokens.
pub async fn summarize(
    config: &ModelConfig,
    package: &ContextPackageDto,
    max_tokens: u32,
) -> Result<String> {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "model": config.model,
        "temperature": 0,
        "max_tokens": max_tokens,
        "messages": [
            {
                "role": "system",
                "content": "You summarize an authorized web assessment for the next operator session. Use only facts present in the supplied context. Keep conflicts visible. Do not assert security impact. Do not invent identifiers. Cite record kinds in parentheses. Return Markdown without a top-level title."
            },
            { "role": "user", "content": context_to_prompt(package) }
        ]
    });
    let response = client
        .post(format!(
            "{}/chat/completions",
            config.base_url.trim_end_matches('/')
        ))
        .bearer_auth(&config.api_key)
        .json(&body)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await
        .map_err(|e| MemoryError::Model(e.to_string()))?;
    if !response.status().is_success() {
        return Err(MemoryError::Model(format!(
            "provider returned {}",
            response.status()
        )));
    }
    let value: serde_json::Value = response
        .json()
        .await
        .map_err(|e| MemoryError::Model(e.to_string()))?;
    let text = value
        .pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .ok_or_else(|| MemoryError::Model("missing completion content".into()))?;
    Ok(text.trim().to_string())
}
