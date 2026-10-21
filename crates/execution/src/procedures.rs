use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use darkbox_contracts::*;
use darkbox_domain::{Run, RunStepRecord};
use darkbox_storage::EligibilityInput;
use serde_json::{Value, json};

use crate::{ExecError, ExecResult, Executor, Outcome, StepOutcomeState, operation_str};

impl Executor {
    pub(crate) async fn procedure_step(
        &self,
        run: &Run,
        step: &RunStepRecord,
        cancelled: Arc<AtomicBool>,
    ) -> ExecResult<Outcome> {
        let key = operation_str(step, "procedure_key")
            .ok_or_else(|| ExecError::Invalid("procedure_key is required".into()))?;
        let target_kind = operation_str(step, "target_kind").unwrap_or("endpoint");
        let target_id = operation_str(step, "target_id").unwrap_or("");
        match key {
            "http.configuration_inventory@1.0.0" => {
                self.procedure_configuration(run, step, target_kind, target_id)
                    .await
            }
            "role.visibility_comparison@1.0.0" => {
                self.procedure_visibility(run, step, cancelled).await
            }
            "flow.reproduction@1.0.0" => self.procedure_flow(run, step, cancelled).await,
            other => Err(ExecError::CapabilityUnavailable(format!(
                "procedure {other} has no deterministic executor"
            ))),
        }
    }

    async fn procedure_configuration(
        &self,
        run: &Run,
        step: &RunStepRecord,
        target_kind: &str,
        target_id: &str,
    ) -> ExecResult<Outcome> {
        if target_kind != "endpoint" {
            return Err(ExecError::Invalid(
                "configuration inventory targets endpoints".into(),
            ));
        }
        if target_id.is_empty() {
            return Err(ExecError::Invalid("target_id is required".into()));
        }
        let endpoint_id: EndpointId = target_id
            .parse()
            .map_err(|_| ExecError::Invalid("invalid endpoint_id".into()))?;
        let mut inner = step.clone();
        inner.operation = json!({ "endpoint_id": endpoint_id });
        let outcome = self.observe_step(run, &inner).await?;
        if outcome.state != StepOutcomeState::Succeeded {
            return Ok(outcome);
        }
        let evidence_id = outcome
            .result
            .get("evidence_id")
            .and_then(|v| v.as_str())
            .and_then(|v| v.parse().ok());
        let conclusion = format!(
            "Captured response {} for {}. This inventory records configuration; it does not assert impact or absence of a control.",
            outcome
                .result
                .get("status")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            endpoint_id
        );
        let result_id = self
            .db
            .insert_test_result(
                run.workspace_id,
                run.assessment_id,
                &ProcedureKey::new("http.configuration_inventory", "1.0.0"),
                "endpoint",
                &endpoint_id.to_string(),
                None,
                TestResultState::Inconclusive,
                &conclusion,
                &evidence_id.into_iter().collect::<Vec<_>>(),
            )
            .await?;
        Ok(Outcome {
            state: StepOutcomeState::Succeeded,
            effect: EffectStatus::NotApplicable,
            summary: "configuration inventory completed".into(),
            result: json!({
                "test_result_id": result_id,
                "evidence_id": evidence_id,
                "status": outcome.result.get("status"),
                "state": TestResultState::Inconclusive.as_str(),
            }),
        })
    }

    async fn procedure_visibility(
        &self,
        run: &Run,
        step: &RunStepRecord,
        cancelled: Arc<AtomicBool>,
    ) -> ExecResult<Outcome> {
        let identity_ids: Vec<IdentityId> = step
            .operation
            .get("identity_ids")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|v| v.as_str())
                    .filter_map(|v| v.parse().ok())
                    .collect()
            })
            .unwrap_or_default();
        if identity_ids.len() < 2 {
            return Err(ExecError::Blocked(
                "two test identities are required for visibility comparison".into(),
            ));
        }
        let mut identities = Vec::new();
        for id in &identity_ids {
            let identity = self
                .db
                .get_identity(run.workspace_id, run.assessment_id, *id)
                .await?;
            identities.push(json!({
                "id": identity.id,
                "label": identity.label,
                "role": identity.role_label,
                "tenant_keys": identity.tenant_keys,
                "secret_ref": identity.secret_ref,
            }));
        }
        let base_url = operation_str(step, "base_url").unwrap_or("http://127.0.0.1:8080");
        let mut checks: Vec<Value> = step
            .operation
            .get("checks")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if checks.is_empty() {
            let endpoints = self
                .db
                .inventory(
                    run.workspace_id,
                    run.assessment_id,
                    &darkbox_storage::InventoryQuery {
                        limit: 40,
                        ..Default::default()
                    },
                )
                .await?
                .endpoints;
            let mut candidates: Vec<_> = endpoints
                .iter()
                .filter(|e| e.state != EndpointState::Excluded)
                .collect();
            candidates.sort_by_key(|e| !e.path.starts_with("/app"));
            checks = candidates
                .into_iter()
                .take(20)
                .map(|e| {
                    json!({
                        "label": format!("{} {}{}", e.method, e.host, e.path),
                        "url": format!("{}://{}:{}{}", e.scheme, e.host, e.port, e.path),
                    })
                })
                .collect();
        }
        if checks.is_empty() {
            return Err(ExecError::Blocked(
                "no observed endpoints to compare".into(),
            ));
        }
        if !self
            .db
            .reserve_budget(
                run.id,
                "browser_steps",
                (checks.len() * identity_ids.len()) as i64,
            )
            .await?
        {
            return Err(ExecError::Blocked("budget_exhausted: browser_steps".into()));
        }
        let payload = json!({
            "identities": identities,
            "base_url": base_url,
            "login_path": "/login",
            "checks": checks,
        });
        let result = self
            .browser_call(run, step, "browser.visibility", &payload, cancelled)
            .await?;
        let (mut outcome, evidence_ids) = self
            .apply_browser_payload(run, step, identity_ids.first().copied(), &result.payload)
            .await?;
        let comparisons = result
            .payload
            .get("comparisons")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let differences = comparisons
            .iter()
            .filter(|item| {
                item.get("different")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false)
            })
            .count();
        let state = if outcome.state == StepOutcomeState::Succeeded {
            TestResultState::Inconclusive
        } else {
            TestResultState::Blocked
        };
        let conclusion = format!(
            "{differences} of {} checks differed between {} identities. A visible difference does not prove missing server-side authorization.",
            comparisons.len(),
            identity_ids.len()
        );
        let result_id = self
            .db
            .insert_test_result(
                run.workspace_id,
                run.assessment_id,
                &ProcedureKey::new("role.visibility_comparison", "1.0.0"),
                "assessment",
                &run.assessment_id.to_string(),
                identity_ids.first().copied(),
                state,
                &conclusion,
                &evidence_ids,
            )
            .await?;
        outcome.summary = conclusion.clone();
        outcome.result = json!({
            "test_result_id": result_id,
            "comparisons": comparisons,
            "differences": differences,
            "state": state.as_str(),
        });
        Ok(outcome)
    }

    async fn procedure_flow(
        &self,
        run: &Run,
        step: &RunStepRecord,
        cancelled: Arc<AtomicBool>,
    ) -> ExecResult<Outcome> {
        let flow_id: FlowId = operation_str(step, "flow_id")
            .ok_or_else(|| ExecError::Invalid("flow_id is required".into()))?
            .parse()
            .map_err(|_| ExecError::Invalid("invalid flow_id".into()))?;
        let mut inner = step.clone();
        inner.operation = json!({ "flow_id": flow_id });
        let outcome = self.browser_flow(run, &inner, cancelled).await?;
        let flow = self
            .db
            .get_flow(run.workspace_id, run.assessment_id, flow_id)
            .await?;
        let state = match flow.state {
            FlowState::Reproduced => TestResultState::Passed,
            FlowState::Blocked => TestResultState::Blocked,
            FlowState::Stale => TestResultState::Inconclusive,
            _ => TestResultState::Inconclusive,
        };
        let conclusion = format!(
            "Flow '{}' ended as {}. Scope: {}.",
            flow.name,
            flow.state,
            if state == TestResultState::Passed {
                "reproduced with the recorded identity and tenant"
            } else {
                "not fully reproduced"
            }
        );
        let identity_id = flow.identity_id.or(run.identity_id);
        let result_id = self
            .db
            .insert_test_result(
                run.workspace_id,
                run.assessment_id,
                &ProcedureKey::new("flow.reproduction", "1.0.0"),
                "flow",
                &flow_id.to_string(),
                identity_id,
                state,
                &conclusion,
                &flow.evidence_ids,
            )
            .await?;
        Ok(Outcome {
            state: outcome.state,
            effect: outcome.effect,
            summary: conclusion,
            result: json!({
                "test_result_id": result_id,
                "flow_state": flow.state,
                "state": state.as_str(),
            }),
        })
    }

    /// Applies the typed operations available to the agent. There is no shell.
    pub async fn agent_operation(
        &self,
        run: &Run,
        operation_name: &str,
        args: &Value,
    ) -> ExecResult<Value> {
        match operation_name {
            "inventory.list" => {
                let endpoints = self
                    .db
                    .inventory(
                        run.workspace_id,
                        run.assessment_id,
                        &darkbox_storage::InventoryQuery {
                            limit: args.get("limit").and_then(|v| v.as_i64()).unwrap_or(50),
                            ..Default::default()
                        },
                    )
                    .await?;
                Ok(serde_json::to_value(endpoints).unwrap_or(Value::Null))
            }
            "procedure.eligibility" => {
                let inputs = self
                    .db
                    .eligibility_inputs(run.workspace_id, run.assessment_id)
                    .await?;
                Ok(serde_json::to_value(inputs).unwrap_or(Value::Null))
            }
            other => Err(ExecError::Invalid(format!(
                "unknown agent operation {other}"
            ))),
        }
    }
}

pub fn eligibility_blockers(input: &EligibilityInput) -> Vec<BlockerDto> {
    let mut blockers = Vec::new();
    for capability in &input.required_capabilities {
        if !input.available_capabilities.iter().any(|c| c == capability) {
            blockers.push(BlockerDto {
                code: "capability_unavailable".into(),
                detail: format!("required capability {capability} is not available"),
            });
        }
    }
    if input.target_count == 0 {
        blockers.push(BlockerDto {
            code: "no_eligible_targets".into(),
            detail: "no target of the required type is available in this assessment".into(),
        });
    }
    if input.requires_identity && !input.has_identity {
        blockers.push(BlockerDto {
            code: "identity_required".into(),
            detail: "the procedure needs a configured test identity".into(),
        });
    }
    blockers
}
