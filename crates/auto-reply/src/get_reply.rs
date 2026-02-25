//! Core reply generation — orchestrates agent call and payload building.

use crate::agent_runner::{AgentRunOptions, AgentRunResult};
use crate::types::ReplyPayload;

/// Callback type for payload events.
type PayloadCallback = Option<Box<dyn Fn(&ReplyPayload) + Send + Sync>>;
/// Callback type for string events.
type StrCallback = Option<Box<dyn Fn(&str) + Send + Sync>>;

/// Callbacks for reply lifecycle events.
#[derive(Default)]
pub struct GetReplyCallbacks {
    pub on_reply_start: Option<Box<dyn Fn() + Send + Sync>>,
    pub on_partial_reply: PayloadCallback,
    pub on_tool_result: PayloadCallback,
    pub on_tool_start: StrCallback,
    pub on_block_reply: PayloadCallback,
    pub on_model_selected: StrCallback,
}

/// Result of the full reply generation pipeline.
#[derive(Debug, Clone)]
pub struct GetReplyResult {
    pub payloads: Vec<ReplyPayload>,
    pub agent_result: Option<AgentRunResult>,
    pub skipped: bool,
    pub skip_reason: Option<String>,
}

/// Verbose level controlling tool output visibility.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum VerboseLevel {
    /// No tool output shown.
    Off,
    /// Tool results only.
    #[default]
    Compact,
    /// Tool results + full output.
    Full,
}

/// Configuration for the reply generation pipeline.
#[derive(Debug, Clone)]
pub struct GetReplyConfig {
    pub agent_options: AgentRunOptions,
    pub verbose: VerboseLevel,
    pub system_prompt: Option<String>,
    pub max_reply_length: usize,
}

impl Default for GetReplyConfig {
    fn default() -> Self {
        Self {
            agent_options: AgentRunOptions::default(),
            verbose: VerboseLevel::default(),
            system_prompt: None,
            max_reply_length: 4096,
        }
    }
}
