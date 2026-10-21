use axum::http::{HeaderMap, header};
use chrono::Utc;

use crate::state::{LabState, SESSION_COOKIE, SESSION_TTL_MINUTES, User};

#[derive(Debug)]
pub struct AuthContext {
    pub session_id: String,
    pub user: User,
}

#[derive(Debug)]
pub enum AuthOutcome {
    Ok(AuthContext),
    MissingSession,
    ExpiredSession,
    UnknownUser,
    WrongTenant,
}

pub fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    for header in headers.get_all(header::COOKIE) {
        let Ok(text) = header.to_str() else {
            continue;
        };
        for pair in text.split(';') {
            let Some((key, value)) = pair.trim().split_once('=') else {
                continue;
            };
            if key.trim() == name {
                return Some(value.trim().to_string());
            }
        }
    }
    None
}

pub fn session_cookie(session_id: &str) -> String {
    format!(
        "{SESSION_COOKIE}={session_id}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}",
        SESSION_TTL_MINUTES * 60
    )
}

pub fn clear_cookie() -> String {
    format!("{SESSION_COOKIE}=; HttpOnly; SameSite=Lax; Path=/; Max-Age=0")
}

pub fn authorize(state: &mut LabState, headers: &HeaderMap, tenant: &str) -> AuthOutcome {
    let Some(session_id) = cookie_value(headers, SESSION_COOKIE) else {
        return AuthOutcome::MissingSession;
    };
    let Some(session) = state.sessions.get(&session_id).cloned() else {
        return AuthOutcome::MissingSession;
    };
    if session.expires_at <= Utc::now() {
        state.sessions.remove(&session_id);
        return AuthOutcome::ExpiredSession;
    }
    let Some(user) = state.user(&session.user).cloned() else {
        return AuthOutcome::UnknownUser;
    };
    if !user.tenants.contains(&tenant) {
        return AuthOutcome::WrongTenant;
    }
    AuthOutcome::Ok(AuthContext { session_id, user })
}
