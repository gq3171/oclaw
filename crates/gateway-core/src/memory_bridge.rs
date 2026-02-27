//! Bridges memory-core's MemoryManager to agent-core's MemoryRecaller trait.

use oclaw_agent_core::auto_recall::{MemoryRecaller, RecallResult};
use oclaw_memory_core::MemoryManager;
use std::sync::Arc;

pub struct MemoryManagerRecaller {
    manager: Arc<MemoryManager>,
}

impl MemoryManagerRecaller {
    pub fn new(manager: Arc<MemoryManager>) -> Self {
        Self { manager }
    }
}

#[async_trait::async_trait]
impl MemoryRecaller for MemoryManagerRecaller {
    async fn recall(&self, query: &str, max_results: usize, min_score: f32) -> Vec<RecallResult> {
        let results = match self.manager.search(query).await {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("Memory recall failed: {}", e);
                return Vec::new();
            }
        };

        results
            .into_iter()
            .filter(|r| r.score as f32 >= min_score)
            .take(max_results)
            .map(|r| RecallResult {
                key: if r.path.is_empty() {
                    r.source.clone()
                } else {
                    r.path.clone()
                },
                content: r.snippet,
                score: r.score as f32,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn recall_handles_text_ids_without_type_errors() {
        let db_path =
            std::env::temp_dir().join(format!("oclaw-memory-bridge-{}.db", uuid::Uuid::new_v4()));
        let store = oclaw_memory_core::MemoryStore::new(&db_path);
        store.init().expect("init memory store");
        let manager = Arc::new(oclaw_memory_core::MemoryManager::new(store, None));
        manager
            .add_memory("温暖贴心的小助手", "unit-test")
            .await
            .expect("seed memory");

        let recaller = MemoryManagerRecaller::new(manager);
        let rows = recaller.recall("温暖贴心", 5, 0.0).await;
        assert!(!rows.is_empty(), "expected recall results for seeded text");

        let _ = std::fs::remove_file(db_path);
    }
}
