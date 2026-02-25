use crate::session_key::SessionKey;
use serde::{Deserialize, Serialize};

/// Unified message context built from webhook payloads.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MsgContext {
    pub session_key: SessionKey,
    pub sender_id: String,
    pub sender_name: Option<String>,
    pub content: String,
    pub message_id: Option<String>,
    pub thread_id: Option<String>,
    pub is_mention: bool,
    pub is_dm: bool,
    pub channel_type: String,
    pub raw: serde_json::Value,
    pub timestamp_ms: u64,
}

impl MsgContext {
    pub fn new(
        session_key: SessionKey,
        sender_id: &str,
        content: &str,
    ) -> Self {
        let is_dm = session_key.is_dm();
        let channel_type = session_key.channel.clone();
        Self {
            session_key,
            sender_id: sender_id.to_string(),
            sender_name: None,
            content: content.to_string(),
            message_id: None,
            thread_id: None,
            is_mention: false,
            is_dm,
            channel_type,
            raw: serde_json::Value::Null,
            timestamp_ms: now_ms(),
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
