//! Event envelope shared by SSE, the outbox and exports.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{AssessmentId, EventId, RunId, StepId, WorkspaceId};

pub const ENVELOPE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub event_id: EventId,
    pub schema_version: u32,
    pub workspace_id: WorkspaceId,
    pub assessment_id: AssessmentId,
    pub run_id: Option<RunId>,
    pub step_id: Option<StepId>,
    pub event_type: String,
    pub occurred_at: DateTime<Utc>,
    pub recorded_at: DateTime<Utc>,
    pub correlation_id: String,
    pub payload: serde_json::Value,
}

impl EventEnvelope {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        workspace_id: WorkspaceId,
        assessment_id: AssessmentId,
        run_id: Option<RunId>,
        step_id: Option<StepId>,
        event_type: impl Into<String>,
        correlation_id: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        let now = Utc::now();
        Self {
            event_id: EventId::new(),
            schema_version: ENVELOPE_SCHEMA_VERSION,
            workspace_id,
            assessment_id,
            run_id,
            step_id,
            event_type: event_type.into(),
            occurred_at: now,
            recorded_at: now,
            correlation_id: correlation_id.into(),
            payload,
        }
    }
}
