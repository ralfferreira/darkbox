use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! id_type {
    ($($name:ident),* $(,)?) => {
        $(
            #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
            #[serde(transparent)]
            pub struct $name(pub Uuid);

            impl $name {
                pub fn new() -> Self { Self(Uuid::now_v7()) }
                pub fn as_uuid(self) -> Uuid { self.0 }
            }

            impl Default for $name {
                fn default() -> Self { Self::new() }
            }

            impl std::fmt::Display for $name {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    self.0.fmt(f)
                }
            }

            impl std::str::FromStr for $name {
                type Err = uuid::Error;
                fn from_str(s: &str) -> Result<Self, Self::Err> { Ok(Self(Uuid::parse_str(s)?)) }
            }

            impl From<Uuid> for $name {
                fn from(value: Uuid) -> Self { Self(value) }
            }

            impl From<$name> for Uuid {
                fn from(value: $name) -> Self { value.0 }
            }
        )*
    };
}

id_type!(
    WorkspaceId,
    CompanyId,
    AssessmentId,
    ScopeVersionId,
    AssetId,
    EndpointId,
    IdentityId,
    TenantId,
    BrowserSessionId,
    ActionId,
    FlowId,
    ObservationId,
    EvidenceId,
    RunId,
    StepId,
    ResultId,
    FindingId,
    MemoryPageId,
    AuditEventId,
    EventId
);

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProcedureKey(pub String);

impl ProcedureKey {
    pub fn new(id: &str, version: &str) -> Self {
        Self(format!("{id}@{version}"))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ProcedureKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl std::str::FromStr for ProcedureKey {
    type Err = &'static str;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() {
            Err("empty procedure key")
        } else {
            Ok(Self(s.to_string()))
        }
    }
}
