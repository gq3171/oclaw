use serde::{Deserialize, Serialize};

/// Group-level policy controlling who can interact and how.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GroupPolicy {
    /// Require @mention to trigger the bot in groups.
    pub require_mention: bool,
    /// Tool allow/deny overrides for this group.
    pub tool_policy: Option<ToolPolicyConfig>,
    /// DM handling policy.
    pub dm_policy: DmPolicy,
    /// Allowlist of user IDs or names permitted to interact.
    pub allowlist: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DmPolicy {
    /// Anyone can DM the bot.
    #[default]
    Open,
    /// Require device-pairing before DM.
    Pairing,
    /// Only allowlisted users can DM.
    Allowlist,
    /// DMs are disabled.
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolPolicyConfig {
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}

/// Check whether a sender is permitted by the allowlist.
///
/// Returns `true` if the allowlist is empty (open), contains `"*"`,
/// or matches the sender's ID or name (case-insensitive).
pub fn resolve_allowlist_match(
    sender_id: &str,
    sender_name: Option<&str>,
    allowlist: &[String],
) -> bool {
    if allowlist.is_empty() {
        return true;
    }
    if allowlist.iter().any(|a| a == "*") {
        return true;
    }
    allowlist.iter().any(|entry| {
        entry == sender_id || sender_name.is_some_and(|n| n.eq_ignore_ascii_case(entry))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_allowlist_permits_all() {
        assert!(resolve_allowlist_match("u1", Some("Alice"), &[]));
    }

    #[test]
    fn wildcard_permits_all() {
        let list = vec!["*".to_string()];
        assert!(resolve_allowlist_match("u1", Some("Alice"), &list));
    }

    #[test]
    fn match_by_id() {
        let list = vec!["u1".to_string()];
        assert!(resolve_allowlist_match("u1", None, &list));
        assert!(!resolve_allowlist_match("u2", None, &list));
    }

    #[test]
    fn match_by_name_case_insensitive() {
        let list = vec!["alice".to_string()];
        assert!(resolve_allowlist_match("u1", Some("Alice"), &list));
        assert!(resolve_allowlist_match("u1", Some("ALICE"), &list));
        assert!(!resolve_allowlist_match("u1", Some("Bob"), &list));
    }

    #[test]
    fn no_name_no_match() {
        let list = vec!["alice".to_string()];
        assert!(!resolve_allowlist_match("u1", None, &list));
    }
}
