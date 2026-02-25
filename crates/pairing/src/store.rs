//! Pairing request storage.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

use crate::setup_code::generate_setup_code;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingRequest {
    pub id: String,
    pub code: String,
    pub created_at: u64,
    pub last_seen_at: u64,
    pub meta: HashMap<String, String>,
    pub approved: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum PairingError {
    #[error("too many pending requests (max {0})")]
    TooManyPending(usize),
    #[error("request not found: {0}")]
    NotFound(String),
    #[error("request expired: {0}")]
    Expired(String),
}

pub struct PairingStore {
    requests: Vec<PairingRequest>,
    max_pending: usize,
    ttl_ms: u64,
}

impl PairingStore {
    pub fn new(max_pending: usize, ttl_ms: u64) -> Self {
        Self {
            requests: Vec::new(),
            max_pending,
            ttl_ms,
        }
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    pub fn create_request(&mut self) -> Result<PairingRequest, PairingError> {
        self.cleanup_expired();
        let pending = self.requests.iter().filter(|r| !r.approved).count();
        if pending >= self.max_pending {
            return Err(PairingError::TooManyPending(self.max_pending));
        }

        let now = Self::now_ms();
        let req = PairingRequest {
            id: uuid::Uuid::new_v4().to_string(),
            code: generate_setup_code(),
            created_at: now,
            last_seen_at: now,
            meta: HashMap::new(),
            approved: false,
        };
        self.requests.push(req.clone());
        Ok(req)
    }

    pub fn verify_code(&self, code: &str) -> Option<&PairingRequest> {
        let now = Self::now_ms();
        self.requests.iter().find(|r| {
            r.code == code && !r.approved && (now - r.created_at) < self.ttl_ms
        })
    }

    pub fn approve(&mut self, id: &str) -> Result<(), PairingError> {
        let req = self.requests.iter_mut().find(|r| r.id == id)
            .ok_or_else(|| PairingError::NotFound(id.to_string()))?;
        req.approved = true;
        req.last_seen_at = Self::now_ms();
        Ok(())
    }

    pub fn reject(&mut self, id: &str) -> Result<(), PairingError> {
        let idx = self.requests.iter().position(|r| r.id == id)
            .ok_or_else(|| PairingError::NotFound(id.to_string()))?;
        self.requests.remove(idx);
        Ok(())
    }

    pub fn cleanup_expired(&mut self) {
        let now = Self::now_ms();
        self.requests.retain(|r| r.approved || (now - r.created_at) < self.ttl_ms);
    }

    pub fn list_pending(&self) -> Vec<&PairingRequest> {
        self.requests.iter().filter(|r| !r.approved).collect()
    }

    pub fn list_approved(&self) -> Vec<&PairingRequest> {
        self.requests.iter().filter(|r| r.approved).collect()
    }
}

impl Default for PairingStore {
    fn default() -> Self {
        Self::new(3, 3_600_000) // max 3 pending, 1 hour TTL
    }
}
