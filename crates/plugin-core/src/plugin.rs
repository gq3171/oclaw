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

    async fn hook_before_request(&self, _request: &str) -> Result<Option<String>, String> {
        Ok(None)
    }

    async fn hook_after_response(&self, _response: &str) -> Result<Option<String>, String> {
        Ok(None)
    }

    async fn hook_on_error(&self, _error: &str) -> Result<(), String> {
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
