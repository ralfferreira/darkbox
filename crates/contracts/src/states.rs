use serde::{Deserialize, Serialize};

macro_rules! string_state {
    ($name:ident { $($variant:ident => $text:literal),* $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum $name {
            $($variant),*
        }

        impl $name {
            pub fn as_str(self) -> &'static str {
                match self {
                    $($name::$variant => $text),*
                }
            }
            pub const ALL: &'static [Self] = &[$($name::$variant),*];
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl std::str::FromStr for $name {
            type Err = String;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($text => Ok($name::$variant),)*
                    other => Err(format!("unknown {} value: {other}", stringify!($name))),
                }
            }
        }
    };
}

string_state!(EndpointState {
    Candidate => "candidate",
    Observed => "observed",
    Unreachable => "unreachable",
    Excluded => "excluded",
});

string_state!(FlowState {
    Proposed => "proposed",
    Observed => "observed",
    Reproduced => "reproduced",
    Stale => "stale",
    Blocked => "blocked",
});

string_state!(TestResultState {
    NotApplicable => "not_applicable",
    Passed => "passed",
    Failed => "failed",
    Inconclusive => "inconclusive",
    Error => "error",
    Blocked => "blocked",
});

string_state!(FindingState {
    Hypothesis => "hypothesis",
    NeedsReview => "needs_review",
    Confirmed => "confirmed",
    Rejected => "rejected",
    Remediated => "remediated",
    Reopened => "reopened",
});

string_state!(ActionClass {
    Query => "query",
    Session => "session",
    ReversibleChange => "reversible_change",
    ExternalEffect => "external_effect",
    Destructive => "destructive",
});

impl ActionClass {
    /// Classes enabled by default in the initial release.
    pub fn default_enabled(self) -> bool {
        matches!(self, ActionClass::Query | ActionClass::Session)
    }
}

string_state!(AssetKind {
    Origin => "origin",
    Host => "host",
    Domain => "domain",
});

string_state!(EvidenceKind {
    HttpExchange => "http_exchange",
    HttpResponseBody => "http_response_body",
    BrowserScreenshot => "browser_screenshot",
    BrowserTrace => "browser_trace",
    ToolOutput => "tool_output",
    ExportManifest => "export_manifest",
});

string_state!(EvidenceClassification {
    Sanitized => "sanitized",
    RawRestricted => "raw_restricted",
    MetadataOnly => "metadata_only",
});

string_state!(RunKind {
    Discovery => "discovery",
    Observation => "observation",
    Session => "session",
    FlowReproduction => "flow_reproduction",
    Procedure => "procedure",
    Reconciliation => "reconciliation",
    NetworkScan => "network_scan",
    FirecrawlMap => "firecrawl_map",
    FirecrawlScrape => "firecrawl_scrape",
    BurpImport => "burp_import",
    Agent => "agent",
});

string_state!(StepKind {
    HttpObserve => "http_observe",
    HttpProbe => "http_probe",
    DiscoveryImport => "discovery_import",
    DiscoveryCrawler => "discovery_crawler",
    BrowserSession => "browser_session",
    BrowserFlow => "browser_flow",
    ProcedureStep => "procedure_step",
    Reconcile => "reconcile",
    NetworkScan => "network_scan",
    FirecrawlMap => "firecrawl_map",
    FirecrawlScrape => "firecrawl_scrape",
    BurpImport => "burp_import",
    AgentTurn => "agent_turn",
});

string_state!(EffectStatus {
    NotDispatched => "not_dispatched",
    Dispatched => "dispatched",
    Confirmed => "confirmed",
    OutcomeUnknown => "outcome_unknown",
    NotApplicable => "not_applicable",
});

string_state!(RunState {
    Queued => "queued",
    Running => "running",
    WaitingOperator => "waiting_operator",
    Succeeded => "succeeded",
    Failed => "failed",
    Interrupted => "interrupted",
    Cancelling => "cancelling",
    Cancelled => "cancelled",
});

impl RunState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            RunState::Succeeded | RunState::Failed | RunState::Cancelled
        )
    }

    pub fn can_transition(self, next: RunState) -> bool {
        use RunState::*;
        matches!(
            (self, next),
            (Queued, Running)
                | (Queued, Cancelling)
                | (Running, WaitingOperator)
                | (Running, Succeeded)
                | (Running, Failed)
                | (Running, Interrupted)
                | (Running, Cancelling)
                | (WaitingOperator, Queued)
                | (WaitingOperator, Cancelling)
                | (Interrupted, Cancelling)
                | (Interrupted, Queued)
                | (Cancelling, Cancelled)
        )
    }

    pub fn transition(self, next: RunState) -> Result<RunState, TransitionError> {
        if self.can_transition(next) {
            Ok(next)
        } else {
            Err(TransitionError {
                from: self,
                to: next,
            })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransitionError {
    pub from: RunState,
    pub to: RunState,
}

impl std::fmt::Display for TransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "illegal run transition {} -> {}", self.from, self.to)
    }
}

impl std::error::Error for TransitionError {}

// Effect outcome is tracked separately from the run state. A run never
// succeeds while a required step still carries `outcome_unknown`.
string_state!(DenyCode {
    OutOfScope => "out_of_scope",
    Excluded => "excluded",
    ActionClassDenied => "action_class_denied",
    OutsideWindow => "outside_window",
    PrivateNetworkDenied => "private_network_denied",
    AuxiliaryTargetOnly => "auxiliary_target_only",
    CapabilityUnavailable => "capability_unavailable",
    IdentityExpired => "identity_expired",
    BudgetExhausted => "budget_exhausted",
    CaptureFailed => "capture_failed",
    ExternalResultUnknown => "external_result_unknown",
    ToolError => "tool_error",
    Cancelled => "cancelled",
    UnsupportedScheme => "unsupported_scheme",
    PortNotAllowed => "port_not_allowed",
    PathNotAllowed => "path_not_allowed",
    HostNotIncluded => "host_not_included",
});

string_state!(ProcedureTarget {
    Endpoint => "endpoint",
    Flow => "flow",
    Identity => "identity",
    Assessment => "assessment",
});

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    pub max_duration_seconds: i64,
    pub max_requests: i64,
    pub max_browser_steps: i64,
    pub max_model_calls: i64,
    pub max_cost_micros: i64,
    pub max_artifact_bytes: i64,
    pub max_concurrency_per_origin: i64,
}

impl Budget {
    pub fn demo() -> Self {
        Self {
            max_duration_seconds: 900,
            max_requests: 400,
            max_browser_steps: 200,
            max_model_calls: 40,
            max_cost_micros: 2_000_000,
            max_artifact_bytes: 64 * 1024 * 1024,
            max_concurrency_per_origin: 2,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetSpent {
    pub requests: i64,
    pub browser_steps: i64,
    pub model_calls: i64,
    pub cost_micros: i64,
    /// Cost reported above the run budget. It is persisted, not rolled back.
    #[serde(default)]
    pub overage_micros: i64,
    pub artifact_bytes: i64,
}

impl BudgetSpent {
    pub fn fits(&self, budget: &Budget) -> Result<(), DenyCode> {
        if self.requests > budget.max_requests
            || self.browser_steps > budget.max_browser_steps
            || self.model_calls > budget.max_model_calls
            || self.cost_micros > budget.max_cost_micros
            || self.artifact_bytes > budget.max_artifact_bytes
        {
            Err(DenyCode::BudgetExhausted)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityState {
    Available,
    Unavailable,
    Degraded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub id: String,
    pub provider: String,
    pub version: String,
    pub state: CapabilityState,
    pub detail: String,
}
