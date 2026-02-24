/// Memory auto-recall: search long-term memory before LLM request and inject as context.
use oclaws_llm_core::chat::{ChatMessage, MessageRole};

#[derive(Debug, Clone)]
pub struct AutoRecallConfig {
    pub enabled: bool,
    pub max_results: usize,
    pub min_score: f32,
}

impl Default for AutoRecallConfig {
    fn default() -> Self {
        Self { enabled: false, max_results: 5, min_score: 0.3 }
    }
}

#[derive(Debug, Clone)]
pub struct RecallResult {
    pub key: String,
    pub content: String,
    pub score: f32,
}

/// Format recalled memories into a system message for injection.
pub fn format_recall_context(results: &[RecallResult]) -> Option<ChatMessage> {
    if results.is_empty() {
        return None;
    }
    let mut body = String::from("## Recalled from memory\n");
    for r in results {
        body.push_str(&format!("- [{}] (score {:.2}): {}\n", r.key, r.score, r.content));
    }
    Some(ChatMessage {
        role: MessageRole::System,
        content: body,
        name: Some("memory_recall".into()),
        tool_calls: None,
        tool_call_id: None,
    })
}

/// Trait for memory backends to implement recall.
#[async_trait::async_trait]
pub trait MemoryRecaller: Send + Sync {
    async fn recall(&self, query: &str, max_results: usize, min_score: f32) -> Vec<RecallResult>;
}
