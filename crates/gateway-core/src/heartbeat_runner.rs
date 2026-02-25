//! Heartbeat runner — periodic agent turns driven by workspace HEARTBEAT.md.
//!
//! Bridges `workspace-core::HeartbeatConfig` with `cron-core::CronExecutor`
//! to run context-aware periodic checks.

use std::sync::Arc;
use tracing::{info, debug};

use oclaws_workspace_core::heartbeat::{
    HeartbeatConfig, HeartbeatFile, should_drop_heartbeat_reply,
};
use oclaws_workspace_core::files::Workspace;

/// Callback trait for delivering heartbeat results to channels.
#[async_trait::async_trait]
pub trait HeartbeatDelivery: Send + Sync {
    /// Run an agent turn with the heartbeat prompt, return the reply.
    async fn run_heartbeat_turn(
        &self,
        prompt: &str,
        model: Option<&str>,
    ) -> Result<String, String>;

    /// Deliver a heartbeat alert to the target channel.
    async fn deliver_alert(
        &self,
        target: &str,
        text: &str,
    ) -> Result<(), String>;
}

/// Manages periodic heartbeat execution for an agent.
pub struct HeartbeatRunner {
    config: HeartbeatConfig,
    workspace: Arc<Workspace>,
    delivery: Arc<dyn HeartbeatDelivery>,
    last_run_ms: u64,
}

impl HeartbeatRunner {
    pub fn new(
        config: HeartbeatConfig,
        workspace: Arc<Workspace>,
        delivery: Arc<dyn HeartbeatDelivery>,
    ) -> Self {
        Self {
            config,
            workspace,
            delivery,
            last_run_ms: 0,
        }
    }

    /// Check if it's time to run a heartbeat tick.
    pub fn should_tick(&self, now_ms: u64) -> bool {
        if self.config.interval_secs == 0 {
            return false;
        }
        let interval_ms = self.config.interval_secs * 1000;
        now_ms.saturating_sub(self.last_run_ms) >= interval_ms
    }

    /// Check if current time is within active hours.
    fn is_within_active_hours(&self) -> bool {
        let Some(hours) = &self.config.active_hours else {
            return true; // no restriction
        };
        let now = chrono::Local::now().time();
        let Ok(start) = chrono::NaiveTime::parse_from_str(&hours.start, "%H:%M") else {
            return true;
        };
        let Ok(end) = chrono::NaiveTime::parse_from_str(&hours.end, "%H:%M") else {
            return true;
        };
        if start <= end {
            now >= start && now <= end
        } else {
            // Wraps midnight (e.g. 22:00 - 06:00)
            now >= start || now <= end
        }
    }

    /// Execute a single heartbeat tick.
    pub async fn tick(&mut self) -> Result<(), String> {
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;

        if !self.should_tick(now_ms) {
            return Ok(());
        }

        // Check active hours
        if !self.is_within_active_hours() {
            debug!("Heartbeat skipped: outside active hours");
            self.last_run_ms = now_ms;
            return Ok(());
        }

        // Check if HEARTBEAT.md has any tasks
        let hb_file = HeartbeatFile::load(&self.workspace).await
            .map_err(|e| e.to_string())?;
        if let Some(ref hb) = hb_file
            && !hb.has_tasks {
                debug!("Heartbeat skipped: HEARTBEAT.md is empty");
                self.last_run_ms = now_ms;
                return Ok(());
            }

        info!("Running heartbeat tick");
        let prompt = self.config.effective_prompt();

        // Run agent turn
        let reply = self.delivery
            .run_heartbeat_turn(&prompt, None)
            .await?;

        self.last_run_ms = now_ms;

        // Check if reply should be dropped (HEARTBEAT_OK ack)
        if should_drop_heartbeat_reply(&reply, self.config.ack_max_chars) {
            debug!("Heartbeat: nothing to report (HEARTBEAT_OK)");
            return Ok(());
        }

        // Deliver alert to target channel
        info!("Heartbeat alert: delivering to {}", self.config.target);
        self.delivery
            .deliver_alert(&self.config.target, &reply)
            .await?;

        Ok(())
    }
}

/// Concrete HeartbeatDelivery backed by the gateway's LLM provider and channel manager.
pub struct GatewayHeartbeatDelivery {
    provider: Arc<dyn oclaws_llm_core::providers::LlmProvider>,
    channel_manager: Option<Arc<tokio::sync::RwLock<oclaws_channel_core::ChannelManager>>>,
}

impl GatewayHeartbeatDelivery {
    pub fn new(
        provider: Arc<dyn oclaws_llm_core::providers::LlmProvider>,
        channel_manager: Option<Arc<tokio::sync::RwLock<oclaws_channel_core::ChannelManager>>>,
    ) -> Self {
        Self { provider, channel_manager }
    }
}

#[async_trait::async_trait]
impl HeartbeatDelivery for GatewayHeartbeatDelivery {
    async fn run_heartbeat_turn(
        &self,
        prompt: &str,
        _model: Option<&str>,
    ) -> Result<String, String> {
        let model = self.provider.default_model().to_string();
        let config = oclaws_agent_core::agent::AgentConfig::new(
            "heartbeat-agent", &model, "heartbeat",
        ).with_system_prompt(prompt);
        let mut agent = oclaws_agent_core::agent::Agent::new(
            config, self.provider.clone(),
        );
        agent.initialize().await.map_err(|e| e.to_string())?;
        agent.run("Run your heartbeat check now.")
            .await.map_err(|e| e.to_string())
    }

    async fn deliver_alert(
        &self,
        target: &str,
        text: &str,
    ) -> Result<(), String> {
        let Some(ref cm) = self.channel_manager else {
            info!("Heartbeat alert (no channel manager): {}", text);
            return Ok(());
        };
        let mgr = cm.read().await;
        // target format: "channel_name" or "channel_name:chat_id"
        let (channel_name, _chat_id) = target.split_once(':')
            .unwrap_or((target, ""));
        let Some(channel) = mgr.get(channel_name).await else {
            info!("Heartbeat alert (channel {} not found): {}", channel_name, text);
            return Ok(());
        };
        let msg = oclaws_channel_core::traits::ChannelMessage {
            id: uuid::Uuid::new_v4().to_string(),
            channel: channel_name.to_string(),
            sender: "heartbeat".to_string(),
            content: text.to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            metadata: std::collections::HashMap::new(),
        };
        let ch = channel.read().await;
        ch.send_message(&msg).await
            .map_err(|e| format!("Heartbeat delivery failed: {}", e))?;
        Ok(())
    }
}
