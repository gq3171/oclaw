//! Preset tool profiles — predefined sets of allowed tools.

use serde::{Deserialize, Serialize};

/// Predefined tool profile determining which tools are available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolProfile {
    /// Minimal: only session status.
    Minimal,
    /// Coding: filesystem, runtime, sessions, memory, image.
    Coding,
    /// Messaging: messaging tools, session read access.
    Messaging,
    /// Full: all tools allowed.
    #[default]
    Full,
}

impl ToolProfile {
    /// Return the list of allowed tool names/groups for this profile.
    pub fn allowed_tools(&self) -> Vec<&'static str> {
        match self {
            Self::Minimal => vec!["session_status"],
            Self::Coding => vec![
                "group:fs", "group:runtime", "group:sessions",
                "group:memory", "group:web", "media_describe",
            ],
            Self::Messaging => vec![
                "group:messaging", "sessions_list",
                "sessions_history", "sessions_send", "session_status",
            ],
            Self::Full => vec!["*"],
        }
    }

    /// Check if a specific tool is allowed by this profile.
    pub fn allows_tool(&self, tool_name: &str) -> bool {
        let allowed = self.allowed_tools();
        if allowed.contains(&"*") {
            return true;
        }
        // Direct match
        if allowed.contains(&tool_name) {
            return true;
        }
        // Group expansion
        for entry in &allowed {
            if crate::groups::is_group_ref(entry) {
                let members = crate::groups::resolve_tool_group(entry);
                if members.contains(&tool_name) {
                    return true;
                }
            }
        }
        false
    }
}
