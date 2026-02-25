use std::sync::Arc;
use tokio::sync::RwLock;

use oclaws_channel_core::traits::ChannelMessage;
use oclaws_channel_core::ChannelManager;
use oclaws_cron_core::runner::CronExecutor;
use oclaws_cron_core::types::{CronDelivery, CronJob};
use oclaws_llm_core::providers::LlmProvider;
use oclaws_plugin_core::PluginRegistrations;
use oclaws_tools_core::tool::ToolRegistry;

use super::agent_bridge;

pub struct GatewayCronExecutor {
    provider: Arc<dyn LlmProvider>,
    tool_registry: Option<Arc<ToolRegistry>>,
    plugin_regs: Option<Arc<PluginRegistrations>>,
    channel_manager: Option<Arc<RwLock<ChannelManager>>>,
}

impl GatewayCronExecutor {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            tool_registry: None,
            plugin_regs: None,
            channel_manager: None,
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

    pub fn with_channel_manager(mut self, manager: Arc<RwLock<ChannelManager>>) -> Self {
        self.channel_manager = Some(manager);
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

        let mut executor = agent_bridge::ToolRegistryExecutor::new(registry.clone());
        if let Some(ref regs) = self.plugin_regs {
            executor = executor.with_plugin_registrations(regs.clone());
        }

        let session_id = format!("cron:{}", job.session_target);
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

    async fn deliver(
        &self,
        delivery: &CronDelivery,
        content: &str,
    ) -> Result<(), String> {
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
