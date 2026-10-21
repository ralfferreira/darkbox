//! Run execution: leases, bounded external operations, evidence ingestion.

mod adapters;
pub mod bridge;
mod browser;
mod http;
mod procedures;

use std::path::PathBuf;
use std::sync::Arc;

use darkbox_contracts::*;
use darkbox_domain::{Run, RunStepRecord};
use darkbox_evidence::{BlobStore, LocalBlobStore};
use darkbox_policy::{PolicyEngine, RequestContext, TargetRole};
use darkbox_storage::{ClaimedStep, Db, DbError};
use serde_json::json;
use tracing::{info, warn};

pub use http::detect_technologies;
pub use procedures::eligibility_blockers;

#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    pub worker_id: String,
    pub lease_seconds: i64,
    pub max_response_bytes: usize,
    pub request_timeout_seconds: u64,
    pub max_redirects: usize,
    pub blob_root: PathBuf,
    pub repo_root: PathBuf,
    pub bridge: bridge::BridgeConfig,
}

impl ExecutorConfig {
    pub fn from_env() -> Self {
        let repo_root = std::env::var("DARKBOX_REPO_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let blob_root = std::env::var("DARKBOX_BLOB_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| repo_root.join("data/objects"));
        let worker_id = std::env::var("DARKBOX_WORKER_ID")
            .unwrap_or_else(|_| format!("worker-{}", std::process::id()));
        Self {
            worker_id,
            lease_seconds: std::env::var("DARKBOX_LEASE_SECONDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60),
            max_response_bytes: std::env::var("DARKBOX_MAX_RESPONSE_BYTES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(256 * 1024),
            request_timeout_seconds: 20,
            max_redirects: 5,
            blob_root,
            bridge: bridge::BridgeConfig::from_env(&repo_root),
            repo_root,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepOutcomeState {
    Succeeded,
    Failed,
    Blocked,
    Skipped,
}

impl StepOutcomeState {
    pub fn as_str(self) -> &'static str {
        match self {
            StepOutcomeState::Succeeded => "succeeded",
            StepOutcomeState::Failed => "failed",
            StepOutcomeState::Blocked => "blocked",
            StepOutcomeState::Skipped => "skipped",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub state: StepOutcomeState,
    pub effect: EffectStatus,
    pub summary: String,
    pub result: serde_json::Value,
}

impl Outcome {
    pub fn succeeded(summary: impl Into<String>, result: serde_json::Value) -> Self {
        Self {
            state: StepOutcomeState::Succeeded,
            effect: EffectStatus::NotApplicable,
            summary: summary.into(),
            result,
        }
    }
    pub fn failed(code: &str, summary: impl Into<String>) -> Self {
        Self {
            state: StepOutcomeState::Failed,
            effect: EffectStatus::NotApplicable,
            summary: summary.into(),
            result: json!({ "error": code }),
        }
    }
    pub fn blocked(code: &str, summary: impl Into<String>) -> Self {
        Self {
            state: StepOutcomeState::Blocked,
            effect: EffectStatus::NotApplicable,
            summary: summary.into(),
            result: json!({ "error": code }),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error(transparent)]
    Sqlx(#[from] sqlx::Error),
    #[error(transparent)]
    Evidence(#[from] darkbox_evidence::EvidenceError),
    #[error(transparent)]
    Bridge(#[from] bridge::BridgeError),
    #[error("capability unavailable: {0}")]
    CapabilityUnavailable(String),
    #[error("blocked: {0}")]
    Blocked(String),
    #[error("network: {0}")]
    Network(String),
    #[error("invalid operation: {0}")]
    Invalid(String),
}

impl ExecError {
    fn classify(&self) -> Outcome {
        match self {
            ExecError::CapabilityUnavailable(message) => {
                Outcome::blocked("capability_unavailable", message.clone())
            }
            ExecError::Blocked(message) => Outcome::blocked("blocked", message.clone()),
            ExecError::Bridge(bridge::BridgeError::Cancelled) => {
                Outcome::blocked("cancelled", "adapter cancelled")
            }
            other => Outcome::failed("tool_error", other.to_string()),
        }
    }
}

pub type ExecResult<T> = std::result::Result<T, ExecError>;

pub struct Executor {
    pub db: Db,
    store: Arc<dyn BlobStore>,
    http: reqwest::Client,
    config: ExecutorConfig,
}

impl Executor {
    pub fn new(db: Db, config: ExecutorConfig) -> std::result::Result<Self, ExecError> {
        let store = Arc::new(LocalBlobStore::new(&config.blob_root)?);
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(std::time::Duration::from_secs(
                config.request_timeout_seconds,
            ))
            .user_agent("DarkBox/0.3 (authorized assessment)")
            .build()
            .map_err(|e| ExecError::Network(e.to_string()))?;
        Ok(Self {
            db,
            store,
            http,
            config,
        })
    }

    pub fn with_store(
        db: Db,
        config: ExecutorConfig,
        store: Arc<dyn BlobStore>,
    ) -> std::result::Result<Self, ExecError> {
        let mut executor = Self::new(db, config)?;
        executor.store = store;
        Ok(executor)
    }

    pub fn config(&self) -> &ExecutorConfig {
        &self.config
    }

    pub fn store(&self) -> &dyn BlobStore {
        self.store.as_ref()
    }

    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// Claims and executes one step. Returns false when the queue is empty.
    pub async fn run_once(&self, assessment: Option<AssessmentId>) -> ExecResult<bool> {
        let Some(claimed) = self
            .db
            .claim_step(
                &self.config.worker_id,
                self.config.lease_seconds,
                assessment,
            )
            .await?
        else {
            return Ok(false);
        };
        self.execute_claimed(claimed).await?;
        Ok(true)
    }

    async fn execute_claimed(&self, claimed: ClaimedStep) -> ExecResult<()> {
        let ClaimedStep { run, step } = claimed;
        let cancelled = self.cancelled_flag(run.id).await?;
        let outcome = if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
            Outcome {
                state: StepOutcomeState::Skipped,
                effect: EffectStatus::NotDispatched,
                summary: "cancelled before dispatch".into(),
                result: json!({ "error": "cancelled" }),
            }
        } else {
            let result = self.dispatch(&run, &step, cancelled.clone()).await;
            match result {
                Ok(outcome) => outcome,
                Err(error) => {
                    warn!(run = %run.id, step = %step.id, error = %error, "step failed");
                    error.classify()
                }
            }
        };
        let _ = self
            .db
            .finish_step(
                step.id,
                &self.config.worker_id,
                outcome.state.as_str(),
                outcome.effect,
                &outcome.summary,
                outcome.result.clone(),
            )
            .await?;
        self.finalize_run(run.id).await?;
        Ok(())
    }

    async fn dispatch(
        &self,
        run: &Run,
        step: &RunStepRecord,
        cancelled: Arc<std::sync::atomic::AtomicBool>,
    ) -> ExecResult<Outcome> {
        self.enforce_time_budget(run).await?;
        let kind: StepKind = step.kind;
        match kind {
            StepKind::DiscoveryImport => self.import_urls(run, step).await,
            StepKind::DiscoveryCrawler => self.crawl(run, step, cancelled).await,
            StepKind::HttpObserve => self.observe_step(run, step).await,
            StepKind::HttpProbe => self.probe_step(run, step).await,
            StepKind::BrowserSession => self.browser_session(run, step, cancelled).await,
            StepKind::BrowserFlow => self.browser_flow(run, step, cancelled).await,
            StepKind::ProcedureStep => self.procedure_step(run, step, cancelled).await,
            StepKind::NetworkScan => self.network_scan(run, step).await,
            StepKind::FirecrawlMap => self.firecrawl_map(run, step).await,
            StepKind::FirecrawlScrape => self.firecrawl_scrape(run, step).await,
            StepKind::BurpImport => self.burp_import(run, step).await,
            StepKind::AgentTurn => Ok(Outcome::blocked(
                "agent_service_required",
                "agent turns are executed by the TypeScript agent service, not the Rust worker",
            )),
            StepKind::Reconcile => Ok(Outcome::succeeded(
                "reconciliation recorded",
                json!({ "note": step.operation.get("note").cloned().unwrap_or_default() }),
            )),
        }
    }

    async fn finalize_run(&self, run_id: RunId) -> ExecResult<()> {
        let run = self
            .db
            .get_run(self.run_workspace(run_id).await?, run_id)
            .await?;
        if run.state == RunState::Cancelling {
            let pending: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM run_steps WHERE run_id=$1 AND state IN ('pending','running')",
            )
            .bind(run_id.as_uuid())
            .fetch_one(self.db.pool())
            .await?;
            if pending == 0 {
                self.db
                    .set_run_state(run_id, &[RunState::Cancelling], RunState::Cancelled, None)
                    .await?;
            }
            return Ok(());
        }
        if let Some(next) = darkbox_storage::finish_run_if_done(&self.db, run_id).await? {
            info!(run = %run_id, state = %next, "run finished");
            let summary = self.db.run_summary(run.workspace_id, run_id).await?;
            let steps = self.db.run_step_dtos(run_id).await?;
            let mut body = format!(
                "Run `{}` finished as `{}`.\n\nKind: {}. Policy version: {}. Requests: {}/{}. Browser steps: {}/{}. Effect unknown: {}.\n\nSteps:\n",
                run_id,
                next,
                summary.kind,
                summary.policy_version,
                summary.spent.requests,
                summary.budget.max_requests,
                summary.spent.browser_steps,
                summary.budget.max_browser_steps,
                summary.effect_unknown
            );
            for step in &steps {
                body.push_str(&format!(
                    "- {} {} {} (attempt {}, effect {}) — {}\n",
                    step.ordinal,
                    step.kind,
                    step.state,
                    step.attempt,
                    step.effect_status,
                    step.summary
                ));
            }
            if let Some(reason) = &summary.reason {
                body.push_str(&format!("\nReason: {reason}\n"));
            }
            let run_text = run_id.to_string();
            let short = &run_text[run_text.len().saturating_sub(8)..];
            let _ = self
                .db
                .save_memory_page(
                    run.workspace_id,
                    run.assessment_id,
                    &format!("run-{short}"),
                    &format!("Run {short} checkpoint"),
                    &body,
                    &[MemoryReferenceDto {
                        kind: "run".into(),
                        id: run_id.to_string(),
                        label: format!("run {short}"),
                    }],
                    "current",
                    "executor",
                )
                .await;
        }
        Ok(())
    }

    async fn run_workspace(&self, run_id: RunId) -> ExecResult<WorkspaceId> {
        let id: uuid::Uuid = sqlx::query_scalar("SELECT workspace_id FROM runs WHERE id=$1")
            .bind(run_id.as_uuid())
            .fetch_one(self.db.pool())
            .await?;
        Ok(id.into())
    }

    async fn enforce_time_budget(&self, run: &Run) -> ExecResult<()> {
        let elapsed = chrono::Utc::now().signed_duration_since(run.created_at);
        if elapsed.num_seconds() > run.budget.max_duration_seconds {
            return Err(ExecError::Blocked("budget_exhausted: duration".into()));
        }
        Ok(())
    }

    async fn cancelled_flag(
        &self,
        run_id: RunId,
    ) -> ExecResult<Arc<std::sync::atomic::AtomicBool>> {
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let db = self.db.clone();
        let watch = flag.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                match db.run_state(run_id).await {
                    Ok(RunState::Cancelling) | Ok(RunState::Cancelled) => {
                        watch.store(true, std::sync::atomic::Ordering::Relaxed);
                        break;
                    }
                    Ok(RunState::Succeeded) | Ok(RunState::Failed) | Ok(RunState::Interrupted) => {
                        break;
                    }
                    _ => {}
                }
            }
        });
        Ok(flag)
    }

    pub async fn authorize_url(
        &self,
        run: &Run,
        raw: &str,
        action: ActionClass,
        role: TargetRole,
    ) -> ExecResult<(url::Url, ScopeDecision)> {
        let url = url::Url::parse(raw)
            .map_err(|e| ExecError::Invalid(format!("invalid URL {raw}: {e}")))?;
        let host = url
            .host_str()
            .ok_or_else(|| ExecError::Invalid("missing host".into()))?
            .to_string();
        let port = url
            .port_or_known_default()
            .ok_or_else(|| ExecError::Invalid("missing port".into()))?;
        let path = if url.path().is_empty() {
            "/"
        } else {
            url.path()
        };
        let current = self
            .db
            .get_scope(run.workspace_id, run.assessment_id, None)
            .await?;
        let resolved = if host.parse::<std::net::IpAddr>().is_ok() {
            vec![host.parse().unwrap()]
        } else {
            match tokio::time::timeout(
                std::time::Duration::from_secs(3),
                tokio::net::lookup_host((host.as_str(), port)),
            )
            .await
            {
                Ok(Ok(addrs)) => addrs.map(|addr| addr.ip()).collect(),
                _ => Vec::new(),
            }
        };
        let mut ctx = RequestContext::new(url.scheme(), &host, port, path);
        ctx.action_class = action;
        ctx.role = role;
        ctx.resolved_ips = resolved;
        let pinned = PolicyEngine::new(&run.policy, run.policy_version);
        let current_engine = PolicyEngine::new(&current.policy, current.version);
        let decision = PolicyEngine::evaluate_against(&pinned, &current_engine, &ctx);
        Ok((url, decision))
    }

    pub(crate) async fn store_evidence_text(
        &self,
        run: &Run,
        step_id: StepId,
        identity_id: Option<IdentityId>,
        kind: EvidenceKind,
        summary: &str,
        text: &str,
    ) -> ExecResult<EvidenceId> {
        let sanitized = darkbox_evidence::sanitize_text(text, self.config.max_response_bytes);
        let text = sanitized.text.as_str();
        let bytes = text.as_bytes();
        if !self
            .db
            .reserve_budget(run.id, "artifact_bytes", bytes.len() as i64)
            .await?
        {
            return Err(ExecError::Blocked(
                "budget_exhausted: artifact_bytes".into(),
            ));
        }
        let key = darkbox_evidence::opaque_key();
        self.store.put(&key, bytes).await?;
        let retention = self.db.workspace_retention(run.workspace_id).await?;
        let id = self
            .db
            .insert_evidence(
                run.workspace_id,
                run.assessment_id,
                darkbox_storage::NewEvidence {
                    kind,
                    classification: EvidenceClassification::Sanitized,
                    object_key: key,
                    sha256: darkbox_evidence::sha256_hex(bytes),
                    size_bytes: bytes.len() as i64,
                    truncated: sanitized.truncated,
                    sanitized: true,
                    content_type: "text/plain; charset=utf-8".into(),
                    run_id: Some(run.id),
                    step_id: Some(step_id),
                    identity_id,
                    summary: summary.to_string(),
                    source: "executor".into(),
                },
                Some(retention),
            )
            .await?;
        Ok(id)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn add_observation(
        &self,
        run: &Run,
        subject_kind: &str,
        subject_id: uuid::Uuid,
        property: &str,
        value: serde_json::Value,
        confidence: f64,
        evidence_id: Option<EvidenceId>,
    ) -> ExecResult<()> {
        self.db
            .add_observation(
                run.workspace_id,
                run.assessment_id,
                subject_kind,
                subject_id,
                property,
                value,
                "executor",
                confidence,
                evidence_id,
            )
            .await?;
        Ok(())
    }

    pub async fn worker_loop(
        self: Arc<Self>,
        assessment: Option<AssessmentId>,
        stop_after_empty: bool,
    ) {
        loop {
            match self.run_once(assessment).await {
                Ok(true) => continue,
                Ok(false) => {
                    if stop_after_empty {
                        break;
                    }
                    if let Err(error) = self.db.recover_expired_leases().await {
                        warn!(error = %error, "lease recovery failed");
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(750)).await;
                }
                Err(error) => {
                    warn!(error = %error, "worker iteration failed");
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
            }
        }
    }
}

pub(crate) fn operation<'a>(step: &'a RunStepRecord, key: &str) -> Option<&'a serde_json::Value> {
    step.operation.get(key)
}

pub(crate) fn operation_str<'a>(step: &'a RunStepRecord, key: &str) -> Option<&'a str> {
    operation(step, key).and_then(|v| v.as_str())
}

pub(crate) fn operation_u64(step: &RunStepRecord, key: &str, default: u64) -> u64 {
    operation(step, key)
        .and_then(|v| v.as_u64())
        .unwrap_or(default)
}

pub fn capabilities(config: &ExecutorConfig) -> Vec<Capability> {
    let katana = which("katana");
    let python = which("python3");
    let adapter_script = config.repo_root.join("adapters/browser-python/adapter.py");
    let adapter_ready = python.is_some() && adapter_script.is_file();
    let camoufox = std::env::var("DARKBOX_CAMOUFOX_AVAILABLE").is_ok();
    let mut capabilities = vec![
        Capability {
            id: "http.capture".into(),
            provider: "builtin".into(),
            version: "0.3.0".into(),
            state: CapabilityState::Available,
            detail: format!(
                "scope-checked requests, {} byte capture limit, manual redirects",
                config.max_response_bytes
            ),
        },
        Capability {
            id: "discovery.import".into(),
            provider: "builtin".into(),
            version: "0.3.0".into(),
            state: CapabilityState::Available,
            detail: "operator-supplied URLs, preserved as candidates".into(),
        },
        Capability {
            id: "discovery.crawler".into(),
            provider: "builtin".into(),
            version: "0.3.0".into(),
            state: CapabilityState::Available,
            detail: "bounded breadth-first crawl; every request re-checked against policy".into(),
        },
        Capability {
            id: "discovery.katana".into(),
            provider: "katana".into(),
            version: katana
                .as_deref()
                .map(|_| "detected")
                .unwrap_or("missing")
                .into(),
            state: if katana.is_some() {
                CapabilityState::Degraded
            } else {
                CapabilityState::Unavailable
            },
            detail: if katana.is_some() {
                "binary found; adapter is not enabled for scoped runs because it cannot guarantee per-request scope checks".into()
            } else {
                "katana is not installed; built-in crawler is used".into()
            },
        },
        Capability {
            id: "browser.session".into(),
            provider: "python-adapter".into(),
            version: "0.3.0".into(),
            state: if adapter_ready {
                CapabilityState::Available
            } else {
                CapabilityState::Unavailable
            },
            detail: if adapter_ready && camoufox {
                "JSON Lines adapter with Camoufox available".into()
            } else if adapter_ready {
                "JSON Lines adapter in deterministic HTTP mode; Camoufox is not installed".into()
            } else {
                "python3 or the adapter script is missing; browser flows are blocked".into()
            },
        },
        Capability {
            id: "model.provider".into(),
            provider: if std::env::var("DARKBOX_AGENT_URL").is_ok() {
                "vercel-ai-sdk"
            } else if std::env::var("DARKBOX_MODEL_API_KEY").is_ok() {
                "openai-compatible"
            } else {
                "none"
            }
            .into(),
            version: "1".into(),
            state: if std::env::var("DARKBOX_AGENT_URL").is_ok()
                || std::env::var("DARKBOX_MODEL_API_KEY").is_ok()
            {
                CapabilityState::Available
            } else {
                CapabilityState::Unavailable
            },
            detail: if let Ok(url) = std::env::var("DARKBOX_AGENT_URL") {
                format!(
                    "agent service configured at {url}; provider health is reported by that service"
                )
            } else if std::env::var("DARKBOX_MODEL_API_KEY").is_ok() {
                "provider key configured for memory synthesis; the AI SDK agent service is separate and not configured".into()
            } else {
                "no model provider configured; memory synthesis stays deterministic".into()
            },
        },
    ];
    capabilities.extend(darkbox_integrations::capabilities());
    capabilities
}

fn which(binary: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(binary))
        .find(|candidate| candidate.is_file())
}
