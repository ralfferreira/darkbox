use std::collections::{HashSet, VecDeque};
use std::sync::LazyLock;

use darkbox_contracts::*;
use darkbox_domain::Run;
use darkbox_evidence::{CaptureLimits, CapturedExchange, capture};
use darkbox_policy::TargetRole;
use darkbox_storage::DbError;
use regex::Regex;
use serde_json::json;
use url::Url;

use crate::{
    ExecError, ExecResult, Executor, Outcome, StepOutcomeState, operation_str, operation_u64,
};

static LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?i)(?:href|src|action)\s*=\s*["']([^"']+)["']"#).unwrap());
static PATH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"["'](/[A-Za-z0-9_\-./?=&%#:]{2,})["']"#).unwrap());
static SERVER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?i)<meta[^>]+name=["']generator["'][^>]+content=["']([^"']+)"#).unwrap()
});

pub struct FetchResult {
    pub final_url: Url,
    pub status: Option<u16>,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub truncated: bool,
    pub blocked: Option<String>,
    pub network_error: Option<String>,
    pub hops: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum TargetOutcome {
    Observed,
    Excluded,
    Unreachable { error: String },
}

pub fn detect_technologies(headers: &[(String, String)], body: &str) -> Vec<(String, f64)> {
    let mut found: Vec<(String, f64)> = Vec::new();
    for (name, value) in headers {
        let name = name.to_ascii_lowercase();
        if name == "server" {
            found.push((value.trim().to_string(), 0.9));
        }
        if name == "x-powered-by" || name == "x-generator" {
            found.push((value.trim().to_string(), 0.75));
        }
    }
    if let Some(captures) = SERVER_RE.captures(body)
        && let Some(generator) = captures.get(1)
    {
        found.push((generator.as_str().trim().to_string(), 0.65));
    }
    found.retain(|(name, _)| !name.is_empty() && name.len() < 80);
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found.dedup_by(|a, b| a.0.eq_ignore_ascii_case(&b.0));
    found
}

impl Executor {
    pub(crate) async fn import_urls(
        &self,
        run: &Run,
        step: &darkbox_domain::RunStepRecord,
    ) -> ExecResult<Outcome> {
        let urls: Vec<String> = step
            .operation
            .get("urls")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let observe = step
            .operation
            .get("observe")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let mut observed = 0usize;
        let mut excluded = 0usize;
        let mut unreachable = 0usize;
        let mut errors: Vec<String> = Vec::new();
        for raw in urls
            .iter()
            .take(operation_u64(step, "max_urls", 100) as usize)
        {
            match self.observe_target(run, step, raw, "import", observe).await {
                Ok(Some(TargetOutcome::Observed)) => observed += 1,
                Ok(Some(TargetOutcome::Excluded)) => excluded += 1,
                Ok(Some(TargetOutcome::Unreachable { error })) => {
                    unreachable += 1;
                    errors.push(format!("{raw}: {error}"));
                }
                Ok(None) => {}
                Err(ExecError::Blocked(message)) => {
                    excluded += 1;
                    errors.push(format!("{raw}: {message}"));
                }
                Err(error) => return Err(error),
            }
        }
        let summary = format!(
            "imported {} URLs: {} observed, {} excluded, {} unreachable",
            urls.len(),
            observed,
            excluded,
            unreachable
        );
        Ok(Outcome {
            state: if observed == 0 && !urls.is_empty() {
                StepOutcomeState::Failed
            } else {
                StepOutcomeState::Succeeded
            },
            effect: EffectStatus::NotApplicable,
            summary,
            result: json!({
                "imported": urls.len(),
                "observed": observed,
                "excluded": excluded,
                "unreachable": unreachable,
                "errors": errors,
            }),
        })
    }

    async fn observe_target(
        &self,
        run: &Run,
        step: &darkbox_domain::RunStepRecord,
        raw: &str,
        source: &str,
        observe: bool,
    ) -> ExecResult<Option<TargetOutcome>> {
        let key = darkbox_domain::EndpointKey::parse(raw, "GET")
            .map_err(|e| ExecError::Invalid(e.to_string()))?;
        let endpoint_id = self
            .db
            .upsert_candidate(
                run.workspace_id,
                run.assessment_id,
                &key,
                source,
                "operator supplied",
                Some(run.id),
                None,
            )
            .await?;
        if !observe {
            return Ok(None);
        }
        let (_, decision) = self
            .authorize_url(run, raw, ActionClass::Query, TargetRole::TestTarget)
            .await?;
        if !decision.allowed {
            let rule = decision.rule.clone();
            self.db
                .mark_observed(
                    run.workspace_id,
                    run.assessment_id,
                    endpoint_id,
                    EndpointState::Excluded,
                    None,
                    "policy",
                )
                .await?;
            self.add_observation(
                run,
                "endpoint",
                endpoint_id.as_uuid(),
                "scope.excluded",
                json!({ "rule": rule, "policy_version": decision.policy_version }),
                1.0,
                None,
            )
            .await?;
            return Ok(Some(TargetOutcome::Excluded));
        }
        let url = Url::parse(raw).map_err(|e| ExecError::Invalid(e.to_string()))?;
        match self.fetch(run, step, url, "GET", true).await? {
            fetch if fetch.blocked.is_some() => {
                let _rule = fetch.blocked.unwrap();
                self.db
                    .mark_observed(
                        run.workspace_id,
                        run.assessment_id,
                        endpoint_id,
                        EndpointState::Excluded,
                        None,
                        "policy",
                    )
                    .await?;
                Ok(Some(TargetOutcome::Excluded))
            }
            fetch if fetch.network_error.is_some() => {
                let error = fetch.network_error.unwrap();
                self.db
                    .mark_observed(
                        run.workspace_id,
                        run.assessment_id,
                        endpoint_id,
                        EndpointState::Unreachable,
                        None,
                        source,
                    )
                    .await?;
                Ok(Some(TargetOutcome::Unreachable { error }))
            }
            fetch => {
                let evidence = self
                    .capture_and_observe(run, step, endpoint_id, &fetch, None)
                    .await?;
                let _ = evidence;
                Ok(Some(TargetOutcome::Observed))
            }
        }
    }

    pub(crate) async fn crawl(
        &self,
        run: &Run,
        step: &darkbox_domain::RunStepRecord,
        cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> ExecResult<Outcome> {
        let mut queue: VecDeque<(Url, u32)> = VecDeque::new();
        let start: Vec<String> = step
            .operation
            .get("start_urls")
            .or_else(|| step.operation.get("urls"))
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        for raw in &start {
            if let Ok(url) = Url::parse(raw) {
                queue.push_back((url, 0));
            }
        }
        let max_urls = operation_u64(step, "max_urls", 40) as usize;
        let max_depth = operation_u64(step, "max_depth", 2) as u32;
        let mut visited: HashSet<String> = HashSet::new();
        let mut observed = 0usize;
        let mut excluded = 0usize;
        let mut discovered = 0usize;
        let mut errors: Vec<String> = Vec::new();

        while let Some((url, depth)) = queue.pop_front() {
            if cancelled.load(std::sync::atomic::Ordering::Relaxed) {
                break;
            }
            if visited.len() >= max_urls {
                break;
            }
            let key = darkbox_domain::EndpointKey::from_url(&url, "GET")
                .map_err(|e| ExecError::Invalid(e.to_string()))?;
            let dedupe = key.record_key();
            if !visited.insert(dedupe) {
                continue;
            }
            let raw = url.to_string();
            let endpoint_id = self
                .db
                .upsert_candidate(
                    run.workspace_id,
                    run.assessment_id,
                    &key,
                    "crawler",
                    &format!("depth {depth}"),
                    Some(run.id),
                    None,
                )
                .await?;
            let (_, decision) = self
                .authorize_url(run, &raw, ActionClass::Query, TargetRole::TestTarget)
                .await?;
            if !decision.allowed {
                self.db
                    .mark_observed(
                        run.workspace_id,
                        run.assessment_id,
                        endpoint_id,
                        EndpointState::Excluded,
                        None,
                        "policy",
                    )
                    .await?;
                self.add_observation(
                    run,
                    "endpoint",
                    endpoint_id.as_uuid(),
                    "scope.excluded",
                    json!({ "rule": decision.rule, "policy_version": decision.policy_version }),
                    1.0,
                    None,
                )
                .await?;
                excluded += 1;
                continue;
            }
            let fetch = match self.fetch(run, step, url.clone(), "GET", true).await {
                Ok(fetch) => fetch,
                Err(ExecError::Blocked(_)) => {
                    excluded += 1;
                    continue;
                }
                Err(ExecError::Db(DbError::NotFound(_))) => continue,
                Err(error) => {
                    errors.push(format!("{raw}: {error}"));
                    continue;
                }
            };
            if fetch.blocked.is_some() {
                excluded += 1;
                continue;
            }
            if fetch.network_error.is_some() {
                let error = fetch.network_error.clone().unwrap_or_default();
                self.db
                    .mark_observed(
                        run.workspace_id,
                        run.assessment_id,
                        endpoint_id,
                        EndpointState::Unreachable,
                        None,
                        "crawler",
                    )
                    .await?;
                errors.push(format!("{raw}: {error}"));
                continue;
            }
            self.capture_and_observe(run, step, endpoint_id, &fetch, None)
                .await?;
            observed += 1;

            if depth < max_depth {
                let links = extract_candidates(&fetch.final_url, &fetch.body);
                for link in links {
                    if visited.len() + queue.len() >= max_urls * 2 {
                        break;
                    }
                    if let Ok(parsed) = Url::parse(&link) {
                        let candidate_key =
                            darkbox_domain::EndpointKey::from_url(&parsed, "GET").ok();
                        if let Some(candidate_key) = candidate_key {
                            let candidate = self
                                .db
                                .upsert_candidate(
                                    run.workspace_id,
                                    run.assessment_id,
                                    &candidate_key,
                                    "crawler-link",
                                    &format!("discovered from {raw}"),
                                    Some(run.id),
                                    None,
                                )
                                .await?;
                            discovered += 1;
                            let redirect_is_allowed = self
                                .authorize_url(
                                    run,
                                    &link,
                                    ActionClass::Query,
                                    TargetRole::TestTarget,
                                )
                                .await
                                .map(|(_, d)| d.allowed)
                                .unwrap_or(false);
                            if redirect_is_allowed {
                                queue.push_back((parsed, depth + 1));
                            } else {
                                self.db
                                    .mark_observed(
                                        run.workspace_id,
                                        run.assessment_id,
                                        candidate,
                                        EndpointState::Excluded,
                                        None,
                                        "policy",
                                    )
                                    .await?;
                                excluded += 1;
                            }
                        }
                    }
                }
            }
        }
        Ok(Outcome {
            state: if observed == 0 && !errors.is_empty() {
                StepOutcomeState::Failed
            } else {
                StepOutcomeState::Succeeded
            },
            effect: EffectStatus::NotApplicable,
            summary: format!(
                "crawled {} pages, discovered {} links, {} excluded, {} errors",
                observed,
                discovered,
                excluded,
                errors.len()
            ),
            result: json!({
                "observed": observed,
                "discovered": discovered,
                "excluded": excluded,
                "errors": errors,
                "max_urls": max_urls,
                "max_depth": max_depth,
            }),
        })
    }

    pub(crate) async fn observe_step(
        &self,
        run: &Run,
        step: &darkbox_domain::RunStepRecord,
    ) -> ExecResult<Outcome> {
        let endpoint_id: EndpointId = operation_str(step, "endpoint_id")
            .ok_or_else(|| ExecError::Invalid("endpoint_id is required".into()))?
            .parse()
            .map_err(|_| ExecError::Invalid("invalid endpoint_id".into()))?;
        let endpoint = self
            .db
            .endpoint_by_id(run.workspace_id, run.assessment_id, endpoint_id)
            .await?;
        let raw = format!(
            "{}://{}:{}{}",
            endpoint.scheme, endpoint.host, endpoint.port, endpoint.path
        );
        let (_, decision) = self
            .authorize_url(run, &raw, ActionClass::Query, TargetRole::TestTarget)
            .await?;
        if !decision.allowed {
            self.db
                .mark_observed(
                    run.workspace_id,
                    run.assessment_id,
                    endpoint_id,
                    EndpointState::Excluded,
                    None,
                    "policy",
                )
                .await?;
            return Ok(Outcome::blocked(
                decision
                    .code
                    .map(|c| c.as_str())
                    .unwrap_or(DenyCode::OutOfScope.as_str()),
                format!("{} denied by {}", raw, decision.rule),
            ));
        }
        let url = Url::parse(&raw).map_err(|e| ExecError::Invalid(e.to_string()))?;
        let fetch = self.fetch(run, step, url, "GET", true).await?;
        if let Some(rule) = &fetch.blocked {
            self.db
                .mark_observed(
                    run.workspace_id,
                    run.assessment_id,
                    endpoint_id,
                    EndpointState::Excluded,
                    None,
                    "policy",
                )
                .await?;
            return Ok(Outcome::blocked(
                DenyCode::Excluded.as_str(),
                format!("redirect blocked by {rule}"),
            ));
        }
        if let Some(error) = &fetch.network_error {
            self.db
                .mark_observed(
                    run.workspace_id,
                    run.assessment_id,
                    endpoint_id,
                    EndpointState::Unreachable,
                    None,
                    "executor",
                )
                .await?;
            return Ok(Outcome::failed("capture_failed", error.clone()));
        }
        let evidence = self
            .capture_and_observe(run, step, endpoint_id, &fetch, None)
            .await?;
        Ok(Outcome::succeeded(
            format!("observed {} {}", fetch.status.unwrap_or(0), fetch.final_url),
            json!({
                "status": fetch.status,
                "final_url": fetch.final_url.to_string(),
                "evidence_id": evidence,
                "hops": fetch.hops,
                "truncated": fetch.truncated,
            }),
        ))
    }

    pub(crate) async fn probe_step(
        &self,
        run: &Run,
        step: &darkbox_domain::RunStepRecord,
    ) -> ExecResult<Outcome> {
        let ids: Vec<EndpointId> = step
            .operation
            .get("endpoint_ids")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|v| v.parse().ok())
                    .collect()
            })
            .unwrap_or_default();
        let requested = ids.len();
        let mut observed = 0usize;
        let mut excluded = 0usize;
        let mut failed = 0usize;
        for id in ids {
            let endpoint = self
                .db
                .endpoint_by_id(run.workspace_id, run.assessment_id, id)
                .await
                .ok();
            let Some(endpoint) = endpoint else { continue };
            let raw = format!(
                "{}://{}:{}{}",
                endpoint.scheme, endpoint.host, endpoint.port, endpoint.path
            );
            let url = Url::parse(&raw).map_err(|e| ExecError::Invalid(e.to_string()))?;
            let fetch = self.fetch(run, step, url, "GET", true).await?;
            if fetch.blocked.is_some() {
                excluded += 1;
                continue;
            }
            if fetch.network_error.is_some() {
                failed += 1;
                continue;
            }
            self.capture_and_observe(run, step, id, &fetch, None)
                .await?;
            observed += 1;
        }
        Ok(Outcome {
            state: if observed == 0 && requested > 0 {
                StepOutcomeState::Failed
            } else {
                StepOutcomeState::Succeeded
            },
            effect: EffectStatus::NotApplicable,
            summary: format!("{observed} observed, {excluded} excluded, {failed} failed"),
            result: json!({ "observed": observed, "excluded": excluded, "failed": failed }),
        })
    }

    pub(crate) async fn fetch(
        &self,
        run: &Run,
        step: &darkbox_domain::RunStepRecord,
        start: Url,
        method: &str,
        allow_redirects: bool,
    ) -> ExecResult<FetchResult> {
        let mut current = start;
        let mut hops = Vec::new();
        let mut last_status = None;
        let mut last_headers = Vec::new();
        let mut last_body = String::new();
        let mut last_truncated = false;
        let idempotent = matches!(method, "GET" | "HEAD" | "OPTIONS");
        let mut dispatched = false;

        for hop in 0..=self.config().max_redirects {
            let (url, decision) = self
                .authorize_url(
                    run,
                    current.as_str(),
                    ActionClass::Query,
                    TargetRole::TestTarget,
                )
                .await?;
            if !decision.allowed {
                return Ok(FetchResult {
                    final_url: current,
                    status: last_status,
                    headers: last_headers,
                    body: last_body,
                    truncated: last_truncated,
                    blocked: Some(format!(
                        "{} ({})",
                        decision.rule,
                        decision.code.map(|c| c.as_str()).unwrap_or("out_of_scope")
                    )),
                    network_error: None,
                    hops,
                });
            }
            if !self.db.reserve_budget(run.id, "requests", 1).await? {
                return Err(ExecError::Blocked("budget_exhausted: requests".into()));
            }
            if !idempotent && !dispatched {
                let _ = self
                    .db
                    .mark_step_dispatched(
                        step.id,
                        &self.config().worker_id,
                        &format!("{method} {url}"),
                    )
                    .await?;
                dispatched = true;
            }
            let request = self
                .http()
                .request(
                    reqwest::Method::from_bytes(method.as_bytes())
                        .map_err(|e| ExecError::Invalid(e.to_string()))?,
                    url.clone(),
                )
                .header(reqwest::header::ACCEPT, "text/html,application/json,*/*")
                .header(reqwest::header::ACCEPT_ENCODING, "identity");
            let response = match request.send().await {
                Ok(response) => response,
                Err(error) => {
                    return Ok(FetchResult {
                        final_url: url,
                        status: None,
                        headers: Vec::new(),
                        body: String::new(),
                        truncated: false,
                        blocked: None,
                        network_error: Some(error.to_string()),
                        hops,
                    });
                }
            };
            let status = response.status().as_u16();
            let headers = response
                .headers()
                .iter()
                .map(|(name, value)| {
                    (
                        name.as_str().to_string(),
                        value.to_str().unwrap_or("<non-utf8>").to_string(),
                    )
                })
                .collect::<Vec<_>>();
            let mut body = String::new();
            let mut truncated = false;
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("")
                .to_string();
            if !content_type.starts_with("image/")
                && !content_type.starts_with("font/")
                && !content_type.starts_with("video/")
            {
                let mut response = response;
                while let Some(chunk) = response
                    .chunk()
                    .await
                    .map_err(|e| ExecError::Network(e.to_string()))?
                {
                    let remaining = self.config().max_response_bytes.saturating_sub(body.len());
                    if chunk.len() > remaining {
                        body.push_str(&String::from_utf8_lossy(&chunk[..remaining]));
                        truncated = true;
                        break;
                    }
                    body.push_str(&String::from_utf8_lossy(&chunk));
                }
            }

            let location = headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("location"))
                .map(|(_, value)| value.clone());
            if allow_redirects
                && (300..400).contains(&status)
                && let Some(location) = location
                && hop < self.config().max_redirects
            {
                let next = url
                    .join(&location)
                    .map_err(|e| ExecError::Invalid(format!("bad redirect: {e}")))?;
                hops.push(format!("{status} -> {next}"));
                last_status = Some(status);
                last_headers = headers;
                last_body = body;
                last_truncated = truncated;
                current = next;
                continue;
            }
            return Ok(FetchResult {
                final_url: url,
                status: Some(status),
                headers,
                body,
                truncated,
                blocked: None,
                network_error: None,
                hops,
            });
        }
        Ok(FetchResult {
            final_url: current,
            status: last_status,
            headers: last_headers,
            body: last_body,
            truncated: last_truncated,
            blocked: None,
            network_error: Some("too many redirects".into()),
            hops,
        })
    }

    async fn capture_and_observe(
        &self,
        run: &Run,
        step: &darkbox_domain::RunStepRecord,
        endpoint_id: EndpointId,
        fetch: &FetchResult,
        identity_id: Option<IdentityId>,
    ) -> ExecResult<EvidenceId> {
        let mut exchange = CapturedExchange::new("GET", fetch.final_url.as_str());
        exchange.status = fetch.status;
        exchange.response_headers = fetch.headers.clone();
        exchange.response_body = Some(fetch.body.clone());
        exchange.truncated = fetch.truncated;
        if !fetch.hops.is_empty() {
            exchange.note = Some(format!("redirects: {}", fetch.hops.join(", ")));
        }
        let captured = capture(&exchange, &CaptureLimits::default());
        let evidence_id = self
            .store_evidence_text(
                run,
                step.id,
                identity_id,
                EvidenceKind::HttpExchange,
                &format!("{} {}", fetch.status.unwrap_or(0), fetch.final_url),
                &captured.text,
            )
            .await?;
        let subject = endpoint_id.as_uuid();
        self.add_observation(
            run,
            "endpoint",
            subject,
            "http.status",
            json!(fetch.status),
            1.0,
            Some(evidence_id),
        )
        .await?;
        let content_type = fetch
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
            .map(|(_, value)| value.clone())
            .unwrap_or_default();
        if !content_type.is_empty() {
            self.add_observation(
                run,
                "endpoint",
                subject,
                "http.content_type",
                json!(content_type),
                1.0,
                Some(evidence_id),
            )
            .await?;
        }
        self.add_observation(
            run,
            "endpoint",
            subject,
            "http.final_url",
            json!(fetch.final_url.to_string()),
            1.0,
            Some(evidence_id),
        )
        .await?;
        if let Some(title) = extract_title(&fetch.body) {
            self.add_observation(
                run,
                "endpoint",
                subject,
                "html.title",
                json!(title),
                0.9,
                Some(evidence_id),
            )
            .await?;
        }
        for (technology, confidence) in detect_technologies(&fetch.headers, &fetch.body) {
            self.add_observation(
                run,
                "endpoint",
                subject,
                "technology",
                json!({ "name": technology }),
                confidence,
                Some(evidence_id),
            )
            .await?;
        }
        let state = match fetch.status {
            Some(0) | None => EndpointState::Unreachable,
            Some(_) => EndpointState::Observed,
        };
        self.db
            .mark_observed(
                run.workspace_id,
                run.assessment_id,
                endpoint_id,
                state,
                identity_id,
                "executor",
            )
            .await?;
        Ok(evidence_id)
    }
}

fn extract_title(body: &str) -> Option<String> {
    let lower = body.to_ascii_lowercase();
    let start = lower.find("<title")?;
    let open_end = lower[start..].find('>')? + start + 1;
    let end = lower[open_end..].find("</title>")? + open_end;
    let title = body[open_end..end].trim();
    if title.is_empty() || title.len() > 200 {
        None
    } else {
        Some(title.to_string())
    }
}

fn extract_candidates(base: &Url, body: &str) -> Vec<String> {
    let mut out = Vec::new();
    for captures in LINK_RE.captures_iter(body) {
        if let Some(value) = captures.get(1) {
            let raw = value.as_str().trim();
            if raw.is_empty() || raw.starts_with("mailto:") || raw.starts_with("javascript:") {
                continue;
            }
            if let Ok(url) = base.join(raw)
                && matches!(url.scheme(), "http" | "https")
            {
                out.push(url.to_string());
            }
        }
    }
    for captures in PATH_RE.captures_iter(body) {
        if let Some(value) = captures.get(1)
            && let Ok(url) = base.join(value.as_str())
            && matches!(url.scheme(), "http" | "https")
        {
            out.push(url.to_string());
        }
    }
    out.sort();
    out.dedup();
    out.truncate(200);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_links_and_js_routes() {
        let base = Url::parse("http://127.0.0.1:8080/about").unwrap();
        let body = r#"<a href="/pricing">p</a><script src="assets/app.js"></script>
            <script>const APP_ROUTES = ["/app/north/reports", "/app/north/settings"];</script>"#;
        let links = extract_candidates(&base, body);
        assert!(links.contains(&"http://127.0.0.1:8080/pricing".to_string()));
        assert!(links.contains(&"http://127.0.0.1:8080/assets/app.js".to_string()));
        assert!(links.contains(&"http://127.0.0.1:8080/app/north/reports".to_string()));
    }

    #[test]
    fn detects_server_and_generator() {
        let headers = vec![("server".to_string(), "lab/1.0".to_string())];
        let found = detect_technologies(
            &headers,
            "<meta name=\"generator\" content=\"Synthetic 2\">",
        );
        assert!(found.iter().any(|(name, _)| name == "lab/1.0"));
        assert!(found.iter().any(|(name, _)| name == "Synthetic 2"));
    }
}
