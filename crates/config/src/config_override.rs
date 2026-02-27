//! Layered config overrides — account → channel → session.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ConfigOverride {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<String>>,
}

impl ConfigOverride {
    /// Merge `other` on top of `self`. Fields in `other` take precedence.
    pub fn merge(&self, other: &ConfigOverride) -> ConfigOverride {
        ConfigOverride {
            system_prompt: other
                .system_prompt
                .clone()
                .or_else(|| self.system_prompt.clone()),
            model: other.model.clone().or_else(|| self.model.clone()),
            temperature: other.temperature.or(self.temperature),
            max_tokens: other.max_tokens.or(self.max_tokens),
            tools: other.tools.clone().or_else(|| self.tools.clone()),
        }
    }
}

/// Resolves overrides across layers: base ← account ← channel.
pub struct OverrideResolver {
    account_overrides: std::collections::HashMap<String, ConfigOverride>,
    channel_overrides: std::collections::HashMap<String, ConfigOverride>,
}

impl OverrideResolver {
    pub fn new() -> Self {
        Self {
            account_overrides: std::collections::HashMap::new(),
            channel_overrides: std::collections::HashMap::new(),
        }
    }

    pub fn set_account_override(&mut self, account_id: &str, ovr: ConfigOverride) {
        self.account_overrides.insert(account_id.to_string(), ovr);
    }

    pub fn set_channel_override(&mut self, channel: &str, ovr: ConfigOverride) {
        self.channel_overrides.insert(channel.to_string(), ovr);
    }

    /// Resolve the final override: base ← account ← channel.
    pub fn resolve(
        &self,
        base: &ConfigOverride,
        account_id: Option<&str>,
        channel: Option<&str>,
    ) -> ConfigOverride {
        let mut result = base.clone();
        if let Some(aid) = account_id
            && let Some(acct) = self.account_overrides.get(aid)
        {
            result = result.merge(acct);
        }
        if let Some(ch) = channel
            && let Some(ch_ovr) = self.channel_overrides.get(ch)
        {
            result = result.merge(ch_ovr);
        }
        result
    }
}

impl Default for OverrideResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_prefers_other() {
        let base = ConfigOverride {
            model: Some("gpt-4".into()),
            temperature: Some(0.7),
            ..Default::default()
        };
        let other = ConfigOverride {
            model: Some("claude-3".into()),
            ..Default::default()
        };
        let merged = base.merge(&other);
        assert_eq!(merged.model.as_deref(), Some("claude-3"));
        assert_eq!(merged.temperature, Some(0.7));
    }

    #[test]
    fn resolve_layers() {
        let mut resolver = OverrideResolver::new();
        resolver.set_account_override(
            "acct1",
            ConfigOverride {
                model: Some("gpt-4o".into()),
                ..Default::default()
            },
        );
        resolver.set_channel_override(
            "telegram",
            ConfigOverride {
                temperature: Some(0.3),
                ..Default::default()
            },
        );

        let base = ConfigOverride {
            model: Some("gpt-4".into()),
            temperature: Some(0.7),
            max_tokens: Some(1000),
            ..Default::default()
        };

        let result = resolver.resolve(&base, Some("acct1"), Some("telegram"));
        assert_eq!(result.model.as_deref(), Some("gpt-4o")); // account wins
        assert_eq!(result.temperature, Some(0.3)); // channel wins
        assert_eq!(result.max_tokens, Some(1000)); // base preserved
    }

    #[test]
    fn resolve_no_overrides() {
        let resolver = OverrideResolver::new();
        let base = ConfigOverride {
            model: Some("base-model".into()),
            ..Default::default()
        };
        let result = resolver.resolve(&base, None, None);
        assert_eq!(result.model.as_deref(), Some("base-model"));
    }
}
