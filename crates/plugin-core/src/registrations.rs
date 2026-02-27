/// Aggregate of all dynamic registrations from plugins.
use std::sync::Arc;
use tokio::sync::RwLock;

/// A tool registered by a plugin.
#[derive(Clone)]
pub struct PluginToolReg {
    pub plugin_id: String,
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub executor: Arc<dyn PluginToolExecutor>,
}

impl std::fmt::Debug for PluginToolReg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginToolReg")
            .field("plugin_id", &self.plugin_id)
            .field("name", &self.name)
            .finish()
    }
}

/// Trait for executing a plugin-registered tool.
#[async_trait::async_trait]
pub trait PluginToolExecutor: Send + Sync {
    async fn execute(&self, params: serde_json::Value) -> Result<serde_json::Value, String>;
}

/// A command registered by a plugin (e.g. /my-command).
#[derive(Debug, Clone)]
pub struct PluginCommandReg {
    pub plugin_id: String,
    pub name: String,
    pub description: String,
    pub accepts_args: bool,
    pub handler: Arc<dyn PluginCommandHandler>,
}

#[async_trait::async_trait]
pub trait PluginCommandHandler: Send + Sync + std::fmt::Debug {
    async fn handle(&self, args: &str) -> Result<String, String>;
}

/// A background service registered by a plugin.
#[derive(Debug, Clone)]
pub struct PluginServiceReg {
    pub plugin_id: String,
    pub id: String,
    pub service: Arc<dyn PluginService>,
}

#[async_trait::async_trait]
pub trait PluginService: Send + Sync + std::fmt::Debug {
    async fn start(&self) -> Result<(), String>;
    async fn stop(&self) -> Result<(), String>;
}

/// An HTTP route registered by a plugin.
#[derive(Debug, Clone)]
pub struct PluginHttpRouteReg {
    pub plugin_id: String,
    pub path: String,
    pub handler: Arc<dyn PluginHttpHandler>,
}

#[async_trait::async_trait]
pub trait PluginHttpHandler: Send + Sync + std::fmt::Debug {
    async fn handle(&self, method: &str, body: &[u8]) -> Result<(u16, String), String>;
}

/// Aggregate registry holding all plugin-contributed registrations.
#[derive(Default, Clone)]
pub struct PluginRegistrations {
    pub tools: Arc<RwLock<Vec<PluginToolReg>>>,
    pub commands: Arc<RwLock<Vec<PluginCommandReg>>>,
    pub services: Arc<RwLock<Vec<PluginServiceReg>>>,
    pub http_routes: Arc<RwLock<Vec<PluginHttpRouteReg>>>,
}

impl PluginRegistrations {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn tools_for_plugin(&self, plugin_id: &str) -> Vec<String> {
        self.tools
            .read()
            .await
            .iter()
            .filter(|t| t.plugin_id == plugin_id)
            .map(|t| t.name.clone())
            .collect()
    }

    pub async fn remove_plugin(&self, plugin_id: &str) {
        self.tools
            .write()
            .await
            .retain(|t| t.plugin_id != plugin_id);
        self.commands
            .write()
            .await
            .retain(|c| c.plugin_id != plugin_id);
        self.services
            .write()
            .await
            .retain(|s| s.plugin_id != plugin_id);
        self.http_routes
            .write()
            .await
            .retain(|r| r.plugin_id != plugin_id);
    }
}
