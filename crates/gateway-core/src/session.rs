use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct Session {
    pub key: String,
    pub agent_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: i64,
    pub metadata: HashMap<String, String>,
}

impl Session {
    pub fn new(key: &str, agent_id: &str) -> Self {
        let now = chrono::Utc::now().timestamp_millis();
        Self {
            key: key.to_string(),
            agent_id: agent_id.to_string(),
            created_at: now,
            updated_at: now,
            message_count: 0,
            metadata: HashMap::new(),
        }
    }
}

pub type SessionStore = Arc<RwLock<HashMap<String, Session>>>;

pub fn create_session_store() -> SessionStore {
    Arc::new(RwLock::new(HashMap::new()))
}
