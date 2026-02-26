//! Tool group resolution — maps group names to individual tool names.

/// Resolve a tool group identifier to its member tool names.
///
/// Groups use the `group:xxx` convention. Unknown groups return an empty list.
pub fn resolve_tool_group(group: &str) -> Vec<&'static str> {
    match group {
        "group:memory" => vec!["memory_search", "memory_get"],
        "group:web" => vec!["web_search", "web_fetch", "link_reader"],
        "group:fs" => vec!["read_file", "write_file", "list_dir"],
        "group:runtime" => vec!["bash"],
        "group:sessions" => vec![
            "sessions_list", "sessions_history", "sessions_send",
            "sessions_spawn", "subagents", "session_status",
        ],
        "group:ui" => vec!["browse"],
        "group:automation" => vec!["cron"],
        "group:messaging" => vec!["message"],
        "group:media" => vec!["media_describe", "tts"],
        _ => vec![],
    }
}

/// Check if a name is a group reference (starts with `group:`).
pub fn is_group_ref(name: &str) -> bool {
    name.starts_with("group:")
}

/// Expand a mixed list of tool names and group references into flat tool names.
pub fn expand_tool_list(items: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        if is_group_ref(item) {
            for t in resolve_tool_group(item) {
                let s = t.to_string();
                if !out.contains(&s) {
                    out.push(s);
                }
            }
        } else if !out.contains(item) {
            out.push(item.clone());
        }
    }
    out
}

/// Resolve common tool name aliases (mirrors Node's TOOL_NAME_ALIASES).
/// e.g. "bash" → "exec", "apply-patch" → "apply_patch"
pub fn resolve_tool_alias(name: &str) -> &str {
    match name {
        "exec"         => "bash",
        "apply-patch"  => "apply_patch",
        _              => name,
    }
}

/// Tools that only the owner (trusted sender) may invoke.
pub fn owner_only_tools() -> &'static [&'static str] {
    &["cron", "gateway", "sessions_spawn"]
}

/// Returns true if the tool requires owner-level access.
pub fn is_owner_only_tool(name: &str) -> bool {
    owner_only_tools().contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_known_group() {
        let tools = resolve_tool_group("group:web");
        assert!(tools.contains(&"web_search"));
        assert!(tools.contains(&"web_fetch"));
        assert!(tools.contains(&"link_reader"));
    }

    #[test]
    fn resolve_unknown_group() {
        assert!(resolve_tool_group("group:nonexistent").is_empty());
    }

    #[test]
    fn resolve_memory_group_has_search_and_get() {
        let tools = resolve_tool_group("group:memory");
        assert!(tools.contains(&"memory_search"), "group:memory must include memory_search");
        assert!(tools.contains(&"memory_get"), "group:memory must include memory_get");
        assert!(!tools.contains(&"memory"), "group:memory must NOT contain bare 'memory'");
    }

    #[test]
    fn expand_mixed_list() {
        let items = vec![
            "bash".to_string(),
            "group:memory".to_string(),
            "bash".to_string(), // duplicate
        ];
        let expanded = expand_tool_list(&items);
        assert!(expanded.contains(&"bash".to_string()));
        assert!(expanded.contains(&"memory_search".to_string()));
        assert!(expanded.contains(&"memory_get".to_string()));
        // bash should only appear once
        assert_eq!(expanded.iter().filter(|t| t.as_str() == "bash").count(), 1);
    }

    #[test]
    fn is_group_ref_check() {
        assert!(is_group_ref("group:web"));
        assert!(!is_group_ref("bash"));
    }

    #[test]
    fn tool_alias_exec_maps_to_bash() {
        assert_eq!(resolve_tool_alias("exec"), "bash");
    }

    #[test]
    fn tool_alias_apply_patch() {
        assert_eq!(resolve_tool_alias("apply-patch"), "apply_patch");
    }

    #[test]
    fn tool_alias_passthrough() {
        assert_eq!(resolve_tool_alias("memory_search"), "memory_search");
    }

    #[test]
    fn owner_only_tools_list() {
        assert!(is_owner_only_tool("cron"));
        assert!(is_owner_only_tool("gateway"));
        assert!(is_owner_only_tool("sessions_spawn"));
        assert!(!is_owner_only_tool("bash"));
        assert!(!is_owner_only_tool("memory_search"));
    }
}
