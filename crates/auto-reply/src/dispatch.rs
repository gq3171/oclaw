//! Main dispatch entry point for inbound messages.

use crate::types::ReplyPayload;

/// Result of dispatching an inbound message.
#[derive(Debug, Clone)]
pub struct DispatchResult {
    pub replied: bool,
    pub skipped: bool,
    pub skip_reason: Option<String>,
    pub payloads: Vec<ReplyPayload>,
}
