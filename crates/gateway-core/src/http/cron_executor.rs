use std::sync::Arc;
use tokio::sync::RwLock;

use oclaw_channel_core::ChannelManager;
use oclaw_channel_core::traits::ChannelMessage;
use oclaw_cron_core::runner::CronExecutor;
use oclaw_cron_core::types::{CronDelivery, CronJob};
use oclaw_llm_core::providers::LlmProvider;
use oclaw_plugin_core::HookPipeline;
use oclaw_plugin_core::PluginRegistrations;
use oclaw_tools_core::tool::ToolRegistry;

use crate::message::SessionManager;

use super::agent_bridge;

pub struct GatewayCronExecutor {
    provider: Arc<dyn LlmProvider>,
    tool_registry: Option<Arc<ToolRegistry>>,
    plugin_regs: Option<Arc<PluginRegistrations>>,
    hook_pipeline: Option<Arc<HookPipeline>>,
    channel_manager: Option<Arc<RwLock<ChannelManager>>>,
    session_manager: Option<Arc<RwLock<SessionManager>>>,
    full_config: Option<Arc<RwLock<oclaw_config::settings::Config>>>,
    session_usage_tokens: Option<Arc<std::sync::Mutex<std::collections::HashMap<String, u64>>>>,
    usage_snapshot: Option<Arc<RwLock<crate::http::GatewayUsageSnapshot>>>,
}

impl GatewayCronExecutor {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            tool_registry: None,
            plugin_regs: None,
            hook_pipeline: None,
            channel_manager: None,
            session_manager: None,
            full_config: None,
            session_usage_tokens: None,
            usage_snapshot: None,
        }
    }

    pub fn with_tool_registry(mut self, registry: Arc<ToolRegistry>) -> Self {
        self.tool_registry = Some(registry);
        self
    }

    pub fn with_plugin_registrations(mut self, regs: Arc<PluginRegistrations>) -> Self {
        self.plugin_regs = Some(regs);
        self
    }

    pub fn with_hook_pipeline(mut self, hooks: Arc<HookPipeline>) -> Self {
        self.hook_pipeline = Some(hooks);
        self
    }

    pub fn with_channel_manager(mut self, manager: Arc<RwLock<ChannelManager>>) -> Self {
        self.channel_manager = Some(manager);
        self
    }

    pub fn with_session_manager(mut self, manager: Arc<RwLock<SessionManager>>) -> Self {
        self.session_manager = Some(manager);
        self
    }

    pub fn with_full_config(mut self, cfg: Arc<RwLock<oclaw_config::settings::Config>>) -> Self {
        self.full_config = Some(cfg);
        self
    }

    pub fn with_session_usage_tokens(
        mut self,
        usage_tokens: Arc<std::sync::Mutex<std::collections::HashMap<String, u64>>>,
    ) -> Self {
        self.session_usage_tokens = Some(usage_tokens);
        self
    }

    pub fn with_usage_snapshot(
        mut self,
        usage_snapshot: Arc<RwLock<crate::http::GatewayUsageSnapshot>>,
    ) -> Self {
        self.usage_snapshot = Some(usage_snapshot);
        self
    }
}

#[async_trait::async_trait]
impl CronExecutor for GatewayCronExecutor {
    async fn run_agent_turn(
        &self,
        job: &CronJob,
        message: &str,
        _model: Option<&str>,
        timeout_secs: Option<u64>,
    ) -> Result<String, String> {
        let Some(ref registry) = self.tool_registry else {
            return Err("No tool registry configured".to_string());
        };

        let mut executor = agent_bridge::ToolRegistryExecutor::new(registry.clone())
            .with_llm_provider(self.provider.clone());
        if let Some(ref regs) = self.plugin_regs {
            executor = executor.with_plugin_registrations(regs.clone());
        }
        if let Some(ref hooks) = self.hook_pipeline {
            executor = executor.with_hook_pipeline(hooks.clone());
        }
        if let Some(ref cm) = self.channel_manager {
            executor = executor.with_channel_manager(cm.clone());
        }
        if let Some(ref sm) = self.session_manager {
            executor = executor.with_session_manager(sm.clone());
        }
        if let Some(ref cfg) = self.full_config {
            executor = executor.with_full_config(cfg.clone());
        }
        if let Some(ref usage_tokens) = self.session_usage_tokens {
            executor = executor.with_session_usage_tokens(usage_tokens.clone());
        }
        if let Some(ref snapshot) = self.usage_snapshot {
            executor = executor.with_usage_snapshot(snapshot.clone());
        }

        let session_id = format!("cron:{}", job.session_target);
        executor = executor.with_session_id(session_id.clone());
        let timeout = std::time::Duration::from_secs(timeout_secs.unwrap_or(300));

        let fut = agent_bridge::agent_reply_with_session(
            &self.provider,
            &executor,
            message,
            Some(&session_id),
        );

        match tokio::time::timeout(timeout, fut).await {
            Ok(result) => result,
            Err(_) => Err("agent turn timed out".to_string()),
        }
    }

    async fn deliver(&self, delivery: &CronDelivery, content: &str) -> Result<(), String> {
        let Some(ref cm) = self.channel_manager else {
            return Err("No channel manager configured".to_string());
        };

        let mgr = cm.read().await;
        let msg = ChannelMessage {
            id: uuid::Uuid::new_v4().to_string(),
            channel: delivery.channel.clone(),
            sender: "cron".to_string(),
            content: content.to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            metadata: {
                let mut m = std::collections::HashMap::new();
                m.insert("target".to_string(), delivery.target.clone());
                if let Some(ref tt) = delivery.target_type {
                    m.insert("target_type".to_string(), tt.clone());
                }
                m
            },
        };

        mgr.send_to_channel(&delivery.channel, &msg)
            .await
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
}
