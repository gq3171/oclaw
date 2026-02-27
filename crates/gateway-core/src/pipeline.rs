//! Unified message processing pipeline: queue/skills → agent → memory/evolution → reply.
//!
//! Memory recall is handled by tools (`memory_search` / `memory_get`).
//! Durable memory writes come from both near-window flush and auto-capture.
//! Agent binding uses priority routing (peer > guild-roles > guild > team >
//! account > channel > default).

use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

use oclaw_auto_reply::{
    ChatType, FinalizedMsgContext, MsgContext, QueueAction, QueueDropPolicy, QueueMode,
    finalize_inbound_context,
};
use oclaw_config::settings::AgentBinding;
use oclaw_skills_core::{SkillContext, SkillInput};

use oclaw_agent_core::agent::{Agent, AgentConfig};
use oclaw_llm_core::providers::LlmProvider;
use oclaw_workspace_core::bootstrap::{BootstrapRunner, evolution_system_prompt};
use oclaw_workspace_core::evolution::{EvolutionConfig, EvolutionState, should_run_evolution};
use oclaw_workspace_core::heartbeat::HEARTBEAT_OK_TOKEN;
use oclaw_workspace_core::memory_flush::{
    MemoryFlushConfig, SILENT_REPLY_TOKEN, default_flush_prompt, should_run_memory_flush,
};
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
            AgentBinding::Peer {
                channel, peer_id, ..
            } => channel == ctx.channel && peer_id == ctx.peer_id,
            AgentBinding::GuildRoles {
                channel,
                guild_id,
                role_ids,
                ..
            } => {
                channel == ctx.channel
                    && ctx.guild_id.map(|g| g == guild_id).unwrap_or(false)
                    && !role_ids.is_empty()
                    && role_ids.iter().all(|r| ctx.role_ids.contains(r))
            }
            AgentBinding::Guild {
                channel, guild_id, ..
            } => channel == ctx.channel && ctx.guild_id.map(|g| g == guild_id).unwrap_or(false),
            AgentBinding::Team {
                channel, team_id, ..
            } => channel == ctx.channel && ctx.team_id.map(|t| t == team_id).unwrap_or(false),
            AgentBinding::Account {
                channel,
                account_id,
                ..
            } => channel == ctx.channel && ctx.account_id.map(|a| a == account_id).unwrap_or(false),
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
    pub evolution: EvolutionConfig,
}

#[derive(Debug, Clone)]
struct AgentRunOutcome {
    reply: String,
    usage_tokens: u64,
}

#[derive(Debug, Clone, Copy)]
struct QueueRuntimeSettings {
    mode: QueueMode,
    collect_window_ms: u64,
    debounce_ms: u64,
    cap: usize,
    drop_policy: QueueDropPolicy,
}

impl Default for QueueRuntimeSettings {
    fn default() -> Self {
        Self {
            mode: QueueMode::Followup,
            collect_window_ms: 2_000,
            debounce_ms: 0,
            cap: usize::MAX,
            drop_policy: QueueDropPolicy::Summarize,
        }
    }
}

fn queue_mode_from_str(raw: &str) -> QueueMode {
    match raw.trim().to_ascii_lowercase().as_str() {
        "interrupt" => QueueMode::Interrupt,
        "steer" => QueueMode::Steer,
        "collect" => QueueMode::Collect,
        "steerbacklog" | "steer-backlog" | "steer+backlog" => QueueMode::SteerBacklog,
        "queue" | "followup" | "follow-up" => QueueMode::Followup,
        _ => QueueMode::Followup,
    }
}

fn queue_drop_policy_from_str(raw: &str) -> QueueDropPolicy {
    match raw.trim().to_ascii_lowercase().as_str() {
        "old" | "drop-old" => QueueDropPolicy::Old,
        "new" | "drop-new" => QueueDropPolicy::New,
        "summarize" | "summary" => QueueDropPolicy::Summarize,
        _ => QueueDropPolicy::Summarize,
    }
}

fn pointer_u64(messages: &serde_json::Value, ptr: &str) -> Option<u64> {
    messages.pointer(ptr).and_then(|v| v.as_u64())
}

fn pointer_str<'a>(messages: &'a serde_json::Value, ptr: &str) -> Option<&'a str> {
    messages.pointer(ptr).and_then(|v| v.as_str())
}

fn metadata_pick_non_empty(metadata: &HashMap<String, String>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| metadata.get(*key))
        .map(|raw| raw.trim())
        .filter(|trimmed| !trimmed.is_empty())
        .map(ToString::to_string)
}

async fn resolve_queue_runtime_settings(
    state: &HttpState,
    channel_name: &str,
) -> QueueRuntimeSettings {
    let Some(cfg) = state.full_config.as_ref() else {
        return QueueRuntimeSettings::default();
    };
    let cfg = cfg.read().await;
    let Some(messages) = cfg.messages.as_ref() else {
        return QueueRuntimeSettings::default();
    };

    let mut settings = QueueRuntimeSettings::default();
    let by_channel_mode_ptr = format!("/queue/byChannel/{}/mode", channel_name);
    let by_channel_mode_scalar_ptr = format!("/queue/byChannel/{}", channel_name);
    let raw_mode = pointer_str(messages, &by_channel_mode_ptr)
        .or_else(|| pointer_str(messages, &by_channel_mode_scalar_ptr))
        .or_else(|| pointer_str(messages, "/queue/mode"))
        .unwrap_or("followup");
    settings.mode = queue_mode_from_str(raw_mode);

    let by_channel_collect_ptr = format!("/queue/byChannel/{}/collectWindowMs", channel_name);
    let by_channel_collect_scalar_ptr = format!("/queue/collectWindowMsByChannel/{}", channel_name);
    settings.collect_window_ms = pointer_u64(messages, &by_channel_collect_ptr)
        .or_else(|| pointer_u64(messages, &by_channel_collect_scalar_ptr))
        .or_else(|| pointer_u64(messages, "/queue/collectWindowMs"))
        .unwrap_or(2_000)
        .clamp(1, 60_000);

    let by_channel_debounce_ptr = format!("/queue/byChannel/{}/debounceMs", channel_name);
    let by_channel_debounce_scalar_ptr = format!("/queue/debounceMsByChannel/{}", channel_name);
    settings.debounce_ms = pointer_u64(messages, &by_channel_debounce_ptr)
        .or_else(|| pointer_u64(messages, &by_channel_debounce_scalar_ptr))
        .or_else(|| pointer_u64(messages, "/queue/debounceMs"))
        .unwrap_or(0)
        .clamp(0, 60_000);

    let by_channel_cap_ptr = format!("/queue/byChannel/{}/cap", channel_name);
    let by_channel_cap_scalar_ptr = format!("/queue/capByChannel/{}", channel_name);
    settings.cap = pointer_u64(messages, &by_channel_cap_ptr)
        .or_else(|| pointer_u64(messages, &by_channel_cap_scalar_ptr))
        .or_else(|| pointer_u64(messages, "/queue/cap"))
        .and_then(|v| usize::try_from(v).ok())
        .filter(|v| *v > 0)
        .unwrap_or(usize::MAX);

    let by_channel_drop_ptr = format!("/queue/byChannel/{}/dropPolicy", channel_name);
    let by_channel_drop_scalar_ptr = format!("/queue/dropPolicyByChannel/{}", channel_name);
    let raw_drop = pointer_str(messages, &by_channel_drop_ptr)
        .or_else(|| pointer_str(messages, &by_channel_drop_scalar_ptr))
        .or_else(|| pointer_str(messages, "/queue/dropPolicy"))
        .unwrap_or("summarize");
    settings.drop_policy = queue_drop_policy_from_str(raw_drop);

    settings
}

fn build_finalized_context(
    channel_name: &str,
    chat_id: &str,
    text: &str,
    session_id: &str,
    metadata: HashMap<String, String>,
) -> FinalizedMsgContext {
    let chat_type = if metadata
        .get("is_group")
        .map(|v| v == "true")
        .unwrap_or(false)
    {
        ChatType::Group
    } else {
        ChatType::Direct
    };
    let from = metadata
        .get("user_id")
        .cloned()
        .unwrap_or_else(|| chat_id.to_string());
    let msg = MsgContext {
        body: text.to_string(),
        raw_body: Some(text.to_string()),
        from,
        from_name: metadata.get("user_name").cloned(),
        to: chat_id.to_string(),
        provider: channel_name.to_string(),
        surface: Some(channel_name.to_string()),
        chat_type,
        session_key: session_id.to_string(),
        message_id: metadata.get("message_id").cloned(),
        thread_id: metadata.get("thread_id").cloned(),
        was_mentioned: metadata
            .get("was_mentioned")
            .map(|v| v == "true")
            .unwrap_or(false),
        media_paths: Vec::new(),
        timestamp_ms: chrono::Utc::now().timestamp_millis().max(0) as u64,
        raw: serde_json::to_value(&metadata).unwrap_or(serde_json::Value::Null),
    };
    finalize_inbound_context(msg)
}

fn finalized_metadata(ctx: &FinalizedMsgContext) -> HashMap<String, String> {
    serde_json::from_value(ctx.ctx.raw.clone()).unwrap_or_default()
}

fn build_collect_prompt(batch: &[FinalizedMsgContext], summary_prompt: Option<&str>) -> String {
    let mut blocks = vec!["[Queued messages while agent was busy]".to_string()];
    if let Some(summary) = summary_prompt.map(str::trim).filter(|v| !v.is_empty()) {
        blocks.push(summary.to_string());
    }
    for (idx, item) in batch.iter().enumerate() {
        blocks.push(
            format!("---\nQueued #{}\n{}", idx + 1, item.body_for_agent.trim())
                .trim()
                .to_string(),
        );
    }
    blocks.join("\n\n")
}

fn build_collect_context(
    batch: &[FinalizedMsgContext],
    summary_prompt: Option<&str>,
) -> Option<FinalizedMsgContext> {
    let mut merged = batch.last()?.clone();
    let prompt = build_collect_prompt(batch, summary_prompt);
    merged.body_for_agent = prompt.clone();
    merged.body_for_commands = prompt;
    merged.command_authorized = false;
    Some(merged)
}

fn prepend_summary_prompt(ctx: &mut FinalizedMsgContext, summary_prompt: &str) {
    let summary = summary_prompt.trim();
    if summary.is_empty() {
        return;
    }
    let body = ctx.body_for_agent.trim();
    if body.is_empty() {
        ctx.body_for_agent = summary.to_string();
    } else {
        ctx.body_for_agent = format!("{}\n\n{}", summary, body);
    }
}

async fn wait_for_queue_debounce(queue: &Arc<tokio::sync::Mutex<oclaw_auto_reply::MessageQueue>>) {
    loop {
        let (debounce_ms, last_enqueued_at_ms, pending_count) = {
            let q = queue.lock().await;
            (q.debounce_ms(), q.last_enqueued_at_ms(), q.pending_count())
        };
        if debounce_ms == 0 || pending_count == 0 {
            return;
        }
        let now = chrono::Utc::now().timestamp_millis().max(0) as u64;
        let due_at = last_enqueued_at_ms.saturating_add(debounce_ms);
        if now >= due_at {
            return;
        }
        let sleep_ms = due_at.saturating_sub(now).clamp(1, debounce_ms);
        tokio::time::sleep(std::time::Duration::from_millis(sleep_ms)).await;
    }
}

fn parse_skill_invocation(text: &str) -> Option<(String, String)> {
    let trimmed = text.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    let body = trimmed.trim_start_matches('/').trim();
    if body.is_empty() {
        return None;
    }
    let mut parts = body.splitn(2, char::is_whitespace);
    let head = parts.next()?.trim();
    let tail = parts.next().unwrap_or("").trim();
    if head.eq_ignore_ascii_case("skill") {
        if tail.is_empty() {
            return Some((String::new(), String::new()));
        }
        let mut inner = tail.splitn(2, char::is_whitespace);
        let skill_id = inner.next().unwrap_or("").trim();
        let args = inner.next().unwrap_or("").trim();
        return Some((skill_id.to_string(), args.to_string()));
    }
    Some((head.to_string(), tail.to_string()))
}

async fn try_execute_skill(
    state: &HttpState,
    session_id: &str,
    command_text: &str,
    metadata: &HashMap<String, String>,
) -> Result<Option<String>, Box<dyn std::error::Error + Send + Sync>> {
    let Some((requested, raw_args)) = parse_skill_invocation(command_text) else {
        return Ok(None);
    };
    let Some(registry) = state.skill_registry.as_ref() else {
        return Ok(None);
    };
    if requested.is_empty() {
        return Ok(Some("Usage: /skill <id> [json|text args]".to_string()));
    }

    let defs = registry.list().await;
    let requested_norm = requested.to_ascii_lowercase();
    let resolved = defs.iter().find(|d| {
        d.id.eq_ignore_ascii_case(&requested)
            || d.name.eq_ignore_ascii_case(&requested)
            || d.id.replace('_', "-") == requested_norm
    });
    let Some(def) = resolved else {
        // Unknown slash command, leave it to agent.
        return Ok(None);
    };

    let mut params = HashMap::new();
    if !raw_args.is_empty() {
        if raw_args.starts_with('{') {
            match serde_json::from_str::<serde_json::Value>(&raw_args) {
                Ok(serde_json::Value::Object(map)) => {
                    params.extend(map);
                }
                Ok(v) => {
                    params.insert("value".to_string(), v);
                }
                Err(_) => {
                    params.insert("command".to_string(), serde_json::Value::String(raw_args));
                }
            }
        } else {
            params.insert("command".to_string(), serde_json::Value::String(raw_args));
        }
    }

    let mut ctx_meta = HashMap::new();
    if let Some(perms) = metadata.get("permissions") {
        ctx_meta.insert("permissions".to_string(), perms.clone());
    }
    ctx_meta.insert(
        "channel".to_string(),
        metadata.get("channel").cloned().unwrap_or_default(),
    );

    let input = SkillInput {
        name: def.name.clone(),
        description: def.description.clone(),
        parameters: params,
        context: Some(SkillContext {
            user_id: metadata.get("user_id").cloned(),
            session_id: Some(session_id.to_string()),
            request_id: Some(uuid::Uuid::new_v4().to_string()),
            metadata: ctx_meta,
        }),
    };

    let output = registry.execute(&def.id, input).await;
    let Some(output) = output else {
        return Ok(Some(format!("Skill not found: {}", requested)));
    };
    if !output.success {
        return Ok(Some(
            output.error.unwrap_or_else(|| "Skill failed".to_string()),
        ));
    }
    if let Some(result) = output.result {
        let text = match result {
            serde_json::Value::String(s) => s,
            v => serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string()),
        };
        return Ok(Some(text));
    }
    Ok(Some("Skill executed.".to_string()))
}

async fn process_single_message(
    provider: &Arc<dyn LlmProvider>,
    state: &HttpState,
    channel_name: &str,
    chat_id: &str,
    session_id: &str,
    finalized: &FinalizedMsgContext,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let metadata = finalized_metadata(finalized);
    let turn_source_to =
        metadata_pick_non_empty(&metadata, &["chat_id", "to", "target", "recipient"]).or_else(
            || {
                let trimmed = chat_id.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            },
        );
    let turn_source_account_id = metadata_pick_non_empty(&metadata, &["account_id", "accountId"]);
    let turn_source_thread_id = metadata_pick_non_empty(
        &metadata,
        &["thread_id", "threadId", "thread_ts", "root_id"],
    );
    let text = finalized.body_for_agent.as_str();
    let command_text = finalized.body_for_commands.as_str();

    // Resolve agent binding (8-tier priority routing)
    let resolved_agent_id = if let Some(ref cfg) = state.full_config {
        let config = cfg.read().await;
        if let Some(ref bindings) = config.bindings {
            let ctx = MessageContext {
                channel: channel_name,
                peer_id: metadata
                    .get("user_id")
                    .map(|s| s.as_str())
                    .unwrap_or(chat_id),
                guild_id: metadata.get("guild_id").map(|s| s.as_str()),
                team_id: metadata.get("team_id").map(|s| s.as_str()),
                account_id: metadata.get("account_id").map(|s| s.as_str()),
                role_ids: metadata
                    .get("role_ids")
                    .map(|r| {
                        r.split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect()
                    })
                    .unwrap_or_default(),
            };
            let agent_id = resolve_agent_for_message(bindings, &ctx);
            if let Some(ref aid) = agent_id {
                info!(
                    "[pipeline] binding matched agent_id={} for channel={}",
                    aid, channel_name
                );
            }
            agent_id
        } else {
            None
        }
    } else {
        None
    };
    let agent_id = resolved_agent_id.as_deref().unwrap_or("channel-agent");

    let mut ran_agent = false;
    let mut usage_tokens_for_turn = 0u64;
    let reply = if let Some(skill_reply) =
        try_execute_skill(state, session_id, command_text, &metadata).await?
    {
        info!(
            "[pipeline] skill command handled, len={}",
            skill_reply.len()
        );
        skill_reply
    } else {
        ran_agent = true;
        let outcome = run_agent(
            provider,
            state,
            session_id,
            text,
            agent_id,
            channel_name,
            turn_source_to,
            turn_source_account_id,
            turn_source_thread_id,
        )
        .await?;
        usage_tokens_for_turn = outcome.usage_tokens;
        info!(
            "[pipeline] agent replied, len={}, usage_tokens={}",
            outcome.reply.len(),
            outcome.usage_tokens
        );
        outcome.reply
    };

    // Memory flush + evolution only for full agent turns.
    if ran_agent && state.workspace.is_some() && state.tool_registry.is_some() {
        let total_tokens = state.add_session_usage_tokens(session_id, usage_tokens_for_turn);
        let compaction_count = state.bump_session_turn_count(session_id);
        try_memory_flush(provider, state, session_id, total_tokens, compaction_count).await;
        try_evolution(provider, state, session_id, usage_tokens_for_turn).await;
    }

    // Auto-capture durable facts with per-session cap.
    if let Some(ref mm) = state.memory_manager {
        let capture_n = state.bump_auto_capture_count(session_id);
        let cfg = state.auto_capture_config.as_ref();
        let should = oclaw_memory_core::auto_capture::should_capture(capture_n, cfg)
            && capture_n <= cfg.max_captures_per_session;
        if should && !reply.trim().is_empty() {
            let source = format!("auto_capture:{}", session_id);
            if let Err(e) = mm.add_memory_text(reply.trim(), &source).await {
                warn!("[pipeline] auto-capture failed: {}", e);
            } else {
                info!("[pipeline] auto-capture stored for {}", session_id);
            }
        }
    }

    state.echo_tracker.lock().await.remember(&reply);
    send_reply(state, channel_name, &reply, metadata).await?;
    Ok(reply)
}

/// Process a message through the session queue and execute one or more turns.
pub async fn process_message(
    state: &HttpState,
    channel_name: &str,
    chat_id: &str,
    text: &str,
    metadata: HashMap<String, String>,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    info!(
        "[pipeline] process_message: channel={}, chat_id={}",
        channel_name, chat_id
    );

    let provider = state
        .llm_provider
        .as_ref()
        .ok_or("No LLM provider configured")?;

    let is_group = metadata
        .get("is_group")
        .map(|v| v == "true")
        .unwrap_or(false);
    let session_id = crate::session_key::build_session_id(
        channel_name,
        chat_id,
        is_group,
        state.dm_scope,
        state.identity_links.as_deref(),
        None,
    );

    let queue_settings = resolve_queue_runtime_settings(state, channel_name).await;
    let finalized = build_finalized_context(channel_name, chat_id, text, &session_id, metadata);
    let queue = state
        .get_or_create_queue(&session_id, queue_settings.mode)
        .await;
    {
        let mut q = queue.lock().await;
        q.set_mode(queue_settings.mode);
        q.set_collect_window_ms(queue_settings.collect_window_ms);
        q.set_debounce_ms(queue_settings.debounce_ms);
        q.set_cap(queue_settings.cap);
        q.set_drop_policy(queue_settings.drop_policy);
    }

    let action = {
        let mut q = queue.lock().await;
        q.enqueue(finalized)
    };

    let mut current_batch: Vec<FinalizedMsgContext> = match action {
        QueueAction::RunNow(ctx) | QueueAction::Interrupted(ctx) => vec![ctx],
        QueueAction::Collected(batch) => batch,
        QueueAction::Queued => {
            info!("[pipeline] queued message for {}", session_id);
            return Ok("queued".to_string());
        }
    };

    let run_lock = state.get_or_create_run_lock(&session_id).await;
    let _guard = run_lock.lock().await;

    let mut first_reply: Option<String> = None;
    loop {
        let summary_prompt = {
            let mut q = queue.lock().await;
            q.take_summary_prompt("message")
        };
        if queue_settings.mode == QueueMode::Collect
            && (current_batch.len() > 1 || summary_prompt.is_some())
            && let Some(ctx) = build_collect_context(&current_batch, summary_prompt.as_deref())
        {
            let reply =
                process_single_message(provider, state, channel_name, chat_id, &session_id, &ctx)
                    .await?;
            if first_reply.is_none() {
                first_reply = Some(reply);
            }
        } else {
            let mut first_with_summary = true;
            for mut ctx in current_batch.drain(..) {
                if first_with_summary && let Some(summary) = summary_prompt.as_deref() {
                    prepend_summary_prompt(&mut ctx, summary);
                    first_with_summary = false;
                }
                let reply = process_single_message(
                    provider,
                    state,
                    channel_name,
                    chat_id,
                    &session_id,
                    &ctx,
                )
                .await?;
                if first_reply.is_none() {
                    first_reply = Some(reply);
                }
            }
        }

        current_batch.clear();
        let next_batch = {
            let mut q = queue.lock().await;
            if q.mode() == QueueMode::Collect && q.pending_count() > 0 {
                let window = q.collect_window_ms();
                q.mark_run_complete();
                drop(q);
                tokio::time::sleep(std::time::Duration::from_millis(window)).await;
                wait_for_queue_debounce(&queue).await;
                let mut q2 = queue.lock().await;
                q2.take_collect_batch()
            } else {
                let has_pending = q.pending_count() > 0;
                drop(q);
                if has_pending {
                    wait_for_queue_debounce(&queue).await;
                }
                let mut q2 = queue.lock().await;
                q2.complete_and_take_next().map(|next| vec![next])
            }
        };
        let Some(next_batch) = next_batch else {
            break;
        };
        current_batch = next_batch;
    }

    let reply = first_reply.unwrap_or_else(|| "queued".to_string());
    info!("Pipeline replied on {} chat {}", channel_name, chat_id);
    Ok(reply)
}

async fn run_agent(
    provider: &Arc<dyn LlmProvider>,
    state: &HttpState,
    session_id: &str,
    text: &str,
    agent_id: &str,
    turn_source_channel: &str,
    turn_source_to: Option<String>,
    turn_source_account_id: Option<String>,
    turn_source_thread_id: Option<String>,
) -> Result<AgentRunOutcome, Box<dyn std::error::Error + Send + Sync>> {
    let model = provider.default_model().to_string();

    // Collect tool names for self-awareness
    let tool_names: Vec<String> = state
        .tool_registry
        .as_ref()
        .map(|r| {
            r.list_for_llm()
                .iter()
                .filter_map(|v| v["name"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    // Build runtime info for agent self-awareness
    let runtime = RuntimeInfo {
        agent_id: Some(agent_id.to_string()),
        model: Some(model.clone()),
        default_model: Some(provider.default_model().to_string()),
        os: Some(std::env::consts::OS.to_string()),
        arch: Some(std::env::consts::ARCH.to_string()),
        host: std::env::var("HOSTNAME")
            .or_else(|_| std::env::var("COMPUTERNAME"))
            .ok(),
        shell: std::env::var("SHELL").ok(),
        channel: Some(turn_source_channel.to_string()),
        workspace_dir: state
            .workspace
            .as_ref()
            .map(|ws| ws.root().to_string_lossy().to_string()),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
    };

    // Hatching mode: use special first-run identity discovery prompt
    let is_hatching = state
        .needs_hatching
        .load(std::sync::atomic::Ordering::Relaxed);

    let prompt = if is_hatching {
        BootstrapRunner::hatching_system_prompt().to_string()
    } else {
        // Build dynamic system prompt from workspace files (SOUL.md, IDENTITY.md, etc.)
        let base_prompt = if let Some(ref ws) = state.workspace {
            system_prompt::load_and_build_with_runtime(ws, None, false, Some(runtime), &tool_names)
                .await
                .ok()
        } else {
            None
        };
        base_prompt.unwrap_or_else(|| {
            "You are a helpful assistant. Respond in the user's language.".to_string()
        })
    };

    let config = AgentConfig::new(agent_id, &model, "default").with_system_prompt(&prompt);
    let mut agent = Agent::new(config, provider.clone()).with_transcript(session_id);
    if let Some(ref gate) = state.approval_gate {
        agent = agent.with_approval_gate(gate.clone());
    }

    // Keep Node-style pre-turn memory recall enabled when memory manager is configured.
    // This complements tool-driven recall and improves cross-session continuity.
    if let Some(ref mm) = state.memory_manager {
        let recall_cfg = oclaw_agent_core::auto_recall::AutoRecallConfig {
            enabled: true,
            max_results: 5,
            min_score: 0.3,
        };
        let recaller = Arc::new(crate::memory_bridge::MemoryManagerRecaller::new(mm.clone()));
        agent = agent.with_auto_recall(recall_cfg, recaller);
    }

    agent.initialize().await.map_err(|e| e.to_string())?;

    let result: Result<String, Box<dyn std::error::Error + Send + Sync>> =
        if let Some(ref registry) = state.tool_registry {
            let mut executor = ToolRegistryExecutor::new(registry.clone())
                .with_session_manager(state.gateway_server.session_manager.clone())
                .with_session_id(session_id.to_string())
                .with_llm_provider(provider.clone())
                .with_session_usage_tokens(state.session_usage_tokens.clone())
                .with_usage_snapshot(state.usage_snapshot.clone());
            executor = executor.with_turn_source(
                turn_source_channel.to_string(),
                turn_source_to,
                turn_source_account_id,
                turn_source_thread_id,
            );
            if let Some(ref cfg) = state.full_config {
                executor = executor.with_full_config(cfg.clone());
            }
            if let Some(ref hooks) = state.hook_pipeline {
                executor = executor.with_hook_pipeline(hooks.clone());
            }
            if let Some(ref cm) = state.channel_manager {
                executor = executor.with_channel_manager(cm.clone());
            }
            if let Some(ref regs) = state.plugin_registrations {
                executor = executor.with_plugin_registrations(regs.clone());
            }
            agent
                .run_with_tools(text, &executor)
                .await
                .map_err(|e| e.to_string().into())
        } else {
            agent.run(text).await.map_err(|e| e.to_string().into())
        };

    // After a hatching turn, check if identity is fully personalized.
    // Require name + at least one extra (emoji/creature/vibe) to avoid
    // clearing the flag before the multi-turn conversation finishes.
    if is_hatching
        && result.is_ok()
        && let Some(ref ws) = state.workspace
        && let Ok(Some(identity)) = oclaw_workspace_core::identity::AgentIdentity::load(ws).await
    {
        let has_name = identity.name.is_some();
        let has_extras =
            identity.emoji.is_some() || identity.creature.is_some() || identity.vibe.is_some();
        if has_name && has_extras {
            state
                .needs_hatching
                .store(false, std::sync::atomic::Ordering::Relaxed);
            info!(
                "[pipeline] hatching complete — identity personalized: {}",
                identity.display_name()
            );

            // Clear old session transcripts so every channel starts fresh
            if let Err(e) = oclaw_agent_core::transcript::Transcript::clear_all_sessions().await {
                warn!("[pipeline] failed to clear old session transcripts: {}", e);
            } else {
                info!("[pipeline] cleared old session transcripts after hatching");
            }
        }
    }

    let reply = result?;
    let usage_tokens = u64::try_from(agent.usage().total_tokens()).unwrap_or(0);
    Ok(AgentRunOutcome {
        reply,
        usage_tokens,
    })
}

async fn send_reply(
    state: &HttpState,
    channel_name: &str,
    reply: &str,
    metadata: HashMap<String, String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let manager = state.channel_manager.as_ref().ok_or("No channel manager")?;
    let mgr = manager.read().await;
    let channel = mgr
        .get(channel_name)
        .await
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
    ch.send_message(&msg)
        .await
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
    let flush_prompt = config
        .prompt
        .as_deref()
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

    let mut executor = ToolRegistryExecutor::new(registry.clone())
        .with_session_manager(state.gateway_server.session_manager.clone())
        .with_session_id(session_id.to_string())
        .with_llm_provider(provider.clone())
        .with_session_usage_tokens(state.session_usage_tokens.clone())
        .with_usage_snapshot(state.usage_snapshot.clone());
    if let Some(ref cfg) = state.full_config {
        executor = executor.with_full_config(cfg.clone());
    }
    if let Some(ref hooks) = state.hook_pipeline {
        executor = executor.with_hook_pipeline(hooks.clone());
    }
    if let Some(ref cm) = state.channel_manager {
        executor = executor.with_channel_manager(cm.clone());
    }
    match agent
        .run_with_tools("Flush session memories now.", &executor)
        .await
    {
        Ok(reply) => {
            let trimmed = reply.trim();
            if trimmed == SILENT_REPLY_TOKEN || trimmed == HEARTBEAT_OK_TOKEN || trimmed.is_empty()
            {
                info!("[pipeline] memory-flush: nothing to store");
            } else if let Some(ref mm) = state.memory_manager {
                let source = format!("memory_flush:{}", session_id);
                match mm.add_memory_text(trimmed, &source).await {
                    Ok(ids) => {
                        info!(
                            "[pipeline] memory-flush: persisted {} chunk(s) into memory store",
                            ids.len()
                        );
                    }
                    Err(e) => {
                        warn!(
                            "[pipeline] memory-flush: failed to persist to memory store: {}",
                            e
                        );
                    }
                }
            } else {
                info!("[pipeline] memory-flush: memory manager unavailable, skip persistence");
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

    let config = state.pipeline_config.evolution.clone();
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
        if let Err(save_err) = evo_state.save(ws).await {
            warn!("[pipeline] evolution state save failed: {}", save_err);
        }
        return;
    }

    let mut executor = ToolRegistryExecutor::new(registry.clone())
        .with_session_manager(state.gateway_server.session_manager.clone())
        .with_llm_provider(provider.clone())
        .with_session_usage_tokens(state.session_usage_tokens.clone())
        .with_usage_snapshot(state.usage_snapshot.clone());
    if let Some(ref cfg) = state.full_config {
        executor = executor.with_full_config(cfg.clone());
    }
    if let Some(ref hooks) = state.hook_pipeline {
        executor = executor.with_hook_pipeline(hooks.clone());
    }
    if let Some(ref cm) = state.channel_manager {
        executor = executor.with_channel_manager(cm.clone());
    }
    match agent.run_with_tools("Reflect and evolve.", &executor).await {
        Ok(reply) => {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            evo_state.last_evolved_at_message = evo_state.message_count;
            evo_state.last_evolved_date = Some(today);
            evo_state.evolution_count += 1;
            info!(
                "[pipeline] evolution complete ({}): {}",
                evo_state.evolution_count,
                reply.trim()
            );
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
        AgentBinding::Channel {
            channel: channel.to_string(),
            agent_id: agent_id.to_string(),
        }
    }
    fn default_binding(agent_id: &str) -> AgentBinding {
        AgentBinding::Default {
            agent_id: agent_id.to_string(),
        }
    }

    #[test]
    fn peer_beats_channel() {
        let bindings = vec![
            channel_binding("discord", "channel-agent"),
            peer("discord", "u1", "vip-agent"),
        ];
        let ctx = MessageContext {
            channel: "discord",
            peer_id: "u1",
            ..Default::default()
        };
        assert_eq!(
            resolve_agent_for_message(&bindings, &ctx),
            Some("vip-agent".to_string())
        );
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
        assert_eq!(
            resolve_agent_for_message(&bindings, &ctx),
            Some("admin-agent".to_string())
        );
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
        assert_eq!(
            resolve_agent_for_message(&bindings, &ctx),
            Some("guild-agent".to_string())
        );
    }

    #[test]
    fn default_fallback_when_no_match() {
        let bindings = vec![
            peer("telegram", "123", "tg-agent"),
            default_binding("fallback"),
        ];
        let ctx = MessageContext {
            channel: "slack",
            peer_id: "U9",
            ..Default::default()
        };
        assert_eq!(
            resolve_agent_for_message(&bindings, &ctx),
            Some("fallback".to_string())
        );
    }

    #[test]
    fn no_match_returns_none() {
        let bindings = vec![peer("telegram", "123", "tg-agent")];
        let ctx = MessageContext {
            channel: "slack",
            peer_id: "X",
            ..Default::default()
        };
        assert_eq!(resolve_agent_for_message(&bindings, &ctx), None);
    }

    #[test]
    fn empty_bindings_returns_none() {
        let ctx = MessageContext {
            channel: "telegram",
            peer_id: "1",
            ..Default::default()
        };
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
