//! Unified message processing pipeline: agent → flush → reply.
//!
//! Memory recall is now handled by the agent itself via `memory_search` and
//! `memory_get` tools (aligned with Node's tool-based recall pattern).
//! Auto-capture has been removed; durable memory is written via reactive
//! memory flush only.
//!
//! Agent binding resolution selects the named agent for each message using an
//! 8-tier priority system (peer > guild-roles > guild > team > account > channel > default).

use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

use oclaw_config::settings::AgentBinding;

use oclaw_agent_core::agent::{Agent, AgentConfig};
use oclaw_llm_core::providers::LlmProvider;
use oclaw_workspace_core::bootstrap::{BootstrapRunner, evolution_system_prompt};
use oclaw_workspace_core::evolution::{EvolutionConfig, EvolutionState, should_run_evolution};
use oclaw_workspace_core::memory_flush::{
    MemoryFlushConfig, SILENT_REPLY_TOKEN, default_flush_prompt, should_run_memory_flush,
};
use oclaw_workspace_core::heartbeat::HEARTBEAT_OK_TOKEN;
use oclaw_workspace_core::soul::Soul;
use oclaw_workspace_core::system_prompt::{self, RuntimeInfo};

use crate::http::HttpState;
use crate::http::agent_bridge::ToolRegistryExecutor;

/// Message context extracted from metadata for agent binding resolution.
#[derive(Debug, Default)]
pub struct MessageContext<'a> {
    pub channel: &'a str,
    pub peer_id: &'a str,
    pub guild_id: Option<&'a str>,
    pub team_id: Option<&'a str>,
    pub account_id: Option<&'a str>,
    pub role_ids: Vec<String>,
}

/// Resolve which agent binding applies to this message context.
///
/// Evaluates each binding in priority order and returns the agent_id of the
/// first match. Returns `None` if no bindings are configured or none match.
pub fn resolve_agent_for_message(
    bindings: &[AgentBinding],
    ctx: &MessageContext<'_>,
) -> Option<String> {
    let mut best: Option<(u8, &AgentBinding)> = None;

    for binding in bindings {
        let matches = match binding {
            AgentBinding::Peer { channel, peer_id, .. } => {
                channel == ctx.channel && peer_id == ctx.peer_id
            }
            AgentBinding::GuildRoles { channel, guild_id, role_ids, .. } => {
                channel == ctx.channel
                    && ctx.guild_id.map(|g| g == guild_id).unwrap_or(false)
                    && !role_ids.is_empty()
                    && role_ids.iter().all(|r| ctx.role_ids.contains(r))
            }
            AgentBinding::Guild { channel, guild_id, .. } => {
                channel == ctx.channel
                    && ctx.guild_id.map(|g| g == guild_id).unwrap_or(false)
            }
            AgentBinding::Team { channel, team_id, .. } => {
                channel == ctx.channel
                    && ctx.team_id.map(|t| t == team_id).unwrap_or(false)
            }
            AgentBinding::Account { channel, account_id, .. } => {
                channel == ctx.channel
                    && ctx.account_id.map(|a| a == account_id).unwrap_or(false)
            }
            AgentBinding::Channel { channel, .. } => channel == ctx.channel,
            AgentBinding::Default { .. } => true,
        };

        if matches {
            let priority = binding.priority();
            if best.map(|(p, _)| priority < p).unwrap_or(true) {
                best = Some((priority, binding));
            }
        }
    }

    best.map(|(_, b)| b.agent_id().to_string())
}

/// Configuration for the memory-aware pipeline.
#[derive(Default)]
pub struct PipelineConfig {
    pub memory_flush: MemoryFlushConfig,
}

/// Process a message through the full pipeline:
/// 1. Build session ID from channel + chat_id
/// 2. Create Agent with transcript persistence
/// 3. If memory available, attach auto-recall
/// 4. Run agent (with tools if available)
/// 5. Auto-capture key info to memory
/// 6. Track echo and send reply via channel
pub async fn process_message(
    state: &HttpState,
    channel_name: &str,
    chat_id: &str,
    text: &str,
    metadata: HashMap<String, String>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    info!("[pipeline] process_message: channel={}, chat_id={}", channel_name, chat_id);

    let provider = state.llm_provider.as_ref()
        .ok_or("No LLM provider configured")?;

    let is_group = metadata.get("is_group").map(|v| v == "true").unwrap_or(false);
    let session_id = crate::session_key::build_session_id(
        channel_name,
        chat_id,
        is_group,
        state.dm_scope,
        state.identity_links.as_deref(),
        None,
    );

    // Resolve agent binding (8-tier priority routing)
    let resolved_agent_id = if let Some(ref cfg) = state.full_config {
        let config = cfg.read().await;
        if let Some(ref bindings) = config.bindings {
            let ctx = MessageContext {
                channel: channel_name,
                peer_id: metadata.get("user_id").map(|s| s.as_str()).unwrap_or(chat_id),
                guild_id: metadata.get("guild_id").map(|s| s.as_str()),
                team_id: metadata.get("team_id").map(|s| s.as_str()),
                account_id: metadata.get("account_id").map(|s| s.as_str()),
                role_ids: metadata.get("role_ids")
                    .map(|r| r.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
                    .unwrap_or_default(),
            };
            let agent_id = resolve_agent_for_message(bindings, &ctx);
            if let Some(ref aid) = agent_id {
                info!("[pipeline] binding matched agent_id={} for channel={}", aid, channel_name);
            }
            agent_id
        } else {
            None
        }
    } else {
        None
    };
    let agent_id = resolved_agent_id.as_deref().unwrap_or("channel-agent");

    // Build agent (memory recall is now tool-based via memory_search/memory_get)
    let reply = run_agent(provider, state, &session_id, text, agent_id).await?;
    info!("[pipeline] agent replied, len={}", reply.len());

    // Memory flush — only triggers near context limit (not every message)
    if state.workspace.is_some() && state.tool_registry.is_some() {
        // TODO: get actual token counts from agent result when agent exposes them
        // For now use a placeholder; real integration needs AgentRunResult with usage
        let total_tokens = 0u64;  // Will be wired up when agent exposes usage
        let compaction_count = 0u64;
        try_memory_flush(provider, state, &session_id, total_tokens, compaction_count).await;
    }

    // Autonomous evolution — periodic self-reflection and SOUL.md growth
    if state.workspace.is_some() && state.tool_registry.is_some() {
        try_evolution(provider, state, &session_id, 0).await;
    }

    // Echo tracking
    state.echo_tracker.lock().await.remember(&reply);

    // Send reply via channel
    send_reply(state, channel_name, &reply, metadata).await?;

    info!("Pipeline replied on {} chat {}", channel_name, chat_id);
    Ok(reply)
}

async fn run_agent(
    provider: &Arc<dyn LlmProvider>,
    state: &HttpState,
    session_id: &str,
    text: &str,
    agent_id: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let model = provider.default_model().to_string();

    // Collect tool names for self-awareness
    let tool_names: Vec<String> = state.tool_registry.as_ref()
        .map(|r| r.list_for_llm().iter()
            .filter_map(|v| v["name"].as_str().map(|s| s.to_string()))
            .collect())
        .unwrap_or_default();

    // Build runtime info for agent self-awareness
    let runtime = RuntimeInfo {
        agent_id: Some(agent_id.to_string()),
        model: Some(model.clone()),
        default_model: Some(provider.default_model().to_string()),
        os: Some(std::env::consts::OS.to_string()),
        arch: Some(std::env::consts::ARCH.to_string()),
        host: std::env::var("HOSTNAME").or_else(|_| std::env::var("COMPUTERNAME")).ok(),
        shell: std::env::var("SHELL").ok(),
        channel: None,
        workspace_dir: state.workspace.as_ref().map(|ws| ws.root().to_string_lossy().to_string()),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
    };

    // Hatching mode: use special first-run identity discovery prompt
    let is_hatching = state.needs_hatching.load(std::sync::atomic::Ordering::Relaxed);

    let prompt = if is_hatching {
        BootstrapRunner::hatching_system_prompt().to_string()
    } else {
        // Build dynamic system prompt from workspace files (SOUL.md, IDENTITY.md, etc.)
        let base_prompt = if let Some(ref ws) = state.workspace {
            system_prompt::load_and_build_with_runtime(
                ws, None, false, Some(runtime), &tool_names,
            ).await.ok()
        } else {
            None
        };
        base_prompt.unwrap_or_else(|| {
            "You are a helpful assistant. Respond in the user's language.".to_string()
        })
    };

    let config = AgentConfig::new(agent_id, &model, "default")
        .with_system_prompt(&prompt);
    let mut agent = Agent::new(config, provider.clone())
        .with_transcript(session_id);

    // Memory recall is now tool-based (memory_search + memory_get),
    // no longer injected via auto_recall on the agent.

    agent.initialize().await.map_err(|e| e.to_string())?;

    let result = if let Some(ref registry) = state.tool_registry {
        let mut executor = ToolRegistryExecutor::new(registry.clone());
        if let Some(ref regs) = state.plugin_registrations {
            executor = executor.with_plugin_registrations(regs.clone());
        }
        agent.run_with_tools(text, &executor).await.map_err(|e| e.to_string().into())
    } else {
        agent.run(text).await.map_err(|e| e.to_string().into())
    };

    // After a hatching turn, check if identity is fully personalized.
    // Require name + at least one extra (emoji/creature/vibe) to avoid
    // clearing the flag before the multi-turn conversation finishes.
    if is_hatching && result.is_ok()
        && let Some(ref ws) = state.workspace
        && let Ok(Some(identity)) = oclaw_workspace_core::identity::AgentIdentity::load(ws).await
    {
        let has_name = identity.name.is_some();
        let has_extras = identity.emoji.is_some()
            || identity.creature.is_some()
            || identity.vibe.is_some();
        if has_name && has_extras {
            state.needs_hatching.store(false, std::sync::atomic::Ordering::Relaxed);
            info!("[pipeline] hatching complete — identity personalized: {}", identity.display_name());

            // Clear old session transcripts so every channel starts fresh
            if let Err(e) = oclaw_agent_core::transcript::Transcript::clear_all_sessions().await {
                warn!("[pipeline] failed to clear old session transcripts: {}", e);
            } else {
                info!("[pipeline] cleared old session transcripts after hatching");
            }
        }
    }

    result
}

async fn send_reply(
    state: &HttpState,
    channel_name: &str,
    reply: &str,
    metadata: HashMap<String, String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let manager = state.channel_manager.as_ref()
        .ok_or("No channel manager")?;
    let mgr = manager.read().await;
    let channel = mgr.get(channel_name).await
        .ok_or_else(|| format!("{} channel not found", channel_name))?;

    let msg = oclaw_channel_core::traits::ChannelMessage {
        id: uuid::Uuid::new_v4().to_string(),
        channel: channel_name.to_string(),
        sender: "bot".to_string(),
        content: reply.to_string(),
        timestamp: chrono::Utc::now().timestamp_millis(),
        metadata,
    };

    let ch = channel.read().await;
    ch.send_message(&msg).await
        .map_err(|e| format!("Send error: {}", e))?;
    Ok(())
}

/// Run a lightweight agent turn to flush durable memories to workspace files.
///
/// Triggered only when the session is near its context limit (token threshold),
/// NOT on every exchange. Mirrors Node's shouldRunMemoryFlush logic.
pub async fn try_memory_flush(
    provider: &Arc<dyn LlmProvider>,
    state: &HttpState,
    session_id: &str,
    total_tokens: u64,
    compaction_count: u64,
) {
    let config = &state.pipeline_config.memory_flush;
    if !config.enabled {
        return;
    }

    // Get last flush compaction count for this session
    let last_flush = state.last_flush_compaction_count(session_id);

    // Context window from provider (default 128k tokens)
    let context_window = 128_000u64;

    if !should_run_memory_flush(
        total_tokens,
        context_window,
        config,
        last_flush,
        compaction_count,
    ) {
        return;
    }

    let registry = match state.tool_registry.as_ref() {
        Some(r) => r,
        None => return,
    };

    let model = provider.default_model().to_string();
    let flush_prompt = config.prompt.as_deref()
        .map(|s| s.to_string())
        .unwrap_or_else(default_flush_prompt);

    let flush_config = AgentConfig::new("memory-flush", &model, "default")
        .with_system_prompt(&flush_prompt)
        .with_temperature(0.2);
    let mut agent = Agent::new(flush_config, provider.clone());
    if let Err(e) = agent.initialize().await {
        warn!("[pipeline] memory-flush agent init failed: {}", e);
        return;
    }

    let executor = ToolRegistryExecutor::new(registry.clone());
    match agent.run_with_tools("Flush session memories now.", &executor).await {
        Ok(reply) => {
            let trimmed = reply.trim();
            if trimmed == SILENT_REPLY_TOKEN
                || trimmed == HEARTBEAT_OK_TOKEN
                || trimmed.is_empty()
            {
                info!("[pipeline] memory-flush: nothing to store");
            } else {
                info!("[pipeline] memory-flush: wrote durable memories");
            }
            // Record that we flushed at this compaction level
            state.set_last_flush_compaction_count(session_id, compaction_count);
        }
        Err(e) => {
            warn!("[pipeline] memory-flush agent error: {}", e);
        }
    }
}

/// Run a lightweight agent turn for autonomous self-reflection and growth.
///
/// Triggered periodically (every N counted messages) via [`EvolutionState`].
/// The evolution agent reads SOUL.md / USER.md, reflects on recent interactions,
/// and optionally rewrites those files if genuine growth occurred.
///
/// Mirrors the Node OpenClaw "try_evolution" pipeline step.
pub async fn try_evolution(
    provider: &Arc<dyn LlmProvider>,
    state: &HttpState,
    _session_id: &str,
    usage_tokens: u64,
) {
    let ws = match state.workspace.as_ref() {
        Some(ws) => ws,
        None => return,
    };
    let registry = match state.tool_registry.as_ref() {
        Some(r) => r,
        None => return,
    };

    let config = EvolutionConfig::default();
    let mut evo_state = EvolutionState::load(ws).await;
    evo_state.tick(usage_tokens, &config);

    if !should_run_evolution(&evo_state, &config) {
        if let Err(e) = evo_state.save(ws).await {
            warn!("[pipeline] evolution state save failed: {}", e);
        }
        return;
    }

    // Snapshot SOUL.md before potential modifications (idempotent per day).
    if let Err(e) = Soul::backup(ws).await {
        warn!("[pipeline] soul backup failed: {}", e);
    }

    let model = provider.default_model().to_string();
    let prompt = config
        .prompt
        .as_deref()
        .unwrap_or_else(|| evolution_system_prompt())
        .to_string();

    let evo_config = AgentConfig::new("evolution", &model, "default")
        .with_system_prompt(&prompt)
        .with_temperature(0.5);
    let mut agent = Agent::new(evo_config, provider.clone());
    if let Err(e) = agent.initialize().await {
        warn!("[pipeline] evolution agent init failed: {}", e);
        return;
    }

    let executor = ToolRegistryExecutor::new(registry.clone());
    match agent.run_with_tools("Reflect and evolve.", &executor).await {
        Ok(reply) => {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            evo_state.last_evolved_at_message = evo_state.message_count;
            evo_state.last_evolved_date = Some(today);
            evo_state.evolution_count += 1;
            info!("[pipeline] evolution complete ({}): {}", evo_state.evolution_count, reply.trim());
        }
        Err(e) => {
            warn!("[pipeline] evolution agent error: {}", e);
        }
    }

    if let Err(e) = evo_state.save(ws).await {
        warn!("[pipeline] evolution state save failed: {}", e);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oclaw_config::settings::AgentBinding;

    fn peer(channel: &str, peer_id: &str, agent_id: &str) -> AgentBinding {
        AgentBinding::Peer {
            channel: channel.to_string(),
            peer_id: peer_id.to_string(),
            agent_id: agent_id.to_string(),
        }
    }
    fn guild_roles(channel: &str, guild_id: &str, roles: &[&str], agent_id: &str) -> AgentBinding {
        AgentBinding::GuildRoles {
            channel: channel.to_string(),
            guild_id: guild_id.to_string(),
            role_ids: roles.iter().map(|s| s.to_string()).collect(),
            agent_id: agent_id.to_string(),
        }
    }
    fn guild(channel: &str, guild_id: &str, agent_id: &str) -> AgentBinding {
        AgentBinding::Guild {
            channel: channel.to_string(),
            guild_id: guild_id.to_string(),
            agent_id: agent_id.to_string(),
        }
    }
    fn channel_binding(channel: &str, agent_id: &str) -> AgentBinding {
        AgentBinding::Channel { channel: channel.to_string(), agent_id: agent_id.to_string() }
    }
    fn default_binding(agent_id: &str) -> AgentBinding {
        AgentBinding::Default { agent_id: agent_id.to_string() }
    }

    #[test]
    fn peer_beats_channel() {
        let bindings = vec![
            channel_binding("discord", "channel-agent"),
            peer("discord", "u1", "vip-agent"),
        ];
        let ctx = MessageContext { channel: "discord", peer_id: "u1", ..Default::default() };
        assert_eq!(resolve_agent_for_message(&bindings, &ctx), Some("vip-agent".to_string()));
    }

    #[test]
    fn guild_roles_beats_guild() {
        let bindings = vec![
            guild("discord", "G1", "guild-agent"),
            guild_roles("discord", "G1", &["admin"], "admin-agent"),
        ];
        let ctx = MessageContext {
            channel: "discord",
            peer_id: "u1",
            guild_id: Some("G1"),
            role_ids: vec!["admin".to_string()],
            ..Default::default()
        };
        assert_eq!(resolve_agent_for_message(&bindings, &ctx), Some("admin-agent".to_string()));
    }

    #[test]
    fn guild_roles_skipped_when_roles_missing() {
        let bindings = vec![
            guild("discord", "G1", "guild-agent"),
            guild_roles("discord", "G1", &["admin"], "admin-agent"),
        ];
        // User has no roles — falls back to guild binding
        let ctx = MessageContext {
            channel: "discord",
            peer_id: "u2",
            guild_id: Some("G1"),
            role_ids: vec![],
            ..Default::default()
        };
        assert_eq!(resolve_agent_for_message(&bindings, &ctx), Some("guild-agent".to_string()));
    }

    #[test]
    fn default_fallback_when_no_match() {
        let bindings = vec![
            peer("telegram", "123", "tg-agent"),
            default_binding("fallback"),
        ];
        let ctx = MessageContext { channel: "slack", peer_id: "U9", ..Default::default() };
        assert_eq!(resolve_agent_for_message(&bindings, &ctx), Some("fallback".to_string()));
    }

    #[test]
    fn no_match_returns_none() {
        let bindings = vec![peer("telegram", "123", "tg-agent")];
        let ctx = MessageContext { channel: "slack", peer_id: "X", ..Default::default() };
        assert_eq!(resolve_agent_for_message(&bindings, &ctx), None);
    }

    #[test]
    fn empty_bindings_returns_none() {
        let ctx = MessageContext { channel: "telegram", peer_id: "1", ..Default::default() };
        assert_eq!(resolve_agent_for_message(&[], &ctx), None);
    }

    #[test]
    fn agent_binding_json_roundtrip() {
        let bindings: Vec<AgentBinding> = serde_json::from_str(r#"[
            {"type": "peer",        "channel": "telegram", "peerId": "123",  "agentId": "vip"},
            {"type": "guild-roles", "channel": "discord",  "guildId": "G1",  "roleIds": ["admin"], "agentId": "adm"},
            {"type": "guild",       "channel": "discord",  "guildId": "G1",  "agentId": "gld"},
            {"type": "channel",     "channel": "slack",    "agentId": "slk"},
            {"type": "default",     "agentId": "def"}
        ]"#).unwrap();
        assert_eq!(bindings.len(), 5);
        assert_eq!(bindings[0].agent_id(), "vip");
        assert_eq!(bindings[0].priority(), 1);
        assert_eq!(bindings[4].priority(), 7);
    }
}
