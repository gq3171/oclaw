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
        // headers, comments, empty bullet items, and whitespace.
        let has_tasks = raw.lines().any(|line| {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') || t.starts_with("//") || t.starts_with("<!--") {
                return false;
            }
            // Empty list items: "- ", "* ", "+ " with nothing after
            if let Some(rest) = t
                .strip_prefix("- ")
                .or_else(|| t.strip_prefix("* "))
                .or_else(|| t.strip_prefix("+ "))
            {
                return !rest.trim().is_empty();
            }
            // Bare "-", "*", "+" with nothing
            if matches!(t, "-" | "*" | "+") {
                return false;
            }
            true
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

/// Strip HEARTBEAT_OK token from a reply, handling markdown-wrapped variants.
/// Returns (cleaned_text, was_token_found).
/// Handles: HEARTBEAT_OK, **HEARTBEAT_OK**, __HEARTBEAT_OK__, <b>HEARTBEAT_OK</b>
pub fn strip_heartbeat_token(text: &str) -> (String, bool) {
    let trimmed = text.trim();

    // Helper: strip markdown wrappers and check if core is the token
    let unwrap_and_check = |s: &str| -> bool {
        let s = s.trim();
        let s = s
            .strip_prefix("**")
            .and_then(|s| s.strip_suffix("**"))
            .unwrap_or(s)
            .trim();
        let s = s
            .strip_prefix("__")
            .and_then(|s| s.strip_suffix("__"))
            .unwrap_or(s)
            .trim();
        let s = s
            .strip_prefix("<b>")
            .and_then(|s| s.strip_suffix("</b>"))
            .unwrap_or(s)
            .trim();
        let s = s
            .strip_prefix("_")
            .and_then(|s| s.strip_suffix("_"))
            .unwrap_or(s)
            .trim();
        s == HEARTBEAT_OK_TOKEN
    };

    // Exact match (with or without markdown wrapping)
    if unwrap_and_check(trimmed) {
        return (String::new(), true);
    }

    // Strip from start of text
    for prefix_len in [HEARTBEAT_OK_TOKEN.len(), HEARTBEAT_OK_TOKEN.len() + 4] {
        if trimmed.len() >= prefix_len {
            let candidate = &trimmed[..prefix_len.min(trimmed.len())];
            if unwrap_and_check(candidate) {
                let rest = trimmed[prefix_len.min(trimmed.len())..].trim();
                return (rest.to_string(), true);
            }
        }
    }

    // Strip from end of text
    if let Some(rest) = trimmed.strip_suffix(HEARTBEAT_OK_TOKEN) {
        return (rest.trim().to_string(), true);
    }
    if trimmed.ends_with(&format!("**{}**", HEARTBEAT_OK_TOKEN)) {
        let suffix_len = HEARTBEAT_OK_TOKEN.len() + 4;
        let rest = trimmed[..trimmed.len() - suffix_len].trim();
        return (rest.to_string(), true);
    }

    (trimmed.to_string(), false)
}

/// Check if a heartbeat reply should be dropped (ack with minimal content).
pub fn should_drop_heartbeat_reply(text: &str, ack_max_chars: usize) -> bool {
    let (remaining, had_token) = strip_heartbeat_token(text);
    had_token && remaining.len() <= ack_max_chars
}

/// Check if current time is within configured active hours.
/// Handles wrap-around midnight (e.g., 22:00 → 06:00).
/// Note: timezone field is stored but requires chrono-tz for full IANA support.
/// Currently resolves "local" and None to local system time.
pub fn is_within_active_hours(config: &HeartbeatConfig) -> bool {
    let Some(ref hours) = config.active_hours else {
        return true; // no restriction configured
    };
    let parse_time = |s: &str| chrono::NaiveTime::parse_from_str(s, "%H:%M").ok();
    let Some(start) = parse_time(&hours.start) else {
        return true;
    };
    let Some(end) = parse_time(&hours.end) else {
        return true;
    };
    let now = chrono::Local::now().time();
    if start <= end {
        now >= start && now <= end
    } else {
        // Wraps midnight (e.g., 22:00 → 06:00)
        now >= start || now <= end
    }
}

/// Reason a heartbeat tick was triggered. Used for logging and metrics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeartbeatReasonKind {
    /// Periodic interval tick.
    Interval,
    /// Manual trigger via CLI/command.
    Manual,
    /// Retry after a previous failure.
    Retry,
    /// Triggered by an async exec event completing.
    ExecEvent,
    /// Scheduled wake event.
    Wake,
    /// Triggered by a cron job.
    Cron,
    /// Triggered by a hook.
    Hook,
    /// Other/unknown reason.
    Other,
}

impl std::fmt::Display for HeartbeatReasonKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Interval => write!(f, "interval"),
            Self::Manual => write!(f, "manual"),
            Self::Retry => write!(f, "retry"),
            Self::ExecEvent => write!(f, "exec-event"),
            Self::Wake => write!(f, "wake"),
            Self::Cron => write!(f, "cron"),
            Self::Hook => write!(f, "hook"),
            Self::Other => write!(f, "other"),
        }
    }
}
