//! HEARTBEAT.md — periodic agent checklist for proactive outreach.

use crate::files::Workspace;
use serde::{Deserialize, Serialize};

/// Heartbeat configuration for an agent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeartbeatConfig {
    /// Interval in seconds (default: 1800 = 30m).
    pub interval_secs: u64,
    /// Target channel for heartbeat messages ("last" | "none" | channel id).
    pub target: String,
    /// Custom prompt (overrides default).
    pub prompt: Option<String>,
    /// Max chars for HEARTBEAT_OK ack before dropping (default: 300).
    pub ack_max_chars: usize,
    /// Active hours restriction.
    pub active_hours: Option<ActiveHours>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveHours {
    pub start: String, // "HH:MM"
    pub end: String,   // "HH:MM"
    pub timezone: Option<String>,
}

impl Default for HeartbeatConfig {
    fn default() -> Self {
        Self {
            interval_secs: 1800,
            target: "last".to_string(),
            prompt: None,
            ack_max_chars: 300,
            active_hours: None,
        }
    }
}

/// The HEARTBEAT_OK token used by agents to signal "nothing to report".
pub const HEARTBEAT_OK_TOKEN: &str = "HEARTBEAT_OK";

/// Parsed heartbeat file content.
#[derive(Debug, Clone)]
pub struct HeartbeatFile {
    pub raw: String,
    pub has_tasks: bool,
}

impl HeartbeatFile {
    /// Load HEARTBEAT.md from workspace.
    pub async fn load(ws: &Workspace) -> anyhow::Result<Option<Self>> {
        let Some(content) = ws.read_file(&ws.heartbeat_path()).await? else {
            return Ok(None);
        };
        Ok(Some(Self::parse(&content)))
    }

    pub fn parse(raw: &str) -> Self {
        // File is "effectively empty" if it only contains
        // headers, comments, and whitespace.
        let has_tasks = raw.lines().any(|line| {
            let t = line.trim();
            !t.is_empty()
                && !t.starts_with('#')
                && !t.starts_with("//")
                && !t.starts_with("<!--")
        });
        Self {
            raw: raw.to_string(),
            has_tasks,
        }
    }
}

impl HeartbeatConfig {
    /// Build the default heartbeat prompt.
    pub fn effective_prompt(&self) -> String {
        self.prompt.clone().unwrap_or_else(|| {
            "Read HEARTBEAT.md if it exists (workspace context). \
             Follow it strictly. Do not infer or repeat old tasks \
             from prior chats. If nothing needs attention, reply \
             HEARTBEAT_OK."
                .to_string()
        })
    }
}

/// Strip HEARTBEAT_OK token from message edges.
/// Returns the cleaned text and whether the token was found.
pub fn strip_heartbeat_token(text: &str) -> (String, bool) {
    let trimmed = text.trim();
    if trimmed == HEARTBEAT_OK_TOKEN {
        return (String::new(), true);
    }
    // Strip from start
    if let Some(rest) = trimmed.strip_prefix(HEARTBEAT_OK_TOKEN) {
        let rest = rest.trim();
        return (rest.to_string(), true);
    }
    // Strip from end
    if let Some(rest) = trimmed.strip_suffix(HEARTBEAT_OK_TOKEN) {
        let rest = rest.trim();
        return (rest.to_string(), true);
    }
    (trimmed.to_string(), false)
}

/// Check if a heartbeat reply should be dropped (ack with minimal content).
pub fn should_drop_heartbeat_reply(text: &str, ack_max_chars: usize) -> bool {
    let (remaining, had_token) = strip_heartbeat_token(text);
    had_token && remaining.len() <= ack_max_chars
}
