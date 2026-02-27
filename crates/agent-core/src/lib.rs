pub mod agent;
pub mod auth;
pub mod auth_cooldown;
pub mod auto_recall;
pub mod command;
pub mod command_poll;
pub mod compaction;
pub mod context_guard;
pub mod echo_detect;
pub mod error_classify;
pub mod history;
pub mod loop_detect;
pub mod model_fallback;
pub mod pruning;
pub mod reply_dispatch;
pub mod str_util;
pub mod stream_chunker;
pub mod subagent;
pub mod task_graph;
pub mod thinking;
pub mod thread_ownership;
pub mod tool_mutation;
pub mod transcript;
pub mod transcript_repair;
pub mod usage;

pub use agent::{Agent, AgentConfig, AgentState, ToolExecutor};
pub use auth::{AuthManager, AuthProvider, ProviderCredentials};
pub use auth_cooldown::AuthCooldownTracker;
pub use auto_recall::{AutoRecallConfig, MemoryRecaller, RecallResult, format_recall_context};
pub use command::{Command, CommandParser, CommandResult};
pub use command_poll::CommandPollTracker;
pub use compaction::{CompactionConfig, CompactionResult, compact_history, needs_compaction};
pub use context_guard::{ContextGuard, ContextGuardConfig, GuardAction};
pub use echo_detect::EchoTracker;
pub use error_classify::{ErrorClass, classify_error};
pub use history::limit_history_turns;
pub use loop_detect::{LoopDetectionResult, LoopDetector, LoopLevel};
pub use model_fallback::{
    CooldownTracker, FallbackAttempt, FallbackConfig, FallbackResult, ModelCandidate, ModelChain,
    ModelFallback, run_with_fallback,
};
pub use pruning::{PruningConfig, prune_tool_results};
pub use reply_dispatch::{LaneDispatcher, ReplyDispatcher, ReplyKind, ReplyPayload};
pub use stream_chunker::{BreakPreference, ChunkingConfig, StreamChunker};
pub use subagent::{Subagent, SubagentConfig, SubagentRegistry, SubagentStatus};
pub use task_graph::{TaskGraph, TaskGraphResult, TaskGraphRunner, TaskNode};
pub use thinking::{
    ReasoningLevel, ThinkingBlock, ThinkingConfig, drop_thinking_blocks, extract_thinking,
    supports_thinking,
};
pub use thread_ownership::ThreadOwnership;
pub use tool_mutation::{MutationTracker, build_tool_action_fingerprint, is_mutating_tool_call};
pub use transcript::Transcript;
pub use transcript_repair::{
    repair_jsonl_lines, repair_tool_use_result_pairing, sanitize_tool_call_inputs,
};
pub use usage::{UsageAccumulator, UsageSummary};

pub type AgentResult<T> = Result<T, AgentError>;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("Initialization error: {0}")]
    InitError(String),

    #[error("Execution error: {0}")]
    ExecutionError(String),

    #[error("Model error: {0}")]
    ModelError(String),

    #[error("Authentication error: {0}")]
    AuthError(String),

    #[error("Subagent error: {0}")]
    SubagentError(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Rate limited: {0}")]
    RateLimited(String),

    #[error("Context overflow: {0}")]
    ContextOverflow(String),
}

impl serde::Serialize for AgentError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
