//! Auto-reply pipeline — core message processing for oclaw.

pub mod types;
pub mod tokens;
pub mod context;
pub mod normalize;
pub mod envelope;
pub mod agent_runner;
pub mod get_reply;
pub mod dispatch;
pub mod block_streaming;
pub mod block_pipeline;
pub mod reply_dispatcher;
pub mod queue;
pub mod debounce;
pub mod link_detect;
pub mod link_runner;
pub mod link_understanding;

pub use types::ReplyPayload;
pub use context::{ChatType, MsgContext, FinalizedMsgContext, finalize_inbound_context};
pub use normalize::{normalize_reply_payload, NormalizeOptions};
pub use envelope::ReplyEnvelope;
pub use agent_runner::{AgentRunResult, AgentRunOptions, AgentRunError, TokenUsage};
pub use get_reply::{GetReplyCallbacks, GetReplyResult, GetReplyConfig, VerboseLevel};
pub use dispatch::DispatchResult;
pub use block_streaming::{CoalescingConfig, ChunkMode};
pub use block_pipeline::BlockReplyPipeline;
pub use reply_dispatcher::{AutoReplyDispatcher, AutoReplyReceiver, ReplyDispatchKind};
pub use queue::{QueueMode, QueueAction, MessageQueue};
pub use debounce::InboundDebounce;
