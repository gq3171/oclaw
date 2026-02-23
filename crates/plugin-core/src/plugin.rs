use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::manifest::PluginManifest;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginState {
    Unloaded,
    Loading,
    Loaded,
    Initializing,
    Active,
    Paused,
    Stopping,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginConfig {
    pub enabled: bool,
    pub priority: i32,
    pub timeout_ms: u64,
    pub retry_on_error: bool,
    pub max_retries: u32,
    pub settings: HashMap<String, serde_json::Value>,
}

impl Default for PluginConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            priority: 0,
            timeout_ms: 30000,
            retry_on_error: true,
            max_retries: 3,
            settings: HashMap::new(),
        }
    }
}

impl PluginConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_setting<T: serde::Serialize>(mut self, key: &str, value: T) -> Self {
        if let Ok(v) = serde_json::to_value(value) {
            self.settings.insert(key.to_string(), v);
        }
        self
    }
}

#[async_trait]
pub trait Plugin: Send + Sync {
    fn manifest(&self) -> &PluginManifest;
    
    fn state(&self) -> PluginState;

    async fn on_load(&self) -> Result<(), String> {
        Ok(())
    }

    async fn on_init(&self, _config: &PluginConfig) -> Result<(), String> {
        Ok(())
    }

    async fn on_start(&self) -> Result<(), String> {
        Ok(())
    }

    async fn on_stop(&self) -> Result<(), String> {
        Ok(())
    }

    async fn on_unload(&self) -> Result<(), String> {
        Ok(())
    }

    async fn on_config_changed(&self, _config: &PluginConfig) -> Result<(), String> {
        Ok(())
    }

    async fn on_enable(&self) -> Result<(), String> {
        Ok(())
    }

    async fn on_disable(&self) -> Result<(), String> {
        Ok(())
    }

    // --- Request/Response hooks ---
    async fn hook_before_request(&self, _request: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    async fn hook_after_response(&self, _response: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    async fn hook_on_error(&self, _error: &str) -> Result<(), String> {
        Ok(())
    }

    // --- Tool hooks ---
    async fn hook_before_tool_call(&self, _tool: &str, _args: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    async fn hook_after_tool_call(&self, _tool: &str, _result: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    async fn hook_tool_denied(&self, _tool: &str, _reason: &str) -> Result<(), String> {
        Ok(())
    }

    // --- Session hooks ---
    async fn hook_session_start(&self, _session_id: &str) -> Result<(), String> {
        Ok(())
    }
    async fn hook_session_end(&self, _session_id: &str) -> Result<(), String> {
        Ok(())
    }

    // --- Message hooks ---
    async fn hook_before_message(&self, _message: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    async fn hook_after_message(&self, _message: &str) -> Result<Option<String>, String> {
        Ok(None)
    }

    // --- Agent hooks ---
    async fn hook_agent_spawn(&self, _agent_id: &str, _config: &str) -> Result<(), String> {
        Ok(())
    }
    async fn hook_agent_complete(&self, _agent_id: &str, _result: &str) -> Result<(), String> {
        Ok(())
    }

    // --- Security hooks ---
    async fn hook_auth_attempt(&self, _user: &str, _success: bool) -> Result<(), String> {
        Ok(())
    }
    async fn hook_content_filter(&self, _content: &str) -> Result<Option<String>, String> {
        Ok(None)
    }

    // --- LLM hooks ---
    async fn hook_before_llm_call(&self, _model: &str, _payload: &str) -> Result<Option<String>, String> {
        Ok(None)
    }
    async fn hook_after_llm_call(&self, _model: &str, _response: &str) -> Result<Option<String>, String> {
        Ok(None)
    }

    // --- Gateway hooks ---
    async fn hook_gateway_startup(&self) -> Result<(), String> {
        Ok(())
    }
    async fn hook_gateway_shutdown(&self) -> Result<(), String> {
        Ok(())
    }
}

pub struct PluginWrapper {
    inner: Box<dyn Plugin>,
    config: PluginConfig,
}

impl PluginWrapper {
    pub fn new(plugin: impl Plugin + 'static, config: PluginConfig) -> Self {
        Self {
            inner: Box::new(plugin),
            config,
        }
    }

    pub fn inner(&self) -> &dyn Plugin {
        &*self.inner
    }

    pub fn manifest(&self) -> &PluginManifest {
        self.inner.manifest()
    }

    pub fn config(&self) -> &PluginConfig {
        &self.config
    }

    pub fn config_mut(&mut self) -> &mut PluginConfig {
        &mut self.config
    }

    pub async fn initialize(&mut self) -> Result<(), String> {
        self.inner.on_init(&self.config).await
    }

    pub async fn start(&mut self) -> Result<(), String> {
        self.inner.on_start().await
    }

    pub async fn stop(&mut self) -> Result<(), String> {
        self.inner.on_stop().await
    }

    pub async fn unload(&mut self) -> Result<(), String> {
        self.inner.on_unload().await
    }

    pub async fn update_config(&mut self, config: PluginConfig) -> Result<(), String> {
        self.inner.on_config_changed(&config).await?;
        self.config = config;
        Ok(())
    }
}

pub struct BasePlugin {
    manifest: PluginManifest,
    state: PluginState,
}

impl BasePlugin {
    pub fn new(manifest: PluginManifest) -> Self {
        Self {
            manifest,
            state: PluginState::Unloaded,
        }
    }

    pub fn with_state(mut self, state: PluginState) -> Self {
        self.state = state;
        self
    }

    pub fn set_state(&mut self, state: PluginState) {
        self.state = state;
    }
}

#[async_trait]
impl Plugin for BasePlugin {
    fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    fn state(&self) -> PluginState {
        self.state
    }
}
