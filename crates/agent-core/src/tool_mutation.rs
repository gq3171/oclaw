use std::collections::HashSet;

const MUTATING_TOOLS: &[&str] = &[
    "write",
    "edit",
    "apply_patch",
    "exec",
    "bash",
    "process",
    "message",
    "sessions_send",
    "cron",
    "gateway",
    "canvas",
    "nodes",
    "session_status",
];

const PROCESS_MUTATING_ACTIONS: &[&str] = &["write", "send_keys", "submit", "paste", "kill"];
const MESSAGE_MUTATING_ACTIONS: &[&str] = &[
    "send",
    "reply",
    "thread_reply",
    "edit",
    "delete",
    "react",
    "pin",
    "unpin",
];
const READ_ONLY_ACTIONS: &[&str] = &["list", "get", "read", "status", "search", "query"];

pub fn is_mutating_tool_call(name: &str, args: &serde_json::Value) -> bool {
    let lower = name.to_lowercase();
    let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("");

    if !MUTATING_TOOLS.contains(&lower.as_str())
        && !lower.ends_with("_actions")
        && !lower.contains("send")
    {
        return false;
    }

    match lower.as_str() {
        "process" => PROCESS_MUTATING_ACTIONS.contains(&action),
        "message" => {
            MESSAGE_MUTATING_ACTIONS.contains(&action)
                || args.get("content").is_some()
                || args.get("message").is_some()
        }
        "session_status" => args.get("model").is_some(),
        "cron" | "gateway" | "canvas" | "nodes" => !READ_ONLY_ACTIONS.contains(&action),
        _ => true,
    }
}

const TARGET_KEYS: &[&str] = &[
    "path",
    "filePath",
    "oldPath",
    "newPath",
    "to",
    "target",
    "messageId",
    "sessionKey",
    "jobId",
    "id",
    "model",
];

pub fn build_tool_action_fingerprint(
    name: &str,
    args: &serde_json::Value,
    meta: Option<&str>,
) -> Option<String> {
    if !is_mutating_tool_call(name, args) {
        return None;
    }

    let lower = name.to_lowercase();
    let mut parts = vec![format!("tool={}", lower)];

    if let Some(action) = args.get("action").and_then(|v| v.as_str()) {
        parts.push(format!("action={}", action.to_lowercase().trim()));
    }

    let mut found_target = false;
    for &key in TARGET_KEYS {
        if let Some(val) = args.get(key).and_then(normalize_value) {
            parts.push(format!("{}={}", key, val));
            found_target = true;
        }
    }

    if !found_target && let Some(m) = meta {
        let normalized = m.trim().to_lowercase();
        if !normalized.is_empty() {
            parts.push(format!("meta={}", normalized));
        }
    }

    Some(parts.join("|"))
}

fn normalize_value(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::String(s) => Some(s.trim().to_lowercase()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct ToolActionRef {
    pub tool_name: String,
    pub meta: Option<String>,
    pub fingerprint: Option<String>,
}

pub fn is_same_mutation_action(a: &ToolActionRef, b: &ToolActionRef) -> bool {
    match (&a.fingerprint, &b.fingerprint) {
        (Some(fa), Some(fb)) => fa == fb,
        (Some(_), None) | (None, Some(_)) => false,
        (None, None) => {
            a.tool_name == b.tool_name
                && a.meta.as_deref().unwrap_or("") == b.meta.as_deref().unwrap_or("")
        }
    }
}

/// Track seen mutation fingerprints for dedup.
#[derive(Default)]
pub struct MutationTracker {
    seen: HashSet<String>,
}

impl MutationTracker {
    pub fn record(&mut self, name: &str, args: &serde_json::Value) -> Option<String> {
        let fp = build_tool_action_fingerprint(name, args, None)?;
        self.seen.insert(fp.clone());
        Some(fp)
    }

    pub fn has_seen(&self, fingerprint: &str) -> bool {
        self.seen.contains(fingerprint)
    }
}
