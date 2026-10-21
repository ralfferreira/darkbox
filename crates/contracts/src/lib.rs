//! Shared identifiers, states, event envelopes and HTTP DTOs.
//!
//! This crate is the stable boundary between the Rust core and every consumer.
//! It contains no storage, network or framework types.

use serde::{Deserialize, Serialize};

pub mod api;
pub mod events;
pub mod ids;
pub mod policy;
pub mod states;

pub use api::*;
pub use events::*;
pub use ids::*;
pub use policy::*;
pub use states::*;

/// Monotonic schema version for JSON envelopes written outside the database.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Admin,
    Operator,
    Reviewer,
    Reader,
}

impl Role {
    pub fn can_write(self) -> bool {
        matches!(self, Role::Admin | Role::Operator)
    }
    pub fn can_review(self) -> bool {
        matches!(self, Role::Admin | Role::Reviewer)
    }
    pub fn is_admin(self) -> bool {
        matches!(self, Role::Admin)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Operator => "operator",
            Role::Reviewer => "reviewer",
            Role::Reader => "reader",
        }
    }
}

impl std::str::FromStr for Role {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "admin" => Ok(Role::Admin),
            "operator" => Ok(Role::Operator),
            "reviewer" => Ok(Role::Reviewer),
            "reader" => Ok(Role::Reader),
            other => Err(format!("unknown role: {other}")),
        }
    }
}
