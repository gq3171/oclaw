//! Core types for the auto-reply pipeline.

use serde::{Deserialize, Serialize};

/// A reply payload sent back to the channel.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReplyPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_urls: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply_to_id: Option<String>,
    #[serde(default)]
    pub reply_to_current: bool,
    #[serde(default)]
    pub audio_as_voice: bool,
    #[serde(default)]
    pub is_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_data: Option<serde_json::Value>,
}
