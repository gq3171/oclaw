use std::collections::HashSet;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPolicy {
    pub allowlist: Option<HashSet<String>>,
    pub blocklist: Option<HashSet<String>>,
    pub require_mention: bool,
    pub model_override: Option<String>,
}

impl ChannelPolicy {
    pub fn is_allowed(&self, channel_id: &str) -> bool {
        if let Some(blocklist) = &self.blocklist
            && blocklist.contains(channel_id)
        {
            return false;
        }
        if let Some(allowlist) = &self.allowlist {
            return allowlist.contains(channel_id);
        }
        true
    }
}

pub struct ChannelAccessManager {
    policies: std::collections::HashMap<String, ChannelPolicy>,
    default_policy: ChannelPolicy,
}

impl ChannelAccessManager {
    pub fn new() -> Self {
        Self {
            policies: std::collections::HashMap::new(),
            default_policy: ChannelPolicy::default(),
        }
    }

    pub fn set_policy(&mut self, channel_id: &str, policy: ChannelPolicy) {
        self.policies.insert(channel_id.to_string(), policy);
    }

    pub fn set_default_policy(&mut self, policy: ChannelPolicy) {
        self.default_policy = policy;
    }

    pub fn check_access(&self, channel_id: &str) -> bool {
        let policy = self.policies.get(channel_id).unwrap_or(&self.default_policy);
        policy.is_allowed(channel_id)
    }

    pub fn get_model_override(&self, channel_id: &str) -> Option<&str> {
        self.policies
            .get(channel_id)
            .and_then(|p| p.model_override.as_deref())
    }

    pub fn requires_mention(&self, channel_id: &str) -> bool {
        self.policies
            .get(channel_id)
            .map(|p| p.require_mention)
            .unwrap_or(self.default_policy.require_mention)
    }
}

impl Default for ChannelAccessManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_policy_allows_all() {
        let policy = ChannelPolicy::default();
        assert!(policy.is_allowed("any-channel"));
    }

    #[test]
    fn test_blocklist_denies() {
        let policy = ChannelPolicy {
            blocklist: Some(HashSet::from(["blocked".to_string()])),
            ..Default::default()
        };
        assert!(!policy.is_allowed("blocked"));
        assert!(policy.is_allowed("other"));
    }

    #[test]
    fn test_allowlist_restricts() {
        let policy = ChannelPolicy {
            allowlist: Some(HashSet::from(["allowed".to_string()])),
            ..Default::default()
        };
        assert!(policy.is_allowed("allowed"));
        assert!(!policy.is_allowed("other"));
    }

    #[test]
    fn test_blocklist_takes_precedence() {
        let policy = ChannelPolicy {
            allowlist: Some(HashSet::from(["ch1".to_string()])),
            blocklist: Some(HashSet::from(["ch1".to_string()])),
            ..Default::default()
        };
        assert!(!policy.is_allowed("ch1"));
    }

    #[test]
    fn test_manager_per_channel_policy() {
        let mut mgr = ChannelAccessManager::new();
        mgr.set_policy("ch1", ChannelPolicy {
            blocklist: Some(HashSet::from(["ch1".to_string()])),
            ..Default::default()
        });
        assert!(!mgr.check_access("ch1"));
        assert!(mgr.check_access("ch2"));
    }

    #[test]
    fn test_manager_model_override() {
        let mut mgr = ChannelAccessManager::new();
        mgr.set_policy("ch1", ChannelPolicy {
            model_override: Some("gpt-4".to_string()),
            ..Default::default()
        });
        assert_eq!(mgr.get_model_override("ch1"), Some("gpt-4"));
        assert_eq!(mgr.get_model_override("ch2"), None);
    }

    #[test]
    fn test_manager_requires_mention() {
        let mut mgr = ChannelAccessManager::new();
        mgr.set_policy("ch1", ChannelPolicy {
            require_mention: true,
            ..Default::default()
        });
        assert!(mgr.requires_mention("ch1"));
        assert!(!mgr.requires_mention("ch2"));
    }
}
