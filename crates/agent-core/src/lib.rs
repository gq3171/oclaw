pub mod agent;
pub mod str_util;
pub mod subagent;
pub mod model_fallback;
pub mod auth;
pub mod command;
pub mod loop_detect;
pub mod transcript;
pub mod history;
pub mod compaction;
pub mod pruning;
pub mod transcript_repair;
pub mod tool_mutation;
pub mod command_poll;
pub mod auth_cooldown;
pub mod stream_chunker;
pub mod echo_detect;
pub mod reply_dispatch;
pub mod auto_recall;
pub mod thread_ownership;
pub mod context_guard;
pub mod thinking;
pub mod error_classify;
pub mod usage;

pub use agent::{Agent, AgentConfig, AgentState, ToolExecutor};
pub use subagent::{Subagent, SubagentRegistry, SubagentStatus};
pub use model_fallback::{ModelFallback, ModelChain, FallbackConfig, ModelCandidate, CooldownTracker, FallbackAttempt, FallbackResult, run_with_fallback};
pub use auth::{AuthManager, AuthProvider, ProviderCredentials};
pub use loop_detect::{LoopDetector, LoopLevel, LoopDetectionResult};
pub use transcript::Transcript;
pub use history::limit_history_turns;
pub use compaction::{CompactionConfig, CompactionResult, compact_history, needs_compaction};
pub use pruning::{PruningConfig, prune_tool_results};
pub use transcript_repair::{repair_tool_use_result_pairing, sanitize_tool_call_inputs, repair_jsonl_lines};
pub use tool_mutation::{is_mutating_tool_call, build_tool_action_fingerprint, MutationTracker};
pub use command_poll::CommandPollTracker;
pub use auth_cooldown::AuthCooldownTracker;
pub use stream_chunker::{StreamChunker, ChunkingConfig, BreakPreference};
pub use echo_detect::EchoTracker;
pub use reply_dispatch::{ReplyDispatcher, ReplyPayload, ReplyKind, LaneDispatcher};
pub use command::{Command, CommandParser, CommandResult};
pub use auto_recall::{AutoRecallConfig, RecallResult, MemoryRecaller, format_recall_context};
pub use thread_ownership::ThreadOwnership;
pub use context_guard::{ContextGuard, ContextGuardConfig, GuardAction};
pub use thinking::{ThinkingConfig, ReasoningLevel, ThinkingBlock, drop_thinking_blocks, extract_thinking, supports_thinking};
pub use error_classify::{ErrorClass, classify_error};
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
