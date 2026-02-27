//! Auto-reply pipeline — core message processing for oclaw.

pub mod agent_runner;
pub mod block_pipeline;
pub mod block_streaming;
pub mod context;
pub mod debounce;
pub mod dispatch;
pub mod envelope;
pub mod get_reply;
pub mod link_detect;
pub mod link_runner;
pub mod link_understanding;
pub mod normalize;
pub mod queue;
pub mod reply_dispatcher;
pub mod tokens;
pub mod types;

pub use agent_runner::{AgentRunError, AgentRunOptions, AgentRunResult, TokenUsage};
pub use block_pipeline::BlockReplyPipeline;
pub use block_streaming::{ChunkMode, CoalescingConfig};
pub use context::{ChatType, FinalizedMsgContext, MsgContext, finalize_inbound_context};
pub use debounce::InboundDebounce;
pub use dispatch::DispatchResult;
pub use envelope::ReplyEnvelope;
pub use get_reply::{GetReplyCallbacks, GetReplyConfig, GetReplyResult, VerboseLevel};
pub use normalize::{NormalizeOptions, normalize_reply_payload};
pub use queue::{MessageQueue, QueueAction, QueueDropPolicy, QueueMode};
pub use reply_dispatcher::{AutoReplyDispatcher, AutoReplyReceiver, ReplyDispatchKind};
pub use types::ReplyPayload;
