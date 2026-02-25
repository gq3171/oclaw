//! ACP session management.

use std::collections::HashMap;
use std::path::PathBuf;

use crate::types::AcpMessage;

#[derive(Debug, Clone)]
pub struct AcpSession {
    pub session_id: String,
    pub session_key: String,
    pub cwd: PathBuf,
    pub created_at: u64,
    pub last_touched_at: u64,
    pub active_run_id: Option<String>,
    pub messages: Vec<AcpMessage>,
}

pub struct AcpSessionStore {
    sessions: HashMap<String, AcpSession>,
    max_sessions: usize,
}

impl AcpSessionStore {
    pub fn new(max_sessions: usize) -> Self {
        Self {
            sessions: HashMap::new(),
            max_sessions,
        }
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    pub fn create(&mut self, cwd: PathBuf) -> Result<&AcpSession, AcpSessionError> {
        if self.sessions.len() >= self.max_sessions {
            return Err(AcpSessionError::TooManySessions(self.max_sessions));
        }

        let now = Self::now_ms();
        let id = uuid::Uuid::new_v4().to_string();
        let key = format!("acp:{}", &id[..8]);

        let session = AcpSession {
            session_id: id.clone(),
            session_key: key,
            cwd,
            created_at: now,
            last_touched_at: now,
            active_run_id: None,
            messages: Vec::new(),
        };

        self.sessions.insert(id.clone(), session);
        Ok(self.sessions.get(&id).unwrap())
    }

    pub fn get(&self, id: &str) -> Option<&AcpSession> {
        self.sessions.get(id)
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut AcpSession> {
        self.sessions.get_mut(id)
    }

    pub fn touch(&mut self, id: &str) {
        if let Some(s) = self.sessions.get_mut(id) {
            s.last_touched_at = Self::now_ms();
        }
    }

    pub fn cancel_active(&mut self, id: &str) -> bool {
        if let Some(s) = self.sessions.get_mut(id)
            && s.active_run_id.is_some()
        {
            s.active_run_id = None;
            return true;
        }
        false
    }

    pub fn remove(&mut self, id: &str) -> bool {
        self.sessions.remove(id).is_some()
    }

    pub fn list(&self) -> Vec<&AcpSession> {
        self.sessions.values().collect()
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sessions.is_empty()
    }
}

impl Default for AcpSessionStore {
    fn default() -> Self {
        Self::new(64)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AcpSessionError {
    #[error("too many sessions (max {0})")]
    TooManySessions(usize),
    #[error("session not found: {0}")]
    NotFound(String),
}
