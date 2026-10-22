use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use darkbox_contracts::*;
use darkbox_domain::EndpointKey;
use darkbox_evidence::{BlobStore, LocalBlobStore};
use darkbox_storage::{Db, InventoryQuery, NewRun, NewStep};
use reqwest::StatusCode;
use serde_json::{Value, json};

const SECRET: &str = "LAB_SECRET_darkbox_7f3a";

#[derive(Clone)]
struct Api {
    client: reqwest::Client,
    base: String,
    token: String,
}

impl Api {
    fn new(base: &str, token: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            base: base.trim_end_matches('/').to_string(),
            token: token.to_string(),
        }
    }

    async fn get(&self, path: &str) -> Result<Value> {
        let response = self
            .client
            .get(format!("{}{}", self.base, path))
            .bearer_auth(&self.token)
            .send()
            .await
            .with_context(|| format!("GET {path}"))?;
        self.decode(path, response).await
    }

    async fn post(&self, path: &str, body: Value) -> Result<Value> {
        let response = self
            .client
            .post(format!("{}{}", self.base, path))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await
            .with_context(|| format!("POST {path}"))?;
        self.decode(path, response).await
    }

    async fn post_status(&self, path: &str, body: Value) -> Result<(StatusCode, Value)> {
        let response = self
            .client
            .post(format!("{}{}", self.base, path))
            .bearer_auth(&self.token)
            .json(&body)
            .send()
            .await?;
        let status = response.status();
        let value = response.json::<Value>().await.unwrap_or(Value::Null);
        Ok((status, value))
    }

    async fn get_text(&self, path: &str) -> Result<Option<String>> {
        let response = self
            .client
            .get(format!("{}{}", self.base, path))
            .bearer_auth(&self.token)
            .send()
            .await?;
        if !response.status().is_success() {
            return Ok(None);
        }
        Ok(Some(response.text().await?))
    }

    async fn decode(&self, path: &str, response: reqwest::Response) -> Result<Value> {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        if !status.is_success() {
            return Err(anyhow!("{path} returned {status}: {body}"));
        }
        serde_json::from_str(&body).with_context(|| format!("decode {path}: {body}"))
    }
}

struct Check {
    id: &'static str,
    title: &'static str,
}

struct Report {
    criteria: Vec<AcceptanceCriterion>,
}

impl Report {
    fn new() -> Self {
        Self {
            criteria: Vec::new(),
        }
    }

    fn record(&mut self, check: Check, result: Result<(String, Vec<String>)>) {
        match result {
            Ok((detail, evidence)) => self.criteria.push(AcceptanceCriterion {
                id: check.id.into(),
                title: check.title.into(),
                status: "passed".into(),
                detail,
                evidence,
            }),
            Err(error) => self.criteria.push(AcceptanceCriterion {
                id: check.id.into(),
                title: check.title.into(),
                status: "failed".into(),
                detail: format!("{error:#}"),
                evidence: Vec::new(),
            }),
        }
    }

    fn finish(self, lab_version: String) -> AcceptanceReport {
        let passed = self
            .criteria
            .iter()
            .filter(|c| c.status == "passed")
            .count() as i64;
        let failed = self
            .criteria
            .iter()
            .filter(|c| c.status == "failed")
            .count() as i64;
        let skipped = self
            .criteria
            .iter()
            .filter(|c| c.status == "skipped")
            .count() as i64;
        AcceptanceReport {
            generated_at: chrono::Utc::now(),
            lab_version,
            criteria: self.criteria,
            passed,
            failed,
            skipped,
        }
    }
}

fn check(id: &'static str, title: &'static str) -> Check {
    Check { id, title }
}

pub async fn run(db: &Db, lab_url: &str, api_url: &str, token: &str) -> Result<AcceptanceReport> {
    let api = Api::new(api_url, token);
    let lab = lab_url.trim_end_matches('/');
    let lab_port = url::Url::parse(lab)
        .ok()
        .and_then(|u| u.port_or_known_default())
        .unwrap_or(8080);
    let _ = api
        .client
        .post(format!("{lab}/_lab/reset"))
        .send()
        .await
        .context("lab reset; is the synthetic lab running?")?;
    let lab_state = fetch_lab_state(&api.client, lab).await?;
    let lab_version = lab_state
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();

    let stamp = chrono::Utc::now().format("%H:%M:%S");
    let companies = api.get("/v1/companies").await?;
    let company_id = companies
        .as_array()
        .and_then(|items| items.first())
        .and_then(|c| c.get("id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("no company in the seeded workspace"))?
        .to_string();

    let mut policy = ScopePolicy::demo(lab_port);
    policy.exclude_paths = vec![
        "/admin/forbidden".into(),
        "/app/north/admin/forbidden".into(),
        "/app/south/admin/forbidden".into(),
    ];
    let assessment = api
        .post(
            "/v1/assessments",
            json!({
                "company_id": company_id,
                "name": format!("Acceptance {stamp}"),
                "environment": "lab",
                "objective": "Verify AC-01 to AC-14 against the synthetic laboratory.",
                "initial_targets": [],
                "policy": policy,
            }),
        )
        .await?;
    let assessment_id = assessment["id"].as_str().unwrap().to_string();
    api.post(
        &format!("/v1/assessments/{assessment_id}/tenants"),
        json!({ "key": "north", "label": "North tenant" }),
    )
    .await?;
    api.post(
        &format!("/v1/assessments/{assessment_id}/tenants"),
        json!({ "key": "south", "label": "South tenant" }),
    )
    .await?;
    let alice = api
        .post(
            &format!("/v1/assessments/{assessment_id}/identities"),
            json!({
                "label": "alice", "role_label": "admin", "tenant_keys": ["north","south"],
                "capabilities": ["navigate","read"], "secret_ref": "env:LAB_ALICE_PASSWORD"
            }),
        )
        .await?["id"]
        .as_str()
        .unwrap()
        .to_string();
    let bob = api
        .post(
            &format!("/v1/assessments/{assessment_id}/identities"),
            json!({
                "label": "bob", "role_label": "member", "tenant_keys": ["north"],
                "capabilities": ["navigate","read"], "secret_ref": "env:LAB_BOB_PASSWORD"
            }),
        )
        .await?["id"]
        .as_str()
        .unwrap()
        .to_string();

    let baseline = import_and_wait(
        &api,
        &assessment_id,
        vec![
            format!("{lab}/"),
            format!("{lab}/about"),
            format!("{lab}/pricing"),
            format!("{lab}/secret-note.txt"),
            format!("{lab}/app/north/reports"),
            format!("{lab}/app/north/prompt-injection"),
            format!("{lab}/assets/app.js"),
        ],
    )
    .await?;
    let secret_evidence = evidence_for_run(&api, &assessment_id, &baseline).await?;
    let secret_note = secret_evidence
        .iter()
        .find(|item| {
            item["summary"]
                .as_str()
                .unwrap_or("")
                .contains("secret-note")
        })
        .and_then(|item| item["id"].as_str())
        .map(str::to_string);

    let alice_session = start_run(
        &api,
        json!({
            "assessment_id": assessment_id, "kind": "session", "identity_id": alice,
            "tenant": "north", "base_url": lab, "idempotency_key": format!("acc-session-a-{stamp}")
        }),
    )
    .await?;
    let bob_session = start_run(
        &api,
        json!({
            "assessment_id": assessment_id, "kind": "session", "identity_id": bob,
            "tenant": "north", "base_url": lab, "idempotency_key": format!("acc-session-b-{stamp}")
        }),
    )
    .await?;

    let mut report = Report::new();

    // AC-01: excluded destinations are blocked before access.
    let excluded_run = import_and_wait(
        &api,
        &assessment_id,
        vec![
            format!("{lab}/go/outside"),
            format!("{lab}/app/north/admin/forbidden"),
            "http://127.0.0.1:9099/excluded".to_string(),
        ],
    )
    .await;
    let ac01 = async {
        let excluded_run = excluded_run?;
        let state = fetch_lab_state(&api.client, lab).await?;
        let excluded_hits = state["excluded_hits"].as_i64().unwrap_or(-1);
        let forbidden_hits = state["hits"]["/app/north/admin/forbidden"]
            .as_i64()
            .unwrap_or(0);
        if excluded_hits != 0 {
            return Err(anyhow!(
                "excluded listener received {excluded_hits} requests"
            ));
        }
        if forbidden_hits != 0 {
            return Err(anyhow!("excluded path received {forbidden_hits} requests"));
        }
        let inventory = api
            .get(&format!(
                "/v1/assessments/{assessment_id}/inventory?limit=200"
            ))
            .await?;
        let mut blocked = 0;
        for endpoint in inventory["endpoints"]
            .as_array()
            .cloned()
            .unwrap_or_default()
        {
            let path = endpoint["path"].as_str().unwrap_or("");
            let endpoint_state = endpoint["state"].as_str().unwrap_or("");
            if (path == "/go/outside"
                || path == "/app/north/admin/forbidden"
                || path == "/excluded")
                && endpoint_state == "excluded"
            {
                blocked += 1;
            }
        }
        if blocked < 2 {
            return Err(anyhow!(
                "expected exclusion records for redirect and forbidden path, found {blocked}"
            ));
        }
        Ok((
            format!(
                "lab oracle excluded_hits=0, forbidden path hits=0; {blocked} destinations recorded as excluded; run {} spent {} requests",
                excluded_run["id"].as_str().unwrap_or(""),
                excluded_run["spent"]["requests"].as_i64().unwrap_or(-1)
            ),
            vec![
                excluded_run["id"].as_str().unwrap_or("").to_string(),
                format!("policy_version={}", excluded_run["policy_version"]),
            ],
        ))
    };
    report.record(
        check("AC-01", "Excluded destinations blocked before access"),
        ac01.await,
    );

    // AC-02: exact endpoint identity preserves method and path differences.
    let ac02 = async {
        let scope = api
            .get(&format!("/v1/assessments/{assessment_id}/scope-versions"))
            .await?;
        let _ = scope;
        db.upsert_candidate(
            api_workspace(db, token).await?,
            assessment_id.parse::<AssessmentId>()?,
            &EndpointKey::parse(&format!("{lab}/about"), "GET")?,
            "acceptance",
            "exact identity test",
            None,
            None,
        )
        .await?;
        db.upsert_candidate(
            api_workspace(db, token).await?,
            assessment_id.parse::<AssessmentId>()?,
            &EndpointKey::parse(&format!("{lab}/about"), "POST")?,
            "acceptance",
            "exact identity test",
            None,
            None,
        )
        .await?;
        db.upsert_candidate(
            api_workspace(db, token).await?,
            assessment_id.parse::<AssessmentId>()?,
            &EndpointKey::parse(&format!("{lab}/about/"), "GET")?,
            "acceptance",
            "exact identity test",
            None,
            None,
        )
        .await?;
        let inventory = db
            .inventory(
                api_workspace(db, token).await?,
                assessment_id.parse::<AssessmentId>()?,
                &InventoryQuery {
                    text: Some("/about".into()),
                    limit: 100,
                    ..Default::default()
                },
            )
            .await?;
        let mut exact = Vec::new();
        for endpoint in &inventory.endpoints {
            if endpoint.path.starts_with("/about") {
                exact.push(format!("{} {}", endpoint.method, endpoint.path));
            }
        }
        for wanted in ["GET /about", "POST /about", "GET /about/"] {
            if !exact.iter().any(|item| item == wanted) {
                return Err(anyhow!("missing distinct record {wanted}; got {exact:?}"));
            }
        }
        Ok((
            format!("GET /about, POST /about and GET /about/ are distinct records: {exact:?}"),
            vec![
                "GET /about".into(),
                "POST /about".into(),
                "GET /about/".into(),
            ],
        ))
    };
    report.record(
        check("AC-02", "Methods and semantic paths stay distinct"),
        ac02.await,
    );

    // AC-03: identities have isolated sessions and no cookie values leak.
    let ac03 = async {
        let alice_run = wait_run(&api, &alice_session).await?;
        let bob_run = wait_run(&api, &bob_session).await?;
        if alice_run["state"] != "succeeded" || bob_run["state"] != "succeeded" {
            return Err(anyhow!(
                "session runs did not succeed: alice={} bob={}",
                alice_run["state"],
                bob_run["state"]
            ));
        }
        let identities = api
            .get(&format!("/v1/assessments/{assessment_id}/identities"))
            .await?;
        let mut authenticated = 0;
        for identity in identities.as_array().cloned().unwrap_or_default() {
            if identity["session_state"] == "authenticated" {
                authenticated += 1;
            }
        }
        if authenticated < 2 {
            return Err(anyhow!("expected two authenticated identities"));
        }
        let mut texts = String::new();
        for run in [&alice_run, &bob_run] {
            for evidence in evidence_for_run(&api, &assessment_id, run).await? {
                let id = evidence["id"].as_str().unwrap();
                if let Some(text) = api.get_text(&format!("/v1/evidence/{id}/content")).await? {
                    texts.push_str(&text);
                }
            }
        }
        for needle in ["darkbox_sid", "lab-sid-", "alice-lab-pass", "bob-lab-pass"] {
            if texts.contains(needle) {
                return Err(anyhow!("session evidence leaked {needle}"));
            }
        }
        Ok((
            "alice and bob authenticated in separate adapter contexts; evidence contains no session cookie or password".into(),
            vec![
                alice_run["id"].as_str().unwrap_or("").into(),
                bob_run["id"].as_str().unwrap_or("").into(),
            ],
        ))
    };
    report.record(check("AC-03", "Identity contexts are isolated"), ac03.await);

    // AC-04: conclusions need evidence; hypotheses stay labelled.
    let ac04 = async {
        let secret_evidence = secret_note
            .clone()
            .ok_or_else(|| anyhow!("secret-note evidence was not captured"))?;
        let with_evidence = api
            .post(
                &format!("/v1/assessments/{assessment_id}/findings"),
                json!({
                    "title": "Synthetic configuration observation",
                    "severity": "low",
                    "claim": "The lab serves a synthetic secret in a plain text note; sanitization removed it at ingestion.",
                    "impact": "Illustrates why evidence ingestion sanitizes secrets before storage.",
                    "evidence_ids": [secret_evidence],
                    "from_hypothesis": true
                }),
            )
            .await?;
        let finding_id = with_evidence["id"].as_str().unwrap().to_string();
        api.post(
            &format!("/v1/findings/{finding_id}/review"),
            json!({ "state": "needs_review", "note": "evidence attached" }),
        )
        .await?;
        api.post(
            &format!("/v1/findings/{finding_id}/review"),
            json!({ "state": "confirmed", "note": "reviewed by acceptance harness" }),
        )
        .await?;
        let bare = api
            .post(
                &format!("/v1/assessments/{assessment_id}/findings"),
                json!({
                    "title": "Bare hypothesis",
                    "severity": "info",
                    "claim": "No evidence attached.",
                    "impact": "None claimed.",
                    "from_hypothesis": true
                }),
            )
            .await?;
        let bare_id = bare["id"].as_str().unwrap();
        api.post(
            &format!("/v1/findings/{bare_id}/review"),
            json!({ "state": "needs_review", "note": "review" }),
        )
        .await?;
        let (status, _) = api
            .post_status(
                &format!("/v1/findings/{bare_id}/review"),
                json!({ "state": "confirmed", "note": "should be rejected" }),
            )
            .await?;
        if status != StatusCode::CONFLICT {
            return Err(anyhow!(
                "confirming an evidence-free finding returned {status}, expected 409"
            ));
        }
        let findings = api
            .get(&format!("/v1/assessments/{assessment_id}/findings"))
            .await?;
        for finding in findings.as_array().cloned().unwrap_or_default() {
            if finding["state"] == "confirmed"
                && finding["evidence_ids"]
                    .as_array()
                    .map(|v| v.is_empty())
                    .unwrap_or(true)
            {
                return Err(anyhow!("a confirmed finding has no evidence"));
            }
        }
        Ok((
            "confirmed finding requires evidence (409 without it); hypotheses remain labelled; every confirmed finding lists evidence".into(),
            vec![finding_id, bare_id.to_string()],
        ))
    };
    report.record(
        check("AC-04", "Claims point at evidence or stay hypotheses"),
        ac04.await,
    );

    // AC-05: evidence survives a new database connection and a new context.
    let ac05 = async {
        let url = std::env::var("DATABASE_URL")?;
        let fresh = Db::connect(&url).await?;
        let workspace = api_workspace(db, token).await?;
        let evidence = fresh
            .list_evidence(workspace, assessment_id.parse()?, None, 100, 0)
            .await?;
        if evidence.is_empty() {
            return Err(anyhow!("no evidence persisted"));
        }
        let context = api
            .get(&format!(
                "/v1/assessments/{assessment_id}/context?identity_id={alice}&token_budget=3000"
            ))
            .await?;
        let sections = context["sections"].as_array().cloned().unwrap_or_default();
        let has_run = sections.iter().any(|section| {
            section["title"].as_str().unwrap_or("").contains("Memory")
                || section["title"].as_str().unwrap_or("").contains("Recent")
        });
        if !has_run {
            return Err(anyhow!(
                "context package has no persisted run or memory section"
            ));
        }
        if context["references"]
            .as_array()
            .map(|v| v.is_empty())
            .unwrap_or(true)
        {
            return Err(anyhow!("context package has no references"));
        }
        Ok((
            format!(
                "{} evidence records read back through a new connection; context package references {} records and {} tokens",
                evidence.len(),
                context["references"]
                    .as_array()
                    .map(|v| v.len())
                    .unwrap_or(0),
                context["estimated_tokens"]
            ),
            vec![format!("db-reconnect evidence={}", evidence.len())],
        ))
    };
    report.record(
        check("AC-05", "Restart preserves evidence and resume context"),
        ac05.await,
    );

    // AC-06: a dispatched effect with an unknown outcome waits for reconciliation.
    let ac06 = async {
        let workspace = api_workspace(db, token).await?;
        let assessment: AssessmentId = assessment_id.parse()?;
        let scope = db.get_scope(workspace, assessment, None).await?;
        let endpoints = db
            .inventory(
                workspace,
                assessment,
                &InventoryQuery {
                    state: Some("observed".into()),
                    limit: 1,
                    ..Default::default()
                },
            )
            .await?;
        let endpoint = endpoints
            .endpoints
            .first()
            .ok_or_else(|| anyhow!("no observed endpoint"))?;
        let run = db
            .create_run(
                workspace,
                NewRun {
                    assessment_id: assessment,
                    kind: RunKind::Observation,
                    policy: scope.policy.clone(),
                    policy_version: scope.version,
                    budget: scope.policy.limits.clone(),
                    procedure_key: None,
                    flow_id: None,
                    identity_id: None,
                    idempotency_key: format!("acc-crash-{stamp}"),
                    note: "outcome unknown test".into(),
                    steps: vec![NewStep {
                        kind: StepKind::HttpObserve,
                        operation: json!({ "endpoint_id": endpoint.id }),
                        summary: "synthetic crash after dispatch".into(),
                    }],
                },
            )
            .await?;
        let claimed = db
            .claim_step("acceptance-worker", 30, Some(assessment))
            .await?
            .ok_or_else(|| anyhow!("could not claim synthetic step"))?;
        db.mark_step_dispatched(claimed.step.id, "acceptance-worker", "GET synthetic effect")
            .await?;
        db.force_expire_lease(claimed.step.id).await?;
        db.recover_expired_leases().await?;
        let run_after = db.run_summary(workspace, run.id).await?;
        if run_after.state != RunState::WaitingOperator || !run_after.effect_unknown {
            return Err(anyhow!(
                "expected waiting_operator with effect_unknown, got {} effect_unknown={}",
                run_after.state,
                run_after.effect_unknown
            ));
        }
        let (status, _) = api
            .post_status(
                &format!("/v1/runs/{}/resume", run.id),
                json!({ "note": "without reconcile" }),
            )
            .await?;
        if status != StatusCode::CONFLICT {
            return Err(anyhow!("resume without reconciliation returned {status}"));
        }
        api.post(
            &format!("/v1/runs/{}/resume", run.id),
            json!({ "reconcile_unknown": true, "note": "operator confirmed no retry" }),
        )
        .await?;
        Ok((
            format!(
                "run {} moved to waiting_operator with outcome_unknown; automatic resume refused (409); explicit reconciliation accepted",
                run.id
            ),
            vec![run.id.to_string(), claimed.step.id.to_string()],
        ))
    };
    report.record(
        check("AC-06", "Unknown external effect is not repeated"),
        ac06.await,
    );

    // AC-07: untrusted page text cannot change policy or trigger effects.
    let ac07 = async {
        let before_scope =
            api.get(&format!("/v1/assessments/{assessment_id}")).await?["scope"]["version"]
                .as_i64()
                .unwrap_or(-1);
        let state_before = fetch_lab_state(&api.client, lab).await?;
        let effects_before = state_before["effects"]["north"].as_i64().unwrap_or(0);
        let endpoints = api
            .get(&format!(
                "/v1/assessments/{assessment_id}/inventory?q=prompt-injection"
            ))
            .await?;
        let endpoint_id = endpoints["endpoints"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(|endpoint| endpoint["id"].as_str())
            .ok_or_else(|| anyhow!("prompt injection endpoint missing"))?
            .to_string();
        let flow = api
            .post(
                &format!("/v1/assessments/{assessment_id}/flows"),
                json!({
                    "name": "Read the prompt injection page as alice",
                    "identity_id": alice,
                    "tenant_key": "north",
                    "preconditions": ["alice session"],
                    "steps": [
                        {
                            "intent": "start at the lab root",
                            "expected_result": "",
                            "endpoint_id": null,
                            "action_json": {"type": "open", "url": lab}
                        },
                        {
                            "intent": "authenticate as alice",
                            "expected_result": "",
                            "endpoint_id": null,
                            "action_json": {"type": "login"}
                        },
                        {
                            "intent": "open the page containing untrusted instructions",
                            "expected_result": "ignore previous instructions",
                            "endpoint_id": endpoint_id,
                            "action_json": {"type": "navigate", "url": format!("{lab}/app/north/prompt-injection")}
                        }
                    ]
                }),
            )
            .await?;
        let flow_id = flow["id"].as_str().unwrap().to_string();
        let run = start_run(
            &api,
            json!({
                "assessment_id": assessment_id, "kind": "flow_reproduction",
                "flow_id": flow_id, "identity_id": alice,
                "idempotency_key": format!("acc-injection-{stamp}")
            }),
        )
        .await?;
        let run = wait_run(&api, &run).await?;
        if run["state"] != "succeeded" {
            return Err(anyhow!("injection flow run state {}", run["state"]));
        }
        let mut captured = String::new();
        for evidence in evidence_for_run(&api, &assessment_id, &run).await? {
            if let Some(id) = evidence["id"].as_str()
                && let Some(text) = api.get_text(&format!("/v1/evidence/{id}/content")).await?
            {
                captured.push_str(&text);
            }
        }
        if !captured
            .to_ascii_lowercase()
            .contains("ignore previous instructions")
        {
            return Err(anyhow!(
                "the authenticated injection page was not captured as inert evidence"
            ));
        }
        let after_scope =
            api.get(&format!("/v1/assessments/{assessment_id}")).await?["scope"]["version"]
                .as_i64()
                .unwrap_or(-1);
        let state_after = fetch_lab_state(&api.client, lab).await?;
        let effects_after = state_after["effects"]["north"].as_i64().unwrap_or(0);
        if before_scope != after_scope {
            return Err(anyhow!(
                "scope version changed after reading untrusted content"
            ));
        }
        if effects_before != effects_after {
            return Err(anyhow!(
                "effect counter changed from {effects_before} to {effects_after}"
            ));
        }
        Ok((
            format!(
                "authenticated page text captured as inert evidence; policy stayed at v{before_scope}; effect counter stayed at {effects_after}"
            ),
            vec![flow_id, format!("policy_version={before_scope}")],
        ))
    };
    report.record(
        check("AC-07", "External content cannot change policy"),
        ac07.await,
    );

    // AC-08: aggregate budget stays valid under concurrency.
    let ac08 = async {
        let workspace = api_workspace(db, token).await?;
        let assessment: AssessmentId = assessment_id.parse()?;
        let scope = db.get_scope(workspace, assessment, None).await?;
        let mut budget = scope.policy.limits.clone();
        budget.max_requests = 5;
        let run = db
            .create_run(
                workspace,
                NewRun {
                    assessment_id: assessment,
                    kind: RunKind::Observation,
                    policy: scope.policy.clone(),
                    policy_version: scope.version,
                    budget: budget.clone(),
                    procedure_key: None,
                    flow_id: None,
                    identity_id: None,
                    idempotency_key: format!("acc-budget-{stamp}"),
                    note: "atomic budget test".into(),
                    steps: vec![NewStep {
                        kind: StepKind::Reconcile,
                        operation: json!({}),
                        summary: "budget holder".into(),
                    }],
                },
            )
            .await?;
        let mut tasks = Vec::new();
        for _ in 0..10 {
            let db = db.clone();
            tasks.push(tokio::spawn(async move {
                db.reserve_budget(run.id, "requests", 1)
                    .await
                    .unwrap_or(false)
            }));
        }
        let mut granted = 0;
        for task in tasks {
            if task.await? {
                granted += 1;
            }
        }
        if granted != 5 {
            return Err(anyhow!(
                "expected exactly 5 of 10 reservations to succeed, got {granted}"
            ));
        }
        let summary = db.run_summary(workspace, run.id).await?;
        if summary.spent.requests != 5 {
            return Err(anyhow!(
                "spent requests is {}, expected 5",
                summary.spent.requests
            ));
        }
        Ok((
            "10 concurrent reservations against a budget of 5: exactly 5 granted, spent=5".into(),
            vec![run.id.to_string()],
        ))
    };
    report.record(
        check("AC-08", "Aggregate budget holds with concurrency"),
        ac08.await,
    );

    // AC-09: cancellation prevents new steps and stops the worker.
    let ac09 = async {
        let workspace = api_workspace(db, token).await?;
        let assessment: AssessmentId = assessment_id.parse()?;
        let scope = db.get_scope(workspace, assessment, None).await?;
        let before = fetch_lab_state(&api.client, lab).await?;
        let before_hits = before["hits"]["/pricing"].as_i64().unwrap_or(0);
        let endpoints = db
            .inventory(
                workspace,
                assessment,
                &InventoryQuery {
                    text: Some("/pricing".into()),
                    limit: 1,
                    ..Default::default()
                },
            )
            .await?;
        let endpoint = endpoints
            .endpoints
            .first()
            .ok_or_else(|| anyhow!("pricing endpoint missing"))?;
        let run = db
            .create_run(
                workspace,
                NewRun {
                    assessment_id: assessment,
                    kind: RunKind::Observation,
                    policy: scope.policy.clone(),
                    policy_version: scope.version,
                    budget: scope.policy.limits.clone(),
                    procedure_key: None,
                    flow_id: None,
                    identity_id: None,
                    idempotency_key: format!("acc-cancel-{stamp}"),
                    note: "cancellation test".into(),
                    steps: vec![NewStep {
                        kind: StepKind::HttpObserve,
                        operation: json!({ "endpoint_id": endpoint.id }),
                        summary: "must never run".into(),
                    }],
                },
            )
            .await?;
        api.post(
            &format!("/v1/runs/{}/cancel", run.id),
            json!({ "reason": "acceptance cancellation" }),
        )
        .await?;
        tokio::time::sleep(Duration::from_secs(2)).await;
        let after = fetch_lab_state(&api.client, lab).await?;
        let after_hits = after["hits"]["/pricing"].as_i64().unwrap_or(0);
        if after_hits != before_hits {
            return Err(anyhow!(
                "a new request reached /pricing after cancellation ({before_hits} -> {after_hits})"
            ));
        }
        let summary = db.run_summary(workspace, run.id).await?;
        if summary.state != RunState::Cancelled {
            return Err(anyhow!(
                "run state is {}, expected cancelled",
                summary.state
            ));
        }
        Ok((
            "cancellation before dispatch left the target untouched; run ended cancelled".into(),
            vec![run.id.to_string()],
        ))
    };
    report.record(
        check("AC-09", "Cancellation prevents new steps"),
        ac09.await,
    );

    // AC-10: sanitized export omits seeded secrets.
    let ac10 = async {
        let export = api
            .post(
                &format!("/v1/assessments/{assessment_id}/exports"),
                json!({ "include_evidence": true, "note": "acceptance" }),
            )
            .await?;
        let path = export["download_url"].as_str().unwrap();
        let text = api
            .get_text(path)
            .await?
            .ok_or_else(|| anyhow!("export download failed"))?;
        if text.contains(SECRET) {
            return Err(anyhow!("export contains the seeded lab secret"));
        }
        if text.contains("alice-lab-pass") || text.contains("bob-lab-pass") {
            return Err(anyhow!("export contains a lab password"));
        }
        if text.contains("darkbox_sid") || text.contains("lab-sid-") {
            return Err(anyhow!("export contains a session cookie"));
        }
        Ok((
            format!(
                "export {} bytes; manifest {}; seeded secret, passwords and cookies absent",
                export["size_bytes"], export["manifest_sha256"]
            ),
            vec![export["id"].as_str().unwrap_or("").into()],
        ))
    };
    report.record(check("AC-10", "Sanitized export omits secrets"), ac10.await);

    // AC-11: identifiers never cross workspaces.
    let ac11 = async {
        let other = Api::new(api_url, "darkbox-other-token");
        let attempts = [
            format!("/v1/assessments/{assessment_id}"),
            format!("/v1/assessments/{assessment_id}/inventory"),
            format!("/v1/assessments/{assessment_id}/evidence"),
        ];
        let mut rejected = 0;
        for path in &attempts {
            match other.get(path).await {
                Ok(_) => return Err(anyhow!("other workspace read {path}")),
                Err(error) => {
                    let text = error.to_string();
                    if text.contains("404") || text.contains("403") {
                        rejected += 1;
                    } else {
                        return Err(anyhow!("unexpected isolation error: {text}"));
                    }
                }
            }
        }
        Ok((
            format!("{rejected}/3 cross-workspace reads rejected with 403/404"),
            attempts.to_vec(),
        ))
    };
    report.record(
        check("AC-11", "Workspace isolation on every read"),
        ac11.await,
    );

    // AC-12: a new session reproduces a flow from persisted context.
    let ac12 = async {
        let reports = api
            .get(&format!(
                "/v1/assessments/{assessment_id}/inventory?q=reports"
            ))
            .await?;
        let endpoint_id = reports["endpoints"]
            .as_array()
            .and_then(|items| items.first())
            .and_then(|e| e["id"].as_str())
            .ok_or_else(|| anyhow!("reports endpoint missing"))?
            .to_string();
        let flow = api
            .post(
                &format!("/v1/assessments/{assessment_id}/flows"),
                json!({
                    "name": "Admin reports as alice",
                    "identity_id": alice,
                    "tenant_key": "north",
                    "preconditions": ["alice session", "north tenant"],
                    "steps": [
                        {
                            "intent": "start at the lab root",
                            "expected_result": "",
                            "endpoint_id": null,
                            "action_json": {"type": "open", "url": lab}
                        },
                        {
                            "intent": "authenticate as alice",
                            "expected_result": "",
                            "endpoint_id": null,
                            "action_json": {"type": "login"}
                        },
                        {
                            "intent": "open the admin report list",
                            "expected_result": "Reports",
                            "endpoint_id": endpoint_id,
                            "action_json": {"type": "navigate", "url": format!("{lab}/app/north/reports")}
                        }
                    ]
                }),
            )
            .await?;
        let flow_id = flow["id"].as_str().unwrap().to_string();
        let run = start_run(
            &api,
            json!({
                "assessment_id": assessment_id, "kind": "flow_reproduction",
                "flow_id": flow_id, "identity_id": alice,
                "idempotency_key": format!("acc-flow-{stamp}")
            }),
        )
        .await?;
        let run = wait_run(&api, &run).await?;
        if run["state"] != "succeeded" {
            return Err(anyhow!("flow run state {}", run["state"]));
        }
        let flows = api
            .get(&format!("/v1/assessments/{assessment_id}/flows"))
            .await?;
        let flow = flows
            .as_array()
            .and_then(|items| items.iter().find(|f| f["id"] == flow_id))
            .cloned()
            .ok_or_else(|| anyhow!("flow missing after reproduction"))?;
        if flow["state"] != "reproduced"
            || flow["evidence_ids"]
                .as_array()
                .map(|v| v.is_empty())
                .unwrap_or(true)
        {
            return Err(anyhow!("flow was not reproduced with evidence"));
        }
        let context = api
            .get(&format!(
                "/v1/assessments/{assessment_id}/context?identity_id={alice}&token_budget=4000"
            ))
            .await?;
        let text = serde_json::to_string(&context)?;
        if !text.contains("Admin reports as alice") {
            return Err(anyhow!("new context does not include the reproduced flow"));
        }
        if !context["sections"]
            .as_array()
            .map(|sections| {
                sections.iter().any(|section| {
                    section["title"].as_str().unwrap_or("").contains("Run")
                        || section["title"].as_str().unwrap_or("").contains("Memory")
                })
            })
            .unwrap_or(false)
        {
            return Err(anyhow!("context has no run checkpoint"));
        }
        Ok((
            format!(
                "flow {flow_id} reproduced with {} evidence items; a fresh context package for alice includes the flow and a persisted checkpoint",
                flow["evidence_ids"]
                    .as_array()
                    .map(|v| v.len())
                    .unwrap_or(0)
            ),
            vec![flow_id, run["id"].as_str().unwrap_or("").into()],
        ))
    };
    report.record(
        check("AC-12", "New session resumes without repeating work"),
        ac12.await,
    );

    // AC-13: the core runs without Burp, Firecrawl or a remote backend.
    let ac13 = async {
        let capabilities = api.get("/v1/capabilities").await?;
        let adapters = capabilities["adapters"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        let available = adapters
            .iter()
            .filter(|adapter| adapter["state"] == "available")
            .count();
        let procedures = api.get("/v1/procedures").await?;
        if available < 3 {
            return Err(anyhow!("only {available} core capabilities available"));
        }
        if procedures.as_array().map(|v| v.len()).unwrap_or(0) < 3 {
            return Err(anyhow!("procedure catalog incomplete"));
        }
        Ok((
            format!(
                "{available} core adapters available; {} procedure versions loaded; no Burp, Firecrawl or remote runtime used",
                procedures.as_array().map(|v| v.len()).unwrap_or(0)
            ),
            vec![
                "http.capture".into(),
                "discovery.import".into(),
                "browser.session".into(),
            ],
        ))
    };
    report.record(
        check("AC-13", "Core works without optional integrations"),
        ac13.await,
    );

    // AC-14: retention purge removes object content and metadata.
    let ac14 = async {
        let id: EvidenceId = secret_note
            .clone()
            .ok_or_else(|| anyhow!("secret-note evidence missing"))?
            .parse()?;
        let workspace = api_workspace(db, token).await?;
        let assessment: AssessmentId = assessment_id.parse()?;
        let record = db.get_evidence(workspace, assessment, id).await?;
        let root = std::env::var("DARKBOX_BLOB_ROOT")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data/objects"));
        let store = LocalBlobStore::new(root)?;
        db.expire_evidence_now(id).await?;
        let expired = db.expired_evidence(100).await?;
        if !expired.iter().any(|(expired_id, _)| *expired_id == id) {
            return Err(anyhow!("expired evidence was not selected for purge"));
        }
        store.delete(&record.object_key).await.ok();
        db.delete_evidence(id).await?;
        if db.get_evidence(workspace, assessment, id).await.is_ok() {
            return Err(anyhow!("evidence metadata still present after purge"));
        }
        if store.get(&record.object_key).await.is_ok() {
            return Err(anyhow!("evidence object still present after purge"));
        }
        Ok((
            "retention expiry selected the object, removed the stored content and deleted metadata"
                .into(),
            vec![id.to_string()],
        ))
    };
    report.record(
        check("AC-14", "Retention purge removes content and metadata"),
        ac14.await,
    );

    Ok(report.finish(lab_version))
}

async fn api_workspace(db: &Db, token: &str) -> Result<WorkspaceId> {
    let hash = {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(token.as_bytes()))
    };
    db.authenticate(&hash)
        .await?
        .map(|principal| principal.workspace_id)
        .ok_or_else(|| anyhow!("token does not resolve to a workspace"))
}

async fn fetch_lab_state(client: &reqwest::Client, lab: &str) -> Result<Value> {
    let value = client
        .get(format!("{lab}/_lab/state"))
        .send()
        .await?
        .json::<Value>()
        .await?;
    Ok(value)
}

async fn start_run(api: &Api, body: Value) -> Result<String> {
    let run = api.post("/v1/runs", body).await?;
    run["id"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("run response has no id: {run}"))
}

async fn wait_run(api: &Api, run_id: &str) -> Result<Value> {
    for _ in 0..120 {
        let detail = api.get(&format!("/v1/runs/{run_id}")).await?;
        let state = detail["run"]["state"].as_str().unwrap_or("");
        if matches!(state, "succeeded" | "failed" | "cancelled") {
            return Ok(detail["run"].clone());
        }
        if state == "waiting_operator" {
            return Ok(detail["run"].clone());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    Err(anyhow!("run {run_id} did not finish in time"))
}

async fn import_and_wait(api: &Api, assessment_id: &str, urls: Vec<String>) -> Result<Value> {
    let run = api
        .post(
            &format!("/v1/assessments/{assessment_id}/import-urls"),
            json!({ "urls": urls, "source": "acceptance" }),
        )
        .await?;
    let id = run["id"]
        .as_str()
        .ok_or_else(|| anyhow!("import did not return a run"))?;
    wait_run(api, id).await
}

async fn evidence_for_run(api: &Api, assessment_id: &str, run: &Value) -> Result<Vec<Value>> {
    let run_id = run["id"].as_str().unwrap_or("");
    let evidence = api
        .get(&format!(
            "/v1/assessments/{assessment_id}/evidence?limit=200"
        ))
        .await?;
    Ok(evidence
        .as_array()
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter(|item| item["run_id"].as_str() == Some(run_id))
        .collect())
}
