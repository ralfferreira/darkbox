use serde::{Deserialize, Serialize};

use crate::states::{ActionClass, Budget, DenyCode};
/// Structured scope policy. Immutable once stored as a scope version.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScopePolicy {
    pub include_hosts: Vec<String>,
    pub exclude_hosts: Vec<String>,
    pub include_paths: Vec<String>,
    pub exclude_paths: Vec<String>,
    pub allowed_schemes: Vec<String>,
    pub allowed_ports: Vec<u16>,
    pub allow_private_network: bool,
    pub window: Option<TimeWindow>,
    pub action_classes: Vec<ActionClass>,
    pub navigation_dependencies: Vec<AuxiliaryDestination>,
    pub test_targets: Vec<AuxiliaryDestination>,
    pub limits: Budget,
    pub retain_raw: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimeWindow {
    pub start: chrono::DateTime<chrono::Utc>,
    pub end: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AuxiliaryDestination {
    pub host: String,
    pub ports: Vec<u16>,
    pub schemes: Vec<String>,
}

impl ScopePolicy {
    pub fn demo(lab_port: u16) -> Self {
        Self {
            include_hosts: vec!["127.0.0.1".into(), "localhost".into()],
            exclude_hosts: vec!["excluded.darkbox.test".into()],
            include_paths: vec!["/".into()],
            exclude_paths: vec!["/admin/forbidden".into()],
            allowed_schemes: vec!["http".into()],
            allowed_ports: vec![lab_port],
            allow_private_network: true,
            window: None,
            action_classes: vec![ActionClass::Query, ActionClass::Session],
            navigation_dependencies: vec![],
            test_targets: vec![],
            limits: Budget::demo(),
            retain_raw: false,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.include_hosts.is_empty() {
            return Err("include_hosts must not be empty".into());
        }
        if self.allowed_schemes.is_empty() {
            return Err("allowed_schemes must not be empty".into());
        }
        if self.allowed_ports.is_empty() {
            return Err("allowed_ports must not be empty".into());
        }
        for host in self.include_hosts.iter().chain(self.exclude_hosts.iter()) {
            if host.trim().is_empty() || host.contains('/') || host.contains(' ') {
                return Err(format!("invalid host rule: {host:?}"));
            }
        }
        for path in self.include_paths.iter() {
            if !path.starts_with('/') {
                return Err(format!("include path must start with /: {path:?}"));
            }
        }
        if let Some(window) = &self.window
            && window.end <= window.start
        {
            return Err("scope window end must be after start".into());
        }
        if self.limits.max_requests < 1 {
            return Err("max_requests must be positive".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ScopeDecision {
    pub allowed: bool,
    pub code: Option<DenyCode>,
    pub rule: String,
    pub policy_version: i64,
    pub target: String,
    pub action_class: ActionClass,
}
