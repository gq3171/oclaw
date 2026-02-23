//! TUI State Management - Simplified

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Application state manager - Placeholder
pub struct AppStateManager {
    state: Arc<RwLock<HashMap<String, super::StateValue>>>,
}

impl AppStateManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(HashMap::new())),
        }
    }
    
    pub async fn get(&self, key: &str) -> Option<super::StateValue> {
        self.state.read().await.get(key).cloned()
    }
    
    pub async fn set(&self, key: &str, value: super::StateValue) {
        let mut state = self.state.write().await;
        state.insert(key.to_string(), value);
    }
    
    pub async fn remove(&self, key: &str) {
        let mut state = self.state.write().await;
        state.remove(key);
    }
    
    pub async fn clear(&self) {
        let mut state = self.state.write().await;
        state.clear();
    }
}

impl Default for AppStateManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for AppStateManager {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
        }
    }
}
