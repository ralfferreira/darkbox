use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use darkbox_contracts::ScopePolicy;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    #[error("adapter spawn failed: {0}")]
    Spawn(String),
    #[error("adapter protocol error: {0}")]
    Protocol(String),
    #[error("adapter timed out")]
    Timeout,
    #[error("adapter cancelled")]
    Cancelled,
    #[error("adapter output exceeded limit")]
    OutputLimit,
    #[error("adapter exited: {0}")]
    Exited(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Serialize)]
pub struct AdapterLimits {
    pub timeout_seconds: u64,
    pub max_output_bytes: usize,
    pub max_steps: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct AdapterRequest<'a> {
    pub schema_version: u32,
    pub request_id: String,
    pub kind: String,
    pub workspace_id: String,
    pub assessment_id: String,
    pub run_id: String,
    pub step_id: String,
    pub scope_version: i64,
    pub policy: &'a ScopePolicy,
    pub payload: &'a serde_json::Value,
    pub limits: AdapterLimits,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AdapterMessage {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub request_id: String,
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone)]
pub struct AdapterResult {
    pub payload: serde_json::Value,
    pub progress: Vec<serde_json::Value>,
    pub stderr_tail: String,
}

#[derive(Debug, Clone)]
pub struct BridgeConfig {
    pub program: String,
    pub args: Vec<String>,
    pub timeout: Duration,
    pub max_output_bytes: usize,
}

impl BridgeConfig {
    pub fn from_env(repo_root: &std::path::Path) -> Self {
        let program = std::env::var("DARKBOX_ADAPTER_PYTHON").unwrap_or_else(|_| "python3".into());
        let script = std::env::var("DARKBOX_BROWSER_ADAPTER")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| repo_root.join("adapters/browser-python/adapter.py"));
        let mode = std::env::var("DARKBOX_BROWSER_MODE").unwrap_or_else(|_| "auto".into());
        Self {
            program,
            args: vec![script.to_string_lossy().into_owned(), "--mode".into(), mode],
            timeout: Duration::from_secs(
                std::env::var("DARKBOX_ADAPTER_TIMEOUT")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(120),
            ),
            max_output_bytes: 8 * 1024 * 1024,
        }
    }
}

/// Runs one request against a JSON Lines adapter. The child is killed on
/// timeout, protocol failure, or cancellation.
pub async fn call_adapter(
    config: &BridgeConfig,
    request: &AdapterRequest<'_>,
    cancelled: Arc<AtomicBool>,
) -> Result<AdapterResult, BridgeError> {
    let mut command = Command::new(&config.program);
    command
        .args(&config.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    #[cfg(unix)]
    {
        command.process_group(0);
    }
    let mut child = command
        .spawn()
        .map_err(|e| BridgeError::Spawn(e.to_string()))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| BridgeError::Spawn("no stdin".into()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| BridgeError::Spawn("no stdout".into()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| BridgeError::Spawn("no stderr".into()))?;

    let stderr_task = tokio::spawn(async move {
        let mut reader = BufReader::new(stderr);
        let mut tail = String::new();
        let mut line = String::new();
        while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
            if tail.len() < 16_384 {
                tail.push_str(&line);
            }
            line.clear();
        }
        tail
    });

    let line = serde_json::to_string(request).map_err(|e| BridgeError::Protocol(e.to_string()))?;
    stdin.write_all(line.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await?;
    drop(stdin);

    let mut reader = BufReader::new(stdout);
    let mut buffer = String::new();
    let mut total = 0usize;
    let mut progress = Vec::new();
    let deadline = tokio::time::Instant::now() + config.timeout;
    let mut result: Option<serde_json::Value> = None;

    loop {
        if cancelled.load(Ordering::Relaxed) {
            kill_tree(&mut child);
            return Err(BridgeError::Cancelled);
        }
        let read = tokio::select! {
            _ = tokio::time::sleep_until(deadline) => {
                kill_tree(&mut child);
                return Err(BridgeError::Timeout);
            }
            read = reader.read_line(&mut buffer) => read?,
        };
        if read == 0 {
            break;
        }
        total += read;
        if total > config.max_output_bytes {
            kill_tree(&mut child);
            return Err(BridgeError::OutputLimit);
        }
        let trimmed = buffer.trim();
        if trimmed.is_empty() {
            buffer.clear();
            continue;
        }
        let message: AdapterMessage = serde_json::from_str(trimmed)
            .map_err(|e| BridgeError::Protocol(format!("invalid JSON line: {e}")))?;
        buffer.clear();
        match message.kind.as_str() {
            "progress" => progress.push(message.payload),
            "result" => {
                result = Some(message.payload);
                break;
            }
            "error" => {
                let detail = message
                    .payload
                    .get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("adapter error");
                kill_tree(&mut child);
                let tail = stderr_task.await.unwrap_or_default();
                return Err(BridgeError::Protocol(format!("{detail}; stderr: {tail}")));
            }
            _ => {}
        }
    }
    let stderr_tail = tokio::time::timeout(Duration::from_secs(2), stderr_task)
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or_default();
    match result {
        Some(payload) => {
            let _ = child.wait().await;
            Ok(AdapterResult {
                payload,
                progress,
                stderr_tail,
            })
        }
        None => {
            let status = child.wait().await.ok().and_then(|s| s.code());
            Err(BridgeError::Exited(format!(
                "status={status:?}; stderr={stderr_tail}"
            )))
        }
    }
}

fn kill_tree(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        // The child runs in its own process group, so the negative pid reaches
        // browser subprocesses too.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    let _ = child.start_kill();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn request<'a>(policy: &'a ScopePolicy, payload: &'a serde_json::Value) -> AdapterRequest<'a> {
        AdapterRequest {
            schema_version: 1,
            request_id: "req-1".into(),
            kind: "browser.session".into(),
            workspace_id: "w".into(),
            assessment_id: "a".into(),
            run_id: "r".into(),
            step_id: "s".into(),
            scope_version: 1,
            policy,
            payload,
            limits: AdapterLimits {
                timeout_seconds: 5,
                max_output_bytes: 1024,
                max_steps: 4,
            },
        }
    }

    #[tokio::test]
    async fn rejects_invalid_json() {
        let dir = std::env::temp_dir().join(format!("darkbox-bridge-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("bad.py");
        let mut file = std::fs::File::create(&script).unwrap();
        writeln!(file, "print('not json')").unwrap();
        let config = BridgeConfig {
            program: "python3".into(),
            args: vec![script.to_string_lossy().into_owned()],
            timeout: Duration::from_secs(5),
            max_output_bytes: 1024,
        };
        let policy = ScopePolicy::demo(8080);
        let payload = serde_json::json!({});
        let error = call_adapter(
            &config,
            &request(&policy, &payload),
            Arc::new(AtomicBool::new(false)),
        )
        .await
        .unwrap_err();
        assert!(matches!(error, BridgeError::Protocol(_)));
    }
}
