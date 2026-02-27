use crate::session_key::SessionKey;
use serde::{Deserialize, Serialize};

/// Agent configuration bound to a session via routing rules.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub system_prompt: Option<String>,
    pub tools: Option<Vec<String>>,
    pub model_override: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindingRule {
    pub priority: i32,
    pub matcher: BindingMatcher,
    pub binding: Binding,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BindingMatcher {
    Channel { channel: String },
    User { user_id: String },
    Group { channel: String, group_id: String },
    Default,
}

pub struct BindingRouter {
    rules: Vec<BindingRule>,
}

impl BindingRouter {
    pub fn new(mut rules: Vec<BindingRule>) -> Self {
        rules.sort_by(|a, b| b.priority.cmp(&a.priority));
        Self { rules }
    }

    pub fn resolve(&self, key: &SessionKey) -> Option<&Binding> {
        for rule in &self.rules {
            if Self::matches(&rule.matcher, key) {
                return Some(&rule.binding);
            }
        }
        None
    }

    fn matches(matcher: &BindingMatcher, key: &SessionKey) -> bool {
        match matcher {
            BindingMatcher::Channel { channel } => key.channel == *channel,
            BindingMatcher::User { user_id } => key.user_id.as_deref() == Some(user_id.as_str()),
            BindingMatcher::Group { channel, group_id } => {
                key.channel == *channel && key.chat_id == *group_id
            }
            BindingMatcher::Default => true,
        }
    }
}
