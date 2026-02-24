use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::manifest::PluginManifest;
use crate::plugin::{Plugin, PluginConfig, PluginWrapper};
use crate::{PluginResult, PluginError};

pub struct PluginRegistry {
    pub(crate) plugins: Arc<RwLock<HashMap<String, PluginWrapper>>>,
    dependencies: Arc<RwLock<HashMap<String, Vec<String>>>>,
    allow: Arc<RwLock<Option<Vec<String>>>>,
    deny: Arc<RwLock<Vec<String>>>,
}

impl PluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: Arc::new(RwLock::new(HashMap::new())),
            dependencies: Arc::new(RwLock::new(HashMap::new())),
            allow: Arc::new(RwLock::new(None)),
            deny: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Set allow/deny lists for plugin access control.
    pub async fn set_access_control(&self, allow: Option<Vec<String>>, deny: Vec<String>) {
        *self.allow.write().await = allow;
        *self.deny.write().await = deny;
    }

    pub async fn register(&self, plugin: impl Plugin + 'static, config: PluginConfig) -> PluginResult<String> {
        let manifest = plugin.manifest();
        let plugin_id = manifest.id.clone();

        // Allow/deny check
        let deny = self.deny.read().await;
        if deny.contains(&plugin_id) {
            return Err(PluginError::LoadError(format!("Plugin '{}' is denied", plugin_id)));
        }
        drop(deny);
        let allow = self.allow.read().await;
        if let Some(ref list) = *allow {
            if !list.contains(&plugin_id) {
                return Err(PluginError::LoadError(format!("Plugin '{}' not in allow list", plugin_id)));
            }
        }
        drop(allow);

        self.check_dependencies(manifest, &config).await?;

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
                .map_err(PluginError::ExecutionError)?;
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
                .map_err(PluginError::InitError)?;
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
                .map_err(PluginError::ExecutionError)?;
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
                .map_err(PluginError::ExecutionError)?;
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
                .map_err(PluginError::ConfigError)?;
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
        
        for dep_id in manifest.dependencies.keys() {
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

/// Hook execution pipeline. Runs hooks across all active plugins in priority order.
pub struct HookPipeline {
    registry: Arc<RwLock<HashMap<String, PluginWrapper>>>,
}

impl HookPipeline {
    pub fn from_registry(registry: &PluginRegistry) -> Self {
        Self {
            registry: registry.plugins.clone(),
        }
    }

    /// Run a transforming hook. Each plugin can optionally transform the value.
    async fn run_transform<F>(&self, initial: &str, hook_fn: F) -> Result<String, String>
    where
        F: for<'a> Fn(&'a PluginWrapper, String) -> Pin<Box<dyn std::future::Future<Output = Result<Option<String>, String>> + Send + 'a>>,
    {
        let plugins = self.registry.read().await;
        let mut sorted: Vec<&PluginWrapper> = plugins.values()
            .filter(|p| p.config().enabled)
            .collect();
        sorted.sort_by_key(|p| std::cmp::Reverse(p.config().priority));

        let mut value = initial.to_string();
        for plugin in sorted {
            match hook_fn(plugin, value.clone()).await {
                Ok(Some(transformed)) => value = transformed,
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!("Hook error in plugin {}: {}", plugin.manifest().id, e);
                }
            }
        }
        Ok(value)
    }

    /// Run a notification hook (no return value transformation).
    async fn run_notify<F>(&self, hook_fn: F)
    where
        F: for<'a> Fn(&'a PluginWrapper) -> Pin<Box<dyn std::future::Future<Output = Result<(), String>> + Send + 'a>>,
    {
        let plugins = self.registry.read().await;
        for plugin in plugins.values().filter(|p| p.config().enabled) {
            if let Err(e) = hook_fn(plugin).await {
                tracing::warn!("Hook error in plugin {}: {}", plugin.manifest().id, e);
            }
        }
    }

    // --- Public hook execution methods ---

    pub async fn before_request(&self, request: &str) -> Result<String, String> {
        self.run_transform(request, |p, v| Box::pin(async move {
            p.inner().hook_before_request(&v).await
        })).await
    }

    pub async fn after_response(&self, response: &str) -> Result<String, String> {
        self.run_transform(response, |p, v| Box::pin(async move {
            p.inner().hook_after_response(&v).await
        })).await
    }

    pub async fn before_tool_call(&self, tool: &str, args: &str) -> Result<String, String> {
        let t = tool.to_string();
        self.run_transform(args, move |p, v| {
            let t = t.clone();
            Box::pin(async move { p.inner().hook_before_tool_call(&t, &v).await })
        }).await
    }

    pub async fn after_tool_call(&self, tool: &str, result: &str) -> Result<String, String> {
        let t = tool.to_string();
        self.run_transform(result, move |p, v| {
            let t = t.clone();
            Box::pin(async move { p.inner().hook_after_tool_call(&t, &v).await })
        }).await
    }

    pub async fn before_message(&self, message: &str) -> Result<String, String> {
        self.run_transform(message, |p, v| Box::pin(async move {
            p.inner().hook_before_message(&v).await
        })).await
    }

    pub async fn after_message(&self, message: &str) -> Result<String, String> {
        self.run_transform(message, |p, v| Box::pin(async move {
            p.inner().hook_after_message(&v).await
        })).await
    }

    pub async fn before_llm_call(&self, model: &str, payload: &str) -> Result<String, String> {
        let m = model.to_string();
        self.run_transform(payload, move |p, v| {
            let m = m.clone();
            Box::pin(async move { p.inner().hook_before_llm_call(&m, &v).await })
        }).await
    }

    pub async fn after_llm_call(&self, model: &str, response: &str) -> Result<String, String> {
        let m = model.to_string();
        self.run_transform(response, move |p, v| {
            let m = m.clone();
            Box::pin(async move { p.inner().hook_after_llm_call(&m, &v).await })
        }).await
    }

    pub async fn content_filter(&self, content: &str) -> Result<String, String> {
        self.run_transform(content, |p, v| Box::pin(async move {
            p.inner().hook_content_filter(&v).await
        })).await
    }

    pub async fn on_error(&self, error: &str) {
        let e = error.to_string();
        self.run_notify(move |p| {
            let err = e.clone();
            Box::pin(async move { p.inner().hook_on_error(&err).await })
        }).await;
    }

    pub async fn session_start(&self, session_id: &str) {
        let sid = session_id.to_string();
        self.run_notify(move |p| {
            let s = sid.clone();
            Box::pin(async move { p.inner().hook_session_start(&s).await })
        }).await;
    }

    pub async fn session_end(&self, session_id: &str) {
        let sid = session_id.to_string();
        self.run_notify(move |p| {
            let s = sid.clone();
            Box::pin(async move { p.inner().hook_session_end(&s).await })
        }).await;
    }

    pub async fn gateway_startup(&self) {
        self.run_notify(|p| Box::pin(async move { p.inner().hook_gateway_startup().await })).await;
    }

    pub async fn gateway_shutdown(&self) {
        self.run_notify(|p| Box::pin(async move { p.inner().hook_gateway_shutdown().await })).await;
    }

    pub async fn agent_spawn(&self, agent_id: &str, config: &str) {
        let aid = agent_id.to_string();
        let cfg = config.to_string();
        self.run_notify(move |p| {
            let a = aid.clone();
            let c = cfg.clone();
            Box::pin(async move { p.inner().hook_agent_spawn(&a, &c).await })
        }).await;
    }

    pub async fn tool_denied(&self, tool: &str, reason: &str) {
        let t = tool.to_string();
        let r = reason.to_string();
        self.run_notify(move |p| {
            let t = t.clone();
            let r = r.clone();
            Box::pin(async move { p.inner().hook_tool_denied(&t, &r).await })
        }).await;
    }

    pub async fn auth_attempt(&self, user: &str, success: bool) {
        let u = user.to_string();
        self.run_notify(move |p| {
            let u = u.clone();
            Box::pin(async move { p.inner().hook_auth_attempt(&u, success).await })
        }).await;
    }

    pub async fn agent_complete(&self, agent_id: &str, result: &str) {
        let aid = agent_id.to_string();
        let res = result.to_string();
        self.run_notify(move |p| {
            let a = aid.clone();
            let r = res.clone();
            Box::pin(async move { p.inner().hook_agent_complete(&a, &r).await })
        }).await;
    }

    pub async fn before_compaction(&self, messages: &str) -> Result<String, String> {
        self.run_transform(messages, |p, v| Box::pin(async move {
            p.inner().hook_before_compaction(&v).await
        })).await
    }

    pub async fn after_compaction(&self, summary: &str) -> Result<String, String> {
        self.run_transform(summary, |p, v| Box::pin(async move {
            p.inner().hook_after_compaction(&v).await
        })).await
    }

    pub async fn subagent_spawning(&self, agent_id: &str, config: &str) -> Result<String, String> {
        let aid = agent_id.to_string();
        self.run_transform(config, move |p, v| {
            let a = aid.clone();
            Box::pin(async move { p.inner().hook_subagent_spawning(&a, &v).await })
        }).await
    }

    pub async fn subagent_spawned(&self, agent_id: &str) {
        let aid = agent_id.to_string();
        self.run_notify(move |p| {
            let a = aid.clone();
            Box::pin(async move { p.inner().hook_subagent_spawned(&a).await })
        }).await;
    }

    pub async fn subagent_ended(&self, agent_id: &str, result: &str) {
        let aid = agent_id.to_string();
        let res = result.to_string();
        self.run_notify(move |p| {
            let a = aid.clone();
            let r = res.clone();
            Box::pin(async move { p.inner().hook_subagent_ended(&a, &r).await })
        }).await;
    }

    pub async fn before_reset(&self, session_id: &str) {
        let sid = session_id.to_string();
        self.run_notify(move |p| {
            let s = sid.clone();
            Box::pin(async move { p.inner().hook_before_reset(&s).await })
        }).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::PluginManifest;
    use crate::plugin::{BasePlugin, PluginConfig};

    fn test_manifest(id: &str) -> PluginManifest {
        PluginManifest {
            id: id.to_string(),
            name: id.to_string(),
            version: "1.0.0".to_string(),
            description: None,
            author: None,
            homepage: None,
            repository: None,
            license: None,
            entry_point: "main".to_string(),
            dependencies: Default::default(),
            optional_dependencies: Default::default(),
            tags: vec![],
            capabilities: vec![],
            hooks: vec![],
            platform: None,
            builtin: false,
            kind: None,
            config_schema: None,
            ui_hints: Default::default(),
            channels: vec![],
            providers: vec![],
            skills: vec![],
        }
    }

    #[tokio::test]
    async fn test_register_and_list() {
        let reg = PluginRegistry::new();
        let plugin = BasePlugin::new(test_manifest("p1"));
        reg.register(plugin, PluginConfig::default()).await.unwrap();
        let list = reg.list().await;
        assert_eq!(list.len(), 1);
        assert!(list.contains(&"p1".to_string()));
    }

    #[tokio::test]
    async fn test_duplicate_register_fails() {
        let reg = PluginRegistry::new();
        let p1 = BasePlugin::new(test_manifest("dup"));
        reg.register(p1, PluginConfig::default()).await.unwrap();
        let p2 = BasePlugin::new(test_manifest("dup"));
        assert!(reg.register(p2, PluginConfig::default()).await.is_err());
    }

    #[tokio::test]
    async fn test_unregister() {
        let reg = PluginRegistry::new();
        let plugin = BasePlugin::new(test_manifest("rm"));
        reg.register(plugin, PluginConfig::default()).await.unwrap();
        reg.unregister("rm").await.unwrap();
        assert!(!reg.is_registered("rm").await);
    }

    #[tokio::test]
    async fn test_unregister_missing_fails() {
        let reg = PluginRegistry::new();
        assert!(reg.unregister("nope").await.is_err());
    }

    #[tokio::test]
    async fn test_count() {
        let reg = PluginRegistry::new();
        assert_eq!(reg.count().await, 0);
        let p = BasePlugin::new(test_manifest("c1"));
        reg.register(p, PluginConfig::default()).await.unwrap();
        assert_eq!(reg.count().await, 1);
    }
}
