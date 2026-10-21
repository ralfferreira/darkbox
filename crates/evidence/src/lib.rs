//! Evidence capture, sanitization and object storage.
//!
//! Object keys are opaque. The store never derives a path from target data.

use std::path::PathBuf;

use async_trait::async_trait;
use darkbox_contracts::EvidenceKind;
use serde::{Deserialize, Serialize};

mod sanitize;

pub use sanitize::*;

#[derive(Debug, thiserror::Error)]
pub enum EvidenceError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("object not found")]
    NotFound,
    #[error("object exceeds configured limit")]
    TooLarge,
    #[error("invalid object key")]
    InvalidKey,
}

#[async_trait]
pub trait BlobStore: Send + Sync {
    async fn put(&self, key: &str, bytes: &[u8]) -> Result<(), EvidenceError>;
    async fn get(&self, key: &str) -> Result<Vec<u8>, EvidenceError>;
    async fn delete(&self, key: &str) -> Result<(), EvidenceError>;
}

#[derive(Debug, Clone)]
pub struct LocalBlobStore {
    root: PathBuf,
}

impl LocalBlobStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, EvidenceError> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    fn path(&self, key: &str) -> Result<PathBuf, EvidenceError> {
        if key.is_empty()
            || key.contains("..")
            || key.contains('/')
            || key.contains('\\')
            || !key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return Err(EvidenceError::InvalidKey);
        }
        Ok(self.root.join(key))
    }
}

#[async_trait]
impl BlobStore for LocalBlobStore {
    async fn put(&self, key: &str, bytes: &[u8]) -> Result<(), EvidenceError> {
        let path = self.path(key)?;
        let tmp = path.with_extension("part");
        tokio::fs::write(&tmp, bytes).await?;
        tokio::fs::rename(&tmp, &path).await?;
        Ok(())
    }

    async fn get(&self, key: &str) -> Result<Vec<u8>, EvidenceError> {
        let path = self.path(key)?;
        match tokio::fs::read(&path).await {
            Ok(bytes) => Ok(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(EvidenceError::NotFound)
            }
            Err(error) => Err(EvidenceError::Io(error)),
        }
    }

    async fn delete(&self, key: &str) -> Result<(), EvidenceError> {
        let path = self.path(key)?;
        match tokio::fs::remove_file(&path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(EvidenceError::Io(error)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedExchange {
    pub method: String,
    pub url: String,
    pub request_headers: Vec<(String, String)>,
    pub request_body: Option<String>,
    pub status: Option<u16>,
    pub response_headers: Vec<(String, String)>,
    pub response_body: Option<String>,
    pub truncated: bool,
    pub note: Option<String>,
}

impl CapturedExchange {
    pub fn new(method: &str, url: &str) -> Self {
        Self {
            method: method.to_string(),
            url: url.to_string(),
            request_headers: Vec::new(),
            request_body: None,
            status: None,
            response_headers: Vec::new(),
            response_body: None,
            truncated: false,
            note: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CaptureLimits {
    pub max_body_bytes: usize,
}

impl Default for CaptureLimits {
    fn default() -> Self {
        Self {
            max_body_bytes: 256 * 1024,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SanitizedCapture {
    pub text: String,
    pub sha256: String,
    pub size_bytes: i64,
    pub truncated: bool,
    pub redactions: usize,
    pub content_type: String,
}

pub fn capture(exchange: &CapturedExchange, limits: &CaptureLimits) -> SanitizedCapture {
    let mut text = String::new();
    text.push_str(&format!("{} {}\n", exchange.method, exchange.url));
    for (name, value) in &exchange.request_headers {
        text.push_str(&format!("{name}: {value}\n"));
    }
    if let Some(body) = &exchange.request_body {
        text.push('\n');
        text.push_str(body);
        text.push('\n');
    }
    if let Some(status) = exchange.status {
        text.push_str(&format!("\n< HTTP {status}\n"));
        for (name, value) in &exchange.response_headers {
            text.push_str(&format!("{name}: {value}\n"));
        }
        if let Some(body) = &exchange.response_body {
            text.push('\n');
            text.push_str(body);
        }
    }
    if let Some(note) = &exchange.note {
        text.push_str(&format!("\n\n[capture note] {note}"));
    }
    let sanitized = sanitize_text(&text, limits.max_body_bytes);
    SanitizedCapture {
        sha256: sha256_hex(sanitized.text.as_bytes()),
        size_bytes: sanitized.text.len() as i64,
        truncated: exchange.truncated || sanitized.truncated,
        redactions: sanitized.redactions,
        content_type: "text/plain; charset=utf-8".into(),
        text: sanitized.text,
    }
}

pub fn kind_for_exchange() -> EvidenceKind {
    EvidenceKind::HttpExchange
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

pub fn opaque_key() -> String {
    format!("ev_{}", uuid::Uuid::now_v7().simple())
}
