//! Unified message processing pipeline: agent → flush → reply.
//!
//! Memory recall is now handled by the agent itself via `memory_search` and
//! `memory_get` tools (aligned with Node's tool-based recall pattern).
//! Auto-capture has been removed; durable memory is written via reactive
//! memory flush only.

use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

use oclaws_agent_core::agent::{Agent, AgentConfig};
use oclaws_llm_core::providers::LlmProvider;
use oclaws_workspace_core::bootstrap::BootstrapRunner;
use oclaws_workspace_core::memory_flush::{MemoryFlushConfig, SILENT_REPLY_TOKEN};
use oclaws_workspace_core::system_prompt::{self, RuntimeInfo};

use crate::http::HttpState;
use crate::http::agent_bridge::ToolRegistryExecutor;

/// Configuration for the memory-aware pipeline.
pub struct PipelineConfig {
    pub memory_flush: MemoryFlushConfig,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            memory_flush: MemoryFlushConfig::default(),
        }
    }
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

    // Build agent (memory recall is now tool-based via memory_search/memory_get)
    let reply = run_agent(provider, state, &session_id, text).await?;
    info!("[pipeline] agent replied, len={}", reply.len());

    // Memory flush — write durable memories to workspace files
    if state.workspace.is_some() && state.tool_registry.is_some() {
        try_memory_flush(provider, state, &session_id, text, &reply).await;
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
        agent_id: Some("channel-agent".to_string()),
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

    let config = AgentConfig::new("channel-agent", &model, "default")
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

    // Clear hatching flag after successful first-run conversation
    if is_hatching && result.is_ok() {
        state.needs_hatching.store(false, std::sync::atomic::Ordering::Relaxed);
        info!("[pipeline] hatching complete — identity established");
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

    let msg = oclaws_channel_core::traits::ChannelMessage {
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
/// Uses the memory flush system prompt from `BootstrapRunner`. The agent writes
/// key facts to `memory/YYYY-MM-DD.md` via the workspace tool. If nothing is
/// worth storing, the agent replies with `HEARTBEAT_OK` and we skip silently.
async fn try_memory_flush(
    provider: &Arc<dyn LlmProvider>,
    state: &HttpState,
    session_id: &str,
    user_text: &str,
    reply: &str,
) {
    // Skip trivially short exchanges
    if user_text.split_whitespace().count() < 5 && reply.split_whitespace().count() < 10 {
        return;
    }

    let registry = match state.tool_registry.as_ref() {
        Some(r) => r,
        None => return,
    };

    let model = provider.default_model().to_string();
    let flush_prompt = BootstrapRunner::memory_flush_system_prompt();

    let config = AgentConfig::new("memory-flush", &model, "default")
        .with_system_prompt(flush_prompt)
        .with_temperature(0.2);
    let mut agent = Agent::new(config, provider.clone());
    if let Err(e) = agent.initialize().await {
        warn!("[pipeline] memory-flush agent init failed: {}", e);
        return;
    }

    // Feed the conversation as context for the flush agent
    let context = format!(
        "Session: {}\n\nUser: {}\n\nAssistant: {}",
        session_id, user_text, reply
    );

    let executor = ToolRegistryExecutor::new(registry.clone());
    match agent.run_with_tools(&context, &executor).await {
        Ok(flush_reply) => {
            let trimmed = flush_reply.trim();
            if trimmed == SILENT_REPLY_TOKEN || trimmed.is_empty() {
                info!("[pipeline] memory-flush: nothing to store");
            } else {
                info!("[pipeline] memory-flush: wrote durable memories");
            }
        }
        Err(e) => {
            warn!("[pipeline] memory-flush agent error: {}", e);
        }
    }
}
