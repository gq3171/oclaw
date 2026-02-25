//! Bridges memory-core's MemoryManager to agent-core's MemoryRecaller trait.

use std::sync::Arc;
use oclaws_agent_core::auto_recall::{MemoryRecaller, RecallResult};
use oclaws_memory_core::MemoryManager;

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
                key: if r.path.is_empty() { r.source.clone() } else { r.path.clone() },
                content: r.snippet,
                score: r.score as f32,
            })
            .collect()
    }
}
