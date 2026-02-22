use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::manifest::PluginManifest;
use crate::plugin::{Plugin, PluginConfig, PluginWrapper};
use crate::{PluginResult, PluginError};

pub struct PluginRegistry {
    plugins: Arc<RwLock<HashMap<String, PluginWrapper>>>,
    dependencies: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: Arc::new(RwLock::new(HashMap::new())),
            dependencies: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register(&self, plugin: impl Plugin + 'static, config: PluginConfig) -> PluginResult<String> {
        let manifest = plugin.manifest();
        let plugin_id = manifest.id.clone();

        self.check_dependencies(&manifest, &config).await?;

        let mut plugins = self.plugins.write().await;
        
        if plugins.contains_key(&plugin_id) {
            return Err(PluginError::LoadError(format!(
                "Plugin '{}' already registered",
                plugin_id
            )));
        }

        let wrapper = PluginWrapper::new(plugin, config);
        plugins.insert(plugin_id.clone(), wrapper);

        tracing::info!("Plugin registered: {}", plugin_id);
        Ok(plugin_id)
    }

    pub async fn unregister(&self, plugin_id: &str) -> PluginResult<()> {
        let mut plugins = self.plugins.write().await;
        
        if let Some(mut wrapper) = plugins.remove(plugin_id) {
            wrapper.unload().await
                .map_err(|e| PluginError::ExecutionError(e))?;
            tracing::info!("Plugin unregistered: {}", plugin_id);
            Ok(())
        } else {
            Err(PluginError::NotFound(format!(
                "Plugin '{}' not found",
                plugin_id
            )))
        }
    }

    pub async fn get_manifest(&self, plugin_id: &str) -> Option<PluginManifest> {
        self.plugins.read()
            .await
            .get(plugin_id)
            .map(|p| p.manifest().clone())
    }

    pub async fn list(&self) -> Vec<String> {
        self.plugins.read().await.keys().cloned().collect()
    }

    pub async fn list_by_tag(&self, tag: &str) -> Vec<String> {
        self.plugins.read()
            .await
            .values()
            .filter(|p| p.manifest().tags.contains(&tag.to_string()))
            .map(|p| p.manifest().id.clone())
            .collect()
    }

    pub async fn list_by_capability(&self, capability: &str) -> Vec<String> {
        self.plugins.read()
            .await
            .values()
            .filter(|p| p.manifest().capabilities.contains(&capability.to_string()))
            .map(|p| p.manifest().id.clone())
            .collect()
    }

    pub async fn initialize(&self, plugin_id: &str) -> PluginResult<()> {
        let mut plugins = self.plugins.write().await;
        
        if let Some(wrapper) = plugins.get_mut(plugin_id) {
            wrapper.initialize().await
                .map_err(|e| PluginError::InitError(e))?;
            tracing::info!("Plugin initialized: {}", plugin_id);
            Ok(())
        } else {
            Err(PluginError::NotFound(format!(
                "Plugin '{}' not found",
                plugin_id
            )))
        }
    }

    pub async fn start(&self, plugin_id: &str) -> PluginResult<()> {
        let mut plugins = self.plugins.write().await;
        
        if let Some(wrapper) = plugins.get_mut(plugin_id) {
            wrapper.start().await
                .map_err(|e| PluginError::ExecutionError(e))?;
            tracing::info!("Plugin started: {}", plugin_id);
            Ok(())
        } else {
            Err(PluginError::NotFound(format!(
                "Plugin '{}' not found",
                plugin_id
            )))
        }
    }

    pub async fn stop(&self, plugin_id: &str) -> PluginResult<()> {
        let mut plugins = self.plugins.write().await;
        
        if let Some(wrapper) = plugins.get_mut(plugin_id) {
            wrapper.stop().await
                .map_err(|e| PluginError::ExecutionError(e))?;
            tracing::info!("Plugin stopped: {}", plugin_id);
            Ok(())
        } else {
            Err(PluginError::NotFound(format!(
                "Plugin '{}' not found",
                plugin_id
            )))
        }
    }

    pub async fn start_all(&self) -> PluginResult<()> {
        let plugin_ids: Vec<String> = self.list().await;
        
        for plugin_id in plugin_ids {
            if let Err(e) = self.start(&plugin_id).await {
                tracing::warn!("Failed to start plugin '{}': {}", plugin_id, e);
            }
        }
        
        Ok(())
    }

    pub async fn stop_all(&self) -> PluginResult<()> {
        let plugin_ids: Vec<String> = self.list().await;
        
        for plugin_id in plugin_ids {
            if let Err(e) = self.stop(&plugin_id).await {
                tracing::warn!("Failed to stop plugin '{}': {}", plugin_id, e);
            }
        }
        
        Ok(())
    }

    pub async fn update_config(&self, plugin_id: &str, config: PluginConfig) -> PluginResult<()> {
        let mut plugins = self.plugins.write().await;
        
        if let Some(wrapper) = plugins.get_mut(plugin_id) {
            wrapper.update_config(config).await
                .map_err(|e| PluginError::ConfigError(e))?;
            tracing::info!("Plugin config updated: {}", plugin_id);
            Ok(())
        } else {
            Err(PluginError::NotFound(format!(
                "Plugin '{}' not found",
                plugin_id
            )))
        }
    }

    pub async fn is_registered(&self, plugin_id: &str) -> bool {
        self.plugins.read().await.contains_key(plugin_id)
    }

    pub async fn count(&self) -> usize {
        self.plugins.read().await.len()
    }

    async fn check_dependencies(&self, manifest: &PluginManifest, _config: &PluginConfig) -> PluginResult<()> {
        if manifest.dependencies.is_empty() {
            return Ok(());
        }

        let plugins = self.plugins.read().await;
        
        for (dep_id, _version) in &manifest.dependencies {
            if !plugins.contains_key(dep_id) {
                return Err(PluginError::DependencyError(format!(
                    "Missing dependency '{}' for plugin '{}'",
                    dep_id, manifest.id
                )));
            }
        }
        
        Ok(())
    }

    pub async fn add_dependency(&self, plugin_id: String, dependency: String) {
        let mut deps = self.dependencies.write().await;
        deps.entry(plugin_id).or_insert_with(Vec::new).push(dependency);
    }

    pub async fn get_dependencies(&self, plugin_id: &str) -> Vec<String> {
        self.dependencies.read().await.get(plugin_id).cloned().unwrap_or_default()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub struct PluginManager {
    registry: PluginRegistry,
}

impl PluginManager {
    pub fn new() -> Self {
        Self {
            registry: PluginRegistry::new(),
        }
    }

    pub fn registry(&self) -> &PluginRegistry {
        &self.registry
    }

    pub async fn load_and_start(&self, plugin: impl Plugin + 'static) -> PluginResult<String> {
        let config = PluginConfig::default();
        let plugin_id = self.registry.register(plugin, config).await?;
        self.registry.initialize(&plugin_id).await?;
        self.registry.start(&plugin_id).await?;
        Ok(plugin_id)
    }

    pub async fn shutdown(&self) -> PluginResult<()> {
        self.registry.stop_all().await
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}
