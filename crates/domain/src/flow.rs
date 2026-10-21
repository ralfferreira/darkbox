use darkbox_contracts::{ActionClass, FlowState, IdentityId};

/// Whether a flow can be attempted for the given identity. Missing session
/// state is a block, never a negative result.
pub fn reproduction_blockers(
    state: FlowState,
    identity: Option<IdentityId>,
    requires_identity: bool,
    authenticated: bool,
    has_unknown_effect: bool,
) -> Vec<&'static str> {
    let mut blockers = Vec::new();
    if requires_identity && identity.is_none() {
        blockers.push("identity_required");
    }
    if identity.is_some() && !authenticated {
        blockers.push("session_expired");
    }
    if has_unknown_effect {
        blockers.push("external_result_unknown");
    }
    if matches!(state, FlowState::Stale) {
        blockers.push("flow_stale");
    }
    blockers
}

pub fn effect_class_from(action_class: ActionClass) -> &'static str {
    match action_class {
        ActionClass::Query => "query",
        ActionClass::Session => "session",
        ActionClass::ReversibleChange => "reversible_change",
        ActionClass::ExternalEffect => "external_effect",
        ActionClass::Destructive => "destructive",
    }
}
