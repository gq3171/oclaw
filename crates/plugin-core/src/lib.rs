pub mod discovery;
pub mod hook_strategy;
pub mod loader;
pub mod manifest;
pub mod plugin;
pub mod plugin_api;
pub mod registrations;
pub mod registry;

pub use discovery::{DiscoveredPlugin, PluginDiscovery};
pub use hook_strategy::{HookExecutorConfig, HookStrategy, MergeStrategy, json_merge};
pub use loader::PluginLoader;
pub use manifest::PluginManifest;
pub use plugin::{Plugin, PluginConfig, PluginState};
pub use plugin_api::PluginApi;
pub use registrations::PluginRegistrations;
pub use registry::{HookPipeline, PluginManager, PluginRegistry};

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
