use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingRequest {
    pub id: String,
    pub from_user_id: String,
    pub from_display_name: String,
    pub to_user_id: Option<String>,
    pub to_identifier: Option<String>,
    pub code: String,
    pub status: PairingStatus,
    pub created_at: i64,
    pub expires_at: i64,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PairingStatus {
    Pending,
    Approved,
    Rejected,
    Expired,
    Revoked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DMPairing {
    pub id: String,
    pub user_a: String,
    pub user_b: String,
    pub created_at: i64,
    pub last_message_at: i64,
    pub metadata: HashMap<String, String>,
}

pub struct PairingManager {
    requests: Arc<RwLock<HashMap<String, PairingRequest>>>,
    pairings: Arc<RwLock<HashMap<String, DMPairing>>>,
    code_store: Arc<RwLock<HashMap<String, String>>>,
    timeout_secs: i64,
    code_length: usize,
}

impl PairingManager {
    pub fn new(timeout_secs: i64) -> Self {
        Self {
            requests: Arc::new(RwLock::new(HashMap::new())),
            pairings: Arc::new(RwLock::new(HashMap::new())),
            code_store: Arc::new(RwLock::new(HashMap::new())),
            timeout_secs,
            code_length: 8,
        }
    }

    pub fn with_code_length(mut self, length: usize) -> Self {
        self.code_length = length;
        self
    }

    fn generate_code(&self) -> String {
        use rand::Rng;
        let mut rng = rand::rng();
        let chars: Vec<char> = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789".chars().collect();
        (0..self.code_length)
            .map(|_| chars[rng.random_range(0..chars.len())])
            .collect()
    }

    pub async fn create_request(
        &self,
        from_user_id: &str,
        from_display_name: &str,
        to_identifier: Option<&str>,
    ) -> Result<PairingRequest, PairingError> {
        let code = self.generate_code();
        let now = chrono::Utc::now().timestamp();
        let id = uuid::Uuid::new_v4().to_string();

        let request = PairingRequest {
            id: id.clone(),
            from_user_id: from_user_id.to_string(),
            from_display_name: from_display_name.to_string(),
            to_user_id: None,
            to_identifier: to_identifier.map(String::from),
            code: code.clone(),
            status: PairingStatus::Pending,
            created_at: now,
            expires_at: now + self.timeout_secs,
            metadata: HashMap::new(),
        };

        let mut requests = self.requests.write().await;
        requests.insert(id, request.clone());

        let mut code_store = self.code_store.write().await;
        code_store.insert(code, request.id.clone());

        Ok(request)
    }

    pub async fn approve_request(
        &self,
        request_id: &str,
        to_user_id: &str,
    ) -> Result<DMPairing, PairingError> {
        let mut requests = self.requests.write().await;

        let request = requests
            .get_mut(request_id)
            .ok_or(PairingError::RequestNotFound)?;

        if request.status != PairingStatus::Pending {
            return Err(PairingError::InvalidStatus);
        }

        let now = chrono::Utc::now().timestamp();
        if now > request.expires_at {
            request.status = PairingStatus::Expired;
            return Err(PairingError::RequestExpired);
        }

        request.status = PairingStatus::Approved;
        request.to_user_id = Some(to_user_id.to_string());

        let pairing = DMPairing {
            id: uuid::Uuid::new_v4().to_string(),
            user_a: request.from_user_id.clone(),
            user_b: to_user_id.to_string(),
            created_at: now,
            last_message_at: now,
            metadata: request.metadata.clone(),
        };

        let mut pairings = self.pairings.write().await;
        pairings.insert(pairing.id.clone(), pairing.clone());

        Ok(pairing)
    }

    pub async fn reject_request(&self, request_id: &str) -> Result<(), PairingError> {
        let mut requests = self.requests.write().await;

        let request = requests
            .get_mut(request_id)
            .ok_or(PairingError::RequestNotFound)?;

        if request.status != PairingStatus::Pending {
            return Err(PairingError::InvalidStatus);
        }

        request.status = PairingStatus::Rejected;
        Ok(())
    }

    pub async fn verify_code(&self, code: &str) -> Result<PairingRequest, PairingError> {
        let code_store = self.code_store.read().await;

        let request_id = code_store.get(code).ok_or(PairingError::InvalidCode)?;

        let requests = self.requests.read().await;

        let request = requests
            .get(request_id)
            .ok_or(PairingError::RequestNotFound)?;

        let now = chrono::Utc::now().timestamp();
        if now > request.expires_at {
            return Err(PairingError::RequestExpired);
        }

        if request.status != PairingStatus::Pending {
            return Err(PairingError::InvalidStatus);
        }

        Ok(request.clone())
    }

    pub async fn get_pairing(&self, user_a: &str, user_b: &str) -> Option<DMPairing> {
        let pairings = self.pairings.read().await;

        pairings
            .values()
            .find(|p| {
                (p.user_a == user_a && p.user_b == user_b)
                    || (p.user_a == user_b && p.user_b == user_a)
            })
            .cloned()
    }

    pub async fn list_user_pairings(&self, user_id: &str) -> Vec<DMPairing> {
        let pairings = self.pairings.read().await;

        pairings
            .values()
            .filter(|p| p.user_a == user_id || p.user_b == user_id)
            .cloned()
            .collect()
    }

    pub async fn remove_pairing(&self, pairing_id: &str) -> Result<(), PairingError> {
        let mut pairings = self.pairings.write().await;

        if pairings.remove(pairing_id).is_none() {
            return Err(PairingError::PairingNotFound);
        }

        Ok(())
    }

    pub async fn cleanup_expired(&self) {
        let now = chrono::Utc::now().timestamp();

        let mut requests = self.requests.write().await;
        let expired: Vec<String> = requests
            .iter()
            .filter(|(_, r)| r.expires_at < now || r.status == PairingStatus::Expired)
            .map(|(id, _)| id.clone())
            .collect();

        for id in expired {
            if let Some(request) = requests.remove(&id) {
                let mut code_store = self.code_store.write().await;
                code_store.remove(&request.code);
            }
        }
    }
}

impl Default for PairingManager {
    fn default() -> Self {
        Self::new(300)
    }
}

#[derive(Debug)]
pub enum PairingError {
    RequestNotFound,
    PairingNotFound,
    InvalidCode,
    RequestExpired,
    InvalidStatus,
}

impl std::fmt::Display for PairingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PairingError::RequestNotFound => write!(f, "Pairing request not found"),
            PairingError::PairingNotFound => write!(f, "Pairing not found"),
            PairingError::InvalidCode => write!(f, "Invalid pairing code"),
            PairingError::RequestExpired => write!(f, "Pairing request expired"),
            PairingError::InvalidStatus => write!(f, "Invalid request status"),
        }
    }
}

impl std::error::Error for PairingError {}
