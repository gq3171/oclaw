//! Message envelope — wraps reply payloads for channel delivery.

use crate::types::ReplyPayload;

/// Delivery envelope wrapping a reply payload with routing metadata.
#[derive(Debug, Clone)]
pub struct ReplyEnvelope {
    pub session_key: String,
    pub channel: String,
    pub target: String,
    pub payload: ReplyPayload,
    pub thread_id: Option<String>,
    pub reply_to_message_id: Option<String>,
}

impl ReplyEnvelope {
    pub fn new(
        session_key: String,
        channel: String,
        target: String,
        payload: ReplyPayload,
    ) -> Self {
        let reply_to = if payload.reply_to_current {
            payload.reply_to_id.clone()
        } else {
            None
        };
        Self {
            session_key,
            channel,
            target,
            thread_id: None,
            reply_to_message_id: reply_to,
            payload,
        }
    }

    pub fn with_thread(mut self, thread_id: Option<String>) -> Self {
        self.thread_id = thread_id;
        self
    }

    /// Whether this envelope targets a thread reply.
    pub fn is_thread_reply(&self) -> bool {
        self.thread_id.is_some()
    }
}
