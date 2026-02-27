//! Heartbeat runner — periodic agent turns driven by workspace HEARTBEAT.md.
//!
//! Bridges `workspace-core::HeartbeatConfig` with `cron-core::CronExecutor`
//! to run context-aware periodic checks.

use std::sync::Arc;
use tracing::{debug, info};

use oclaw_workspace_core::files::Workspace;
use oclaw_workspace_core::heartbeat::{
    HeartbeatConfig, HeartbeatFile, should_drop_heartbeat_reply,
};

const EXEC_EVENT_PROMPT: &str = "An async command you ran earlier has completed. The result is shown in the system messages above. \
Please relay the command output to the user in a helpful way. If the command succeeded, share the relevant output. \
If it failed, explain what went wrong.";

fn resolve_heartbeat_reason_kind(reason: Option<&str>) -> &'static str {
    let trimmed = reason.map(str::trim).unwrap_or_default();
    if trimmed.eq_ignore_ascii_case("retry") {
        return "retry";
    }
    if trimmed.eq_ignore_ascii_case("interval") {
        return "interval";
    }
    if trimmed.eq_ignore_ascii_case("manual") {
        return "manual";
    }
    if trimmed.eq_ignore_ascii_case("exec-event") {
        return "exec-event";
    }
    if trimmed.eq_ignore_ascii_case("wake") {
        return "wake";
    }
    let lowered = trimmed.to_ascii_lowercase();
    if lowered.starts_with("cron:") {
        return "cron";
    }
    if lowered.starts_with("hook:") {
        return "hook";
    }
    "other"
}

fn is_exec_completion_event(evt: &str) -> bool {
    evt.to_ascii_lowercase().contains("exec finished")
}

fn is_heartbeat_ack_event(evt: &str) -> bool {
    let trimmed = evt.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    let prefix = "heartbeat_ok";
    if !lower.starts_with(prefix) {
        return false;
    }
    let suffix = &lower[prefix.len()..];
    if suffix.is_empty() {
        return true;
    }
    !suffix
        .chars()
        .next()
        .map(|c| c.is_ascii_alphanumeric() || c == '_')
        .unwrap_or(false)
}

fn is_heartbeat_noise_event(evt: &str) -> bool {
    let lower = evt.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return false;
    }
    is_heartbeat_ack_event(&lower)
        || lower.contains("heartbeat poll")
        || lower.contains("heartbeat wake")
}

fn is_cron_system_event(evt: &str) -> bool {
    if evt.trim().is_empty() {
        return false;
    }
    !is_heartbeat_noise_event(evt) && !is_exec_completion_event(evt)
}

fn build_cron_event_prompt(pending_events: &[String]) -> String {
    let event_text = pending_events
        .iter()
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    if event_text.is_empty() {
        return "A scheduled cron event was triggered, but no event content was found. Reply HEARTBEAT_OK."
            .to_string();
    }
    format!(
        "A scheduled reminder has been triggered. The reminder content is:\n\n{}\n\nPlease relay this reminder to the user in a helpful and friendly way.",
        event_text
    )
}

/// Callback trait for delivering heartbeat results to channels.
#[async_trait::async_trait]
pub trait HeartbeatDelivery: Send + Sync {
    /// Run an agent turn with the heartbeat prompt, return the reply.
    async fn run_heartbeat_turn(&self, prompt: &str, model: Option<&str>)
    -> Result<String, String>;

    /// Deliver a heartbeat alert to the target channel.
    async fn deliver_alert(&self, target: &str, text: &str) -> Result<(), String>;
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

    fn should_bypass_empty_heartbeat_file(reason: Option<&str>) -> bool {
        let Some(reason) = reason.map(str::trim).filter(|s| !s.is_empty()) else {
            return false;
        };
        matches!(
            resolve_heartbeat_reason_kind(Some(reason)),
            "wake" | "exec-event" | "cron" | "hook"
        )
    }

    /// Execute a heartbeat tick.
    /// `force=true` bypasses interval gating; active-hours gating still applies.
    pub async fn tick_with_options(
        &mut self,
        force: bool,
        reason: Option<&str>,
        system_events: &[String],
    ) -> Result<(), String> {
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;

        if !force && !self.should_tick(now_ms) {
            return Ok(());
        }

        // Check active hours
        if !self.is_within_active_hours() {
            debug!("Heartbeat skipped: outside active hours");
            self.last_run_ms = now_ms;
            return Ok(());
        }

        let bypass_empty_gate = Self::should_bypass_empty_heartbeat_file(reason);
        if !bypass_empty_gate {
            // Check if HEARTBEAT.md has any tasks
            let hb_file = HeartbeatFile::load(&self.workspace)
                .await
                .map_err(|e| e.to_string())?;
            if let Some(ref hb) = hb_file
                && !hb.has_tasks
            {
                debug!("Heartbeat skipped: HEARTBEAT.md is empty");
                self.last_run_ms = now_ms;
                return Ok(());
            }
        }

        info!("Running heartbeat tick");
        let reason_kind = resolve_heartbeat_reason_kind(reason);
        let clean_events: Vec<String> = system_events
            .iter()
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
            .map(ToString::to_string)
            .collect();
        let has_exec_completion = clean_events.iter().any(|evt| is_exec_completion_event(evt));
        let cron_events: Vec<String> = clean_events
            .iter()
            .filter(|evt| reason_kind == "cron" && is_cron_system_event(evt))
            .cloned()
            .collect();

        let mut prompt = if has_exec_completion {
            EXEC_EVENT_PROMPT.to_string()
        } else if !cron_events.is_empty() {
            build_cron_event_prompt(&cron_events)
        } else {
            self.config.effective_prompt()
        };

        if !clean_events.is_empty() && !has_exec_completion && cron_events.is_empty() {
            prompt.push_str("\n\nSystem events queued for this wake:\n");
            for event in &clean_events {
                prompt.push_str("- ");
                prompt.push_str(event);
                prompt.push('\n');
            }
        }

        // Run agent turn
        let reply = self.delivery.run_heartbeat_turn(&prompt, None).await?;

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

    /// Execute a single heartbeat tick.
    pub async fn tick(&mut self) -> Result<(), String> {
        self.tick_with_options(false, None, &[]).await
    }
}

/// Concrete HeartbeatDelivery backed by the gateway's LLM provider and channel manager.
pub struct GatewayHeartbeatDelivery {
    provider: Arc<dyn oclaw_llm_core::providers::LlmProvider>,
    channel_manager: Option<Arc<tokio::sync::RwLock<oclaw_channel_core::ChannelManager>>>,
}

impl GatewayHeartbeatDelivery {
    pub fn new(
        provider: Arc<dyn oclaw_llm_core::providers::LlmProvider>,
        channel_manager: Option<Arc<tokio::sync::RwLock<oclaw_channel_core::ChannelManager>>>,
    ) -> Self {
        Self {
            provider,
            channel_manager,
        }
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
        let config =
            oclaw_agent_core::agent::AgentConfig::new("heartbeat-agent", &model, "heartbeat")
                .with_system_prompt(prompt);
        let mut agent = oclaw_agent_core::agent::Agent::new(config, self.provider.clone());
        agent.initialize().await.map_err(|e| e.to_string())?;
        agent
            .run("Run your heartbeat check now.")
            .await
            .map_err(|e| e.to_string())
    }

    async fn deliver_alert(&self, target: &str, text: &str) -> Result<(), String> {
        let Some(ref cm) = self.channel_manager else {
            info!("Heartbeat alert (no channel manager): {}", text);
            return Ok(());
        };
        let mgr = cm.read().await;
        // target format: "channel_name" or "channel_name:chat_id"
        let (channel_name, _chat_id) = target.split_once(':').unwrap_or((target, ""));
        let Some(channel) = mgr.get(channel_name).await else {
            info!(
                "Heartbeat alert (channel {} not found): {}",
                channel_name, text
            );
            return Ok(());
        };
        let msg = oclaw_channel_core::traits::ChannelMessage {
            id: uuid::Uuid::new_v4().to_string(),
            channel: channel_name.to_string(),
            sender: "heartbeat".to_string(),
            content: text.to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            metadata: std::collections::HashMap::new(),
        };
        let ch = channel.read().await;
        ch.send_message(&msg)
            .await
            .map_err(|e| format!("Heartbeat delivery failed: {}", e))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cron_filter_ignores_heartbeat_noise_and_exec_completion() {
        assert!(!is_cron_system_event("HEARTBEAT_OK"));
        assert!(!is_cron_system_event("heartbeat poll: pending"));
        assert!(!is_cron_system_event("heartbeat wake complete"));
        assert!(!is_cron_system_event("exec finished: command done"));
        assert!(is_cron_system_event("cron reminder: pay rent"));
    }

    #[test]
    fn build_cron_prompt_handles_empty_and_non_empty_events() {
        let empty = build_cron_event_prompt(&[]);
        assert!(empty.contains("no event content"));

        let filled = build_cron_event_prompt(&["A".to_string(), "B".to_string()]);
        assert!(filled.contains("A"));
        assert!(filled.contains("B"));
    }

    #[test]
    fn resolve_reason_kind_supports_node_style_reasons() {
        assert_eq!(resolve_heartbeat_reason_kind(Some("wake")), "wake");
        assert_eq!(
            resolve_heartbeat_reason_kind(Some("exec-event")),
            "exec-event"
        );
        assert_eq!(resolve_heartbeat_reason_kind(Some("cron:daily")), "cron");
        assert_eq!(resolve_heartbeat_reason_kind(Some("hook:wake")), "hook");
        assert_eq!(resolve_heartbeat_reason_kind(Some("interval")), "interval");
    }
}
