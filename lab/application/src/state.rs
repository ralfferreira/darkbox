use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, MutexGuard};

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;

pub const SESSION_TTL_MINUTES: i64 = 30;
pub const SESSION_COOKIE: &str = "darkbox_sid";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Admin,
    Member,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::Member => "member",
        }
    }

    pub fn is_admin(self) -> bool {
        matches!(self, Role::Admin)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tenant {
    North,
    South,
}

impl Tenant {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "north" => Some(Tenant::North),
            "south" => Some(Tenant::South),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Tenant::North => "north",
            Tenant::South => "south",
        }
    }
}

#[derive(Clone, Debug)]
pub struct User {
    pub username: &'static str,
    pub password: &'static str,
    pub role: Role,
    pub tenants: Vec<&'static str>,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub user: String,
    // Kept on the record for verifier inspection; authorization only needs expiry and membership.
    #[allow(dead_code)]
    pub tenant: Option<String>,
    #[allow(dead_code)]
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TamperEntry {
    pub seq: u64,
    pub kind: String,
    pub path: String,
}

pub struct LabState {
    pub users: Vec<User>,
    pub sessions: HashMap<String, Session>,
    pub hits: BTreeMap<String, u64>,
    pub total_hits: u64,
    pub effects: BTreeMap<String, u64>,
    pub excluded_hits: u64,
    pub tamper_log: Vec<TamperEntry>,
    next_session: u64,
    next_tamper: u64,
}

impl LabState {
    fn seeded() -> Self {
        let users = vec![
            User {
                username: "alice",
                password: "alice-lab-pass",
                role: Role::Admin,
                tenants: vec!["north", "south"],
            },
            User {
                username: "bob",
                password: "bob-lab-pass",
                role: Role::Member,
                tenants: vec!["north"],
            },
        ];
        let mut effects = BTreeMap::new();
        effects.insert("north".to_string(), 0);
        effects.insert("south".to_string(), 0);
        Self {
            users,
            sessions: HashMap::new(),
            hits: BTreeMap::new(),
            total_hits: 0,
            effects,
            excluded_hits: 0,
            tamper_log: Vec::new(),
            next_session: 0,
            next_tamper: 0,
        }
    }

    pub fn user(&self, username: &str) -> Option<&User> {
        self.users.iter().find(|user| user.username == username)
    }

    pub fn create_session(&mut self, user: &User) -> String {
        self.next_session += 1;
        let id = format!("lab-sid-{:06}", self.next_session);
        let now = Utc::now();
        self.sessions.insert(
            id.clone(),
            Session {
                user: user.username.to_string(),
                tenant: user.tenants.first().map(|tenant| tenant.to_string()),
                created_at: now,
                expires_at: now + Duration::minutes(SESSION_TTL_MINUTES),
            },
        );
        id
    }

    pub fn record_hit(&mut self, path: &str) {
        *self.hits.entry(path.to_string()).or_insert(0) += 1;
        self.total_hits += 1;
    }

    pub fn record_tamper(&mut self, kind: &str, path: &str) {
        self.next_tamper += 1;
        self.tamper_log.push(TamperEntry {
            seq: self.next_tamper,
            kind: kind.to_string(),
            path: path.to_string(),
        });
    }

    pub fn reset(&mut self) {
        self.hits.clear();
        self.total_hits = 0;
        for value in self.effects.values_mut() {
            *value = 0;
        }
        self.sessions.clear();
        self.excluded_hits = 0;
        self.tamper_log.clear();
    }

    pub fn state_json(&self) -> serde_json::Value {
        serde_json::json!({
            "version": "lab-1",
            "hits": &self.hits,
            "total_hits": self.total_hits,
            "effects": &self.effects,
            "session_count": self.sessions.len(),
            "excluded_hits": self.excluded_hits,
            "tamper_log": &self.tamper_log,
        })
    }
}

#[derive(Clone)]
pub struct App {
    pub state: Arc<Mutex<LabState>>,
    pub lab_port: u16,
    pub excluded_port: u16,
}

impl App {
    pub fn new(lab_port: u16, excluded_port: u16) -> Self {
        Self {
            state: Arc::new(Mutex::new(LabState::seeded())),
            lab_port,
            excluded_port,
        }
    }

    pub fn lock(&self) -> MutexGuard<'_, LabState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn record_hit(&self, path: &str) {
        self.lock().record_hit(path);
    }

    pub fn record_tamper(&self, kind: &str, path: &str) {
        self.lock().record_tamper(kind, path);
    }

    pub fn state_json(&self) -> serde_json::Value {
        self.lock().state_json()
    }
}
