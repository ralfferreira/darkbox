//! Narrative memory and bounded context recovery.
//!
//! Retrieval filters by workspace and assessment before ranking. The context
//! package never includes evidence bodies, cookies or tokens.

use darkbox_contracts::*;
use darkbox_storage::{Db, DbError};
use serde_json::json;

pub mod model;

const DEFAULT_TOKEN_BUDGET: i64 = 3000;

#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error(transparent)]
    Db(#[from] DbError),
    #[error("model: {0}")]
    Model(String),
}

pub type Result<T> = std::result::Result<T, MemoryError>;

pub struct ContextBuilder<'a> {
    pub db: &'a Db,
    pub workspace_id: WorkspaceId,
    pub assessment_id: AssessmentId,
    pub identity_id: Option<IdentityId>,
    pub token_budget: i64,
}

impl<'a> ContextBuilder<'a> {
    pub fn new(
        db: &'a Db,
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        identity_id: Option<IdentityId>,
    ) -> Self {
        Self {
            db,
            workspace_id,
            assessment_id,
            identity_id,
            token_budget: DEFAULT_TOKEN_BUDGET,
        }
    }

    pub fn with_budget(mut self, budget: i64) -> Self {
        self.token_budget = budget.clamp(300, 32_000);
        self
    }

    pub async fn build(&self) -> Result<ContextPackageDto> {
        let assessment = self
            .db
            .get_assessment(self.workspace_id, self.assessment_id)
            .await?;
        let scope = self
            .db
            .get_scope(self.workspace_id, self.assessment_id, None)
            .await?;
        let coverage = self
            .db
            .coverage(self.workspace_id, self.assessment_id)
            .await?;
        let observations = self
            .db
            .recent_observations(self.workspace_id, self.assessment_id, 15)
            .await?;
        let flows = self
            .db
            .list_flows(self.workspace_id, self.assessment_id)
            .await?;
        let findings = self
            .db
            .list_findings(self.workspace_id, self.assessment_id, None)
            .await?;
        let pages = self
            .db
            .list_memory_pages(self.workspace_id, self.assessment_id)
            .await?;
        let runs = self
            .db
            .list_runs(self.workspace_id, self.assessment_id, 10)
            .await?;
        let identities = self
            .db
            .list_identities(self.workspace_id, self.assessment_id)
            .await?;

        let mut sections = Vec::new();
        let mut omitted = Vec::new();
        let mut references = Vec::new();

        sections.push(ContextSectionDto {
            title: "Objective".into(),
            body: format!(
                "Assessment: {}\nCompany: {}\nEnvironment: {}\nObjective: {}\nState: {}",
                assessment.name,
                assessment_company(self.db, self.workspace_id, self.assessment_id).await?,
                assessment.environment,
                assessment.objective,
                assessment.state
            ),
            reference_kind: Some("assessment".into()),
        });
        references.push(MemoryReferenceDto {
            kind: "assessment".into(),
            id: self.assessment_id.to_string(),
            label: assessment.name.clone(),
        });

        sections.push(ContextSectionDto {
            title: format!("Effective scope v{}", scope.version),
            body: format!(
                "Include hosts: {}\nExclude hosts: {}\nExclude paths: {}\nPorts: {}\nAction classes: {}\nPrivate network: {}\nBudget: {} requests, {} browser steps, {} model calls, {}s",
                scope.policy.include_hosts.join(", "),
                scope.policy.exclude_hosts.join(", "),
                scope.policy.exclude_paths.join(", "),
                scope
                    .policy
                    .allowed_ports
                    .iter()
                    .map(|p| p.to_string())
                    .collect::<Vec<_>>()
                    .join(", "),
                scope
                    .policy
                    .action_classes
                    .iter()
                    .map(|a| a.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                scope.policy.allow_private_network,
                scope.policy.limits.max_requests,
                scope.policy.limits.max_browser_steps,
                scope.policy.limits.max_model_calls,
                scope.policy.limits.max_duration_seconds
            ),
            reference_kind: Some("scope".into()),
        });

        sections.push(ContextSectionDto {
            title: "Available typed operations".into(),
            body: "Use these through the API; there is no shell.\n- firecrawl_scrape: render an in-scope public URL through the managed browser and store Markdown as evidence. Requires the web.firecrawl capability.\n- firecrawl_map: list candidate URLs without fetching their content. Candidates still need scope and observation.\n- network_scan: TCP connect scan restricted to policy-approved hosts and ports. Requires network.nmap.\n- http.observe: direct bounded request when the page does not require a browser.\n- flow.reproduction: replay a recorded flow with its identity and tenant.".into(),
            reference_kind: None,
        });

        sections.push(ContextSectionDto {
            title: "Coverage with explicit denominator".into(),
            body: format!(
                "Known candidates: {}; observed: {}; excluded: {}; unreachable: {}. Flows reproduced {} of {}. Findings confirmed {}; awaiting review {}. Evidence items {} ({} bytes).",
                coverage.known_candidates,
                coverage.observed_endpoints,
                coverage.excluded_endpoints,
                coverage.unreachable_endpoints,
                coverage.flows_reproduced,
                coverage.flows_known,
                coverage.findings_confirmed,
                coverage.findings_needs_review,
                coverage.evidence_count,
                coverage.evidence_bytes
            ),
            reference_kind: None,
        });

        let identity = self
            .identity_id
            .and_then(|id| identities.iter().find(|i| i.id == id));
        if let Some(identity) = identity {
            sections.push(ContextSectionDto {
                title: "Identity context".into(),
                body: format!(
                    "Using identity '{}' ({}), tenants {}, session {}. Historical records from other identities are not reused.",
                    identity.label,
                    identity.role_label,
                    identity.tenant_keys.join(", "),
                    identity.session_state
                ),
                reference_kind: Some("identity".into()),
            });
            references.push(MemoryReferenceDto {
                kind: "identity".into(),
                id: identity.id.to_string(),
                label: identity.label.clone(),
            });
        }

        if !observations.is_empty() {
            let body = observations
                .iter()
                .take(8)
                .map(|o| {
                    format!(
                        "- {} {} = {} (confidence {:.2}, {})",
                        o.subject_kind, o.property, o.value, o.confidence, o.source
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(ContextSectionDto {
                title: "Recent observations".into(),
                body,
                reference_kind: Some("observation".into()),
            });
        }

        let relevant_flows: Vec<_> = flows
            .iter()
            .filter(|flow| {
                self.identity_id.is_none()
                    || flow.identity_id.is_none()
                    || flow.identity_id == self.identity_id
            })
            .collect();
        if !relevant_flows.is_empty() {
            let body = relevant_flows
                .iter()
                .take(10)
                .map(|flow| {
                    format!(
                        "- {} [{}] v{} identity={} tenant={} steps={}",
                        flow.name,
                        flow.state,
                        flow.version,
                        flow.identity_id
                            .map(|i| i.to_string())
                            .unwrap_or_else(|| "none".into()),
                        flow.tenant_key.as_deref().unwrap_or("unknown"),
                        flow.steps.len()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(ContextSectionDto {
                title: "Known flows".into(),
                body,
                reference_kind: Some("flow".into()),
            });
        }

        if !findings.is_empty() {
            let body = findings
                .iter()
                .take(10)
                .map(|finding| {
                    format!(
                        "- {} [{}] severity={} evidence={}",
                        finding.title,
                        finding.state,
                        finding.severity,
                        finding.evidence_ids.len()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(ContextSectionDto {
                title: "Findings and hypotheses".into(),
                body,
                reference_kind: Some("finding".into()),
            });
            for finding in findings.iter().take(10) {
                references.push(MemoryReferenceDto {
                    kind: "finding".into(),
                    id: finding.id.to_string(),
                    label: finding.title.clone(),
                });
            }
        }

        let waiting: Vec<_> = runs
            .iter()
            .filter(|run| {
                matches!(
                    run.state,
                    RunState::WaitingOperator | RunState::Interrupted | RunState::Failed
                )
            })
            .collect();
        if !waiting.is_empty() {
            let body = waiting
                .iter()
                .map(|run| {
                    format!(
                        "- run {} [{}] kind={} effect_unknown={} reason={}",
                        run.id,
                        run.state,
                        run.kind,
                        run.effect_unknown,
                        run.reason.as_deref().unwrap_or("")
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            sections.push(ContextSectionDto {
                title: "Runs that need attention".into(),
                body,
                reference_kind: Some("run".into()),
            });
        }

        for page in pages.iter().take(6) {
            let body = truncate_to(&page.body_markdown, 1400);
            sections.push(ContextSectionDto {
                title: format!("Memory: {} (v{})", page.title, page.version),
                body,
                reference_kind: Some("memory".into()),
            });
            references.push(MemoryReferenceDto {
                kind: "memory".into(),
                id: page.id.to_string(),
                label: page.title.clone(),
            });
        }

        let mut chosen = Vec::new();
        let mut used = 0i64;
        for section in sections {
            let estimate = estimate_tokens(&section.title) + estimate_tokens(&section.body);
            if used + estimate > self.token_budget {
                omitted.push(format!(
                    "{} ({} tokens, budget exhausted)",
                    section.title, estimate
                ));
                continue;
            }
            used += estimate;
            chosen.push(section);
        }

        Ok(ContextPackageDto {
            assessment_id: self.assessment_id,
            objective: assessment.objective,
            policy_version: scope.version,
            identity_id: self.identity_id,
            generated_at: chrono::Utc::now(),
            token_budget: self.token_budget,
            estimated_tokens: used,
            sections: chosen,
            omitted,
            references,
        })
    }
}

async fn assessment_company(
    db: &Db,
    workspace_id: WorkspaceId,
    assessment_id: AssessmentId,
) -> Result<String> {
    let assessment = db.get_assessment(workspace_id, assessment_id).await?;
    Ok(db
        .get_company(workspace_id, assessment.company_id)
        .await?
        .name)
}

fn truncate_to(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        text.to_string()
    } else {
        let mut out: String = text.chars().take(max_chars).collect();
        out.push_str("\n[truncated in context package]");
        out
    }
}

pub fn estimate_tokens(text: &str) -> i64 {
    ((text.chars().count() as f64) / 4.0).ceil() as i64
}

pub fn context_to_prompt(package: &ContextPackageDto) -> String {
    let mut out = String::new();
    out.push_str(&format!("OBJECTIVE\n{}\n\n", package.objective));
    for section in &package.sections {
        out.push_str(&format!("## {}\n{}\n\n", section.title, section.body));
    }
    if !package.omitted.is_empty() {
        out.push_str("## Omitted\n");
        for item in &package.omitted {
            out.push_str(&format!("- {item}\n"));
        }
    }
    out
}

/// Deterministic synthesis used when no model is configured. Every statement
/// points at a record; conflicts are preserved.
pub async fn synthesize(
    db: &Db,
    workspace_id: WorkspaceId,
    assessment_id: AssessmentId,
    actor: &str,
) -> Result<MemoryPageDto> {
    let package = ContextBuilder::new(db, workspace_id, assessment_id, None)
        .with_budget(4000)
        .build()
        .await?;
    let runs = db.list_runs(workspace_id, assessment_id, 5).await?;
    let findings = db.list_findings(workspace_id, assessment_id, None).await?;
    let mut body = String::new();
    body.push_str("# Assessment state\n\n");
    body.push_str(&format!("{}\n\n", package.objective));
    body.push_str("## Coverage\n\n");
    for section in package
        .sections
        .iter()
        .filter(|s| s.title.starts_with("Coverage"))
    {
        body.push_str(&section.body);
        body.push_str("\n\n");
    }
    body.push_str("## Recent runs\n\n");
    if runs.is_empty() {
        body.push_str("No runs recorded yet.\n\n");
    } else {
        for run in &runs {
            body.push_str(&format!(
                "- Run `{}` {} ({}) started {}; spend {} requests.\n",
                run.id,
                run.state,
                run.kind,
                run.started_at
                    .map(|t| t.to_rfc3339())
                    .unwrap_or_else(|| "not started".into()),
                run.spent.requests
            ));
        }
        body.push('\n');
    }
    body.push_str("## Findings\n\n");
    if findings.is_empty() {
        body.push_str("No findings recorded. Absence here is not a security conclusion.\n\n");
    } else {
        for finding in &findings {
            body.push_str(&format!(
                "- [{}] {}: {} (evidence: {}).\n",
                finding.state,
                finding.severity,
                finding.claim,
                finding.evidence_ids.len()
            ));
        }
        body.push('\n');
    }
    body.push_str("## Open gaps\n\n");
    if package.omitted.is_empty() {
        body.push_str("- None recorded in the context budget.\n");
    } else {
        for omitted in &package.omitted {
            body.push_str(&format!("- Omitted from context: {omitted}\n"));
        }
    }
    body.push_str(
        "\n_This page is a derived synthesis. Observations remain the canonical source._\n",
    );

    let mut created_by = actor.to_string();
    let mut state = "synthesis".to_string();
    if std::env::var("DARKBOX_MODEL_SYNTHESIS")
        .map(|v| v == "1")
        .unwrap_or(false)
        && let Some(config) = model::ModelConfig::from_env()
    {
        match model::summarize(&config, &package, 900).await {
            Ok(summary) => {
                body = format!(
                    "# Assessment synthesis\n\n{summary}\n\n_Generated from the bounded context package. Observations remain the canonical source; conflicts are not resolved by this text._\n"
                );
                created_by = format!("model:{}", config.name());
                state = "synthesis_derived".to_string();
            }
            Err(error) => {
                body.push_str(&format!(
                    "\n\n_Model synthesis failed ({error}); deterministic synthesis kept._\n"
                ));
            }
        }
    }

    let references = vec![MemoryReferenceDto {
        kind: "assessment".into(),
        id: assessment_id.to_string(),
        label: "assessment".into(),
    }];
    Ok(db
        .save_memory_page(
            workspace_id,
            assessment_id,
            "assessment-state",
            "Assessment state",
            &body,
            &references,
            "current",
            &created_by,
        )
        .await?)
    .map(|mut page| {
        page.state = state;
        page
    })
}

pub fn export_markdown(pages: &[MemoryPageDto], assessment_name: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {assessment_name} — memory export\n\n"));
    out.push_str(&format!(
        "Generated {} by DarkBox. Narrative pages are derived from records; they do not replace evidence.\n\n",
        chrono::Utc::now().to_rfc3339()
    ));
    for page in pages {
        out.push_str(&format!("## {} (v{})\n\n", page.title, page.version));
        out.push_str(&page.body_markdown);
        out.push_str("\n\n");
        if !page.references.is_empty() {
            out.push_str("References:\n");
            for reference in &page.references {
                out.push_str(&format!("- {} `{}`\n", reference.kind, reference.id));
            }
            out.push('\n');
        }
    }
    out
}

pub fn checkpoint_body(run: &RunSummary, steps: &[RunStepDto]) -> String {
    let mut body = String::new();
    body.push_str(&format!(
        "Run `{}` finished as `{}`.\n\n",
        run.id, run.state
    ));
    body.push_str(&format!(
        "Kind: {}. Policy version: {}. Requests: {}/{}. Browser steps: {}/{}. Model calls: {}/{}. Cost: {} / {} micros.\n\n",
        run.kind,
        run.policy_version,
        run.spent.requests,
        run.budget.max_requests,
        run.spent.browser_steps,
        run.budget.max_browser_steps,
        run.spent.model_calls,
        run.budget.max_model_calls,
        run.spent.cost_micros,
        run.budget.max_cost_micros
    ));
    if run.effect_unknown {
        body.push_str(
            "**An external effect has an unknown outcome. It is not repeated automatically.**\n\n",
        );
    }
    body.push_str("Steps:\n");
    for step in steps {
        body.push_str(&format!(
            "- {} {} {} (attempt {}, effect {}) — {}\n",
            step.ordinal, step.kind, step.state, step.attempt, step.effect_status, step.summary
        ));
    }
    if let Some(reason) = &run.reason {
        body.push_str(&format!("\nReason: {reason}\n"));
    }
    body
}

pub fn run_references(run_id: RunId) -> Vec<MemoryReferenceDto> {
    vec![MemoryReferenceDto {
        kind: "run".into(),
        id: run_id.to_string(),
        label: "run".into(),
    }]
}

pub fn json_manifest<T: serde::Serialize>(value: &T) -> serde_json::Value {
    json!(value)
}
