//! IDENTITY.md — agent name, emoji, avatar, creature type, vibe.

use crate::files::Workspace;
use serde::{Deserialize, Serialize};

/// Parsed agent identity from IDENTITY.md.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentIdentity {
    pub name: Option<String>,
    pub emoji: Option<String>,
    pub creature: Option<String>,
    pub vibe: Option<String>,
    pub avatar: Option<String>,
}

impl AgentIdentity {
    /// Load and parse IDENTITY.md from workspace.
    pub async fn load(ws: &Workspace) -> anyhow::Result<Option<Self>> {
        let Some(content) = ws.read_file(&ws.identity_path()).await? else {
            return Ok(None);
        };
        Ok(Some(Self::parse(&content)))
    }

    /// Parse markdown key-value pairs from IDENTITY.md.
    pub fn parse(raw: &str) -> Self {
        let mut id = AgentIdentity::default();
        for line in raw.lines() {
            let trimmed = line.trim();
            // Match "- **Key:** value" or "- Key: value" patterns
            let entry = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))
                .unwrap_or("");
            let entry = entry.replace("**", "");
            if let Some((key, val)) = entry.split_once(':') {
                let key = key.trim().to_lowercase();
                let val = val.trim().trim_matches('_').trim();
                if val.is_empty() || val.starts_with("(") {
                    continue; // placeholder
                }
                match key.as_str() {
                    "name" => id.name = Some(val.to_string()),
                    "emoji" => id.emoji = Some(val.to_string()),
                    "creature" => id.creature = Some(val.to_string()),
                    "vibe" => id.vibe = Some(val.to_string()),
                    "avatar" => id.avatar = Some(val.to_string()),
                    _ => {}
                }
            }
        }
        id
    }

    /// Display name with optional emoji prefix.
    pub fn display_name(&self) -> String {
        match (&self.emoji, &self.name) {
            (Some(e), Some(n)) => format!("{} {}", e, n),
            (None, Some(n)) => n.clone(),
            (Some(e), None) => format!("{} Assistant", e),
            (None, None) => "Assistant".to_string(),
        }
    }

    /// Serialize identity back to markdown format.
    pub fn to_markdown(&self) -> String {
        let mut lines = vec!["# IDENTITY.md - Who Am I?\n".to_string()];
        if let Some(n) = &self.name {
            lines.push(format!("- **Name:** {}", n));
        }
        if let Some(c) = &self.creature {
            lines.push(format!("- **Creature:** {}", c));
        }
        if let Some(v) = &self.vibe {
            lines.push(format!("- **Vibe:** {}", v));
        }
        if let Some(e) = &self.emoji {
            lines.push(format!("- **Emoji:** {}", e));
        }
        if let Some(a) = &self.avatar {
            lines.push(format!("- **Avatar:** {}", a));
        }
        lines.join("\n") + "\n"
    }
}
