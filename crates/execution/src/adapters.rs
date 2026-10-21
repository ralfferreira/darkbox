use std::collections::BTreeSet;
use std::net::IpAddr;

use darkbox_contracts::*;
use darkbox_domain::Run;
use darkbox_evidence::{CaptureLimits, CapturedExchange, capture};
use darkbox_integrations::{BurpAdapter, FirecrawlAdapter, NmapAdapter};
use darkbox_policy::{NetworkContext, PolicyEngine};
use serde_json::json;
use url::Url;

use crate::{
    ExecError, ExecResult, Executor, Outcome, StepOutcomeState, operation_str, operation_u64,
};

impl Executor {
    pub(crate) async fn network_scan(
        &self,
        run: &Run,
        step: &darkbox_domain::RunStepRecord,
    ) -> ExecResult<Outcome> {
        let adapter = NmapAdapter::detect()
            .map_err(|error| ExecError::CapabilityUnavailable(error.to_string()))?;
        let mut targets: Vec<String> = step
            .operation
            .get("targets")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if targets.is_empty() {
            targets = self
                .db
                .distinct_hosts(run.workspace_id, run.assessment_id)
                .await?;
        }
        targets.retain(|host| !host.trim().is_empty());
        targets.truncate(20);
        if targets.is_empty() {
            return Err(ExecError::Invalid(
                "network scan needs at least one host".into(),
            ));
        }
        let requested_ports: BTreeSet<u16> = step
            .operation
            .get("ports")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|value| value.as_u64())
                    .filter_map(|value| u16::try_from(value).ok())
                    .collect()
            })
            .unwrap_or_default();
        let current = self
            .db
            .get_scope(run.workspace_id, run.assessment_id, None)
            .await?;
        let base_ports: Vec<u16> = if requested_ports.is_empty() {
            current.policy.allowed_ports.clone()
        } else {
            requested_ports
                .into_iter()
                .filter(|port| current.policy.allowed_ports.contains(port))
                .collect()
        };
        if base_ports.is_empty() {
            return Err(ExecError::Blocked(
                "no requested port is allowed by the current policy".into(),
            ));
        }

        let mut scanned_hosts = 0usize;
        let mut open_ports = 0usize;
        let mut skipped: Vec<String> = Vec::new();
        let mut evidence_ids = Vec::new();
        for target in &targets {
            let mut allowed_ports = Vec::new();
            for port in &base_ports {
                let decision = self.authorize_network(run, target, *port).await?;
                if decision.allowed {
                    allowed_ports.push(*port);
                }
            }
            if allowed_ports.is_empty() {
                skipped.push(format!("{target}: no port allowed by policy"));
                continue;
            }
            if !self.db.reserve_budget(run.id, "requests", 1).await? {
                return Err(ExecError::Blocked("budget_exhausted: requests".into()));
            }
            let report = adapter
                .scan(std::slice::from_ref(target), &allowed_ports)
                .await;
            let report = match report {
                Ok(report) => report,
                Err(darkbox_integrations::nmap::NmapError::Failed(message)) => {
                    skipped.push(format!("{target}: {message}"));
                    continue;
                }
                Err(error) => {
                    return Err(ExecError::Invalid(format!("nmap failed: {error}")));
                }
            };
            let evidence = self
                .store_evidence_text(
                    run,
                    step.id,
                    None,
                    EvidenceKind::ToolOutput,
                    &format!("nmap XML for {target}"),
                    &report.xml,
                )
                .await?;
            evidence_ids.push(evidence);
            let asset = self
                .db
                .upsert_asset(
                    run.workspace_id,
                    run.assessment_id,
                    AssetKind::Host,
                    target,
                    "nmap scan",
                )
                .await?;
            self.add_observation(
                run,
                "asset",
                asset.as_uuid(),
                "network.scan",
                json!({
                    "scanner": report.version,
                    "command": report.command,
                    "ports": allowed_ports,
                    "evidence": evidence,
                }),
                1.0,
                Some(evidence),
            )
            .await?;
            for host in &report.hosts {
                if !host.address.is_empty() {
                    self.add_observation(
                        run,
                        "asset",
                        asset.as_uuid(),
                        "network.address",
                        json!({
                            "address": host.address,
                            "hostname": host.hostname,
                            "status": host.status,
                        }),
                        0.9,
                        Some(evidence),
                    )
                    .await?;
                }
                for port in &host.ports {
                    if port.state != "open" {
                        continue;
                    }
                    open_ports += 1;
                    self.add_observation(
                        run,
                        "asset",
                        asset.as_uuid(),
                        "network.open_port",
                        json!({
                            "port": port.port,
                            "protocol": port.protocol,
                            "service": port.service,
                            "product": port.product,
                            "version": port.version,
                        }),
                        0.95,
                        Some(evidence),
                    )
                    .await?;
                }
            }
            scanned_hosts += 1;
        }
        Ok(Outcome {
            state: if scanned_hosts == 0 {
                StepOutcomeState::Blocked
            } else {
                StepOutcomeState::Succeeded
            },
            effect: EffectStatus::NotApplicable,
            summary: format!(
                "scanned {} {} on policy-approved ports; {} open {}",
                scanned_hosts,
                if scanned_hosts == 1 { "host" } else { "hosts" },
                open_ports,
                if open_ports == 1 { "port" } else { "ports" }
            ),
            result: json!({
                "hosts_scanned": scanned_hosts,
                "open_ports": open_ports,
                "skipped": skipped,
                "ports_attempted": base_ports,
                "evidence_ids": evidence_ids,
            }),
        })
    }

    async fn authorize_network(
        &self,
        run: &Run,
        host: &str,
        port: u16,
    ) -> ExecResult<ScopeDecision> {
        let current = self
            .db
            .get_scope(run.workspace_id, run.assessment_id, None)
            .await?;
        let resolved = if host.parse::<IpAddr>().is_ok() {
            vec![host.parse().unwrap()]
        } else {
            match tokio::time::timeout(
                std::time::Duration::from_secs(3),
                tokio::net::lookup_host((host, port)),
            )
            .await
            {
                Ok(Ok(addrs)) => addrs.map(|addr| addr.ip()).collect(),
                _ => Vec::new(),
            }
        };
        let mut ctx = NetworkContext::new(host, port);
        ctx.resolved_ips = resolved;
        let pinned = PolicyEngine::new(&run.policy, run.policy_version);
        let first = pinned.evaluate_network(&ctx);
        if !first.allowed {
            return Ok(first);
        }
        Ok(PolicyEngine::new(&current.policy, current.version).evaluate_network(&ctx))
    }

    pub(crate) async fn firecrawl_map(
        &self,
        run: &Run,
        step: &darkbox_domain::RunStepRecord,
    ) -> ExecResult<Outcome> {
        let adapter = FirecrawlAdapter::from_env()
            .map_err(|error| ExecError::CapabilityUnavailable(error.to_string()))?;
        let target = operation_str(step, "url")
            .map(str::to_string)
            .or_else(|| {
                step.operation
                    .get("targets")
                    .and_then(|value| value.as_array())
                    .and_then(|items| items.first())
                    .and_then(|value| value.as_str())
                    .map(str::to_string)
            })
            .ok_or_else(|| ExecError::Invalid("firecrawl map needs a target URL".into()))?;
        self.authorize_firecrawl(run, &target).await?;
        if !self.db.reserve_budget(run.id, "requests", 1).await? {
            return Err(ExecError::Blocked("budget_exhausted: requests".into()));
        }
        let limit = operation_u64(step, "limit", 100) as u32;
        let search = operation_str(step, "search");
        let links = adapter
            .map(&target, limit, search)
            .await
            .map_err(|error| ExecError::Network(error.to_string()))?;
        let evidence = self
            .store_evidence_text(
                run,
                step.id,
                None,
                EvidenceKind::ToolOutput,
                &format!("firecrawl map {target} ({} links)", links.len()),
                &serde_json::to_string_pretty(&json!({
                    "provider": "firecrawl",
                    "mode": if adapter.is_mcp() { "mcp" } else { "rest" },
                    "target": target,
                    "links": links,
                }))
                .unwrap_or_default(),
            )
            .await?;
        let asset = self
            .db
            .upsert_asset(
                run.workspace_id,
                run.assessment_id,
                AssetKind::Origin,
                &target,
                "firecrawl map",
            )
            .await?;
        let mut candidates = 0usize;
        let mut excluded = 0usize;
        for link in &links {
            let Ok(key) = darkbox_domain::EndpointKey::parse(link, "GET") else {
                continue;
            };
            let endpoint = self
                .db
                .upsert_candidate(
                    run.workspace_id,
                    run.assessment_id,
                    &key,
                    "firecrawl-map",
                    &format!("discovered by firecrawl map from {target}"),
                    Some(run.id),
                    None,
                )
                .await?;
            candidates += 1;
            let (_, decision) = self
                .authorize_url(
                    run,
                    link,
                    ActionClass::Query,
                    darkbox_policy::TargetRole::TestTarget,
                )
                .await?;
            if !decision.allowed {
                self.db
                    .mark_observed(
                        run.workspace_id,
                        run.assessment_id,
                        endpoint,
                        EndpointState::Excluded,
                        None,
                        "policy",
                    )
                    .await?;
                excluded += 1;
            }
        }
        self.add_observation(
            run,
            "asset",
            asset.as_uuid(),
            "discovery.firecrawl",
            json!({
                "target": target,
                "links": candidates,
                "excluded": excluded,
                "evidence": evidence,
            }),
            0.9,
            Some(evidence),
        )
        .await?;
        Ok(Outcome::succeeded(
            format!("firecrawl map: {candidates} candidates, {excluded} excluded"),
            json!({
                "candidates": candidates,
                "excluded": excluded,
                "evidence_id": evidence,
                "provider": if adapter.is_mcp() { "firecrawl-mcp" } else { "firecrawl-api" },
            }),
        ))
    }

    pub(crate) async fn firecrawl_scrape(
        &self,
        run: &Run,
        step: &darkbox_domain::RunStepRecord,
    ) -> ExecResult<Outcome> {
        let adapter = FirecrawlAdapter::from_env()
            .map_err(|error| ExecError::CapabilityUnavailable(error.to_string()))?;
        let mut targets: Vec<String> = step
            .operation
            .get("targets")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|value| value.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        if let Some(url) = operation_str(step, "url") {
            targets.push(url.to_string());
        }
        targets.truncate(operation_u64(step, "max_urls", 5) as usize);
        if targets.is_empty() {
            return Err(ExecError::Invalid(
                "firecrawl scrape needs at least one URL".into(),
            ));
        }
        let objective = operation_str(step, "objective")
            .unwrap_or("Observe the page for an authorized assessment.");
        let mut observed = 0usize;
        let mut blocked = Vec::new();
        let mut evidence_ids = Vec::new();
        for target in &targets {
            match self.authorize_firecrawl(run, target).await {
                Ok(()) => {}
                Err(error) => {
                    blocked.push(format!("{target}: {error}"));
                    continue;
                }
            }
            if !self.db.reserve_budget(run.id, "requests", 1).await? {
                return Err(ExecError::Blocked("budget_exhausted: requests".into()));
            }
            let page = match adapter.scrape(target, objective).await {
                Ok(page) => page,
                Err(error) => {
                    blocked.push(format!("{target}: {error}"));
                    continue;
                }
            };
            let key = darkbox_domain::EndpointKey::parse(target, "GET")
                .map_err(|error| ExecError::Invalid(error.to_string()))?;
            let endpoint = self
                .db
                .upsert_candidate(
                    run.workspace_id,
                    run.assessment_id,
                    &key,
                    "firecrawl-scrape",
                    "managed browser scrape",
                    Some(run.id),
                    None,
                )
                .await?;
            let mut document = page.markdown.clone();
            if document.trim().is_empty() {
                document = page.html.clone();
            }
            if let Some(title) = &page.title {
                document = format!("# {title}\n\n{document}");
            }
            let evidence = self
                .store_evidence_text(
                    run,
                    step.id,
                    None,
                    EvidenceKind::HttpResponseBody,
                    &format!("firecrawl scrape {target}"),
                    &document,
                )
                .await?;
            evidence_ids.push(evidence);
            self.add_observation(
                run,
                "endpoint",
                endpoint.as_uuid(),
                "http.status",
                json!(page.status_code),
                0.9,
                Some(evidence),
            )
            .await?;
            self.add_observation(
                run,
                "endpoint",
                endpoint.as_uuid(),
                "http.final_url",
                json!(target),
                1.0,
                Some(evidence),
            )
            .await?;
            if let Some(title) = &page.title {
                self.add_observation(
                    run,
                    "endpoint",
                    endpoint.as_uuid(),
                    "html.title",
                    json!(title),
                    0.9,
                    Some(evidence),
                )
                .await?;
            }
            self.add_observation(
                run,
                "endpoint",
                endpoint.as_uuid(),
                "capture.firecrawl",
                json!({
                    "provider": "firecrawl",
                    "mode": if adapter.is_mcp() { "mcp" } else { "rest" },
                    "markdown_bytes": page.markdown.len(),
                    "links": page.links.len(),
                    "evidence": evidence,
                }),
                0.9,
                Some(evidence),
            )
            .await?;
            self.db
                .mark_observed(
                    run.workspace_id,
                    run.assessment_id,
                    endpoint,
                    EndpointState::Observed,
                    None,
                    "firecrawl",
                )
                .await?;
            for link in page.links.iter().take(500) {
                let Ok(link_key) = darkbox_domain::EndpointKey::parse(link, "GET") else {
                    continue;
                };
                let _ = self
                    .db
                    .upsert_candidate(
                        run.workspace_id,
                        run.assessment_id,
                        &link_key,
                        "firecrawl-link",
                        &format!("discovered by firecrawl scrape of {target}"),
                        Some(run.id),
                        None,
                    )
                    .await?;
            }
            observed += 1;
        }
        Ok(Outcome {
            state: if observed == 0 {
                StepOutcomeState::Blocked
            } else {
                StepOutcomeState::Succeeded
            },
            effect: EffectStatus::NotApplicable,
            summary: format!(
                "firecrawl scrape: {observed} observed, {} blocked",
                blocked.len()
            ),
            result: json!({
                "observed": observed,
                "blocked": blocked,
                "evidence_ids": evidence_ids,
                "provider": if adapter.is_mcp() { "firecrawl-mcp" } else { "firecrawl-api" },
            }),
        })
    }

    async fn authorize_firecrawl(&self, run: &Run, target: &str) -> ExecResult<()> {
        let url = Url::parse(target)
            .map_err(|error| ExecError::Invalid(format!("invalid URL {target}: {error}")))?;
        let (_, decision) = self
            .authorize_url(
                run,
                target,
                ActionClass::Query,
                darkbox_policy::TargetRole::TestTarget,
            )
            .await?;
        if !decision.allowed {
            return Err(ExecError::Blocked(format!(
                "firecrawl target denied by {}",
                decision.rule
            )));
        }
        let allow_private = std::env::var("DARKBOX_FIRECRAWL_ALLOW_PRIVATE")
            .map(|value| value == "1")
            .unwrap_or(false);
        if let Some(host) = url.host_str() {
            let private = if let Ok(ip) = host.parse::<IpAddr>() {
                darkbox_policy::is_private(&ip)
            } else {
                match tokio::time::timeout(
                    std::time::Duration::from_secs(3),
                    tokio::net::lookup_host((host, url.port_or_known_default().unwrap_or(443))),
                )
                .await
                {
                    Ok(Ok(addrs)) => addrs
                        .map(|addr| addr.ip())
                        .any(|ip| darkbox_policy::is_private(&ip)),
                    _ => false,
                }
            };
            if private && !allow_private {
                return Err(ExecError::Blocked(
                    "firecrawl cannot attest egress to a private or loopback address".into(),
                ));
            }
        }
        Ok(())
    }

    pub(crate) async fn burp_import(
        &self,
        run: &Run,
        step: &darkbox_domain::RunStepRecord,
    ) -> ExecResult<Outcome> {
        let adapter = BurpAdapter::from_env()
            .map_err(|error| ExecError::CapabilityUnavailable(error.to_string()))?;
        let status = adapter
            .probe()
            .await
            .map_err(|error| ExecError::CapabilityUnavailable(error.to_string()))?;
        if !status.available {
            return Err(ExecError::CapabilityUnavailable(status.detail));
        }
        let limit = operation_u64(step, "limit", 100) as u32;
        let exchanges = adapter
            .import(limit)
            .await
            .map_err(|error| ExecError::Network(error.to_string()))?;
        let mut imported = 0usize;
        let mut skipped = Vec::new();
        let mut evidence_ids = Vec::new();
        for exchange in exchanges {
            let absolute = absolutize(&exchange);
            let Ok(url) = Url::parse(&absolute) else {
                skipped.push(format!("invalid URL in exchange: {}", exchange.url));
                continue;
            };
            let (_, decision) = self
                .authorize_url(
                    run,
                    url.as_str(),
                    ActionClass::Query,
                    darkbox_policy::TargetRole::TestTarget,
                )
                .await?;
            if !decision.allowed {
                skipped.push(format!("{absolute}: {}", decision.rule));
                continue;
            }
            let key = match darkbox_domain::EndpointKey::from_url(&url, &exchange.method) {
                Ok(key) => key,
                Err(_) => {
                    skipped.push(format!("invalid endpoint: {absolute}"));
                    continue;
                }
            };
            let endpoint = self
                .db
                .upsert_candidate(
                    run.workspace_id,
                    run.assessment_id,
                    &key,
                    "burp",
                    &format!("imported from {}", status.server),
                    Some(run.id),
                    None,
                )
                .await?;
            let mut captured = CapturedExchange::new(&exchange.method, url.as_str());
            captured.request_headers = exchange.request_headers.clone();
            captured.request_body = exchange.request_body.clone();
            captured.status = exchange.status;
            captured.response_headers = exchange.response_headers.clone();
            captured.response_body = exchange.response_body.clone();
            captured.note = exchange.note.clone();
            let sanitized = capture(&captured, &CaptureLimits::default());
            let evidence = self
                .store_evidence_text(
                    run,
                    step.id,
                    None,
                    EvidenceKind::HttpExchange,
                    &format!(
                        "{} {} -> {}",
                        exchange.method,
                        absolute,
                        exchange
                            .status
                            .map(|status| status.to_string())
                            .unwrap_or_else(|| "unknown".into())
                    ),
                    &sanitized.text,
                )
                .await?;
            evidence_ids.push(evidence);
            self.add_observation(
                run,
                "endpoint",
                endpoint.as_uuid(),
                "http.status",
                json!(exchange.status),
                0.8,
                Some(evidence),
            )
            .await?;
            self.add_observation(
                run,
                "endpoint",
                endpoint.as_uuid(),
                "capture.burp",
                json!({
                    "server": status.server,
                    "version": status.version,
                    "tool": status.import_tool,
                    "captured_at": exchange.captured_at,
                    "evidence": evidence,
                }),
                0.8,
                Some(evidence),
            )
            .await?;
            self.db
                .mark_observed(
                    run.workspace_id,
                    run.assessment_id,
                    endpoint,
                    EndpointState::Observed,
                    None,
                    "burp",
                )
                .await?;
            imported += 1;
        }
        Ok(Outcome {
            state: if imported == 0 {
                StepOutcomeState::Blocked
            } else {
                StepOutcomeState::Succeeded
            },
            effect: EffectStatus::NotApplicable,
            summary: format!(
                "burp import: {imported} exchanges retained, {} skipped",
                skipped.len()
            ),
            result: json!({
                "imported": imported,
                "skipped": skipped,
                "evidence_ids": evidence_ids,
                "burp": {
                    "server": status.server,
                    "version": status.version,
                    "tool": status.import_tool,
                }
            }),
        })
    }
}

fn absolutize(exchange: &darkbox_integrations::BurpExchange) -> String {
    if exchange.url.starts_with("http://") || exchange.url.starts_with("https://") {
        return exchange.url.clone();
    }
    let host = exchange
        .request_headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.as_str())
        .unwrap_or("");
    if host.is_empty() {
        return exchange.url.clone();
    }
    let scheme = exchange
        .request_headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("x-forwarded-proto"))
        .map(|(_, value)| value.as_str())
        .unwrap_or_else(|| {
            if host.starts_with("127.") || host.starts_with("localhost") {
                "http"
            } else {
                "https"
            }
        });
    format!("{scheme}://{host}{}", exchange.url)
}
