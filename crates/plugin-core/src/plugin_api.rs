/// PluginApi — the dynamic registration interface given to plugins.
/// Equivalent to Node's OpenClawPluginApi.
use crate::registrations::*;
use std::sync::Arc;

/// The API handle passed to a plugin during activation.
pub struct PluginApi {
    pub id: String,
    pub name: String,
    pub version: String,
    regs: PluginRegistrations,
}

impl PluginApi {
    pub fn new(id: &str, name: &str, version: &str, regs: PluginRegistrations) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            version: version.to_string(),
            regs,
        }
    }

    pub async fn register_tool(
        &self,
        name: &str,
        description: &str,
        schema: serde_json::Value,
        executor: Arc<dyn PluginToolExecutor>,
    ) {
        self.regs.tools.write().await.push(PluginToolReg {
            plugin_id: self.id.clone(),
            name: name.to_string(),
            description: description.to_string(),
            input_schema: schema,
            executor,
        });
    }

    pub async fn register_command(
        &self,
        name: &str,
        description: &str,
        accepts_args: bool,
        handler: Arc<dyn PluginCommandHandler>,
    ) {
        self.regs.commands.write().await.push(PluginCommandReg {
            plugin_id: self.id.clone(),
            name: name.to_string(),
            description: description.to_string(),
            accepts_args,
            handler,
        });
    }

    pub async fn register_service(&self, id: &str, service: Arc<dyn PluginService>) {
        self.regs.services.write().await.push(PluginServiceReg {
            plugin_id: self.id.clone(),
            id: id.to_string(),
            service,
        });
    }

    pub async fn register_http_route(&self, path: &str, handler: Arc<dyn PluginHttpHandler>) {
        self.regs
            .http_routes
            .write()
            .await
            .push(PluginHttpRouteReg {
                plugin_id: self.id.clone(),
                path: path.to_string(),
                handler,
            });
    }
}
