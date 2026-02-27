//! ACP gateway server — handles external client connections.

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, info};

use crate::permissions::AcpPermissions;
use crate::session::{AcpSessionError, AcpSessionStore};

pub struct AcpServer {
    sessions: Arc<RwLock<AcpSessionStore>>,
    default_permissions: AcpPermissions,
}

impl AcpServer {
    pub fn new(default_permissions: AcpPermissions) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(AcpSessionStore::default())),
            default_permissions,
        }
    }

    pub async fn create_session(&self, cwd: std::path::PathBuf) -> Result<String, AcpSessionError> {
        let mut store = self.sessions.write().await;
        let session = store.create(cwd)?;
        let id = session.session_id.clone();
        info!(session_id = %id, "ACP session created");
        Ok(id)
    }

    pub async fn get_session_info(&self, id: &str) -> Option<serde_json::Value> {
        let store = self.sessions.read().await;
        store.get(id).map(|s| {
            serde_json::json!({
                "session_id": s.session_id,
                "session_key": s.session_key,
                "cwd": s.cwd.display().to_string(),
                "created_at": s.created_at,
                "last_touched_at": s.last_touched_at,
                "active_run_id": s.active_run_id,
                "message_count": s.messages.len(),
            })
        })
    }

    pub async fn cancel_session(&self, id: &str) -> bool {
        let mut store = self.sessions.write().await;
        let cancelled = store.cancel_active(id);
        if cancelled {
            debug!(session_id = %id, "ACP session run cancelled");
        }
        cancelled
    }

    pub async fn remove_session(&self, id: &str) -> bool {
        let mut store = self.sessions.write().await;
        store.remove(id)
    }

    pub async fn list_sessions(&self) -> Vec<serde_json::Value> {
        let store = self.sessions.read().await;
        store
            .list()
            .iter()
            .map(|s| {
                serde_json::json!({
                    "session_id": s.session_id,
                    "session_key": s.session_key,
                    "created_at": s.created_at,
                    "message_count": s.messages.len(),
                })
            })
            .collect()
    }

    pub fn default_permissions(&self) -> &AcpPermissions {
        &self.default_permissions
    }
}
