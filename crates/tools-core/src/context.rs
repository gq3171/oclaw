use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Execution context passed to every tool invocation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolContext {
    pub session_key: Option<String>,
    pub channel: Option<String>,
    pub user_id: Option<String>,
    pub agent_id: String,
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
    #[serde(default)]
    pub env_vars: HashMap<String, String>,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
    #[serde(default)]
    pub metadata: HashMap<String, serde_json::Value>,
}

fn default_timeout() -> u64 {
    30_000
}

impl Default for ToolContext {
    fn default() -> Self {
        Self {
            session_key: None,
            channel: None,
            user_id: None,
            agent_id: String::new(),
            working_dir: None,
            env_vars: HashMap::new(),
            timeout_ms: default_timeout(),
            metadata: HashMap::new(),
        }
    }
}

impl ToolContext {
    pub fn new(agent_id: &str) -> Self {
        Self {
            agent_id: agent_id.to_string(),
            ..Default::default()
        }
    }

    pub fn with_session(mut self, key: &str) -> Self {
        self.session_key = Some(key.to_string());
        self
    }

    pub fn with_channel(mut self, ch: &str) -> Self {
        self.channel = Some(ch.to_string());
        self
    }
}
