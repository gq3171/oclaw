//! Tool group resolution — maps group names to individual tool names.

/// Resolve a tool group identifier to its member tool names.
///
/// Groups use the `group:xxx` convention. Unknown groups return an empty list.
pub fn resolve_tool_group(group: &str) -> Vec<&'static str> {
    match group {
        "group:memory" => vec!["memory"],
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
    fn expand_mixed_list() {
        let items = vec![
            "bash".to_string(),
            "group:memory".to_string(),
            "bash".to_string(), // duplicate
        ];
        let expanded = expand_tool_list(&items);
        assert_eq!(expanded, vec!["bash", "memory"]);
    }

    #[test]
    fn is_group_ref_check() {
        assert!(is_group_ref("group:web"));
        assert!(!is_group_ref("bash"));
    }
}
