//! Long-lived browser host managed by the core.
//!
//! The adapter owns Playwright and browser contexts. The core owns policy,
//! budgets and evidence. Each request carries the run's pinned policy, and the
//! adapter enforces it on every request the page makes, including subresources
//! and redirects.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use darkbox_contracts::{AssessmentId, ScopePolicy, WorkspaceId};
use serde_json::{Value, json};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

#[derive(Debug, thiserror::Error)]
pub enum BrowserHostError {
    #[error("browser host is not configured: {0}")]
    NotConfigured(String),
    #[error("browser host failed to start: {0}")]
    Start(String),
    #[error("browser host request failed: {0}")]
    Request(String),
    #[error("browser host returned {status}: {message}")]
    Rejected { status: u16, message: String },
}

#[derive(Debug, Clone)]
struct HostConfig {
    url: Option<String>,
    token: Option<String>,
    mode: String,
    program: String,
    script: PathBuf,
    spawn: bool,
}

pub struct BrowserRuntime {
    config: HostConfig,
    inner: Mutex<RuntimeState>,
}

struct RuntimeState {
    base_url: Option<String>,
    child: Option<Child>,
    stderr_tail: String,
    engine: Option<Value>,
}

struct Request<'a> {
    kind: &'a str,
    session_key: Option<&'a str>,
    workspace_id: WorkspaceId,
    assessment_id: AssessmentId,
    run_id: darkbox_contracts::RunId,
    policy: &'a ScopePolicy,
    payload: Value,
}

static GENERATED_TOKEN: std::sync::OnceLock<String> = std::sync::OnceLock::new();

impl BrowserRuntime {
    pub fn from_env(repo_root: &std::path::Path) -> Self {
        let configured_url = std::env::var("DARKBOX_BROWSER_DAEMON_URL")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let program = std::env::var("DARKBOX_ADAPTER_PYTHON").unwrap_or_else(|_| {
            let venv = repo_root.join(".venv/bin/python");
            if venv.is_file() {
                venv.to_string_lossy().into_owned()
            } else {
                "python3".into()
            }
        });
        let script = std::env::var("DARKBOX_BROWSER_ADAPTER")
            .map(PathBuf::from)
            .unwrap_or_else(|_| repo_root.join("adapters/browser-python/adapter.py"));
        let mode = std::env::var("DARKBOX_BROWSER_MODE").unwrap_or_else(|_| "auto".into());
        Self {
            config: HostConfig {
                url: configured_url.clone(),
                token: std::env::var("DARKBOX_BROWSER_TOKEN").ok(),
                mode,
                program,
                script,
                spawn: configured_url.is_none(),
            },
            inner: Mutex::new(RuntimeState {
                base_url: configured_url,
                child: None,
                stderr_tail: String::new(),
                engine: None,
            }),
        }
    }

    pub fn mode(&self) -> &str {
        &self.config.mode
    }

    pub fn configured_url(&self) -> Option<&str> {
        self.config.url.as_deref()
    }

    async fn ensure(&self) -> Result<String, BrowserHostError> {
        let mut state = self.inner.lock().await;
        if let Some(url) = &state.base_url {
            return Ok(url.clone());
        }
        if !self.config.spawn {
            return Err(BrowserHostError::NotConfigured(
                "DARKBOX_BROWSER_DAEMON_URL is not set".into(),
            ));
        }
        if self.config.mode == "mock" {
            return Err(BrowserHostError::NotConfigured(
                "DARKBOX_BROWSER_MODE=mock is not a browser; set browser mode to auto, playwright or camoufox".into(),
            ));
        }
        if !self.config.script.is_file() {
            return Err(BrowserHostError::NotConfigured(format!(
                "adapter script is missing at {}",
                self.config.script.display()
            )));
        }
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|error| BrowserHostError::Start(error.to_string()))?;
        let port = listener
            .local_addr()
            .map_err(|error| BrowserHostError::Start(error.to_string()))?
            .port();
        drop(listener);
        let token = self
            .config
            .token
            .clone()
            .unwrap_or_else(|| format!("browser-{}", uuid::Uuid::now_v7()));
        let mut command = Command::new(&self.config.program);
        command
            .arg(&self.config.script)
            .arg("--serve")
            .arg("--host")
            .arg("127.0.0.1")
            .arg("--port")
            .arg(port.to_string())
            .arg("--mode")
            .arg(&self.config.mode)
            .env("DARKBOX_BROWSER_TOKEN", &token)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|error| {
            BrowserHostError::Start(format!("{}: {error}", self.config.program))
        })?;
        let stderr = child.stderr.take();
        let tail = Arc::new(Mutex::new(String::new()));
        if let Some(stderr) = stderr {
            let tail = tail.clone();
            tokio::spawn(async move {
                use tokio::io::{AsyncBufReadExt, BufReader};
                let mut reader = BufReader::new(stderr);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    let mut guard = tail.lock().await;
                    if guard.len() < 16_000 {
                        guard.push_str(&line);
                    }
                    line.clear();
                }
            });
        }
        let base_url = format!("http://127.0.0.1:{port}");
        let client = reqwest::Client::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(60);
        loop {
            if tokio::time::Instant::now() > deadline {
                if let Some(mut child) = state.child.take() {
                    let _ = child.kill().await;
                }
                state.stderr_tail = tail.lock().await.clone();
                return Err(BrowserHostError::Start(format!(
                    "browser host did not answer /health within 60s; stderr: {}",
                    state.stderr_tail.chars().take(1200).collect::<String>()
                )));
            }
            match client
                .get(format!("{base_url}/health"))
                .timeout(Duration::from_secs(2))
                .send()
                .await
            {
                Ok(response) if response.status().is_success() => {
                    let body: Value = response.json().await.unwrap_or(Value::Null);
                    if body.get("engine").map(Value::is_object).unwrap_or(false) {
                        state.engine = body.get("engine").cloned();
                        break;
                    }
                }
                _ => {}
            }
            if let Some(status) = child.try_wait().ok().flatten() {
                state.stderr_tail = tail.lock().await.clone();
                return Err(BrowserHostError::Start(format!(
                    "browser host exited with {status}; stderr: {}",
                    state.stderr_tail.chars().take(1200).collect::<String>()
                )));
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        state.child = Some(child);
        state.base_url = Some(base_url.clone());
        if self.config.token.is_none() {
            let _ = GENERATED_TOKEN.set(token);
        }
        Ok(base_url)
    }

    fn token_for_request(&self) -> Option<String> {
        if let Some(token) = &self.config.token {
            return Some(token.clone());
        }
        GENERATED_TOKEN.get().cloned()
    }

    async fn send(
        &self,
        request: Request<'_>,
        timeout: Duration,
    ) -> Result<Value, BrowserHostError> {
        let base = self.ensure().await?;
        let payload = json!({
            "schema_version": 1,
            "request_id": uuid::Uuid::now_v7().to_string(),
            "kind": request.kind,
            "session_key": request.session_key,
            "workspace_id": request.workspace_id.to_string(),
            "assessment_id": request.assessment_id.to_string(),
            "run_id": request.run_id.to_string(),
            "policy": request.policy,
            "payload": request.payload,
            "timeout_seconds": timeout.as_secs(),
        });
        let mut builder = reqwest::Client::new()
            .post(format!("{base}/v1/command"))
            .timeout(timeout)
            .json(&payload);
        if let Some(token) = self.token_for_request() {
            builder = builder.bearer_auth(token);
        }
        let response = builder
            .send()
            .await
            .map_err(|error| BrowserHostError::Request(error.to_string()))?;
        let status = response.status();
        let body: Value = response.json().await.unwrap_or_else(|_| json!({}));
        if !status.is_success() {
            let message = body
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("browser host rejected the command")
                .to_string();
            return Err(BrowserHostError::Rejected {
                status: status.as_u16(),
                message,
            });
        }
        Ok(body)
    }

    pub async fn capability(&self) -> Result<Value, BrowserHostError> {
        let base = self.ensure().await?;
        let mut builder = reqwest::Client::new()
            .get(format!("{base}/v1/capability"))
            .timeout(Duration::from_secs(10));
        if let Some(token) = self.token_for_request() {
            builder = builder.bearer_auth(token);
        }
        let response = builder
            .send()
            .await
            .map_err(|error| BrowserHostError::Request(error.to_string()))?;
        let body: Value = response
            .json()
            .await
            .map_err(|error| BrowserHostError::Request(error.to_string()))?;
        Ok(body)
    }

    pub async fn open(&self, context: OpenContext<'_>) -> Result<Value, BrowserHostError> {
        self.send(
            Request {
                kind: "session.open",
                session_key: Some(context.context_key),
                workspace_id: context.workspace_id,
                assessment_id: context.assessment_id,
                run_id: context.run_id,
                policy: context.policy,
                payload: json!({
                    "identity": context.identity,
                    "tenant": context.tenant,
                    "url": context.url,
                }),
            },
            Duration::from_secs(55),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn command(
        &self,
        kind: &str,
        session_key: &str,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        run_id: darkbox_contracts::RunId,
        policy: &ScopePolicy,
        payload: Value,
    ) -> Result<Value, BrowserHostError> {
        self.send(
            Request {
                kind,
                session_key: Some(session_key),
                workspace_id,
                assessment_id,
                run_id,
                policy,
                payload,
            },
            Duration::from_secs(55),
        )
        .await
    }

    pub async fn abort(&self, session_key: &str) -> Result<bool, BrowserHostError> {
        let base = self.ensure().await?;
        let mut builder = reqwest::Client::new()
            .post(format!("{base}/v1/abort"))
            .timeout(Duration::from_secs(5))
            .json(&json!({ "session_key": session_key }));
        if let Some(token) = self.token_for_request() {
            builder = builder.bearer_auth(token);
        }
        let response = builder
            .send()
            .await
            .map_err(|error| BrowserHostError::Request(error.to_string()))?;
        let body: Value = response.json().await.unwrap_or(Value::Null);
        Ok(body
            .get("aborted")
            .and_then(Value::as_bool)
            .unwrap_or(false))
    }

    pub async fn close(&self, session_key: &str) -> Result<bool, BrowserHostError> {
        let base = self.ensure().await?;
        let mut builder = reqwest::Client::new()
            .post(format!("{base}/v1/close"))
            .timeout(Duration::from_secs(10))
            .json(&json!({ "session_key": session_key }));
        if let Some(token) = self.token_for_request() {
            builder = builder.bearer_auth(token);
        }
        let response = builder
            .send()
            .await
            .map_err(|error| BrowserHostError::Request(error.to_string()))?;
        let body: Value = response.json().await.unwrap_or(Value::Null);
        Ok(body.get("closed").and_then(Value::as_bool).unwrap_or(false))
    }

    pub async fn shutdown(&self) {
        let mut state = self.inner.lock().await;
        if let Some(mut child) = state.child.take() {
            let _ = child.kill().await;
        }
        state.base_url = None;
    }
}

pub struct OpenContext<'a> {
    pub context_key: &'a str,
    pub workspace_id: WorkspaceId,
    pub assessment_id: AssessmentId,
    pub run_id: darkbox_contracts::RunId,
    pub policy: &'a ScopePolicy,
    pub identity: Value,
    pub tenant: Option<String>,
    pub url: &'a str,
}
