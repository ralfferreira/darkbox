//! Scope decisions. The executor calls this module before every external
//! operation. Nothing here inspects network state; the caller supplies the
//! resolved addresses it is about to use.

use std::net::IpAddr;

use chrono::{DateTime, Utc};
use darkbox_contracts::{
    ActionClass, AuxiliaryDestination, DenyCode, ScopeDecision, ScopePolicy, TimeWindow,
};

/// The role the current operation assigns to the destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetRole {
    /// The destination is the subject of a test.
    TestTarget,
    /// The destination is only needed for the flow to function (IdP, CDN).
    NavigationDependency,
}

#[derive(Debug, Clone)]
pub struct RequestContext<'a> {
    pub scheme: &'a str,
    pub host: &'a str,
    pub port: u16,
    pub path: &'a str,
    pub action_class: ActionClass,
    pub role: TargetRole,
    pub resolved_ips: Vec<IpAddr>,
    pub now: DateTime<Utc>,
}

impl<'a> RequestContext<'a> {
    pub fn new(scheme: &'a str, host: &'a str, port: u16, path: &'a str) -> Self {
        Self {
            scheme,
            host,
            port,
            path,
            action_class: ActionClass::Query,
            role: TargetRole::TestTarget,
            resolved_ips: Vec::new(),
            now: Utc::now(),
        }
    }

    fn target(&self) -> String {
        format!("{}://{}:{}{}", self.scheme, self.host, self.port, self.path)
    }
}

pub struct PolicyEngine<'a> {
    policy: &'a ScopePolicy,
    version: i64,
}

impl<'a> PolicyEngine<'a> {
    pub fn new(policy: &'a ScopePolicy, version: i64) -> Self {
        Self { policy, version }
    }

    pub fn policy(&self) -> &ScopePolicy {
        self.policy
    }

    pub fn version(&self) -> i64 {
        self.version
    }

    fn deny(&self, ctx: &RequestContext, code: DenyCode, rule: impl Into<String>) -> ScopeDecision {
        ScopeDecision {
            allowed: false,
            code: Some(code),
            rule: rule.into(),
            policy_version: self.version,
            target: ctx.target(),
            action_class: ctx.action_class,
        }
    }

    fn allow(&self, ctx: &RequestContext, rule: impl Into<String>) -> ScopeDecision {
        ScopeDecision {
            allowed: true,
            code: None,
            rule: rule.into(),
            policy_version: self.version,
            target: ctx.target(),
            action_class: ctx.action_class,
        }
    }

    pub fn evaluate(&self, ctx: &RequestContext) -> ScopeDecision {
        let scheme = ctx.scheme.to_ascii_lowercase();
        if !self
            .policy
            .allowed_schemes
            .iter()
            .any(|s| s.eq_ignore_ascii_case(&scheme))
        {
            return self.deny(ctx, DenyCode::UnsupportedScheme, format!("scheme:{scheme}"));
        }
        if !self.policy.allowed_ports.contains(&ctx.port) {
            return self.deny(ctx, DenyCode::PortNotAllowed, format!("port:{}", ctx.port));
        }

        let host = normalize_host(ctx.host);
        if let Some(pattern) = self
            .policy
            .exclude_hosts
            .iter()
            .find(|pattern| host_rule_matches(pattern, &host))
        {
            return self.deny(ctx, DenyCode::Excluded, format!("exclude_host:{pattern}"));
        }
        if let Some(rule) = self
            .policy
            .exclude_paths
            .iter()
            .find(|rule| path_rule_matches(rule, ctx.path))
        {
            return self.deny(ctx, DenyCode::Excluded, format!("exclude_path:{rule}"));
        }

        if !self.policy.action_classes.contains(&ctx.action_class) {
            return self.deny(
                ctx,
                DenyCode::ActionClassDenied,
                format!("action_class:{}", ctx.action_class),
            );
        }

        if let Some(window) = &self.policy.window
            && !in_window(window, ctx.now)
        {
            return self.deny(ctx, DenyCode::OutsideWindow, "window");
        }

        let is_navigation_dependency = self
            .policy
            .navigation_dependencies
            .iter()
            .any(|destination| auxiliary_matches(destination, &scheme, &host, ctx.port));
        let is_explicit_target = self
            .policy
            .test_targets
            .iter()
            .any(|destination| auxiliary_matches(destination, &scheme, &host, ctx.port));

        if ctx.role == TargetRole::TestTarget && is_navigation_dependency && !is_explicit_target {
            return self.deny(
                ctx,
                DenyCode::AuxiliaryTargetOnly,
                format!("navigation_dependency:{host}"),
            );
        }

        let included_host = self
            .policy
            .include_hosts
            .iter()
            .find(|pattern| host_rule_matches(pattern, &host));
        let included_auxiliary = is_navigation_dependency || is_explicit_target;
        if included_host.is_none() && !included_auxiliary {
            return self.deny(
                ctx,
                DenyCode::HostNotIncluded,
                format!("include_host:{host}"),
            );
        }

        let included_path = self
            .policy
            .include_paths
            .iter()
            .find(|rule| path_rule_matches(rule, ctx.path));
        if included_path.is_none() {
            return self.deny(
                ctx,
                DenyCode::PathNotAllowed,
                format!("include_path:{}", ctx.path),
            );
        }

        for ip in &ctx.resolved_ips {
            if is_private(ip) && !self.policy.allow_private_network {
                return self.deny(
                    ctx,
                    DenyCode::PrivateNetworkDenied,
                    format!("resolved_private:{ip}"),
                );
            }
        }
        if let Ok(ip) = host.parse::<IpAddr>()
            && is_private(&ip)
            && !self.policy.allow_private_network
        {
            return self.deny(
                ctx,
                DenyCode::PrivateNetworkDenied,
                format!("private_literal:{ip}"),
            );
        }
        if !ctx.resolved_ips.is_empty() {
            for ip in &ctx.resolved_ips {
                if ip.is_unspecified() || ip.is_multicast() {
                    return self.deny(ctx, DenyCode::OutOfScope, format!("bad_destination:{ip}"));
                }
            }
        }

        let rule = included_host
            .map(|pattern| format!("include_host:{pattern}"))
            .filter(|_| !included_auxiliary)
            .or_else(|| {
                self.policy
                    .test_targets
                    .iter()
                    .find(|destination| auxiliary_matches(destination, &scheme, &host, ctx.port))
                    .map(|_| format!("test_target:{host}"))
            })
            .or_else(|| {
                self.policy
                    .navigation_dependencies
                    .iter()
                    .find(|destination| auxiliary_matches(destination, &scheme, &host, ctx.port))
                    .map(|_| format!("navigation_dependency:{host}"))
            })
            .unwrap_or_else(|| "include_host".to_string());

        self.allow(ctx, rule)
    }

    /// A pinned run authorization is the intersection of the policy fixed when
    /// the run started and the restrictions in effect now. Both must allow it.
    pub fn evaluate_against<'b>(
        pinned: &'b PolicyEngine<'b>,
        current: &PolicyEngine<'_>,
        ctx: &RequestContext,
    ) -> ScopeDecision {
        let first = pinned.evaluate(ctx);
        if !first.allowed {
            return first;
        }
        current.evaluate(ctx)
    }
}

pub fn normalize_host(host: &str) -> String {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    match host.rsplit_once(':') {
        Some((name, port)) if port.chars().all(|c| c.is_ascii_digit()) => name.to_string(),
        _ => host,
    }
}

/// `*.example.com` matches one or more subdomain labels, never the apex.
/// An exact rule matches only that host. A rule may carry `:port`, which is
/// still checked against the effective port by the engine.
pub fn host_rule_matches(rule: &str, host: &str) -> bool {
    let rule = rule.trim().trim_end_matches('.').to_ascii_lowercase();
    let host = normalize_host(host);
    let rule_host = normalize_host(&rule);
    if let Some(suffix) = rule_host.strip_prefix("*.") {
        host.len() > suffix.len() + 1 && host.ends_with(&format!(".{suffix}"))
    } else {
        rule_host == host
    }
}

pub fn path_rule_matches(rule: &str, path: &str) -> bool {
    let rule = if let Some(prefix) = rule.strip_suffix('*') {
        prefix
    } else {
        rule
    };
    if rule == "/" {
        return path.starts_with('/');
    }
    let normalized_rule = rule.trim_end_matches('/');
    let normalized_path = path.trim_end_matches('/');
    normalized_path == normalized_rule
        || normalized_path.starts_with(&format!("{normalized_rule}/"))
}

fn auxiliary_matches(
    destination: &AuxiliaryDestination,
    scheme: &str,
    host: &str,
    port: u16,
) -> bool {
    host_rule_matches(&destination.host, host)
        && (destination.ports.is_empty() || destination.ports.contains(&port))
        && (destination.schemes.is_empty()
            || destination
                .schemes
                .iter()
                .any(|s| s.eq_ignore_ascii_case(scheme)))
}

fn in_window(window: &TimeWindow, now: DateTime<Utc>) -> bool {
    now >= window.start && now <= window.end
}

pub fn is_private(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo() -> ScopePolicy {
        let mut policy = ScopePolicy::demo(8080);
        policy.include_hosts = vec!["127.0.0.1".into(), "*.lab.test".into()];
        policy.exclude_hosts = vec!["excluded.lab.test".into(), "internal.lab.test".into()];
        policy
    }

    fn ctx(host: &'static str, path: &'static str) -> RequestContext<'static> {
        RequestContext::new("http", host, 8080, path)
    }

    #[test]
    fn exclusions_prevail() {
        let policy = demo();
        let engine = PolicyEngine::new(&policy, 3);
        let decision = engine.evaluate(&ctx("excluded.lab.test", "/"));
        assert!(!decision.allowed);
        assert_eq!(decision.code, Some(DenyCode::Excluded));
    }

    #[test]
    fn path_exclusion_prevails_over_inclusion() {
        let mut policy = demo();
        policy.exclude_paths = vec!["/admin/forbidden".into()];
        let engine = PolicyEngine::new(&policy, 1);
        assert!(
            engine
                .evaluate(&ctx("app.lab.test", "/admin/forbidden/x"))
                .code
                == Some(DenyCode::Excluded)
        );
        assert!(
            engine
                .evaluate(&ctx("app.lab.test", "/admin/other"))
                .allowed
        );
    }

    #[test]
    fn wildcard_covers_subdomains_not_apex() {
        let policy = demo();
        let engine = PolicyEngine::new(&policy, 1);
        assert!(engine.evaluate(&ctx("api.lab.test", "/")).allowed);
        assert!(engine.evaluate(&ctx("deep.api.lab.test", "/")).allowed);
        assert!(!engine.evaluate(&ctx("lab.test", "/")).allowed);
    }

    #[test]
    fn port_and_scheme_are_checked() {
        let policy = demo();
        let engine = PolicyEngine::new(&policy, 1);
        let mut other = ctx("127.0.0.1", "/");
        other.port = 9099;
        assert_eq!(engine.evaluate(&other).code, Some(DenyCode::PortNotAllowed));
        other.port = 8080;
        other.scheme = "https";
        assert_eq!(
            engine.evaluate(&other).code,
            Some(DenyCode::UnsupportedScheme)
        );
    }

    #[test]
    fn private_network_needs_permission() {
        let mut policy = demo();
        policy.allow_private_network = false;
        let engine = PolicyEngine::new(&policy, 1);
        let decision = engine.evaluate(&ctx("127.0.0.1", "/"));
        assert_eq!(decision.code, Some(DenyCode::PrivateNetworkDenied));
        let mut resolved_ctx = ctx("app.lab.test", "/");
        resolved_ctx.resolved_ips = vec!["10.0.0.7".parse().unwrap()];
        assert_eq!(
            engine.evaluate(&resolved_ctx).code,
            Some(DenyCode::PrivateNetworkDenied)
        );
    }

    #[test]
    fn navigation_dependency_is_not_a_test_target() {
        let mut policy = demo();
        policy.navigation_dependencies = vec![AuxiliaryDestination {
            host: "idp.lab.test".into(),
            ports: vec![8080],
            schemes: vec!["http".into()],
        }];
        let engine = PolicyEngine::new(&policy, 1);
        let mut navigation = ctx("idp.lab.test", "/authorize");
        navigation.role = TargetRole::NavigationDependency;
        assert!(engine.evaluate(&navigation).allowed);
        let mut test = navigation.clone();
        test.role = TargetRole::TestTarget;
        assert_eq!(
            engine.evaluate(&test).code,
            Some(DenyCode::AuxiliaryTargetOnly)
        );
    }

    #[test]
    fn window_closes_access() {
        let mut policy = demo();
        policy.window = Some(TimeWindow {
            start: Utc::now() + chrono::Duration::hours(1),
            end: Utc::now() + chrono::Duration::hours(2),
        });
        let engine = PolicyEngine::new(&policy, 1);
        assert_eq!(
            engine.evaluate(&ctx("127.0.0.1", "/")).code,
            Some(DenyCode::OutsideWindow)
        );
    }

    #[test]
    fn decision_reports_the_rule_and_version() {
        let policy = demo();
        let engine = PolicyEngine::new(&policy, 7);
        let decision = engine.evaluate(&ctx("127.0.0.1", "/"));
        assert!(decision.allowed);
        assert_eq!(decision.policy_version, 7);
        assert_eq!(decision.rule, "include_host:127.0.0.1");
    }
}
