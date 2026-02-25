//! Multi-mode delivery for cron job output.
//!
//! Supports channel-based delivery (existing), webhook POST, and log-only modes.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "camelCase")]
pub enum DeliveryMode {
    Channel {
        channel: String,
        target: String,
    },
    Webhook {
        url: String,
        #[serde(default)]
        headers: HashMap<String, String>,
    },
    Log,
}

/// Result of a single delivery attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeliveryOutcome {
    pub mode: String,
    pub success: bool,
    pub error: Option<String>,
}
