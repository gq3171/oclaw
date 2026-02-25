pub mod plugin;
pub mod loader;
pub mod registry;
pub mod manifest;
pub mod registrations;
pub mod plugin_api;
pub mod hook_strategy;
pub mod discovery;

pub use plugin::{Plugin, PluginState, PluginConfig};
pub use loader::PluginLoader;
pub use registry::{PluginRegistry, PluginManager, HookPipeline};
pub use manifest::PluginManifest;
pub use registrations::PluginRegistrations;
pub use plugin_api::PluginApi;
pub use hook_strategy::{HookStrategy, MergeStrategy, HookExecutorConfig, json_merge};
pub use discovery::{PluginDiscovery, DiscoveredPlugin};

pub type PluginResult<T> = Result<T, PluginError>;

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("Load error: {0}")]
    LoadError(String),
    
    #[error("Initialization error: {0}")]
    InitError(String),
    
    #[error("Execution error: {0}")]
    ExecutionError(String),
    
    #[error("Configuration error: {0}")]
    ConfigError(String),
    
    #[error("Not found: {0}")]
    NotFound(String),
    
    #[error("Dependency error: {0}")]
    DependencyError(String),
    
    #[error("Version mismatch: {0}")]
    VersionMismatch(String),
}

impl serde::Serialize for PluginError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
