//! Agent execution loop for the auto-reply pipeline.

use crate::types::ReplyPayload;

/// Token usage tracking for a single agent run.
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

/// Result of a single agent turn.
#[derive(Debug, Clone)]
pub struct AgentRunResult {
    pub payloads: Vec<ReplyPayload>,
    pub tool_calls: usize,
    pub model_used: String,
    pub tokens: TokenUsage,
    pub was_fallback: bool,
}

/// Options controlling agent execution behavior.
#[derive(Debug, Clone)]
pub struct AgentRunOptions {
    /// Primary model to use.
    pub model: String,
    /// Fallback models if primary fails.
    pub fallback_models: Vec<String>,
    /// Maximum tool call iterations per turn.
    pub max_tool_rounds: usize,
    /// Whether streaming is enabled.
    pub streaming: bool,
    /// Thinking/reasoning level: "off", "low", "medium", "high".
    pub thinking_level: String,
}

impl Default for AgentRunOptions {
    fn default() -> Self {
        Self {
            model: "claude-sonnet-4-20250514".into(),
            fallback_models: vec![],
            max_tool_rounds: 25,
            streaming: true,
            thinking_level: "off".into(),
        }
    }
}

/// Error classification for agent runs — determines retry strategy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentRunError {
    /// Context window exceeded — needs compaction.
    ContextOverflow,
    /// Auth failure — needs cooldown or key rotation.
    AuthFailure(String),
    /// Rate limited — retry after delay.
    RateLimited { retry_after_ms: Option<u64> },
    /// Transient error — retry with backoff.
    Transient(String),
    /// Permanent error — do not retry.
    Permanent(String),
}

impl AgentRunError {
    /// Whether this error is retryable.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::RateLimited { .. } | Self::Transient(_)
        )
    }

    /// Whether this error needs context compaction.
    pub fn needs_compaction(&self) -> bool {
        matches!(self, Self::ContextOverflow)
    }
}
