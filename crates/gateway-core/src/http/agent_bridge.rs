use crate::message::SessionManager;
use oclaw_agent_core::agent::{Agent, AgentConfig, ToolExecutor};
use oclaw_agent_core::usage::UsageSummary;
use oclaw_channel_core::{
    ChannelManager,
    traits::ChannelMessage,
    types::{ChannelMedia, MediaData, MediaType, PollRequest},
};
use oclaw_llm_core::chat::{Tool, ToolFunction};
use oclaw_llm_core::providers::LlmProvider;
use oclaw_plugin_core::HookPipeline;
use oclaw_plugin_core::PluginRegistrations;
use oclaw_tools_core::tool::ToolRegistry;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::task::AbortHandle;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SubagentRunEntry {
    #[serde(alias = "runId")]
    run_id: String,
    #[serde(alias = "requesterSessionKey")]
    requester_session_key: String,
    #[serde(alias = "childSessionKey")]
    child_session_key: String,
    label: String,
    task: String,
    model: String,
    #[serde(alias = "startedAt")]
    started_at_ms: i64,
    #[serde(alias = "endedAt")]
    ended_at_ms: Option<i64>,
    status: String,
    error: Option<String>,
    usage: Option<UsageSummary>,
    #[serde(default)]
    #[serde(alias = "completionReply")]
    completion_reply: Option<String>,
    #[serde(alias = "spawnMode")]
    spawn_mode: String,
    cleanup: String,
    #[serde(alias = "threadRequested")]
    thread_requested: bool,
    #[serde(default = "default_true")]
    #[serde(alias = "expectsCompletionMessage")]
    expects_completion_message: bool,
    #[serde(default)]
    #[serde(alias = "suppressAnnounceReason")]
    suppress_announce_reason: Option<String>,
    #[serde(default)]
    #[serde(alias = "announceRetryCount")]
    announce_retry_count: u32,
    #[serde(default)]
    #[serde(alias = "lastAnnounceRetryAt")]
    last_announce_retry_at_ms: Option<i64>,
    #[serde(default)]
    #[serde(alias = "cleanupHandled")]
    #[serde(alias = "announceHandled")]
    cleanup_handled: bool,
    #[serde(default)]
    #[serde(alias = "cleanupCompletedAt")]
    #[serde(alias = "announceCompletedAt")]
    cleanup_completed_at_ms: Option<i64>,
    #[serde(default)]
    #[serde(alias = "endedReason")]
    ended_reason: Option<String>,
    #[serde(default)]
    #[serde(alias = "endedHookEmittedAt")]
    ended_hook_emitted_at_ms: Option<i64>,
    #[serde(default)]
    #[serde(alias = "runTimeoutSeconds")]
    run_timeout_seconds: u64,
    #[serde(default)]
    #[serde(alias = "archiveAtMs")]
    archive_at_ms: Option<i64>,
}

static SUBAGENT_RUNS: Lazy<std::sync::Mutex<HashMap<String, SubagentRunEntry>>> =
    Lazy::new(|| std::sync::Mutex::new(HashMap::new()));
static SUBAGENT_ABORTS: Lazy<std::sync::Mutex<HashMap<String, AbortHandle>>> =
    Lazy::new(|| std::sync::Mutex::new(HashMap::new()));
static SUBAGENT_CLEANUP_TASKS: Lazy<std::sync::Mutex<HashSet<String>>> =
    Lazy::new(|| std::sync::Mutex::new(HashSet::new()));
static SUBAGENT_ANNOUNCE_QUEUES: Lazy<
    std::sync::Mutex<HashMap<String, SubagentAnnounceQueueState>>,
> = Lazy::new(|| std::sync::Mutex::new(HashMap::new()));
static SUBAGENT_ANNOUNCE_DELIVERED: Lazy<std::sync::Mutex<HashMap<String, i64>>> =
    Lazy::new(|| std::sync::Mutex::new(HashMap::new()));
static SUBAGENT_RUNS_LOADED: AtomicBool = AtomicBool::new(false);

const ANNOUNCE_SKIP_TOKEN: &str = "ANNOUNCE_SKIP";
const REPLY_SKIP_TOKEN: &str = "REPLY_SKIP";
const DEFAULT_A2A_PING_PONG_TURNS: i64 = 5;
const MAX_A2A_PING_PONG_TURNS: i64 = 5;
const DEFAULT_SUBAGENT_MAX_SPAWN_DEPTH: u32 = 1;
const DEFAULT_SUBAGENT_MAX_CHILDREN_PER_AGENT: usize = 5;
const SUBAGENT_SPAWN_ACCEPTED_NOTE: &str = "auto-announces on completion, do not poll/sleep. The response will be sent back as an user message.";
const SUBAGENT_SPAWN_SESSION_ACCEPTED_NOTE: &str =
    "thread-bound session stays active after this task; continue in-thread for follow-ups.";
const SUBAGENT_MIN_ANNOUNCE_RETRY_DELAY_MS: i64 = 1_000;
const SUBAGENT_MAX_ANNOUNCE_RETRY_DELAY_MS: i64 = 8_000;
const SUBAGENT_MAX_ANNOUNCE_RETRY_COUNT: u32 = 3;
const SUBAGENT_ANNOUNCE_EXPIRY_MS: i64 = 5 * 60_000;
const SUBAGENT_OUTPUT_RETRY_INTERVAL_MS: i64 = 100;
const SUBAGENT_OUTPUT_RETRY_MAX_WAIT_MS: i64 = 15_000;
const SUBAGENT_OUTPUT_CHANGE_MIN_WAIT_MS: i64 = 250;
const SUBAGENT_OUTPUT_CHANGE_MAX_WAIT_MS: i64 = 2_000;
const SUBAGENT_DEFAULT_ANNOUNCE_QUEUE_DEBOUNCE_MS: i64 = 1_000;
const SUBAGENT_DEFAULT_ANNOUNCE_QUEUE_CAP: usize = 20;
const SUBAGENT_ANNOUNCE_IDEMPOTENCY_TTL_MS: i64 = 10 * 60_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SubagentAnnounceQueueMode {
    Steer,
    Followup,
    Collect,
    SteerBacklog,
    Interrupt,
}

impl Default for SubagentAnnounceQueueMode {
    fn default() -> Self {
        Self::Collect
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SubagentAnnounceDropPolicy {
    Old,
    New,
    Summarize,
}

impl Default for SubagentAnnounceDropPolicy {
    fn default() -> Self {
        Self::Summarize
    }
}

#[derive(Clone, Debug)]
struct SubagentAnnounceQueueSettings {
    mode: SubagentAnnounceQueueMode,
    debounce_ms: i64,
    cap: usize,
    drop_policy: SubagentAnnounceDropPolicy,
}

impl Default for SubagentAnnounceQueueSettings {
    fn default() -> Self {
        Self {
            mode: SubagentAnnounceQueueMode::Collect,
            debounce_ms: SUBAGENT_DEFAULT_ANNOUNCE_QUEUE_DEBOUNCE_MS,
            cap: SUBAGENT_DEFAULT_ANNOUNCE_QUEUE_CAP,
            drop_policy: SubagentAnnounceDropPolicy::Summarize,
        }
    }
}

#[derive(Clone, Debug)]
struct SubagentAnnounceQueueItem {
    run_id: String,
    prompt: String,
    summary_line: Option<String>,
    enqueued_at_ms: i64,
    session_key: String,
    target: Option<AnnounceTarget>,
    origin_key: Option<String>,
}

#[derive(Clone, Debug)]
struct SubagentAnnounceQueueState {
    items: VecDeque<SubagentAnnounceQueueItem>,
    draining: bool,
    last_enqueued_at_ms: i64,
    mode: SubagentAnnounceQueueMode,
    debounce_ms: i64,
    cap: usize,
    drop_policy: SubagentAnnounceDropPolicy,
    dropped_count: usize,
    summary_lines: Vec<String>,
}

impl SubagentAnnounceQueueState {
    fn new(settings: SubagentAnnounceQueueSettings) -> Self {
        Self {
            items: VecDeque::new(),
            draining: false,
            last_enqueued_at_ms: 0,
            mode: settings.mode,
            debounce_ms: settings.debounce_ms,
            cap: settings.cap,
            drop_policy: settings.drop_policy,
            dropped_count: 0,
            summary_lines: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SessionBindingConversationRef {
    channel: String,
    #[serde(default)]
    #[serde(alias = "accountId")]
    account_id: String,
    #[serde(alias = "conversationId")]
    conversation_id: String,
    #[serde(default)]
    #[serde(alias = "parentConversationId")]
    parent_conversation_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SessionBindingRecord {
    #[serde(alias = "bindingId")]
    binding_id: String,
    #[serde(alias = "targetSessionKey")]
    target_session_key: String,
    #[serde(alias = "targetKind")]
    target_kind: String,
    conversation: SessionBindingConversationRef,
    status: String,
    #[serde(alias = "boundAt")]
    bound_at_ms: i64,
    #[serde(default)]
    #[serde(alias = "expiresAt")]
    expires_at_ms: Option<i64>,
    #[serde(default)]
    metadata: Option<serde_json::Value>,
}

#[derive(Clone, Debug)]
struct BoundDeliveryRoute {
    binding: Option<SessionBindingRecord>,
    mode: &'static str,
    reason: &'static str,
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug)]
struct AnnounceTarget {
    channel: String,
    to: String,
    account_id: Option<String>,
    thread_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SessionVisibility {
    SelfOnly,
    Tree,
    Agent,
    All,
}

#[derive(Clone, Copy, Debug)]
enum SessionAccessAction {
    List,
    History,
    Send,
    Status,
}

#[derive(Clone, Debug, Default)]
struct AgentToAgentPolicy {
    enabled: bool,
    allow_patterns: Vec<String>,
}

#[derive(Clone, Debug)]
struct SessionAccessContext {
    requester_key: String,
    requester_agent_id: String,
    visibility: SessionVisibility,
    a2a: AgentToAgentPolicy,
    tree_visible_keys: Option<HashSet<String>>,
}

fn now_epoch_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn normalize_subagent_announce_queue_mode(raw: Option<&str>) -> SubagentAnnounceQueueMode {
    let value = raw
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("collect")
        .to_ascii_lowercase();
    match value.as_str() {
        "steer" => SubagentAnnounceQueueMode::Steer,
        "steerbacklog" | "steer-backlog" | "steer+backlog" => {
            SubagentAnnounceQueueMode::SteerBacklog
        }
        "interrupt" => SubagentAnnounceQueueMode::Interrupt,
        "followup" | "follow-up" | "queue" => SubagentAnnounceQueueMode::Followup,
        _ => SubagentAnnounceQueueMode::Collect,
    }
}

fn normalize_subagent_announce_drop_policy(raw: Option<&str>) -> SubagentAnnounceDropPolicy {
    let value = raw
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("summarize")
        .to_ascii_lowercase();
    match value.as_str() {
        "old" => SubagentAnnounceDropPolicy::Old,
        "new" => SubagentAnnounceDropPolicy::New,
        _ => SubagentAnnounceDropPolicy::Summarize,
    }
}

fn build_announce_origin_key(target: Option<&AnnounceTarget>) -> Option<String> {
    let t = target?;
    Some(format!(
        "{}|{}|{}|{}",
        t.channel.trim().to_ascii_lowercase(),
        t.to.trim().to_ascii_lowercase(),
        t.account_id
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase(),
        t.thread_id
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase(),
    ))
}

fn elide_announce_text(text: &str, limit: usize) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= limit {
        return trimmed.to_string();
    }
    trimmed
        .chars()
        .take(limit.saturating_sub(1))
        .collect::<String>()
        + "…"
}

fn build_announce_summary_line(text: &str) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    elide_announce_text(&compact, 160)
}

fn build_announce_queue_summary_prompt(queue: &SubagentAnnounceQueueState) -> Option<String> {
    if queue.drop_policy != SubagentAnnounceDropPolicy::Summarize || queue.dropped_count == 0 {
        return None;
    }
    let mut lines = vec![format!(
        "[Queue overflow] Dropped {} announce{} due to cap.",
        queue.dropped_count,
        if queue.dropped_count == 1 { "" } else { "s" }
    )];
    if !queue.summary_lines.is_empty() {
        lines.push("Summary:".to_string());
        for line in &queue.summary_lines {
            lines.push(format!("- {}", line));
        }
    }
    Some(lines.join("\n"))
}

fn build_announce_collect_prompt(
    items: &[SubagentAnnounceQueueItem],
    summary: Option<&str>,
) -> Option<String> {
    if items.is_empty() {
        return None;
    }
    let mut blocks = vec!["[Queued announce messages while agent was busy]".to_string()];
    if let Some(text) = summary.map(str::trim).filter(|v| !v.is_empty()) {
        blocks.push(text.to_string());
    }
    for (idx, item) in items.iter().enumerate() {
        let mut header = format!("Queued #{}", idx + 1);
        let source_session = item.session_key.trim();
        if !source_session.is_empty() {
            header.push_str(&format!(" | source_session={}", source_session));
        }
        if item.enqueued_at_ms > 0 {
            if let Some(ts) =
                chrono::DateTime::<chrono::Utc>::from_timestamp_millis(item.enqueued_at_ms)
            {
                header.push_str(&format!(" | enqueued_at={}", ts.to_rfc3339()));
            } else {
                header.push_str(&format!(" | enqueued_at_ms={}", item.enqueued_at_ms));
            }
        }
        blocks.push(format!("---\n{}\n{}", header, item.prompt));
    }
    Some(blocks.join("\n\n"))
}

fn is_subagent_session_key(session_key: &str) -> bool {
    session_key.contains(":subagent:")
}

fn announce_items_cross_channel(items: &VecDeque<SubagentAnnounceQueueItem>) -> bool {
    let mut seen_key: Option<String> = None;
    let mut has_unkeyed = false;
    for item in items {
        if let Some(key) = item.origin_key.as_ref() {
            if let Some(existing) = seen_key.as_ref() {
                if existing != key {
                    return true;
                }
            } else {
                seen_key = Some(key.clone());
            }
        } else {
            has_unkeyed = true;
        }
    }
    seen_key.is_some() && has_unkeyed
}

fn resolve_requester_for_child_session_from_runs(
    snapshot: &[SubagentRunEntry],
    child_session_key: &str,
) -> Option<String> {
    let key = child_session_key.trim();
    if key.is_empty() {
        return None;
    }
    snapshot
        .iter()
        .filter(|entry| entry.child_session_key.trim() == key)
        .max_by_key(|entry| entry.started_at_ms)
        .map(|entry| entry.requester_session_key.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn is_subagent_session_run_active_from_runs(
    snapshot: &[SubagentRunEntry],
    child_session_key: &str,
) -> bool {
    let key = child_session_key.trim();
    if key.is_empty() {
        return false;
    }
    snapshot
        .iter()
        .any(|entry| entry.child_session_key.trim() == key && entry.ended_at_ms.is_none())
}

fn normalize_account_id(raw: Option<&str>) -> String {
    raw.map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_ascii_lowercase())
        .unwrap_or_default()
}

fn parse_conversation_id_from_target(to: &str) -> Option<String> {
    let trimmed = to.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(rest) = trimmed.strip_prefix("channel:") {
        return Some(rest.trim().to_string());
    }
    if let Some(rest) = trimmed.strip_prefix("group:") {
        return Some(rest.trim().to_string());
    }
    Some(trimmed.to_string())
}

fn binding_record_to_announce_target(record: &SessionBindingRecord) -> Option<AnnounceTarget> {
    let channel = normalize_channel_alias(record.conversation.channel.as_str());
    let conversation_id = record.conversation.conversation_id.trim();
    if channel.is_empty() || conversation_id.is_empty() {
        return None;
    }
    let to = if channel == "discord" || channel == "slack" {
        format!("channel:{}", conversation_id)
    } else {
        format!("channel:{}", conversation_id)
    };
    Some(AnnounceTarget {
        channel,
        to,
        account_id: Some(normalize_account_id(Some(
            record.conversation.account_id.as_str(),
        ))),
        thread_id: Some(conversation_id.to_string()),
    })
}

fn target_to_requester_conversation(
    target: &AnnounceTarget,
) -> Option<SessionBindingConversationRef> {
    let channel = normalize_channel_alias(target.channel.as_str());
    if channel.is_empty() {
        return None;
    }
    let conversation_id = if let Some(thread_id) = target
        .thread_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        thread_id.to_string()
    } else {
        parse_conversation_id_from_target(target.to.as_str())?
    };
    let parent_conversation_id = parse_conversation_id_from_target(target.to.as_str());
    Some(SessionBindingConversationRef {
        channel,
        account_id: normalize_account_id(target.account_id.as_deref()),
        conversation_id,
        parent_conversation_id,
    })
}

fn resolve_bound_delivery_route_from_bindings(
    active_bindings: &[SessionBindingRecord],
    requester: Option<&SessionBindingConversationRef>,
    fail_closed: bool,
) -> BoundDeliveryRoute {
    if active_bindings.is_empty() {
        return BoundDeliveryRoute {
            binding: None,
            mode: "fallback",
            reason: "no-active-binding",
        };
    }
    if requester.is_none() {
        if active_bindings.len() == 1 {
            return BoundDeliveryRoute {
                binding: active_bindings.first().cloned(),
                mode: "bound",
                reason: "single-active-binding",
            };
        }
        return BoundDeliveryRoute {
            binding: None,
            mode: "fallback",
            reason: "ambiguous-without-requester",
        };
    }
    let requester = requester.expect("checked above");
    let requester_channel = normalize_channel_alias(&requester.channel);
    let requester_account_id = normalize_account_id(Some(requester.account_id.as_str()));
    let requester_conversation_id = requester.conversation_id.trim();
    if requester_channel.is_empty() || requester_conversation_id.is_empty() {
        return BoundDeliveryRoute {
            binding: None,
            mode: "fallback",
            reason: "invalid-requester",
        };
    }
    let matching_channel_account: Vec<SessionBindingRecord> = active_bindings
        .iter()
        .filter(|entry| {
            normalize_channel_alias(&entry.conversation.channel) == requester_channel
                && normalize_account_id(Some(entry.conversation.account_id.as_str()))
                    == requester_account_id
        })
        .cloned()
        .collect();
    if matching_channel_account.is_empty() {
        if active_bindings.len() == 1 && !fail_closed {
            return BoundDeliveryRoute {
                binding: active_bindings.first().cloned(),
                mode: "bound",
                reason: "single-active-binding-fallback",
            };
        }
        return BoundDeliveryRoute {
            binding: None,
            mode: "fallback",
            reason: "no-requester-match",
        };
    }
    if let Some(exact) = matching_channel_account
        .iter()
        .find(|entry| entry.conversation.conversation_id.trim() == requester_conversation_id)
    {
        return BoundDeliveryRoute {
            binding: Some(exact.clone()),
            mode: "bound",
            reason: "requester-match",
        };
    }
    if matching_channel_account.len() == 1 {
        return BoundDeliveryRoute {
            binding: matching_channel_account.first().cloned(),
            mode: "bound",
            reason: "single-active-binding-fallback",
        };
    }
    BoundDeliveryRoute {
        binding: None,
        mode: "fallback",
        reason: "no-requester-match",
    }
}

fn completion_direct_force_by_route(spawn_mode: &str, route_mode: &str) -> bool {
    spawn_mode.eq_ignore_ascii_case("session")
        && (route_mode.eq_ignore_ascii_case("bound") || route_mode.eq_ignore_ascii_case("hook"))
}

fn should_defer_completion_direct_delivery(
    active_requester_descendants: usize,
    spawn_mode: &str,
    route_mode: &str,
) -> bool {
    if active_requester_descendants == 0 {
        return false;
    }
    !completion_direct_force_by_route(spawn_mode, route_mode)
}

fn resolve_user_path_for_subagent_registry(input: &str) -> PathBuf {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return PathBuf::new();
    }
    let mut candidate = if trimmed == "~" {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("~"))
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(rest)
    } else if let Some(rest) = trimmed.strip_prefix("~\\") {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(rest)
    } else {
        PathBuf::from(trimmed)
    };
    if !candidate.is_absolute()
        && let Ok(cwd) = std::env::current_dir()
    {
        candidate = cwd.join(candidate);
    }
    candidate
}

fn resolve_subagent_registry_state_dir() -> PathBuf {
    let override_dir = std::env::var("OCLAWS_STATE_DIR")
        .ok()
        .or_else(|| std::env::var("OPENCLAW_STATE_DIR").ok())
        .or_else(|| std::env::var("CLAWDBOT_STATE_DIR").ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    if let Some(dir) = override_dir {
        return resolve_user_path_for_subagent_registry(&dir);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".oclaw")
}

fn subagent_registry_file_path() -> PathBuf {
    resolve_subagent_registry_state_dir()
        .join("runtime")
        .join("subagent-runs.json")
}

fn persist_subagent_registry_to_disk(runs: &HashMap<String, SubagentRunEntry>) {
    let path = subagent_registry_file_path();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let payload = serde_json::json!({
        "version": 2,
        "updated_at_ms": chrono::Utc::now().timestamp_millis(),
        "runs": runs
    });
    if let Ok(bytes) = serde_json::to_vec_pretty(&payload) {
        let _ = std::fs::write(path, bytes);
    }
}

fn ensure_subagent_registry_loaded() {
    if SUBAGENT_RUNS_LOADED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }
    let path = subagent_registry_file_path();
    let Ok(raw) = std::fs::read_to_string(path) else {
        return;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return;
    };
    let mut restored: HashMap<String, SubagentRunEntry> = parsed
        .get("runs")
        .cloned()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();
    if restored.is_empty()
        && let Ok(fallback) =
            serde_json::from_value::<HashMap<String, SubagentRunEntry>>(parsed.clone())
    {
        restored = fallback;
    }
    if restored.is_empty() {
        return;
    }
    let now_ms = chrono::Utc::now().timestamp_millis();
    let mut mutated = false;
    for entry in restored.values_mut() {
        if entry.ended_at_ms.is_none() {
            entry.ended_at_ms = Some(now_ms);
            entry.status = "interrupted".to_string();
            entry.error = Some("gateway restarted before subagent run completed".to_string());
            entry.ended_reason = Some("subagent-error".to_string());
            entry.cleanup_handled = false;
            entry.cleanup_completed_at_ms = None;
            if entry.archive_at_ms.is_none() {
                entry.archive_at_ms = Some(now_ms.saturating_add(60 * 60_000));
            }
            mutated = true;
        }
    }
    {
        let mut guard = SUBAGENT_RUNS.lock().unwrap_or_else(|e| e.into_inner());
        for (run_id, entry) in restored {
            guard.insert(run_id, entry);
        }
        if mutated {
            persist_subagent_registry_to_disk(&guard);
        }
    }
}

#[derive(Debug, Clone)]
struct TurnSourceRoute {
    channel: String,
    to: Option<String>,
    account_id: Option<String>,
    thread_id: Option<String>,
}

/// Bridges tools-core's ToolRegistry to agent-core's ToolExecutor trait.
/// Also merges plugin-registered tools when available.
#[derive(Clone)]
pub struct ToolRegistryExecutor {
    registry: Arc<ToolRegistry>,
    plugin_regs: Option<Arc<PluginRegistrations>>,
    hook_pipeline: Option<Arc<HookPipeline>>,
    channel_manager: Option<Arc<RwLock<ChannelManager>>>,
    session_manager: Option<Arc<RwLock<SessionManager>>>,
    full_config: Option<Arc<RwLock<oclaw_config::settings::Config>>>,
    session_usage_tokens: Option<Arc<std::sync::Mutex<HashMap<String, u64>>>>,
    usage_snapshot: Option<Arc<RwLock<crate::http::GatewayUsageSnapshot>>>,
    llm_provider: Option<Arc<dyn LlmProvider>>,
    session_id: Option<String>,
    turn_source: Option<TurnSourceRoute>,
}

impl ToolRegistryExecutor {
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self {
            registry,
            plugin_regs: None,
            hook_pipeline: None,
            channel_manager: None,
            session_manager: None,
            full_config: None,
            session_usage_tokens: None,
            usage_snapshot: None,
            llm_provider: None,
            session_id: None,
            turn_source: None,
        }
    }

    pub fn with_plugin_registrations(mut self, regs: Arc<PluginRegistrations>) -> Self {
        self.plugin_regs = Some(regs);
        self
    }

    pub fn with_hook_pipeline(mut self, pipeline: Arc<HookPipeline>) -> Self {
        self.hook_pipeline = Some(pipeline);
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
        usage_tokens: Arc<std::sync::Mutex<HashMap<String, u64>>>,
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

    pub fn with_llm_provider(mut self, provider: Arc<dyn LlmProvider>) -> Self {
        self.llm_provider = Some(provider);
        self
    }

    pub fn with_session_id(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn with_turn_source(
        mut self,
        channel: impl Into<String>,
        to: Option<String>,
        account_id: Option<String>,
        thread_id: Option<String>,
    ) -> Self {
        let channel = channel.into();
        if channel.trim().is_empty() {
            return self;
        }
        self.turn_source = Some(TurnSourceRoute {
            channel: channel.trim().to_string(),
            to: to.map(|v| v.trim().to_string()).filter(|v| !v.is_empty()),
            account_id: account_id
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
            thread_id: thread_id
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty()),
        });
        self
    }

    fn build_child_executor(&self, session_id: impl Into<String>) -> Self {
        let mut exec = Self::new(self.registry.clone()).with_session_id(session_id);
        if let Some(ref regs) = self.plugin_regs {
            exec = exec.with_plugin_registrations(regs.clone());
        }
        if let Some(ref hooks) = self.hook_pipeline {
            exec = exec.with_hook_pipeline(hooks.clone());
        }
        if let Some(ref cm) = self.channel_manager {
            exec = exec.with_channel_manager(cm.clone());
        }
        if let Some(ref sm) = self.session_manager {
            exec = exec.with_session_manager(sm.clone());
        }
        if let Some(ref cfg) = self.full_config {
            exec = exec.with_full_config(cfg.clone());
        }
        if let Some(ref usage_tokens) = self.session_usage_tokens {
            exec = exec.with_session_usage_tokens(usage_tokens.clone());
        }
        if let Some(ref usage_snapshot) = self.usage_snapshot {
            exec = exec.with_usage_snapshot(usage_snapshot.clone());
        }
        if let Some(ref provider) = self.llm_provider {
            exec = exec.with_llm_provider(provider.clone());
        }
        if let Some(ref turn_source) = self.turn_source {
            exec.turn_source = Some(turn_source.clone());
        }
        exec
    }

    async fn resolve_session_visibility(&self) -> SessionVisibility {
        let Some(cfg_lock) = self.full_config.as_ref() else {
            return SessionVisibility::Tree;
        };
        let cfg = cfg_lock.read().await;
        let raw = cfg
            .tools
            .as_ref()
            .and_then(|tools| tools.pointer("/sessions/visibility"))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_ascii_lowercase())
            .unwrap_or_else(|| "tree".to_string());
        match raw.as_str() {
            "self" => SessionVisibility::SelfOnly,
            "tree" => SessionVisibility::Tree,
            "agent" => SessionVisibility::Agent,
            "all" => SessionVisibility::All,
            _ => SessionVisibility::Tree,
        }
    }

    async fn resolve_agent_to_agent_policy(&self) -> AgentToAgentPolicy {
        let Some(cfg_lock) = self.full_config.as_ref() else {
            return AgentToAgentPolicy::default();
        };
        let cfg = cfg_lock.read().await;
        let enabled = cfg
            .tools
            .as_ref()
            .and_then(|tools| tools.pointer("/agentToAgent/enabled"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let allow_patterns = cfg
            .tools
            .as_ref()
            .and_then(|tools| tools.pointer("/agentToAgent/allow"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|entry| entry.as_str().map(str::trim))
                    .filter(|entry| !entry.is_empty())
                    .map(ToString::to_string)
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();
        AgentToAgentPolicy {
            enabled,
            allow_patterns,
        }
    }

    fn resolve_agent_id_from_session_key(key: &str) -> String {
        let trimmed = key.trim();
        if trimmed.is_empty() {
            return "default".to_string();
        }
        let mut parts = trimmed.split(':');
        if parts.next() == Some("agent")
            && let Some(agent) = parts.next().map(str::trim).filter(|v| !v.is_empty())
        {
            return sanitize_session_token(agent);
        }
        "default".to_string()
    }

    fn allow_pattern_matches(pattern: &str, agent_id: &str) -> bool {
        let pat = pattern.trim();
        if pat.is_empty() {
            return false;
        }
        if pat == "*" {
            return true;
        }
        if !pat.contains('*') {
            return pat.eq_ignore_ascii_case(agent_id);
        }
        let re_src = regex::escape(pat).replace("\\*", ".*");
        regex::Regex::new(&format!("(?i)^{}$", re_src))
            .map(|re| re.is_match(agent_id))
            .unwrap_or(false)
    }

    fn is_a2a_allowed(
        policy: &AgentToAgentPolicy,
        requester_agent: &str,
        target_agent: &str,
    ) -> bool {
        if requester_agent == target_agent {
            return true;
        }
        if !policy.enabled {
            return false;
        }
        if policy.allow_patterns.is_empty() {
            return true;
        }
        let requester_match = policy
            .allow_patterns
            .iter()
            .any(|p| Self::allow_pattern_matches(p, requester_agent));
        let target_match = policy
            .allow_patterns
            .iter()
            .any(|p| Self::allow_pattern_matches(p, target_agent));
        requester_match && target_match
    }

    fn session_action_label(action: SessionAccessAction) -> &'static str {
        match action {
            SessionAccessAction::History => "Session history",
            SessionAccessAction::Send => "Session send",
            SessionAccessAction::List => "Session list",
            SessionAccessAction::Status => "Session status",
        }
    }

    fn cross_visibility_error(action: SessionAccessAction) -> String {
        match action {
            SessionAccessAction::History => {
                "Session history visibility is restricted. Set tools.sessions.visibility=all to allow cross-agent access."
                    .to_string()
            }
            SessionAccessAction::Send => {
                "Session send visibility is restricted. Set tools.sessions.visibility=all to allow cross-agent access."
                    .to_string()
            }
            SessionAccessAction::List => {
                "Session list visibility is restricted. Set tools.sessions.visibility=all to allow cross-agent access."
                    .to_string()
            }
            SessionAccessAction::Status => {
                "Session status visibility is restricted. Set tools.sessions.visibility=all to allow cross-agent access."
                    .to_string()
            }
        }
    }

    fn a2a_disabled_error(action: SessionAccessAction) -> String {
        match action {
            SessionAccessAction::History => {
                "Agent-to-agent history is disabled. Set tools.agentToAgent.enabled=true to allow cross-agent access."
                    .to_string()
            }
            SessionAccessAction::Send => {
                "Agent-to-agent messaging is disabled. Set tools.agentToAgent.enabled=true to allow cross-agent sends."
                    .to_string()
            }
            SessionAccessAction::List => {
                "Agent-to-agent listing is disabled. Set tools.agentToAgent.enabled=true to allow cross-agent visibility."
                    .to_string()
            }
            SessionAccessAction::Status => {
                "Agent-to-agent status is disabled. Set tools.agentToAgent.enabled=true to allow cross-agent access."
                    .to_string()
            }
        }
    }

    fn a2a_denied_error(action: SessionAccessAction) -> String {
        match action {
            SessionAccessAction::History => {
                "Agent-to-agent history denied by tools.agentToAgent.allow.".to_string()
            }
            SessionAccessAction::Send => {
                "Agent-to-agent messaging denied by tools.agentToAgent.allow.".to_string()
            }
            SessionAccessAction::List => {
                "Agent-to-agent listing denied by tools.agentToAgent.allow.".to_string()
            }
            SessionAccessAction::Status => {
                "Agent-to-agent session status denied by tools.agentToAgent.allow.".to_string()
            }
        }
    }

    fn self_visibility_error(action: SessionAccessAction) -> String {
        format!(
            "{} visibility is restricted to the current session (tools.sessions.visibility=self).",
            Self::session_action_label(action)
        )
    }

    fn tree_visibility_error(action: SessionAccessAction) -> String {
        format!(
            "{} visibility is restricted to the current session tree (tools.sessions.visibility=tree).",
            Self::session_action_label(action)
        )
    }

    fn collect_tree_visible_keys(
        &self,
        mgr: &SessionManager,
        requester_key: &str,
    ) -> Result<HashSet<String>, String> {
        let sessions = mgr.list_sessions()?;
        let mut by_parent: HashMap<String, Vec<String>> = HashMap::new();
        for session in sessions {
            if let Ok(meta) = mgr.get_session_metadata(&session.key) {
                let parent = meta
                    .get("spawnedBy")
                    .or_else(|| meta.get("parentSessionKey"))
                    .map(|v| v.trim())
                    .filter(|v| !v.is_empty())
                    .map(ToString::to_string);
                if let Some(parent_key) = parent {
                    by_parent
                        .entry(parent_key)
                        .or_default()
                        .push(session.key.clone());
                }
            }
        }
        let mut out: HashSet<String> = HashSet::new();
        let mut stack = vec![requester_key.to_string()];
        while let Some(current) = stack.pop() {
            if !out.insert(current.clone()) {
                continue;
            }
            if let Some(children) = by_parent.get(&current) {
                for child in children {
                    stack.push(child.clone());
                }
            }
        }
        Ok(out)
    }

    async fn build_session_access_context(
        &self,
        mgr: &SessionManager,
    ) -> Result<SessionAccessContext, String> {
        let requester_key = self
            .session_id
            .clone()
            .unwrap_or_else(|| "agent:default:main".to_string());
        let requester_agent_id = Self::resolve_agent_id_from_session_key(&requester_key);
        let visibility = self.resolve_session_visibility().await;
        let a2a = self.resolve_agent_to_agent_policy().await;
        let tree_visible_keys = if visibility == SessionVisibility::Tree {
            Some(self.collect_tree_visible_keys(mgr, &requester_key)?)
        } else {
            None
        };
        Ok(SessionAccessContext {
            requester_key,
            requester_agent_id,
            visibility,
            a2a,
            tree_visible_keys,
        })
    }

    fn check_session_access(
        &self,
        ctx: &SessionAccessContext,
        target_key: &str,
        action: SessionAccessAction,
    ) -> Result<(), String> {
        let target_agent = Self::resolve_agent_id_from_session_key(target_key);
        let cross_agent = target_agent != ctx.requester_agent_id;
        if cross_agent {
            if ctx.visibility != SessionVisibility::All {
                return Err(Self::cross_visibility_error(action));
            }
            if !ctx.a2a.enabled {
                return Err(Self::a2a_disabled_error(action));
            }
            if !Self::is_a2a_allowed(&ctx.a2a, &ctx.requester_agent_id, &target_agent) {
                return Err(Self::a2a_denied_error(action));
            }
            return Ok(());
        }

        match ctx.visibility {
            SessionVisibility::SelfOnly => {
                if target_key != ctx.requester_key {
                    return Err(Self::self_visibility_error(action));
                }
            }
            SessionVisibility::Tree => {
                if target_key != ctx.requester_key {
                    let ok = ctx
                        .tree_visible_keys
                        .as_ref()
                        .map(|set| set.contains(target_key))
                        .unwrap_or(false);
                    if !ok {
                        return Err(Self::tree_visibility_error(action));
                    }
                }
            }
            SessionVisibility::Agent | SessionVisibility::All => {}
        }
        Ok(())
    }

    async fn resolve_a2a_ping_pong_turns(&self) -> u32 {
        let mut raw = None;
        if let Some(cfg_lock) = self.full_config.as_ref() {
            let cfg = cfg_lock.read().await;
            raw = cfg
                .session
                .as_ref()
                .and_then(|session| session.agent_to_agent.as_ref())
                .and_then(|v| v.get("maxPingPongTurns"))
                .and_then(|v| v.as_i64())
                .or_else(|| {
                    cfg.tools
                        .as_ref()
                        .and_then(|tools| tools.pointer("/agentToAgent/maxPingPongTurns"))
                        .and_then(|v| v.as_i64())
                });
        }
        let turns = raw.unwrap_or(DEFAULT_A2A_PING_PONG_TURNS);
        turns.clamp(0, MAX_A2A_PING_PONG_TURNS) as u32
    }

    async fn resolve_subagent_limits(&self) -> (u32, usize) {
        let mut max_spawn_depth = DEFAULT_SUBAGENT_MAX_SPAWN_DEPTH;
        let mut max_children = DEFAULT_SUBAGENT_MAX_CHILDREN_PER_AGENT;
        if let Some(cfg_lock) = self.full_config.as_ref() {
            let cfg = cfg_lock.read().await;
            if let Some(raw_depth) = cfg
                .agents
                .as_ref()
                .and_then(|agents| agents.pointer("/defaults/subagents/maxSpawnDepth"))
                .and_then(|v| v.as_i64())
                && raw_depth > 0
            {
                max_spawn_depth = raw_depth as u32;
            }
            if let Some(raw_children) = cfg
                .agents
                .as_ref()
                .and_then(|agents| agents.pointer("/defaults/subagents/maxChildrenPerAgent"))
                .and_then(|v| v.as_i64())
                && raw_children > 0
            {
                max_children = raw_children as usize;
            }
        }
        (max_spawn_depth.max(1), max_children.max(1))
    }

    async fn resolve_subagent_archive_after_ms(&self) -> Option<i64> {
        let mut minutes: i64 = 60;
        if let Some(cfg_lock) = self.full_config.as_ref() {
            let cfg = cfg_lock.read().await;
            if let Some(raw_minutes) = cfg
                .agents
                .as_ref()
                .and_then(|agents| agents.pointer("/defaults/subagents/archiveAfterMinutes"))
                .and_then(|v| v.as_i64())
            {
                minutes = raw_minutes;
            }
        }
        if minutes <= 0 {
            return None;
        }
        Some(minutes.saturating_mul(60_000))
    }

    async fn resolve_subagent_allow_agents(&self, requester_agent_id: &str) -> Vec<String> {
        let Some(cfg_lock) = self.full_config.as_ref() else {
            return Vec::new();
        };
        let cfg = cfg_lock.read().await;
        let requester = sanitize_session_token(requester_agent_id);
        let Some(agent_list) = cfg
            .agents
            .as_ref()
            .and_then(|agents| agents.get("list"))
            .and_then(|v| v.as_array())
        else {
            return Vec::new();
        };
        for entry in agent_list {
            let id = entry
                .get("id")
                .and_then(|v| v.as_str())
                .map(sanitize_session_token);
            if id.as_deref() != Some(requester.as_str()) {
                continue;
            }
            return entry
                .pointer("/subagents/allowAgents")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::trim))
                        .filter(|v| !v.is_empty())
                        .map(|v| {
                            if v == "*" {
                                "*".to_string()
                            } else {
                                sanitize_session_token(v)
                            }
                        })
                        .collect::<Vec<String>>()
                })
                .unwrap_or_default();
        }
        Vec::new()
    }

    async fn resolve_subagent_model_hint(&self, target_agent_id: &str) -> Option<String> {
        let Some(cfg_lock) = self.full_config.as_ref() else {
            return None;
        };
        let cfg = cfg_lock.read().await;
        let target = sanitize_session_token(target_agent_id);
        let mut target_primary_model: Option<String> = None;
        if let Some(agent_list) = cfg
            .agents
            .as_ref()
            .and_then(|agents| agents.get("list"))
            .and_then(|v| v.as_array())
        {
            for entry in agent_list {
                let id = entry
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(sanitize_session_token);
                if id.as_deref() != Some(target.as_str()) {
                    continue;
                }
                if let Some(model) = entry
                    .pointer("/subagents/model")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                {
                    return Some(model.to_string());
                }
                if let Some(model) = entry
                    .get("model")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                {
                    target_primary_model = Some(model.to_string());
                }
                break;
            }
        }
        if let Some(model) = cfg
            .agents
            .as_ref()
            .and_then(|agents| agents.pointer("/defaults/subagents/model"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            return Some(model.to_string());
        }
        target_primary_model
    }

    fn is_agent_allowed_by_allowlist(allow_agents: &[String], target_agent: &str) -> bool {
        if allow_agents.is_empty() {
            return false;
        }
        let target = sanitize_session_token(target_agent);
        allow_agents
            .iter()
            .any(|v| v == "*" || sanitize_session_token(v) == target)
    }

    fn normalize_tool_call_name(name: &str) -> &str {
        let normalized = name.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "browser" => "browse",
            "google" | "search" => "web_search",
            "fetch" | "http_get" => "web_fetch",
            "exec" | "shell" => "bash",
            _ => name.trim(),
        }
    }

    fn count_active_children_for_requester(&self, requester_session_key: &str) -> usize {
        self.sweep_archived_subagent_runs();
        ensure_subagent_registry_loaded();
        SUBAGENT_RUNS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .filter(|entry| {
                entry.requester_session_key == requester_session_key && entry.ended_at_ms.is_none()
            })
            .count()
    }

    fn sweep_archived_subagent_runs(&self) {
        ensure_subagent_registry_loaded();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let mut guard = SUBAGENT_RUNS.lock().unwrap_or_else(|e| e.into_inner());
        let before = guard.len();
        guard.retain(|_, entry| {
            let archived = entry
                .archive_at_ms
                .map(|at| at > 0 && at <= now_ms)
                .unwrap_or(false);
            if !archived {
                return true;
            }
            entry.ended_at_ms.is_none()
        });
        if guard.len() != before {
            persist_subagent_registry_to_disk(&guard);
        }
    }

    fn resolve_spawn_depth(&self, mgr: &SessionManager, requester_session_key: &str) -> u32 {
        if let Ok(meta) = mgr.get_session_metadata(requester_session_key)
            && let Some(explicit) = Self::parse_spawn_depth_meta(&meta)
        {
            return explicit.max(0);
        }
        let mut depth: u32 = 0;
        let mut current = requester_session_key.to_string();
        let mut visited: HashSet<String> = HashSet::new();
        loop {
            if !visited.insert(current.clone()) {
                break;
            }
            let Ok(meta) = mgr.get_session_metadata(&current) else {
                break;
            };
            if let Some(explicit) = Self::parse_spawn_depth_meta(&meta) {
                return depth.max(explicit);
            }
            let parent = meta
                .get("spawnedBy")
                .or_else(|| meta.get("parentSessionKey"))
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .map(ToString::to_string);
            if let Some(parent_key) = parent {
                depth = depth.saturating_add(1);
                current = parent_key;
                continue;
            }
            if depth == 0 && current.contains(":subagent:") {
                return 1;
            }
            break;
        }
        depth
    }

    fn parse_spawn_depth_meta(meta: &HashMap<String, String>) -> Option<u32> {
        let raw = meta
            .get("spawnDepth")
            .or_else(|| meta.get("spawn_depth"))
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())?;
        raw.parse::<u32>().ok()
    }

    async fn run_sessions_send_a2a_flow(
        &self,
        provider: Arc<dyn LlmProvider>,
        target_session_key: String,
        display_key: String,
        original_message: String,
        announce_timeout_seconds: u64,
        max_ping_pong_turns: u32,
        requester_session_key: Option<String>,
        requester_channel: Option<String>,
        round_one_reply: Option<String>,
    ) -> Result<(), String> {
        let mut primary_reply = round_one_reply.clone();
        let mut latest_reply = round_one_reply;
        let Some(mut incoming_message) = latest_reply.clone().map(|v| v.trim().to_string()) else {
            return Ok(());
        };
        if incoming_message.is_empty() {
            return Ok(());
        }
        latest_reply = Some(incoming_message.clone());

        let session_manager = self
            .session_manager
            .as_ref()
            .ok_or_else(|| "sessions_send: session manager unavailable for a2a flow".to_string())?;
        let mgr = session_manager.read().await;
        let announce_target = self.resolve_announce_target(&mgr, &target_session_key, &display_key);
        let target_channel = announce_target.as_ref().map(|t| t.channel.clone());

        if max_ping_pong_turns > 0 {
            if let Some(requester_key) = requester_session_key.as_ref() {
                if requester_key != &target_session_key {
                    let mut current_session_key = requester_key.to_string();
                    let mut next_session_key = target_session_key.clone();
                    for turn in 1..=max_ping_pong_turns {
                        let current_role = if current_session_key == *requester_key {
                            "requester"
                        } else {
                            "target"
                        };
                        let reply_prompt = build_agent_to_agent_reply_context(
                            requester_session_key.as_deref(),
                            requester_channel.as_deref(),
                            &display_key,
                            target_channel.as_deref(),
                            current_role,
                            turn,
                            max_ping_pong_turns,
                        );
                        let source_channel = if next_session_key == *requester_key {
                            requester_channel.as_deref()
                        } else {
                            target_channel.as_deref()
                        };
                        let reply_text = match self
                            .run_agent_step(
                                provider.clone(),
                                &current_session_key,
                                &incoming_message,
                                &reply_prompt,
                                announce_timeout_seconds,
                                Some(&next_session_key),
                                source_channel,
                            )
                            .await?
                        {
                            Some(text) => text,
                            None => break,
                        };
                        if is_reply_skip(Some(reply_text.as_str())) {
                            break;
                        }
                        incoming_message = reply_text.clone();
                        latest_reply = Some(reply_text);
                        let swap = current_session_key;
                        current_session_key = next_session_key;
                        next_session_key = swap;
                    }
                }
            }
        }

        let announce_prompt = build_agent_to_agent_announce_context(
            requester_session_key.as_deref(),
            requester_channel.as_deref(),
            &display_key,
            target_channel.as_deref(),
            &original_message,
            primary_reply.as_deref(),
            latest_reply.as_deref(),
        );
        let announce_reply = self
            .run_agent_step(
                provider,
                &target_session_key,
                "Agent-to-agent announce step.",
                &announce_prompt,
                announce_timeout_seconds,
                requester_session_key.as_deref(),
                requester_channel.as_deref(),
            )
            .await?;
        if let Some(reply_text) = announce_reply
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            && !is_announce_skip(Some(reply_text))
            && let Some(target) = announce_target
        {
            let _ = self.send_announce_to_target(&target, reply_text).await;
        }
        if primary_reply.is_none() {
            primary_reply = latest_reply.clone();
        }
        let _ = primary_reply;
        Ok(())
    }

    async fn run_agent_step(
        &self,
        provider: Arc<dyn LlmProvider>,
        session_key: &str,
        message: &str,
        extra_system_prompt: &str,
        timeout_seconds: u64,
        source_session_key: Option<&str>,
        source_channel: Option<&str>,
    ) -> Result<Option<String>, String> {
        let timeout = timeout_seconds.max(1);
        let Some(session_manager) = self.session_manager.as_ref() else {
            return Ok(None);
        };
        let mgr = session_manager.read().await;
        if mgr
            .get_session(session_key)
            .map_err(|e| e.to_string())?
            .is_none()
        {
            let _ = mgr
                .create_session(session_key, "default")
                .map_err(|e| e.to_string())?;
        }
        let model_override = mgr
            .get_session_metadata(session_key)
            .ok()
            .and_then(|meta| session_model_override_from_metadata(&meta));
        mgr.add_message(session_key, "user", message)
            .map_err(|e| e.to_string())?;
        let mut prompt = default_agent_system_prompt(self);
        if !extra_system_prompt.trim().is_empty() {
            prompt.push_str("\n\n");
            prompt.push_str(extra_system_prompt.trim());
        }
        if let Some(source) = source_session_key.map(str::trim).filter(|v| !v.is_empty()) {
            prompt.push_str(&format!("\nSource session: {}.", source));
        }
        if let Some(channel) = source_channel.map(str::trim).filter(|v| !v.is_empty()) {
            prompt.push_str(&format!("\nSource channel: {}.", channel));
        }
        let child_exec = self.build_child_executor(session_key.to_string());
        let out = match tokio::time::timeout(
            Duration::from_secs(timeout),
            agent_reply_with_prompt_and_model_detailed(
                &provider,
                &child_exec,
                message,
                Some(session_key),
                &prompt,
                model_override.as_deref(),
            ),
        )
        .await
        {
            Ok(Ok(out)) => out,
            _ => return Ok(None),
        };
        mgr.add_message(session_key, "assistant", &out.reply)
            .map_err(|e| e.to_string())?;
        let _ = apply_session_usage_delta(&mgr, session_key, &out.usage, &out.model);
        let text = out.reply.trim();
        if text.is_empty() {
            return Ok(None);
        }
        Ok(Some(text.to_string()))
    }

    fn resolve_announce_target(
        &self,
        mgr: &SessionManager,
        session_key: &str,
        display_key: &str,
    ) -> Option<AnnounceTarget> {
        let from_meta = |meta: &HashMap<String, String>| -> Option<AnnounceTarget> {
            let channel = session_metadata_pick(
                meta,
                &["delivery.channel", "lastChannel", "last_channel", "channel"],
            )?;
            let to =
                session_metadata_pick(meta, &["delivery.to", "lastTo", "last_to", "to", "target"])?;
            Some(AnnounceTarget {
                channel,
                to,
                account_id: session_metadata_pick(
                    meta,
                    &[
                        "delivery.accountId",
                        "lastAccountId",
                        "last_account_id",
                        "account_id",
                    ],
                ),
                thread_id: session_metadata_pick(
                    meta,
                    &[
                        "delivery.threadId",
                        "lastThreadId",
                        "last_thread_id",
                        "thread_id",
                    ],
                ),
            })
        };
        if let Ok(meta) = mgr.get_session_metadata(session_key)
            && let Some(target) = from_meta(&meta)
        {
            return Some(target);
        }
        if display_key != session_key
            && let Ok(meta) = mgr.get_session_metadata(display_key)
            && let Some(target) = from_meta(&meta)
        {
            return Some(target);
        }
        if let Ok(sessions) = mgr.list_sessions() {
            for row in sessions {
                if row.key != session_key && row.key != display_key {
                    continue;
                }
                if let Ok(meta) = mgr.get_session_metadata(&row.key)
                    && let Some(target) = from_meta(&meta)
                {
                    return Some(target);
                }
            }
        }
        resolve_announce_target_from_key(session_key)
            .or_else(|| resolve_announce_target_from_key(display_key))
    }

    fn parse_session_bindings_from_metadata(
        &self,
        meta: &HashMap<String, String>,
        target_session_key: &str,
    ) -> Vec<SessionBindingRecord> {
        let now_ms = now_epoch_ms();
        let raw_json = meta
            .get("deliveryBindings")
            .or_else(|| meta.get("subagentDeliveryBindings"))
            .map(String::as_str)
            .unwrap_or_default()
            .trim()
            .to_string();
        let mut out: Vec<SessionBindingRecord> = if raw_json.is_empty() {
            Vec::new()
        } else {
            serde_json::from_str::<Vec<SessionBindingRecord>>(&raw_json)
                .or_else(|_| {
                    serde_json::from_str::<SessionBindingRecord>(&raw_json)
                        .map(|single| vec![single])
                })
                .unwrap_or_default()
        };

        if out.is_empty() {
            let legacy_channel = session_metadata_pick(
                meta,
                &["delivery.channel", "lastChannel", "last_channel", "channel"],
            );
            let legacy_to =
                session_metadata_pick(meta, &["delivery.to", "lastTo", "last_to", "to", "target"]);
            if let (Some(channel), Some(to)) = (legacy_channel, legacy_to)
                && let Some(conversation_id) = parse_conversation_id_from_target(&to)
                && !channel.trim().is_empty()
                && !conversation_id.trim().is_empty()
            {
                out.push(SessionBindingRecord {
                    binding_id: format!("legacy:{}", target_session_key),
                    target_session_key: target_session_key.to_string(),
                    target_kind: "subagent".to_string(),
                    conversation: SessionBindingConversationRef {
                        channel: normalize_channel_alias(&channel),
                        account_id: normalize_account_id(
                            session_metadata_pick(
                                meta,
                                &[
                                    "delivery.accountId",
                                    "lastAccountId",
                                    "last_account_id",
                                    "account_id",
                                ],
                            )
                            .as_deref(),
                        ),
                        conversation_id,
                        parent_conversation_id: session_metadata_pick(
                            meta,
                            &[
                                "delivery.threadId",
                                "lastThreadId",
                                "last_thread_id",
                                "thread_id",
                            ],
                        ),
                    },
                    status: "active".to_string(),
                    bound_at_ms: now_ms,
                    expires_at_ms: None,
                    metadata: None,
                });
            }
        }

        out.retain(|record| {
            let session_match = record.target_session_key.trim() == target_session_key.trim();
            if !session_match {
                return false;
            }
            let status = record.status.trim().to_ascii_lowercase();
            if status != "active" {
                return false;
            }
            if let Some(expires_at) = record.expires_at_ms
                && expires_at > 0
                && expires_at <= now_ms
            {
                return false;
            }
            let channel_ok = !record.conversation.channel.trim().is_empty();
            let conversation_ok = !record.conversation.conversation_id.trim().is_empty();
            channel_ok && conversation_ok
        });
        out
    }

    fn read_session_bindings(
        &self,
        mgr: &SessionManager,
        target_session_key: &str,
    ) -> Vec<SessionBindingRecord> {
        mgr.get_session_metadata(target_session_key)
            .ok()
            .map(|meta| self.parse_session_bindings_from_metadata(&meta, target_session_key))
            .unwrap_or_default()
    }

    fn write_session_bindings(
        &self,
        mgr: &SessionManager,
        target_session_key: &str,
        bindings: &[SessionBindingRecord],
    ) {
        if let Ok(serialized) = serde_json::to_string(bindings) {
            let _ =
                mgr.set_session_metadata_field(target_session_key, "deliveryBindings", &serialized);
            let _ = mgr.set_session_metadata_field(
                target_session_key,
                "subagentDeliveryBindings",
                &serialized,
            );
        }
    }

    fn upsert_session_binding(
        &self,
        mgr: &SessionManager,
        target_session_key: &str,
        target_kind: &str,
        announce_target: &AnnounceTarget,
    ) -> Option<SessionBindingRecord> {
        let conversation_id = parse_conversation_id_from_target(&announce_target.to)?;
        let channel = normalize_channel_alias(&announce_target.channel);
        if channel.is_empty() || conversation_id.trim().is_empty() {
            return None;
        }
        let account_id = normalize_account_id(announce_target.account_id.as_deref());
        let mut bindings = self.read_session_bindings(mgr, target_session_key);
        let now_ms = now_epoch_ms();
        let existing_idx = bindings.iter().position(|entry| {
            normalize_channel_alias(&entry.conversation.channel) == channel
                && normalize_account_id(Some(entry.conversation.account_id.as_str())) == account_id
                && entry.conversation.conversation_id.trim() == conversation_id.trim()
        });

        let updated = if let Some(idx) = existing_idx {
            let mut item = bindings[idx].clone();
            item.status = "active".to_string();
            item.target_kind = target_kind.to_string();
            item.target_session_key = target_session_key.to_string();
            item.conversation.parent_conversation_id = announce_target
                .thread_id
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(ToString::to_string);
            item.bound_at_ms = now_ms;
            bindings[idx] = item.clone();
            item
        } else {
            let item = SessionBindingRecord {
                binding_id: format!("{}:{}", target_session_key, uuid::Uuid::new_v4()),
                target_session_key: target_session_key.to_string(),
                target_kind: target_kind.to_string(),
                conversation: SessionBindingConversationRef {
                    channel: channel.clone(),
                    account_id,
                    conversation_id,
                    parent_conversation_id: announce_target
                        .thread_id
                        .as_deref()
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .map(ToString::to_string),
                },
                status: "active".to_string(),
                bound_at_ms: now_ms,
                expires_at_ms: None,
                metadata: None,
            };
            bindings.push(item.clone());
            item
        };
        self.write_session_bindings(mgr, target_session_key, &bindings);
        Some(updated)
    }

    fn resolve_bound_delivery_route(
        &self,
        mgr: &SessionManager,
        target_session_key: &str,
        requester: Option<&SessionBindingConversationRef>,
        fail_closed: bool,
    ) -> BoundDeliveryRoute {
        let target_key = target_session_key.trim();
        if target_key.is_empty() {
            return BoundDeliveryRoute {
                binding: None,
                mode: "fallback",
                reason: "missing-target-session",
            };
        }
        let active_bindings = self.read_session_bindings(mgr, target_key);
        resolve_bound_delivery_route_from_bindings(&active_bindings, requester, fail_closed)
    }

    async fn send_announce_to_target(
        &self,
        target: &AnnounceTarget,
        text: &str,
    ) -> Result<(), String> {
        let Some(channel_manager) = self.channel_manager.as_ref() else {
            return Ok(());
        };
        let channel = resolve_channel_name(channel_manager, &target.channel).await?;
        let msg = ChannelMessage {
            id: uuid::Uuid::new_v4().to_string(),
            channel: channel.clone(),
            sender: "sessions_send/a2a".to_string(),
            content: text.to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            metadata: resolve_outbound_target_metadata(
                &target.to,
                target.thread_id.as_deref(),
                target.account_id.as_deref(),
            ),
        };
        let _ = channel_manager
            .read()
            .await
            .send_to_channel(&channel, &msg)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn parse_subagent_delivery_target_from_hook(
        value: &serde_json::Value,
    ) -> Option<AnnounceTarget> {
        let root = value
            .get("origin")
            .filter(|v| v.is_object())
            .unwrap_or(value);
        let channel = root
            .get("channel")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(normalize_channel_alias)?;
        if channel == "webchat" {
            return None;
        }
        let to = root
            .get("to")
            .or_else(|| root.get("target"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToString::to_string)?;
        let account_id = root
            .get("accountId")
            .or_else(|| root.get("account_id"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToString::to_string);
        let thread_id = root
            .get("threadId")
            .or_else(|| root.get("thread_id"))
            .and_then(|v| {
                if let Some(s) = v.as_str() {
                    Some(s.to_string())
                } else {
                    v.as_i64().map(|n| n.to_string())
                }
            })
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());
        Some(AnnounceTarget {
            channel,
            to,
            account_id,
            thread_id,
        })
    }

    async fn resolve_subagent_completion_target(
        &self,
        requester_session_key: &str,
        child_session_key: &str,
        child_run_id: &str,
        spawn_mode: &str,
        expects_completion_message: bool,
    ) -> (Option<AnnounceTarget>, &'static str) {
        let mut route_mode = "fallback";
        let mut target = if let Some(session_manager) = self.session_manager.as_ref() {
            let mgr = session_manager.read().await;
            let requester_target =
                self.resolve_announce_target(&mgr, requester_session_key, requester_session_key);
            let requester_ref = requester_target
                .as_ref()
                .and_then(target_to_requester_conversation);
            let bound = self.resolve_bound_delivery_route(
                &mgr,
                child_session_key,
                requester_ref.as_ref(),
                false,
            );
            let _route_reason = bound.reason;
            let bound_target = bound
                .binding
                .as_ref()
                .and_then(binding_record_to_announce_target);
            if bound.mode == "bound"
                && let Some(t) = bound_target
            {
                route_mode = "bound";
                Some(t)
            } else {
                if requester_target.is_some() {
                    route_mode = "fallback";
                }
                requester_target
            }
        } else {
            None
        };

        if let Some(hooks) = self.hook_pipeline.as_ref() {
            let payload = serde_json::json!({
                "child_session_key": child_session_key,
                "childSessionKey": child_session_key,
                "requester_session_key": requester_session_key,
                "requesterSessionKey": requester_session_key,
                "child_run_id": child_run_id,
                "childRunId": child_run_id,
                "spawn_mode": spawn_mode,
                "spawnMode": spawn_mode,
                "expects_completion_message": expects_completion_message,
                "expectsCompletionMessage": expects_completion_message,
                "requester_origin": target.as_ref().map(|t| serde_json::json!({
                    "channel": t.channel,
                    "to": t.to,
                    "accountId": t.account_id,
                    "threadId": t.thread_id
                })),
                "requesterOrigin": target.as_ref().map(|t| serde_json::json!({
                    "channel": t.channel,
                    "to": t.to,
                    "accountId": t.account_id,
                    "threadId": t.thread_id
                })),
                "origin": target.as_ref().map(|t| serde_json::json!({
                    "channel": t.channel,
                    "to": t.to,
                    "accountId": t.account_id,
                    "threadId": t.thread_id
                }))
            });
            if let Ok(patched) = hooks
                .subagent_delivery_target(child_session_key, &payload.to_string())
                .await
                && let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&patched)
                && let Some(hook_target) = Self::parse_subagent_delivery_target_from_hook(&parsed)
            {
                target = Some(hook_target);
                route_mode = "hook";
            }
        }
        (target, route_mode)
    }

    async fn announce_subagent_completion_to_target(
        &self,
        target: &AnnounceTarget,
        subagent_label: &str,
        spawn_mode: &str,
        status: &str,
        error: Option<&str>,
        reply: Option<&str>,
        usage: Option<&UsageSummary>,
        runtime_ms: i64,
    ) -> Result<String, String> {
        let text = build_subagent_completion_announce_text(
            subagent_label,
            spawn_mode,
            status,
            error,
            reply,
            usage,
            runtime_ms,
        );
        if text.trim().is_empty() {
            return Ok(String::new());
        }
        self.send_announce_to_target(target, &text).await?;
        Ok(text)
    }

    async fn resolve_subagent_announce_queue_settings(
        &self,
        requester_session_key: &str,
        channel_hint: Option<&str>,
    ) -> SubagentAnnounceQueueSettings {
        let mut out = SubagentAnnounceQueueSettings::default();
        let mut session_mode_raw: Option<String> = None;
        let mut session_debounce_ms: Option<i64> = None;
        let mut session_cap: Option<usize> = None;
        let mut session_drop_raw: Option<String> = None;

        if let Some(session_manager) = self.session_manager.as_ref() {
            let mgr = session_manager.read().await;
            if let Ok(meta) = mgr.get_session_metadata(requester_session_key) {
                session_mode_raw = meta
                    .get("queueMode")
                    .or_else(|| meta.get("queue_mode"))
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty());
                session_debounce_ms = meta
                    .get("queueDebounceMs")
                    .or_else(|| meta.get("queue_debounce_ms"))
                    .and_then(|v| v.trim().parse::<i64>().ok())
                    .map(|v| v.max(0));
                session_cap = meta
                    .get("queueCap")
                    .or_else(|| meta.get("queue_cap"))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .map(|v| v.max(1));
                session_drop_raw = meta
                    .get("queueDrop")
                    .or_else(|| meta.get("queue_drop"))
                    .or_else(|| meta.get("queueDropPolicy"))
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty());
            }
        }

        let mut cfg_mode_raw: Option<String> = None;
        let mut cfg_debounce_ms: Option<i64> = None;
        let mut cfg_cap: Option<usize> = None;
        let mut cfg_drop_raw: Option<String> = None;
        if let Some(cfg_lock) = self.full_config.as_ref() {
            let cfg = cfg_lock.read().await;
            if let Some(messages) = cfg.messages.as_ref() {
                let channel_key = channel_hint.map(normalize_channel_alias);
                if let Some(channel) = channel_key.as_deref() {
                    let by_channel_mode_ptr = format!("/queue/byChannel/{}", channel);
                    cfg_mode_raw = messages
                        .pointer(&by_channel_mode_ptr)
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .map(ToString::to_string);
                    let by_channel_debounce_ptr = format!("/queue/debounceMsByChannel/{}", channel);
                    cfg_debounce_ms = messages
                        .pointer(&by_channel_debounce_ptr)
                        .and_then(|v| v.as_i64())
                        .map(|v| v.max(0));
                }
                if cfg_mode_raw.is_none() {
                    cfg_mode_raw = messages
                        .pointer("/queue/mode")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .map(ToString::to_string);
                }
                if cfg_debounce_ms.is_none() {
                    cfg_debounce_ms = messages
                        .pointer("/queue/debounceMs")
                        .and_then(|v| v.as_i64());
                }
                cfg_cap = messages
                    .pointer("/queue/cap")
                    .and_then(|v| v.as_i64())
                    .and_then(|v| usize::try_from(v.max(1)).ok());
                cfg_drop_raw = messages
                    .pointer("/queue/dropPolicy")
                    .or_else(|| messages.pointer("/queue/drop"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(ToString::to_string);
            }
        }

        out.mode = normalize_subagent_announce_queue_mode(
            session_mode_raw
                .as_deref()
                .or(cfg_mode_raw.as_deref())
                .or(Some("collect")),
        );
        out.debounce_ms = session_debounce_ms
            .or(cfg_debounce_ms)
            .unwrap_or(SUBAGENT_DEFAULT_ANNOUNCE_QUEUE_DEBOUNCE_MS)
            .max(0);
        out.cap = session_cap
            .or(cfg_cap)
            .unwrap_or(SUBAGENT_DEFAULT_ANNOUNCE_QUEUE_CAP)
            .max(1);
        out.drop_policy = normalize_subagent_announce_drop_policy(
            session_drop_raw
                .as_deref()
                .or(cfg_drop_raw.as_deref())
                .or(Some("summarize")),
        );
        out
    }

    fn sweep_subagent_announce_delivered_map(&self) {
        let now_ms = now_epoch_ms();
        let mut delivered = SUBAGENT_ANNOUNCE_DELIVERED
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        delivered.retain(|_, at| {
            *at > 0
                && now_ms.saturating_sub(*at)
                    <= SUBAGENT_ANNOUNCE_IDEMPOTENCY_TTL_MS.max(SUBAGENT_ANNOUNCE_EXPIRY_MS)
        });
    }

    fn was_subagent_announce_delivered(&self, announce_id: &str) -> bool {
        let key = announce_id.trim();
        if key.is_empty() {
            return false;
        }
        self.sweep_subagent_announce_delivered_map();
        SUBAGENT_ANNOUNCE_DELIVERED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains_key(key)
    }

    fn mark_subagent_announce_delivered(&self, announce_id: &str) {
        let key = announce_id.trim();
        if key.is_empty() {
            return;
        }
        self.sweep_subagent_announce_delivered_map();
        SUBAGENT_ANNOUNCE_DELIVERED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.to_string(), now_epoch_ms());
    }

    fn schedule_subagent_announce_queue_drain(&self, queue_key: String) {
        let exec = self.clone();
        tokio::spawn(async move {
            exec.run_subagent_announce_queue_drain(queue_key).await;
        });
    }

    async fn enqueue_subagent_announce(
        &self,
        queue_key: &str,
        mut item: SubagentAnnounceQueueItem,
        settings: SubagentAnnounceQueueSettings,
    ) -> bool {
        let now_ms = now_epoch_ms();
        let mut should_start_drain = false;
        let mut should_enqueue = true;
        item.origin_key = build_announce_origin_key(item.target.as_ref());
        {
            let mut queues = SUBAGENT_ANNOUNCE_QUEUES
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let queue = queues
                .entry(queue_key.to_string())
                .or_insert_with(|| SubagentAnnounceQueueState::new(settings.clone()));
            queue.mode = settings.mode;
            queue.debounce_ms = settings.debounce_ms.max(0);
            queue.cap = settings.cap.max(1);
            queue.drop_policy = settings.drop_policy;
            queue.last_enqueued_at_ms = now_ms;

            if queue.items.len() >= queue.cap {
                match queue.drop_policy {
                    SubagentAnnounceDropPolicy::New => {
                        should_enqueue = false;
                    }
                    SubagentAnnounceDropPolicy::Old | SubagentAnnounceDropPolicy::Summarize => {
                        let drop_count = queue.items.len() - queue.cap + 1;
                        for _ in 0..drop_count {
                            if let Some(dropped) = queue.items.pop_front()
                                && queue.drop_policy == SubagentAnnounceDropPolicy::Summarize
                            {
                                let summary = dropped
                                    .summary_line
                                    .as_deref()
                                    .filter(|v| !v.trim().is_empty())
                                    .map(str::trim)
                                    .unwrap_or(dropped.prompt.trim());
                                queue.dropped_count = queue.dropped_count.saturating_add(1);
                                queue
                                    .summary_lines
                                    .push(build_announce_summary_line(summary));
                            }
                        }
                        while queue.summary_lines.len() > queue.cap {
                            queue.summary_lines.remove(0);
                        }
                    }
                }
            }
            if should_enqueue {
                queue.items.push_back(item);
            }
            if !queue.draining {
                queue.draining = true;
                should_start_drain = true;
            }
        }
        if should_start_drain {
            self.schedule_subagent_announce_queue_drain(queue_key.to_string());
        }
        should_enqueue
    }

    async fn run_subagent_announce_queue_drain(&self, queue_key: String) {
        #[derive(Clone)]
        enum DrainWork {
            Single {
                item: SubagentAnnounceQueueItem,
                prompt: String,
                consume_count: usize,
                clear_summary: bool,
            },
            Collect {
                item: SubagentAnnounceQueueItem,
                prompt: String,
                consume_count: usize,
                clear_summary: bool,
            },
            Idle,
        }

        loop {
            let (debounce_ms, since_last_enqueue_ms) = {
                let queues = SUBAGENT_ANNOUNCE_QUEUES
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                let Some(queue) = queues.get(&queue_key) else {
                    break;
                };
                if queue.items.is_empty() && queue.dropped_count == 0 {
                    drop(queues);
                    let mut queues = SUBAGENT_ANNOUNCE_QUEUES
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    queues.remove(&queue_key);
                    break;
                }
                (
                    queue.debounce_ms.max(0),
                    now_epoch_ms().saturating_sub(queue.last_enqueued_at_ms),
                )
            };
            if debounce_ms > 0 && since_last_enqueue_ms < debounce_ms {
                let wait_ms = debounce_ms.saturating_sub(since_last_enqueue_ms).max(1);
                tokio::time::sleep(Duration::from_millis(wait_ms as u64)).await;
            }

            let work = {
                let queues = SUBAGENT_ANNOUNCE_QUEUES
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                let Some(queue) = queues.get(&queue_key) else {
                    break;
                };
                if queue.items.is_empty() && queue.dropped_count == 0 {
                    DrainWork::Idle
                } else {
                    let summary_prompt = build_announce_queue_summary_prompt(queue);
                    let has_summary = summary_prompt.is_some();
                    match queue.mode {
                        SubagentAnnounceQueueMode::Collect => {
                            if announce_items_cross_channel(&queue.items) {
                                if let Some(item) = queue.items.front().cloned() {
                                    let prompt = summary_prompt
                                        .clone()
                                        .unwrap_or_else(|| item.prompt.clone());
                                    DrainWork::Single {
                                        item,
                                        prompt,
                                        consume_count: 1,
                                        clear_summary: has_summary,
                                    }
                                } else {
                                    DrainWork::Idle
                                }
                            } else {
                                let items: Vec<SubagentAnnounceQueueItem> =
                                    queue.items.iter().cloned().collect();
                                let Some(last) = items.last().cloned() else {
                                    continue;
                                };
                                let prompt = build_announce_collect_prompt(
                                    &items,
                                    summary_prompt.as_deref(),
                                )
                                .unwrap_or_else(|| last.prompt.clone());
                                DrainWork::Collect {
                                    item: last,
                                    prompt,
                                    consume_count: items.len(),
                                    clear_summary: has_summary,
                                }
                            }
                        }
                        _ => {
                            if let Some(item) = queue.items.front().cloned() {
                                let prompt = summary_prompt
                                    .clone()
                                    .unwrap_or_else(|| item.prompt.clone());
                                DrainWork::Single {
                                    item,
                                    prompt,
                                    consume_count: 1,
                                    clear_summary: has_summary,
                                }
                            } else {
                                DrainWork::Idle
                            }
                        }
                    }
                }
            };

            let (send_ok, consume_count, clear_summary) = match work {
                DrainWork::Single {
                    item,
                    prompt,
                    consume_count,
                    clear_summary,
                } => {
                    let ok = if self.was_subagent_announce_delivered(&item.run_id) {
                        true
                    } else if let Some(target) = item.target.as_ref() {
                        let sent = self.send_announce_to_target(target, &prompt).await.is_ok();
                        if sent {
                            self.mark_subagent_announce_delivered(&item.run_id);
                        }
                        sent
                    } else {
                        false
                    };
                    (ok, consume_count, clear_summary)
                }
                DrainWork::Collect {
                    item,
                    prompt,
                    consume_count,
                    clear_summary,
                } => {
                    let ok = if self.was_subagent_announce_delivered(&item.run_id) {
                        true
                    } else if let Some(target) = item.target.as_ref() {
                        let sent = self.send_announce_to_target(target, &prompt).await.is_ok();
                        if sent {
                            self.mark_subagent_announce_delivered(&item.run_id);
                        }
                        sent
                    } else {
                        false
                    };
                    (ok, consume_count, clear_summary)
                }
                DrainWork::Idle => {
                    let mut queues = SUBAGENT_ANNOUNCE_QUEUES
                        .lock()
                        .unwrap_or_else(|e| e.into_inner());
                    if let Some(queue) = queues.get_mut(&queue_key) {
                        queue.draining = false;
                    }
                    queues.remove(&queue_key);
                    break;
                }
            };

            let mut stop = false;
            {
                let mut queues = SUBAGENT_ANNOUNCE_QUEUES
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                let Some(queue) = queues.get_mut(&queue_key) else {
                    break;
                };
                if send_ok {
                    for _ in 0..consume_count {
                        let _ = queue.items.pop_front();
                    }
                    if clear_summary {
                        queue.dropped_count = 0;
                        queue.summary_lines.clear();
                    }
                } else {
                    queue.last_enqueued_at_ms = now_epoch_ms();
                }
                if queue.items.is_empty() && queue.dropped_count == 0 {
                    queue.draining = false;
                    queues.remove(&queue_key);
                    stop = true;
                }
            }
            if stop {
                break;
            }
        }
    }

    async fn load_latest_subagent_output(&self, child_session_key: &str) -> Option<String> {
        let Some(session_manager) = self.session_manager.as_ref() else {
            return None;
        };
        let mgr = session_manager.read().await;
        let Ok(messages) = mgr.get_messages(child_session_key, 50) else {
            return None;
        };
        for msg in messages {
            let role = msg.role.trim().to_ascii_lowercase();
            if role == "assistant" || role == "toolresult" || role == "tool" {
                let text = msg.content.trim();
                if !text.is_empty() {
                    return Some(text.to_string());
                }
            }
        }
        None
    }

    async fn load_latest_subagent_output_with_retry(
        &self,
        child_session_key: &str,
        max_wait_ms: i64,
    ) -> Option<String> {
        let wait_ms = max_wait_ms
            .max(0)
            .min(SUBAGENT_OUTPUT_RETRY_MAX_WAIT_MS)
            .max(SUBAGENT_OUTPUT_RETRY_INTERVAL_MS);
        let deadline_ms = now_epoch_ms().saturating_add(wait_ms);
        loop {
            let current = self.load_latest_subagent_output(child_session_key).await;
            if current
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .is_some()
            {
                return current;
            }
            if now_epoch_ms() >= deadline_ms {
                return current;
            }
            tokio::time::sleep(Duration::from_millis(
                SUBAGENT_OUTPUT_RETRY_INTERVAL_MS as u64,
            ))
            .await;
        }
    }

    async fn wait_for_subagent_output_change(
        &self,
        child_session_key: &str,
        baseline_reply: &str,
        max_wait_ms: i64,
    ) -> Option<String> {
        let baseline = baseline_reply.trim();
        if baseline.is_empty() {
            return None;
        }
        let wait_ms = max_wait_ms
            .max(0)
            .min(SUBAGENT_OUTPUT_CHANGE_MAX_WAIT_MS)
            .max(SUBAGENT_OUTPUT_CHANGE_MIN_WAIT_MS);
        let deadline_ms = now_epoch_ms().saturating_add(wait_ms);
        let mut latest: Option<String> = Some(baseline.to_string());
        loop {
            let next = self.load_latest_subagent_output(child_session_key).await;
            if let Some(ref text) = next {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    latest = Some(trimmed.to_string());
                    if trimmed != baseline {
                        return Some(trimmed.to_string());
                    }
                }
            }
            if now_epoch_ms() >= deadline_ms {
                break;
            }
            tokio::time::sleep(Duration::from_millis(
                SUBAGENT_OUTPUT_RETRY_INTERVAL_MS as u64,
            ))
            .await;
        }
        latest
    }

    async fn fulfill_runtime_intent(
        &self,
        name: &str,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.resume_pending_subagent_cleanup_tasks().await;
        let Some(action) = result.get("action").and_then(|v| v.as_str()) else {
            return Ok(result);
        };
        match action {
            "send_message" => self.fulfill_send_message(result).await,
            "broadcast_message" => self.fulfill_broadcast_message(result).await,
            "send_reaction" => self.fulfill_send_reaction(result).await,
            "remove_reaction" => self.fulfill_remove_reaction(result).await,
            "list_reactions" => self.fulfill_list_reactions(result).await,
            "read_messages" => self.fulfill_read_messages(result).await,
            "search_messages" => self.fulfill_search_messages(result).await,
            "edit_message" => self.fulfill_edit_message(result).await,
            "delete_message" => self.fulfill_delete_message(result).await,
            "pin_message" => self.fulfill_pin_message(result).await,
            "unpin_message" => self.fulfill_unpin_message(result).await,
            "list_pins" => self.fulfill_list_pins(result).await,
            "get_permissions" => self.fulfill_get_permissions(result).await,
            "channel_info" => {
                self.fulfill_custom_channel_action(result, "channel_info", true)
                    .await
            }
            "channel_create" => {
                self.fulfill_custom_channel_action(result, "channel_create", false)
                    .await
            }
            "channel_edit" => {
                self.fulfill_custom_channel_action(result, "channel_edit", true)
                    .await
            }
            "channel_delete" => {
                self.fulfill_custom_channel_action(result, "channel_delete", true)
                    .await
            }
            "channel_move" => {
                self.fulfill_custom_channel_action(result, "channel_move", true)
                    .await
            }
            "channel_permission_set" => {
                self.fulfill_custom_channel_action(result, "channel_permission_set", true)
                    .await
            }
            "channel_permission_remove" => {
                self.fulfill_custom_channel_action(result, "channel_permission_remove", true)
                    .await
            }
            "category_create" => {
                self.fulfill_custom_channel_action(result, "category_create", false)
                    .await
            }
            "category_edit" => {
                self.fulfill_custom_channel_action(result, "category_edit", true)
                    .await
            }
            "category_delete" => {
                self.fulfill_custom_channel_action(result, "category_delete", true)
                    .await
            }
            "topic_create" => {
                self.fulfill_custom_channel_action(result, "topic_create", true)
                    .await
            }
            "thread_list" => {
                self.fulfill_custom_channel_action(result, "thread_list", false)
                    .await
            }
            "thread_create" => {
                self.fulfill_custom_channel_action(result, "thread_create", true)
                    .await
            }
            "add_participant" => {
                self.fulfill_custom_channel_action(result, "add_participant", true)
                    .await
            }
            "remove_participant" => {
                self.fulfill_custom_channel_action(result, "remove_participant", true)
                    .await
            }
            "leave_group" => {
                self.fulfill_custom_channel_action(result, "leave_group", true)
                    .await
            }
            "member_info" => {
                self.fulfill_custom_channel_action(result, "member_info", false)
                    .await
            }
            "role_info" => {
                self.fulfill_custom_channel_action(result, "role_info", false)
                    .await
            }
            "role_add" => {
                self.fulfill_custom_channel_action(result, "role_add", false)
                    .await
            }
            "role_remove" => {
                self.fulfill_custom_channel_action(result, "role_remove", false)
                    .await
            }
            "kick_member" => {
                self.fulfill_custom_channel_action(result, "kick_member", false)
                    .await
            }
            "ban_member" => {
                self.fulfill_custom_channel_action(result, "ban_member", false)
                    .await
            }
            "timeout_member" => {
                self.fulfill_custom_channel_action(result, "timeout_member", false)
                    .await
            }
            "event_list" => {
                self.fulfill_custom_channel_action(result, "event_list", false)
                    .await
            }
            "event_create" => {
                self.fulfill_custom_channel_action(result, "event_create", false)
                    .await
            }
            "emoji_list" => {
                self.fulfill_custom_channel_action(result, "emoji_list", false)
                    .await
            }
            "emoji_upload" => {
                self.fulfill_custom_channel_action(result, "emoji_upload", false)
                    .await
            }
            "sticker_search" => {
                self.fulfill_custom_channel_action(result, "sticker_search", false)
                    .await
            }
            "sticker_upload" => {
                self.fulfill_custom_channel_action(result, "sticker_upload", false)
                    .await
            }
            "send_sticker" => {
                self.fulfill_custom_channel_action(result, "send_sticker", true)
                    .await
            }
            "voice_status" => {
                self.fulfill_custom_channel_action(result, "voice_status", false)
                    .await
            }
            "rename_group" => {
                self.fulfill_custom_channel_action(result, "rename_group", true)
                    .await
            }
            "set_group_icon" => {
                self.fulfill_custom_channel_action(result, "set_group_icon", true)
                    .await
            }
            "set_presence" => {
                self.fulfill_custom_channel_action(result, "set_presence", false)
                    .await
            }
            "send_with_effect" => {
                self.fulfill_custom_channel_action(result, "send_with_effect", true)
                    .await
            }
            "send_attachment" => self.fulfill_send_attachment(result).await,
            "create_thread" => self.fulfill_create_thread(result).await,
            "send_thread_reply" => self.fulfill_send_thread_reply(result).await,
            "send_poll" => self.fulfill_send_poll(result).await,
            "list_groups" => self.fulfill_list_groups(result).await,
            "list_members" => self.fulfill_list_members(result).await,
            "sessions_list" => self.fulfill_sessions_list(result).await,
            "sessions_history" => self.fulfill_sessions_history(result).await,
            "sessions_send" => self.fulfill_sessions_send(result).await,
            "sessions_spawn" => self.fulfill_sessions_spawn(result).await,
            "subagents_help" | "subagents_agents" | "subagents_list" | "subagents_info"
            | "subagents_log" | "subagents_send" | "subagents_kill" | "subagents_steer"
            | "subagents_spawn" | "subagents_focus" | "subagents_unfocus" => {
                self.fulfill_subagents(result).await
            }
            "session_status" => self.fulfill_session_status(result).await,
            _ => {
                let _ = name;
                Ok(result)
            }
        }
    }

    fn list_runs_for_requester(&self, requester_session_key: &str) -> Vec<SubagentRunEntry> {
        self.sweep_archived_subagent_runs();
        ensure_subagent_registry_loaded();
        let mut rows: Vec<SubagentRunEntry> = SUBAGENT_RUNS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .filter(|r| r.requester_session_key == requester_session_key)
            .cloned()
            .collect();
        rows.sort_by(|a, b| b.started_at_ms.cmp(&a.started_at_ms));
        rows
    }

    fn patch_run<F>(&self, run_id: &str, mut f: F)
    where
        F: FnMut(&mut SubagentRunEntry),
    {
        ensure_subagent_registry_loaded();
        let mut guard = SUBAGENT_RUNS.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = guard.get_mut(run_id) {
            f(entry);
            persist_subagent_registry_to_disk(&guard);
        }
    }

    fn get_run_entry(&self, run_id: &str) -> Option<SubagentRunEntry> {
        ensure_subagent_registry_loaded();
        SUBAGENT_RUNS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(run_id)
            .cloned()
    }

    fn resolve_subagent_announce_retry_delay_ms(&self, retry_count: u32) -> i64 {
        let bounded = retry_count.clamp(0, 10);
        let exp = bounded.saturating_sub(1);
        let base = SUBAGENT_MIN_ANNOUNCE_RETRY_DELAY_MS.saturating_mul(1_i64 << exp);
        base.min(SUBAGENT_MAX_ANNOUNCE_RETRY_DELAY_MS).max(1)
    }

    fn count_active_descendant_runs(&self, root_session_key: &str) -> usize {
        self.collect_descendant_runs(root_session_key)
            .into_iter()
            .filter(|r| r.ended_at_ms.is_none())
            .count()
    }

    fn resolve_requester_for_child_session(&self, child_session_key: &str) -> Option<String> {
        self.sweep_archived_subagent_runs();
        ensure_subagent_registry_loaded();
        let snapshot: Vec<SubagentRunEntry> = SUBAGENT_RUNS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect();
        resolve_requester_for_child_session_from_runs(&snapshot, child_session_key)
    }

    fn is_subagent_session_run_active(&self, child_session_key: &str) -> bool {
        self.sweep_archived_subagent_runs();
        ensure_subagent_registry_loaded();
        let snapshot: Vec<SubagentRunEntry> = SUBAGENT_RUNS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect();
        is_subagent_session_run_active_from_runs(&snapshot, child_session_key)
    }

    async fn session_exists(&self, session_key: &str) -> bool {
        let Some(session_manager) = self.session_manager.as_ref() else {
            return false;
        };
        let mgr = session_manager.read().await;
        mgr.get_session(session_key).ok().flatten().is_some()
    }

    fn begin_subagent_cleanup(&self, run_id: &str) -> bool {
        ensure_subagent_registry_loaded();
        let mut guard = SUBAGENT_RUNS.lock().unwrap_or_else(|e| e.into_inner());
        let Some(entry) = guard.get_mut(run_id) else {
            return false;
        };
        if entry.cleanup_completed_at_ms.is_some() || entry.cleanup_handled {
            return false;
        }
        entry.cleanup_handled = true;
        persist_subagent_registry_to_disk(&guard);
        true
    }

    fn schedule_subagent_cleanup_retry(&self, run_id: String, delay_ms: i64) {
        {
            let mut tasks = SUBAGENT_CLEANUP_TASKS
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if !tasks.insert(run_id.clone()) {
                return;
            }
        }
        let exec = self.clone();
        let run_id_for_task = run_id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(delay_ms.max(1) as u64)).await;
            SUBAGENT_CLEANUP_TASKS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&run_id_for_task);
            exec.run_subagent_cleanup_flow(run_id_for_task.clone())
                .await;
        });
    }

    async fn finalize_subagent_cleanup(
        &self,
        run_id: &str,
        cleanup_mode: &str,
        completed_at_ms: i64,
    ) {
        SUBAGENT_ABORTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(run_id);
        let mut delete_child_session: Option<String> = None;
        {
            let mut guard = SUBAGENT_RUNS.lock().unwrap_or_else(|e| e.into_inner());
            if cleanup_mode.eq_ignore_ascii_case("delete") {
                if let Some(entry) = guard.remove(run_id) {
                    delete_child_session = Some(entry.child_session_key);
                }
            } else if let Some(entry) = guard.get_mut(run_id) {
                entry.cleanup_completed_at_ms = Some(completed_at_ms);
                entry.cleanup_handled = true;
            }
            persist_subagent_registry_to_disk(&guard);
        }
        if let Some(child_session_key) = delete_child_session
            && let Some(session_manager) = self.session_manager.as_ref()
        {
            let _ = session_manager
                .read()
                .await
                .remove_session(&child_session_key);
        }
    }

    async fn emit_subagent_ended_once(&self, run_id: &str) {
        let Some(hooks) = self.hook_pipeline.as_ref() else {
            return;
        };
        let Some(entry) = self.get_run_entry(run_id) else {
            return;
        };
        if entry.spawn_mode.eq_ignore_ascii_case("session") {
            return;
        }
        if entry.ended_hook_emitted_at_ms.is_some() {
            return;
        }
        let reason = entry
            .ended_reason
            .clone()
            .unwrap_or_else(|| "subagent-complete".to_string());
        let outcome = match entry.status.as_str() {
            "failed" => "error",
            "timeout" => "timeout",
            "killed" => "killed",
            _ => "ok",
        };
        hooks
            .subagent_ended(
                &entry.child_session_key,
                &serde_json::json!({
                    "run_id": entry.run_id,
                    "status": entry.status,
                    "reason": reason,
                    "outcome": outcome,
                    "error": entry.error
                })
                .to_string(),
            )
            .await;
        self.patch_run(run_id, |current| {
            current.ended_hook_emitted_at_ms = Some(chrono::Utc::now().timestamp_millis());
        });
    }

    async fn run_subagent_cleanup_flow(&self, run_id: String) {
        if !self.begin_subagent_cleanup(&run_id) {
            return;
        }
        let Some(entry) = self.get_run_entry(&run_id) else {
            return;
        };
        if entry.cleanup_completed_at_ms.is_some() {
            return;
        }
        if entry.suppress_announce_reason.as_deref() == Some("steer-restart") {
            return;
        }

        let now_ms = chrono::Utc::now().timestamp_millis();
        let ended_at_ms = entry.ended_at_ms.unwrap_or(now_ms);
        let ended_ago_ms = now_ms.saturating_sub(ended_at_ms);
        if entry.expects_completion_message
            && self.count_active_descendant_runs(&entry.child_session_key) > 0
        {
            if ended_ago_ms > SUBAGENT_ANNOUNCE_EXPIRY_MS {
                self.emit_subagent_ended_once(&run_id).await;
                self.finalize_subagent_cleanup(&run_id, "keep", now_ms)
                    .await;
                return;
            }
            self.patch_run(&run_id, |e| {
                e.cleanup_handled = false;
                e.last_announce_retry_at_ms = Some(now_ms);
            });
            self.schedule_subagent_cleanup_retry(run_id, SUBAGENT_MIN_ANNOUNCE_RETRY_DELAY_MS);
            return;
        }

        let mut reply_text = if let Some(text) = entry
            .completion_reply
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            Some(text.to_string())
        } else {
            self.load_latest_subagent_output(&entry.child_session_key)
                .await
        };
        if reply_text
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .is_none()
        {
            reply_text = self
                .load_latest_subagent_output_with_retry(
                    &entry.child_session_key,
                    entry.run_timeout_seconds.saturating_mul(1000) as i64,
                )
                .await;
        }
        let runtime_ms = ended_at_ms.saturating_sub(entry.started_at_ms);
        let mut completion_text = build_subagent_completion_announce_text(
            &entry.label,
            &entry.spawn_mode,
            &entry.status,
            entry.error.as_deref(),
            reply_text.as_deref(),
            entry.usage.as_ref(),
            runtime_ms,
        );
        let mut target_requester_session_key = entry.requester_session_key.clone();
        let mut target_requester_is_subagent =
            is_subagent_session_key(&target_requester_session_key);
        let mut missing_requester_fallback = false;
        if target_requester_is_subagent
            && !self.is_subagent_session_run_active(&target_requester_session_key)
        {
            let requester_session_alive = self.session_exists(&target_requester_session_key).await;
            if !requester_session_alive {
                if let Some(fallback_key) =
                    self.resolve_requester_for_child_session(&target_requester_session_key)
                {
                    target_requester_session_key = fallback_key;
                } else {
                    missing_requester_fallback = true;
                }
            }
            target_requester_is_subagent = is_subagent_session_key(&target_requester_session_key);
        }
        if target_requester_is_subagent
            && let Some(baseline) = reply_text
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(ToString::to_string)
            && let Some(updated) = self
                .wait_for_subagent_output_change(
                    &entry.child_session_key,
                    &baseline,
                    entry.run_timeout_seconds.saturating_mul(1000) as i64,
                )
                .await
        {
            reply_text = Some(updated);
            completion_text = build_subagent_completion_announce_text(
                &entry.label,
                &entry.spawn_mode,
                &entry.status,
                entry.error.as_deref(),
                reply_text.as_deref(),
                entry.usage.as_ref(),
                runtime_ms,
            );
        }
        let mut completion_target: Option<AnnounceTarget> = None;
        let mut did_announce = if entry.suppress_announce_reason.as_deref() == Some("killed") {
            false
        } else if missing_requester_fallback {
            false
        } else if target_requester_is_subagent {
            // Nested subagent completions should be injected into the requester session
            // so parent orchestration can absorb and synthesize before external delivery.
            if let Some(provider) = self.llm_provider.as_ref() {
                let remaining_active_subagent_runs =
                    self.count_active_descendant_runs(&target_requester_session_key);
                let reply_instruction = build_subagent_announce_reply_instruction(
                    remaining_active_subagent_runs,
                    true,
                    entry.expects_completion_message,
                );
                let context_message = format!(
                    "[System Message] A subagent task \"{}\" just {}.\n\nResult:\n{}",
                    entry.label,
                    entry.status,
                    completion_text.trim()
                );
                match self
                    .run_agent_step(
                    provider.clone(),
                    &target_requester_session_key,
                    &format!("{}\n\n{}", context_message, reply_instruction),
                    "Subagent completion message. Keep this internal context private, and convert it into requester-session orchestration state.",
                    60,
                    Some(&entry.child_session_key),
                    None,
                )
                .await
                {
                    Ok(reply) => reply.map(|text| !is_reply_skip(Some(&text))).unwrap_or(true),
                    Err(_) => false,
                }
            } else {
                false
            }
        } else {
            let (target, route_mode) = self
                .resolve_subagent_completion_target(
                    &target_requester_session_key,
                    &entry.child_session_key,
                    &entry.run_id,
                    &entry.spawn_mode,
                    entry.expects_completion_message,
                )
                .await;
            completion_target = target.clone();
            let active_requester_descendants =
                self.count_active_descendant_runs(&target_requester_session_key);
            let defer_direct_for_siblings = should_defer_completion_direct_delivery(
                active_requester_descendants,
                &entry.spawn_mode,
                route_mode,
            );
            if self.was_subagent_announce_delivered(&entry.run_id) {
                true
            } else if defer_direct_for_siblings {
                false
            } else if let Some(target_ref) = target.as_ref() {
                match self
                    .announce_subagent_completion_to_target(
                        target_ref,
                        &entry.label,
                        &entry.spawn_mode,
                        &entry.status,
                        entry.error.as_deref(),
                        reply_text.as_deref(),
                        entry.usage.as_ref(),
                        runtime_ms,
                    )
                    .await
                {
                    Ok(text) => {
                        completion_text = text;
                        self.mark_subagent_announce_delivered(&entry.run_id);
                        true
                    }
                    Err(_) => false,
                }
            } else {
                false
            }
        };

        if !did_announce
            && entry.expects_completion_message
            && !target_requester_is_subagent
            && let Some(target) = completion_target.clone()
        {
            let queue_settings = self
                .resolve_subagent_announce_queue_settings(
                    &target_requester_session_key,
                    Some(&target.channel),
                )
                .await;
            let queue_item = SubagentAnnounceQueueItem {
                run_id: entry.run_id.clone(),
                prompt: completion_text.clone(),
                summary_line: Some(entry.label.clone()),
                enqueued_at_ms: now_ms,
                session_key: target_requester_session_key.clone(),
                target: Some(target),
                origin_key: None,
            };
            if self
                .enqueue_subagent_announce(
                    &target_requester_session_key,
                    queue_item,
                    queue_settings,
                )
                .await
            {
                did_announce = true;
            }
        }

        if did_announce || !entry.expects_completion_message {
            self.emit_subagent_ended_once(&run_id).await;
            self.finalize_subagent_cleanup(&run_id, &entry.cleanup, now_ms)
                .await;
            return;
        }

        let retry_count = entry.announce_retry_count.saturating_add(1);
        if retry_count >= SUBAGENT_MAX_ANNOUNCE_RETRY_COUNT
            || ended_ago_ms > SUBAGENT_ANNOUNCE_EXPIRY_MS
        {
            self.patch_run(&run_id, |e| {
                e.announce_retry_count = retry_count;
                e.last_announce_retry_at_ms = Some(now_ms);
            });
            self.emit_subagent_ended_once(&run_id).await;
            self.finalize_subagent_cleanup(&run_id, "keep", now_ms)
                .await;
            return;
        }

        let retry_delay_ms = self.resolve_subagent_announce_retry_delay_ms(retry_count);
        self.patch_run(&run_id, |e| {
            e.announce_retry_count = retry_count;
            e.last_announce_retry_at_ms = Some(now_ms);
            e.cleanup_handled = false;
        });
        self.schedule_subagent_cleanup_retry(run_id, retry_delay_ms);
    }

    async fn resume_pending_subagent_cleanup_tasks(&self) {
        self.sweep_archived_subagent_runs();
        ensure_subagent_registry_loaded();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let pending: Vec<(String, i64)> = SUBAGENT_RUNS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .filter_map(|entry| {
                if entry.ended_at_ms.is_none() || entry.cleanup_completed_at_ms.is_some() {
                    return None;
                }
                if entry.cleanup_handled {
                    return None;
                }
                let retry_delay_ms = self
                    .resolve_subagent_announce_retry_delay_ms(entry.announce_retry_count.max(1));
                let earliest = entry
                    .last_announce_retry_at_ms
                    .unwrap_or(0)
                    .saturating_add(retry_delay_ms);
                let wait_ms = earliest.saturating_sub(now_ms).max(0);
                Some((entry.run_id.clone(), wait_ms))
            })
            .collect();
        for (run_id, wait_ms) in pending {
            self.schedule_subagent_cleanup_retry(run_id, wait_ms);
        }
    }

    async fn start_subagent_run_task(
        &self,
        requester_session_key: String,
        requester_channel: Option<String>,
        child_session_key: String,
        label: String,
        task: String,
        model_hint: String,
        timeout_seconds: u64,
        spawn_mode: String,
        cleanup: String,
        thread_requested: bool,
        expects_completion_message: bool,
        child_depth: u32,
        max_spawn_depth: u32,
    ) -> Result<String, String> {
        self.sweep_archived_subagent_runs();
        ensure_subagent_registry_loaded();
        self.resume_pending_subagent_cleanup_tasks().await;
        let Some(provider) = self.llm_provider.as_ref() else {
            return Err("subagent run requires llm provider".to_string());
        };
        let Some(session_manager) = self.session_manager.as_ref() else {
            return Err("subagent run requires session manager".to_string());
        };
        let archive_at_ms = if spawn_mode.eq_ignore_ascii_case("session") {
            None
        } else {
            self.resolve_subagent_archive_after_ms()
                .await
                .map(|delta| chrono::Utc::now().timestamp_millis().saturating_add(delta))
        };
        let run_id = uuid::Uuid::new_v4().to_string();
        let started_at_ms = chrono::Utc::now().timestamp_millis();
        {
            let mut guard = SUBAGENT_RUNS.lock().unwrap_or_else(|e| e.into_inner());
            guard.insert(
                run_id.clone(),
                SubagentRunEntry {
                    run_id: run_id.clone(),
                    requester_session_key: requester_session_key.clone(),
                    child_session_key: child_session_key.clone(),
                    label: label.clone(),
                    task: task.clone(),
                    model: model_hint.clone(),
                    started_at_ms,
                    ended_at_ms: None,
                    status: "running".to_string(),
                    error: None,
                    usage: None,
                    completion_reply: None,
                    spawn_mode: spawn_mode.clone(),
                    cleanup: cleanup.clone(),
                    thread_requested,
                    expects_completion_message,
                    suppress_announce_reason: None,
                    announce_retry_count: 0,
                    last_announce_retry_at_ms: None,
                    cleanup_handled: false,
                    cleanup_completed_at_ms: None,
                    ended_reason: None,
                    ended_hook_emitted_at_ms: None,
                    run_timeout_seconds: timeout_seconds,
                    archive_at_ms,
                },
            );
            persist_subagent_registry_to_disk(&guard);
        }

        let child_exec = self.build_child_executor(child_session_key.clone());
        let provider = provider.clone();
        let session_manager = session_manager.clone();
        let announce_exec = self.clone();
        let run_id_for_task = run_id.clone();
        let child_session_for_task = child_session_key.clone();
        let model_hint_for_task = model_hint;
        let requester_session_for_task = requester_session_key.clone();
        let requester_channel_for_task = requester_channel.clone();
        let label_for_task = label.clone();
        let task_for_prompt = task.clone();
        let join = tokio::spawn(async move {
            let run_future = async {
                let prompt = build_subagent_system_prompt(
                    &child_exec,
                    requester_session_for_task.as_str(),
                    requester_channel_for_task.as_deref(),
                    child_session_for_task.as_str(),
                    Some(label_for_task.as_str()),
                    task_for_prompt.as_str(),
                    child_depth,
                    max_spawn_depth,
                );
                let out = agent_reply_with_prompt_and_model_detailed(
                    &provider,
                    &child_exec,
                    &task,
                    Some(&child_session_for_task),
                    &prompt,
                    Some(&model_hint_for_task),
                )
                .await?;
                session_manager
                    .read()
                    .await
                    .add_message(&child_session_for_task, "assistant", &out.reply)
                    .map_err(|e| e.to_string())?;
                let guard = session_manager.read().await;
                let _ = apply_session_usage_delta(
                    &guard,
                    &child_session_for_task,
                    &out.usage,
                    &out.model,
                );
                Ok::<AgentReplyDetailed, String>(out)
            };

            match tokio::time::timeout(Duration::from_secs(timeout_seconds.max(1)), run_future)
                .await
            {
                Ok(Ok(out)) => {
                    let mut guard = SUBAGENT_RUNS.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(entry) = guard.get_mut(&run_id_for_task) {
                        entry.status = "done".to_string();
                        entry.ended_at_ms = Some(chrono::Utc::now().timestamp_millis());
                        entry.usage = Some(out.usage.clone());
                        entry.completion_reply =
                            Some(out.reply.trim().to_string()).filter(|v| !v.is_empty());
                        entry.cleanup_handled = false;
                        entry.cleanup_completed_at_ms = None;
                        entry.ended_reason = Some("subagent-complete".to_string());
                    }
                    persist_subagent_registry_to_disk(&guard);
                }
                Ok(Err(err)) => {
                    let mut guard = SUBAGENT_RUNS.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(entry) = guard.get_mut(&run_id_for_task) {
                        entry.status = "failed".to_string();
                        entry.error = Some(err.clone());
                        entry.ended_at_ms = Some(chrono::Utc::now().timestamp_millis());
                        entry.cleanup_handled = false;
                        entry.cleanup_completed_at_ms = None;
                        entry.ended_reason = Some("subagent-error".to_string());
                    }
                    persist_subagent_registry_to_disk(&guard);
                }
                Err(_) => {
                    let mut guard = SUBAGENT_RUNS.lock().unwrap_or_else(|e| e.into_inner());
                    if let Some(entry) = guard.get_mut(&run_id_for_task) {
                        entry.status = "timeout".to_string();
                        entry.error = Some("subagent timed out".to_string());
                        entry.ended_at_ms = Some(chrono::Utc::now().timestamp_millis());
                        entry.cleanup_handled = false;
                        entry.cleanup_completed_at_ms = None;
                        entry.ended_reason = Some("subagent-error".to_string());
                    }
                    persist_subagent_registry_to_disk(&guard);
                }
            }
            announce_exec
                .run_subagent_cleanup_flow(run_id_for_task.clone())
                .await;
            SUBAGENT_ABORTS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&run_id_for_task);
        });
        SUBAGENT_ABORTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(run_id.clone(), join.abort_handle());
        Ok(run_id)
    }

    fn resolve_subagent_target(
        &self,
        runs: &[SubagentRunEntry],
        target: &str,
        recent_window_minutes: u64,
    ) -> Result<SubagentRunEntry, String> {
        ensure_subagent_registry_loaded();
        let token = target.trim();
        if token.is_empty() {
            return Err("missing subagent target".to_string());
        }
        let mut sorted: Vec<SubagentRunEntry> = runs.to_vec();
        sorted.sort_by(|a, b| b.started_at_ms.cmp(&a.started_at_ms));
        if token.eq_ignore_ascii_case("last") {
            return sorted
                .first()
                .cloned()
                .ok_or_else(|| format!("unknown subagent target: {}", token));
        }
        let recent_cutoff_ms = chrono::Utc::now()
            .timestamp_millis()
            .saturating_sub((recent_window_minutes.max(1) as i64).saturating_mul(60_000));
        let mut numeric_order: Vec<SubagentRunEntry> = sorted
            .iter()
            .filter(|r| r.ended_at_ms.is_none())
            .cloned()
            .collect();
        numeric_order.extend(
            sorted
                .iter()
                .filter(|r| {
                    r.ended_at_ms
                        .map(|v| v >= recent_cutoff_ms)
                        .unwrap_or(false)
                })
                .cloned(),
        );
        if let Ok(idx) = token.parse::<usize>() {
            if idx == 0 || idx > numeric_order.len() {
                return Err(format!("invalid subagent index: {}", idx));
            }
            return Ok(numeric_order[idx - 1].clone());
        }
        if token.contains(':') {
            return sorted
                .into_iter()
                .find(|r| r.child_session_key == token)
                .ok_or_else(|| format!("unknown subagent session: {}", token));
        }
        let lowered = token.to_ascii_lowercase();
        let exact_label: Vec<SubagentRunEntry> = runs
            .iter()
            .filter(|r| r.label.trim().to_ascii_lowercase() == lowered)
            .cloned()
            .collect();
        if exact_label.len() == 1 {
            return Ok(exact_label[0].clone());
        }
        if exact_label.len() > 1 {
            return Err(format!("ambiguous subagent label: {}", token));
        }
        let label_prefix: Vec<SubagentRunEntry> = runs
            .iter()
            .filter(|r| r.label.trim().to_ascii_lowercase().starts_with(&lowered))
            .cloned()
            .collect();
        if label_prefix.len() == 1 {
            return Ok(label_prefix[0].clone());
        }
        if label_prefix.len() > 1 {
            return Err(format!("ambiguous subagent label prefix: {}", token));
        }
        let run_id_prefix: Vec<SubagentRunEntry> = runs
            .iter()
            .filter(|r| r.run_id.starts_with(token))
            .cloned()
            .collect();
        if run_id_prefix.len() == 1 {
            return Ok(run_id_prefix[0].clone());
        }
        if run_id_prefix.len() > 1 {
            return Err(format!("ambiguous subagent run id prefix: {}", token));
        }
        Err(format!("unknown subagent target: {}", token))
    }

    fn collect_descendant_runs(&self, root_session_key: &str) -> Vec<SubagentRunEntry> {
        self.sweep_archived_subagent_runs();
        ensure_subagent_registry_loaded();
        let snapshot: Vec<SubagentRunEntry> = SUBAGENT_RUNS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .cloned()
            .collect();
        let mut out: Vec<SubagentRunEntry> = Vec::new();
        let mut stack: Vec<String> = vec![root_session_key.to_string()];
        let mut seen_run_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

        while let Some(requester) = stack.pop() {
            for run in snapshot
                .iter()
                .filter(|r| r.requester_session_key == requester)
            {
                if seen_run_ids.insert(run.run_id.clone()) {
                    out.push(run.clone());
                    stack.push(run.child_session_key.clone());
                }
            }
        }
        out
    }

    async fn kill_run_entry(&self, run: &SubagentRunEntry) {
        if let Some(handle) = SUBAGENT_ABORTS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&run.run_id)
        {
            handle.abort();
        }
        let now_ms = chrono::Utc::now().timestamp_millis();
        self.patch_run(&run.run_id, |entry| {
            entry.status = "killed".to_string();
            entry.ended_at_ms = Some(now_ms);
            entry.error = Some("killed by subagents tool".to_string());
            entry.suppress_announce_reason = Some("killed".to_string());
            entry.cleanup_handled = true;
            entry.cleanup_completed_at_ms = Some(now_ms);
            entry.ended_reason = Some("subagent-killed".to_string());
            if entry.archive_at_ms.is_none() {
                entry.archive_at_ms = Some(now_ms.saturating_add(60 * 60_000));
            }
        });
        if run.cleanup.eq_ignore_ascii_case("delete")
            && let Some(session_manager) = self.session_manager.as_ref()
        {
            let _ = session_manager
                .read()
                .await
                .remove_session(&run.child_session_key);
        }
        self.emit_subagent_ended_once(&run.run_id).await;
    }

    async fn load_active_session_metadata(&self) -> Option<HashMap<String, String>> {
        let (Some(session_manager), Some(session_id)) =
            (self.session_manager.as_ref(), self.session_id.as_deref())
        else {
            return None;
        };
        let mgr = session_manager.read().await;
        mgr.get_session_metadata(session_id).ok()
    }

    async fn fulfill_send_message(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let session_meta = self.load_active_session_metadata().await;
        let resolved = resolve_send_message_request(
            &result,
            session_meta.as_ref(),
            self.turn_source.as_ref(),
        )?;

        let channel_manager = self
            .channel_manager
            .as_ref()
            .ok_or_else(|| "message tool: channel manager unavailable".to_string())?;
        let channel = resolve_channel_name(channel_manager, &resolved.channel_raw).await?;
        let mut metadata = resolve_outbound_target_metadata(
            &resolved.target,
            resolved.reply_to.as_deref(),
            resolved.account_id.as_deref(),
        );
        if let Some(reply_to) = resolved.reply_to.clone() {
            metadata.insert("message_id".to_string(), reply_to);
        }
        let msg = ChannelMessage {
            id: uuid::Uuid::new_v4().to_string(),
            channel: channel.clone(),
            sender: "agent-tool".to_string(),
            content: resolved.text,
            timestamp: chrono::Utc::now().timestamp_millis(),
            metadata,
        };
        let message_id = channel_manager
            .read()
            .await
            .send_to_channel(&channel, &msg)
            .await
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "action": "send_message",
            "status": "sent",
            "channel": channel,
            "target": resolved.target,
            "resolved_from_session": resolved.resolved_from_session,
            "message_id": message_id
        }))
    }

    async fn fulfill_broadcast_message(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let text = json_pick_non_empty_string(&result, &["text", "message", "content"])
            .ok_or_else(|| {
                "message tool: missing text/message/content for broadcast".to_string()
            })?;
        let mut targets = json_pick_non_empty_string_vec(&result, &["targets"]);
        if targets.is_empty()
            && let Some(single) = json_pick_non_empty_string(&result, &["target", "to"])
        {
            targets.push(single);
        }
        if targets.is_empty() {
            return Err("message tool: broadcast requires targets".to_string());
        }
        let account_id = json_pick_non_empty_string(&result, &["account_id", "accountId"]);
        let reply_to = json_pick_non_empty_string(&result, &["reply_to", "replyTo"]);
        let channel_manager = self
            .channel_manager
            .as_ref()
            .ok_or_else(|| "message tool: channel manager unavailable".to_string())?;

        let mut requested_channels = json_pick_non_empty_string_vec(&result, &["channels"]);
        if let Some(single_channel) = json_pick_non_empty_string(&result, &["channel", "provider"])
        {
            requested_channels.push(single_channel);
        }
        let mut resolved_channels: Vec<String> = Vec::new();
        if requested_channels.is_empty() {
            let mut names = channel_manager.read().await.list().await;
            names.sort();
            for name in names {
                if normalize_channel_alias(&name) == "webchat" {
                    continue;
                }
                resolved_channels.push(name);
            }
        } else {
            for requested in requested_channels {
                let resolved = resolve_channel_name(channel_manager, &requested).await?;
                if !resolved_channels.iter().any(|ch| ch == &resolved) {
                    resolved_channels.push(resolved);
                }
            }
        }
        if resolved_channels.is_empty() {
            return Err("message tool: no broadcast channels available".to_string());
        }

        let mut results = Vec::new();
        let mut success = 0usize;
        for channel in &resolved_channels {
            for target in &targets {
                let mut metadata = resolve_outbound_target_metadata(
                    target,
                    reply_to.as_deref(),
                    account_id.as_deref(),
                );
                if let Some(reply) = reply_to.as_deref() {
                    metadata.insert("message_id".to_string(), reply.to_string());
                }
                let msg = ChannelMessage {
                    id: uuid::Uuid::new_v4().to_string(),
                    channel: channel.clone(),
                    sender: "agent-tool".to_string(),
                    content: text.clone(),
                    timestamp: chrono::Utc::now().timestamp_millis(),
                    metadata,
                };
                match channel_manager
                    .read()
                    .await
                    .send_to_channel(channel, &msg)
                    .await
                {
                    Ok(message_id) => {
                        success = success.saturating_add(1);
                        results.push(serde_json::json!({
                            "channel": channel,
                            "target": target,
                            "ok": true,
                            "message_id": message_id
                        }));
                    }
                    Err(err) => {
                        results.push(serde_json::json!({
                            "channel": channel,
                            "target": target,
                            "ok": false,
                            "error": err.to_string()
                        }));
                    }
                }
            }
        }
        let total = results.len();
        let status = if success == total {
            "sent"
        } else if success == 0 {
            "failed"
        } else {
            "partial"
        };
        Ok(serde_json::json!({
            "action": "broadcast_message",
            "status": status,
            "success": success,
            "total": total,
            "results": results
        }))
    }

    async fn fulfill_send_attachment(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let session_meta = self.load_active_session_metadata().await;
        let route = resolve_message_route(
            &result,
            session_meta.as_ref(),
            self.turn_source.as_ref(),
            true,
        )?;
        let target = route
            .target
            .clone()
            .ok_or_else(|| "message tool: missing target for send_attachment".to_string())?;
        let media = resolve_channel_media_payload(&result)?;
        let channel_manager = self
            .channel_manager
            .as_ref()
            .ok_or_else(|| "message tool: channel manager unavailable".to_string())?;
        let channel = resolve_channel_name(channel_manager, &route.channel_raw).await?;
        let message_id = channel_manager
            .read()
            .await
            .send_media(&channel, &target, &media)
            .await
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "action": "send_attachment",
            "status": "sent",
            "channel": channel,
            "target": target,
            "message_id": message_id,
            "resolved_from_session": route.resolved_from_session
        }))
    }

    async fn fulfill_create_thread(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let session_meta = self.load_active_session_metadata().await;
        let route = resolve_message_route(
            &result,
            session_meta.as_ref(),
            self.turn_source.as_ref(),
            false,
        )?;
        let message_id = json_pick_non_empty_string(&result, &["message_id", "messageId"])
            .ok_or_else(|| "message tool: missing message_id/messageId".to_string())?;
        let thread_name = json_pick_non_empty_string(&result, &["thread_name", "threadName"]);
        let channel_manager = self
            .channel_manager
            .as_ref()
            .ok_or_else(|| "message tool: channel manager unavailable".to_string())?;
        let channel = resolve_channel_name(channel_manager, &route.channel_raw).await?;
        let thread_id = channel_manager
            .read()
            .await
            .create_thread(&channel, &message_id, thread_name.as_deref())
            .await
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "action": "create_thread",
            "status": "sent",
            "channel": channel,
            "message_id": message_id,
            "thread_id": thread_id,
            "resolved_from_session": route.resolved_from_session
        }))
    }

    async fn fulfill_send_reaction(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let session_meta = self.load_active_session_metadata().await;
        let route = resolve_message_route(
            &result,
            session_meta.as_ref(),
            self.turn_source.as_ref(),
            false,
        )?;
        let message_id = json_pick_non_empty_string(&result, &["message_id", "messageId"])
            .ok_or_else(|| "message tool: missing message_id/messageId".to_string())?;
        let emoji = json_pick_non_empty_string(&result, &["emoji"])
            .ok_or_else(|| "message tool: missing emoji".to_string())?;
        let channel_manager = self
            .channel_manager
            .as_ref()
            .ok_or_else(|| "message tool: channel manager unavailable".to_string())?;
        let channel = resolve_channel_name(channel_manager, &route.channel_raw).await?;
        let mut metadata = route
            .target
            .as_deref()
            .map(|target| {
                resolve_outbound_target_metadata(target, None, route.account_id.as_deref())
            })
            .unwrap_or_default();
        if let Some(account_id) = route.account_id.as_deref() {
            metadata.insert("account_id".to_string(), account_id.to_string());
        }
        channel_manager
            .read()
            .await
            .send_reaction(&channel, &message_id, &emoji, &metadata)
            .await
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "action": "send_reaction",
            "status": "sent",
            "channel": channel,
            "target": route.target,
            "message_id": message_id,
            "emoji": emoji,
            "resolved_from_session": route.resolved_from_session
        }))
    }

    async fn fulfill_remove_reaction(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let session_meta = self.load_active_session_metadata().await;
        let route = resolve_message_route(
            &result,
            session_meta.as_ref(),
            self.turn_source.as_ref(),
            false,
        )?;
        let message_id = json_pick_non_empty_string(&result, &["message_id", "messageId"])
            .ok_or_else(|| "message tool: missing message_id/messageId".to_string())?;
        let emoji = json_pick_non_empty_string(&result, &["emoji"])
            .ok_or_else(|| "message tool: missing emoji".to_string())?;
        let channel_manager = self
            .channel_manager
            .as_ref()
            .ok_or_else(|| "message tool: channel manager unavailable".to_string())?;
        let channel = resolve_channel_name(channel_manager, &route.channel_raw).await?;
        channel_manager
            .read()
            .await
            .remove_reaction(&channel, &message_id, &emoji)
            .await
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "action": "remove_reaction",
            "status": "sent",
            "channel": channel,
            "target": route.target,
            "message_id": message_id,
            "emoji": emoji,
            "resolved_from_session": route.resolved_from_session
        }))
    }

    async fn fulfill_list_reactions(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let session_meta = self.load_active_session_metadata().await;
        let route = resolve_message_route(
            &result,
            session_meta.as_ref(),
            self.turn_source.as_ref(),
            false,
        )?;
        let message_id = json_pick_non_empty_string(&result, &["message_id", "messageId"])
            .ok_or_else(|| "message tool: missing message_id/messageId".to_string())?;
        let limit = result
            .get("limit")
            .and_then(|v| v.as_u64())
            .and_then(|v| usize::try_from(v).ok());
        let channel_manager = self
            .channel_manager
            .as_ref()
            .ok_or_else(|| "message tool: channel manager unavailable".to_string())?;
        let channel = resolve_channel_name(channel_manager, &route.channel_raw).await?;
        let payload = channel_manager
            .read()
            .await
            .list_reactions(&channel, route.target.as_deref(), &message_id, limit)
            .await
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "action": "list_reactions",
            "status": "ok",
            "channel": channel,
            "target": route.target,
            "message_id": message_id,
            "items": payload,
            "resolved_from_session": route.resolved_from_session
        }))
    }

    async fn fulfill_read_messages(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let session_meta = self.load_active_session_metadata().await;
        let route = resolve_message_route(
            &result,
            session_meta.as_ref(),
            self.turn_source.as_ref(),
            true,
        )?;
        let target = route
            .target
            .clone()
            .ok_or_else(|| "message tool: missing target for read_messages".to_string())?;
        let limit = result
            .get("limit")
            .and_then(|v| v.as_u64())
            .and_then(|v| usize::try_from(v).ok());
        let before = json_pick_non_empty_string(&result, &["before"]);
        let after = json_pick_non_empty_string(&result, &["after"]);
        let around = json_pick_non_empty_string(&result, &["around"]);
        let channel_manager = self
            .channel_manager
            .as_ref()
            .ok_or_else(|| "message tool: channel manager unavailable".to_string())?;
        let channel = resolve_channel_name(channel_manager, &route.channel_raw).await?;
        let payload = channel_manager
            .read()
            .await
            .read_messages(
                &channel,
                &target,
                limit,
                before.as_deref(),
                after.as_deref(),
                around.as_deref(),
            )
            .await
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "action": "read_messages",
            "status": "ok",
            "channel": channel,
            "target": target,
            "items": payload,
            "resolved_from_session": route.resolved_from_session
        }))
    }

    async fn fulfill_search_messages(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let session_meta = self.load_active_session_metadata().await;
        let route = resolve_message_route(
            &result,
            session_meta.as_ref(),
            self.turn_source.as_ref(),
            false,
        )?;
        let query = json_pick_non_empty_string(&result, &["query", "text", "message", "content"])
            .ok_or_else(|| "message tool: missing query".to_string())?;
        let limit = result
            .get("limit")
            .and_then(|v| v.as_u64())
            .and_then(|v| usize::try_from(v).ok());
        let channel_manager = self
            .channel_manager
            .as_ref()
            .ok_or_else(|| "message tool: channel manager unavailable".to_string())?;
        let channel = resolve_channel_name(channel_manager, &route.channel_raw).await?;
        let payload = channel_manager
            .read()
            .await
            .search_messages(&channel, route.target.as_deref(), &query, limit)
            .await
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "action": "search_messages",
            "status": "ok",
            "channel": channel,
            "target": route.target,
            "query": query,
            "items": payload,
            "resolved_from_session": route.resolved_from_session
        }))
    }

    async fn fulfill_edit_message(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let session_meta = self.load_active_session_metadata().await;
        let route = resolve_message_route(
            &result,
            session_meta.as_ref(),
            self.turn_source.as_ref(),
            false,
        )?;
        let message_id = json_pick_non_empty_string(&result, &["message_id", "messageId"])
            .ok_or_else(|| "message tool: missing message_id/messageId".to_string())?;
        let text = json_pick_non_empty_string(&result, &["text", "message", "content"])
            .ok_or_else(|| "message tool: missing text/message/content".to_string())?;
        let channel_manager = self
            .channel_manager
            .as_ref()
            .ok_or_else(|| "message tool: channel manager unavailable".to_string())?;
        let channel = resolve_channel_name(channel_manager, &route.channel_raw).await?;
        channel_manager
            .read()
            .await
            .edit_message(&channel, &message_id, &text)
            .await
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "action": "edit_message",
            "status": "sent",
            "channel": channel,
            "target": route.target,
            "message_id": message_id,
            "resolved_from_session": route.resolved_from_session
        }))
    }

    async fn fulfill_pin_message(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let session_meta = self.load_active_session_metadata().await;
        let route = resolve_message_route(
            &result,
            session_meta.as_ref(),
            self.turn_source.as_ref(),
            true,
        )?;
        let target = route
            .target
            .clone()
            .ok_or_else(|| "message tool: missing target for pin_message".to_string())?;
        let message_id = json_pick_non_empty_string(&result, &["message_id", "messageId"])
            .ok_or_else(|| "message tool: missing message_id/messageId".to_string())?;
        let channel_manager = self
            .channel_manager
            .as_ref()
            .ok_or_else(|| "message tool: channel manager unavailable".to_string())?;
        let channel = resolve_channel_name(channel_manager, &route.channel_raw).await?;
        channel_manager
            .read()
            .await
            .pin_message(&channel, &target, &message_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "action": "pin_message",
            "status": "sent",
            "channel": channel,
            "target": target,
            "message_id": message_id,
            "resolved_from_session": route.resolved_from_session
        }))
    }

    async fn fulfill_unpin_message(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let session_meta = self.load_active_session_metadata().await;
        let route = resolve_message_route(
            &result,
            session_meta.as_ref(),
            self.turn_source.as_ref(),
            true,
        )?;
        let target = route
            .target
            .clone()
            .ok_or_else(|| "message tool: missing target for unpin_message".to_string())?;
        let message_id = json_pick_non_empty_string(&result, &["message_id", "messageId"])
            .ok_or_else(|| "message tool: missing message_id/messageId".to_string())?;
        let channel_manager = self
            .channel_manager
            .as_ref()
            .ok_or_else(|| "message tool: channel manager unavailable".to_string())?;
        let channel = resolve_channel_name(channel_manager, &route.channel_raw).await?;
        channel_manager
            .read()
            .await
            .unpin_message(&channel, &target, &message_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "action": "unpin_message",
            "status": "sent",
            "channel": channel,
            "target": target,
            "message_id": message_id,
            "resolved_from_session": route.resolved_from_session
        }))
    }

    async fn fulfill_list_pins(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let session_meta = self.load_active_session_metadata().await;
        let route = resolve_message_route(
            &result,
            session_meta.as_ref(),
            self.turn_source.as_ref(),
            true,
        )?;
        let target = route
            .target
            .clone()
            .ok_or_else(|| "message tool: missing target for list_pins".to_string())?;
        let limit = result
            .get("limit")
            .and_then(|v| v.as_u64())
            .and_then(|v| usize::try_from(v).ok());
        let channel_manager = self
            .channel_manager
            .as_ref()
            .ok_or_else(|| "message tool: channel manager unavailable".to_string())?;
        let channel = resolve_channel_name(channel_manager, &route.channel_raw).await?;
        let payload = channel_manager
            .read()
            .await
            .list_pins(&channel, &target, limit)
            .await
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "action": "list_pins",
            "status": "ok",
            "channel": channel,
            "target": target,
            "items": payload,
            "resolved_from_session": route.resolved_from_session
        }))
    }

    async fn fulfill_get_permissions(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let session_meta = self.load_active_session_metadata().await;
        let route = resolve_message_route(
            &result,
            session_meta.as_ref(),
            self.turn_source.as_ref(),
            true,
        )?;
        let target = route
            .target
            .clone()
            .ok_or_else(|| "message tool: missing target for get_permissions".to_string())?;
        let channel_manager = self
            .channel_manager
            .as_ref()
            .ok_or_else(|| "message tool: channel manager unavailable".to_string())?;
        let channel = resolve_channel_name(channel_manager, &route.channel_raw).await?;
        let payload = channel_manager
            .read()
            .await
            .get_permissions(&channel, &target)
            .await
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "action": "get_permissions",
            "status": "ok",
            "channel": channel,
            "target": target,
            "items": payload,
            "resolved_from_session": route.resolved_from_session
        }))
    }

    async fn fulfill_custom_channel_action(
        &self,
        mut result: serde_json::Value,
        action_name: &str,
        require_target: bool,
    ) -> Result<serde_json::Value, String> {
        let session_meta = self.load_active_session_metadata().await;
        let route = resolve_message_route(
            &result,
            session_meta.as_ref(),
            self.turn_source.as_ref(),
            require_target,
        )?;
        let channel_manager = self
            .channel_manager
            .as_ref()
            .ok_or_else(|| "message tool: channel manager unavailable".to_string())?;
        let channel = resolve_channel_name(channel_manager, &route.channel_raw).await?;
        if let Some(obj) = result.as_object_mut() {
            obj.insert(
                "action".to_string(),
                serde_json::Value::String(action_name.to_string()),
            );
            if obj.get("target").is_none()
                && let Some(target) = route.target.as_ref()
            {
                obj.insert(
                    "target".to_string(),
                    serde_json::Value::String(target.clone()),
                );
            }
            if obj.get("account_id").is_none()
                && let Some(account_id) = route.account_id.as_ref()
            {
                obj.insert(
                    "account_id".to_string(),
                    serde_json::Value::String(account_id.clone()),
                );
            }
            if obj.get("thread_id").is_none()
                && let Some(thread_id) = route.thread_id.as_ref()
            {
                obj.insert(
                    "thread_id".to_string(),
                    serde_json::Value::String(thread_id.clone()),
                );
            }
        }
        let payload = channel_manager
            .read()
            .await
            .custom_action(&channel, action_name, route.target.as_deref(), &result)
            .await
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "action": action_name,
            "status": "ok",
            "channel": channel,
            "target": route.target,
            "result": payload,
            "resolved_from_session": route.resolved_from_session
        }))
    }

    async fn fulfill_delete_message(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let session_meta = self.load_active_session_metadata().await;
        let route = resolve_message_route(
            &result,
            session_meta.as_ref(),
            self.turn_source.as_ref(),
            false,
        )?;
        let message_id = json_pick_non_empty_string(&result, &["message_id", "messageId"])
            .ok_or_else(|| "message tool: missing message_id/messageId".to_string())?;
        let channel_manager = self
            .channel_manager
            .as_ref()
            .ok_or_else(|| "message tool: channel manager unavailable".to_string())?;
        let channel = resolve_channel_name(channel_manager, &route.channel_raw).await?;
        channel_manager
            .read()
            .await
            .delete_message(&channel, &message_id)
            .await
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "action": "delete_message",
            "status": "sent",
            "channel": channel,
            "target": route.target,
            "message_id": message_id,
            "resolved_from_session": route.resolved_from_session
        }))
    }

    async fn fulfill_send_thread_reply(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let session_meta = self.load_active_session_metadata().await;
        let route = resolve_message_route(
            &result,
            session_meta.as_ref(),
            self.turn_source.as_ref(),
            true,
        )?;
        let thread_id = json_pick_non_empty_string(&result, &["thread_id", "threadId"])
            .or_else(|| route.target.clone())
            .ok_or_else(|| "message tool: missing thread_id/threadId/target".to_string())?;
        let reply_to = json_pick_non_empty_string(&result, &["reply_to", "replyTo"]);
        let text = json_pick_non_empty_string(&result, &["text", "message", "content"])
            .ok_or_else(|| "message tool: missing text/message/content".to_string())?;
        let target = route
            .target
            .clone()
            .ok_or_else(|| "message tool: missing target for thread reply".to_string())?;
        let channel_manager = self
            .channel_manager
            .as_ref()
            .ok_or_else(|| "message tool: channel manager unavailable".to_string())?;
        let channel = resolve_channel_name(channel_manager, &route.channel_raw).await?;
        let mut metadata = resolve_outbound_target_metadata(
            &target,
            Some(&thread_id),
            route.account_id.as_deref(),
        );
        if let Some(reply) = reply_to.as_ref() {
            metadata.insert("reply_to".to_string(), reply.clone());
            metadata.insert("message_id".to_string(), reply.clone());
        }
        let msg = ChannelMessage {
            id: uuid::Uuid::new_v4().to_string(),
            channel: channel.clone(),
            sender: "agent-tool".to_string(),
            content: text,
            timestamp: chrono::Utc::now().timestamp_millis(),
            metadata,
        };
        let message_id = channel_manager
            .read()
            .await
            .send_thread_reply(&channel, &thread_id, &msg)
            .await
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "action": "send_thread_reply",
            "status": "sent",
            "channel": channel,
            "target": target,
            "thread_id": thread_id,
            "reply_to": reply_to,
            "message_id": message_id,
            "resolved_from_session": route.resolved_from_session
        }))
    }

    async fn fulfill_send_poll(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let session_meta = self.load_active_session_metadata().await;
        let route = resolve_message_route(
            &result,
            session_meta.as_ref(),
            self.turn_source.as_ref(),
            true,
        )?;
        let target = route
            .target
            .clone()
            .ok_or_else(|| "message tool: missing target/to for poll".to_string())?;
        let question =
            json_pick_non_empty_string(&result, &["question", "poll_question", "pollQuestion"])
                .ok_or_else(|| "message tool: missing poll question".to_string())?;
        let options: Vec<String> = result
            .get("options")
            .or_else(|| result.get("poll_options"))
            .or_else(|| result.get("pollOption"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::trim))
                    .filter(|s| !s.is_empty())
                    .map(ToString::to_string)
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();
        if options.is_empty() {
            return Err("message tool: missing poll options".to_string());
        }
        let is_anonymous = result
            .get("is_anonymous")
            .or_else(|| result.get("poll_anonymous"))
            .or_else(|| result.get("pollAnonymous"))
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let allows_multiple = result
            .get("allows_multiple")
            .or_else(|| result.get("poll_multiple"))
            .or_else(|| result.get("pollMulti"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let channel_manager = self
            .channel_manager
            .as_ref()
            .ok_or_else(|| "message tool: channel manager unavailable".to_string())?;
        let channel = resolve_channel_name(channel_manager, &route.channel_raw).await?;
        let poll = PollRequest {
            question,
            options,
            is_anonymous,
            allows_multiple,
        };
        let message_id = channel_manager
            .read()
            .await
            .send_poll(&channel, &target, &poll)
            .await
            .map_err(|e| e.to_string())?;
        Ok(serde_json::json!({
            "action": "send_poll",
            "status": "sent",
            "channel": channel,
            "target": target,
            "message_id": message_id,
            "resolved_from_session": route.resolved_from_session
        }))
    }

    async fn fulfill_list_groups(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let channel_manager = self
            .channel_manager
            .as_ref()
            .ok_or_else(|| "message tool: channel manager unavailable".to_string())?;
        let limit = result
            .get("limit")
            .and_then(|v| v.as_u64())
            .and_then(|v| usize::try_from(v).ok());
        let requested_channel = json_pick_non_empty_string(&result, &["channel", "provider"]);
        let channels = if let Some(requested) = requested_channel {
            vec![resolve_channel_name(channel_manager, &requested).await?]
        } else {
            let mut all = channel_manager.read().await.list().await;
            all.sort();
            all.into_iter()
                .filter(|name| normalize_channel_alias(name) != "webchat")
                .collect::<Vec<String>>()
        };
        if channels.is_empty() {
            return Err("message tool: no channel available for list_groups".to_string());
        }
        let mut items = Vec::new();
        let mut errors = Vec::new();
        for channel in channels {
            match channel_manager.read().await.list_groups(&channel).await {
                Ok(groups) => {
                    for group in groups {
                        items.push(serde_json::json!({
                            "channel": channel,
                            "id": group.id,
                            "name": group.name,
                            "member_count": group.member_count,
                            "group_type": group.group_type
                        }));
                    }
                }
                Err(err) => {
                    errors.push(serde_json::json!({
                        "channel": channel,
                        "error": err.to_string()
                    }));
                }
            }
        }
        if let Some(max) = limit
            && items.len() > max
        {
            items.truncate(max);
        }
        let status = if errors.is_empty() {
            "ok"
        } else if items.is_empty() {
            "failed"
        } else {
            "partial"
        };
        Ok(serde_json::json!({
            "action": "list_groups",
            "status": status,
            "items": items,
            "errors": errors
        }))
    }

    async fn fulfill_list_members(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let session_meta = self.load_active_session_metadata().await;
        let route = resolve_message_route(
            &result,
            session_meta.as_ref(),
            self.turn_source.as_ref(),
            false,
        )?;
        let group_id =
            json_pick_non_empty_string(&result, &["group_id", "groupId", "target", "to"])
                .or_else(|| route.target.clone())
                .ok_or_else(|| {
                    "message tool: missing group_id/target for list_members".to_string()
                })?;
        let limit = result
            .get("limit")
            .and_then(|v| v.as_u64())
            .and_then(|v| usize::try_from(v).ok());
        let channel_manager = self
            .channel_manager
            .as_ref()
            .ok_or_else(|| "message tool: channel manager unavailable".to_string())?;
        let channel = resolve_channel_name(channel_manager, &route.channel_raw).await?;
        let mut members = channel_manager
            .read()
            .await
            .list_members(&channel, &group_id)
            .await
            .map_err(|e| e.to_string())?;
        if let Some(max) = limit
            && members.len() > max
        {
            members.truncate(max);
        }
        let members_json = members
            .into_iter()
            .map(|member| {
                serde_json::json!({
                    "id": member.id,
                    "name": member.name,
                    "channel": member.channel,
                    "avatar": member.avatar,
                    "status": member.status
                })
            })
            .collect::<Vec<serde_json::Value>>();
        Ok(serde_json::json!({
            "action": "list_members",
            "status": "ok",
            "channel": channel,
            "group_id": group_id,
            "items": members_json,
            "resolved_from_session": route.resolved_from_session
        }))
    }

    async fn fulfill_sessions_list(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let channel_filter = result
            .get("channel")
            .and_then(|v| v.as_str())
            .map(normalize_channel_alias);
        let active_only = result
            .get("active_only")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let limit = result.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;

        let session_manager = self
            .session_manager
            .as_ref()
            .ok_or_else(|| "sessions_list: session manager unavailable".to_string())?;
        let mgr = session_manager.read().await;
        let access_ctx = self.build_session_access_context(&mgr).await?;
        let mut sessions = mgr.list_sessions().map_err(|e| e.to_string())?;
        sessions.retain(|s| {
            self.check_session_access(&access_ctx, &s.key, SessionAccessAction::List)
                .is_ok()
        });

        if let Some(filter) = channel_filter {
            sessions.retain(|s| {
                mgr.get_session_metadata(&s.key)
                    .ok()
                    .and_then(|m| {
                        m.get("channel")
                            .cloned()
                            .map(|c| normalize_channel_alias(&c))
                    })
                    .map(|c| c == filter)
                    .unwrap_or(false)
            });
        }

        if active_only {
            sessions.retain(|s| {
                !mgr.get_session_metadata(&s.key)
                    .ok()
                    .and_then(|m| m.get("terminated").cloned())
                    .map(|v| v == "true")
                    .unwrap_or(false)
            });
        }

        sessions.truncate(limit);
        let payload: Vec<serde_json::Value> = sessions
            .into_iter()
            .map(|s| {
                let metadata = mgr.get_session_metadata(&s.key).unwrap_or_default();
                serde_json::json!({
                    "key": s.key,
                    "agent_id": s.agent_id,
                    "created_at": s.created_at,
                    "updated_at": s.updated_at,
                    "message_count": s.message_count,
                    "label": metadata.get("label").cloned(),
                    "spawned_by": metadata
                        .get("spawnedBy")
                        .or_else(|| metadata.get("parentSessionKey"))
                        .cloned()
                })
            })
            .collect();
        Ok(serde_json::json!({
            "action": "sessions_list",
            "sessions": payload,
            "count": payload.len()
        }))
    }

    async fn fulfill_sessions_history(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let key = resolve_session_key(
            result
                .get("session_key")
                .and_then(|v| v.as_str())
                .or_else(|| result.get("sessionKey").and_then(|v| v.as_str())),
            self,
        )
        .ok_or_else(|| "sessions_history: missing session_key".to_string())?;
        let limit = result.get("limit").and_then(|v| v.as_u64()).unwrap_or(50) as usize;
        let session_manager = self
            .session_manager
            .as_ref()
            .ok_or_else(|| "sessions_history: session manager unavailable".to_string())?;
        let mgr = session_manager.read().await;
        let access_ctx = self.build_session_access_context(&mgr).await?;
        if let Err(err) = self.check_session_access(&access_ctx, &key, SessionAccessAction::History)
        {
            return Ok(serde_json::json!({
                "action": "sessions_history",
                "session_key": key,
                "status": "forbidden",
                "error": err
            }));
        }
        let mut messages = mgr.get_messages(&key, limit).map_err(|e| e.to_string())?;
        messages.reverse();
        let rows: Vec<serde_json::Value> = messages
            .into_iter()
            .map(|m| {
                serde_json::json!({
                    "role": m.role,
                    "content": m.content,
                    "timestamp": m.timestamp
                })
            })
            .collect();
        Ok(serde_json::json!({
            "action": "sessions_history",
            "session_key": key,
            "messages": rows,
            "count": rows.len()
        }))
    }

    async fn fulfill_sessions_send(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let mut key = resolve_session_key(
            result
                .get("session_key")
                .and_then(|v| v.as_str())
                .or_else(|| result.get("sessionKey").and_then(|v| v.as_str())),
            self,
        );
        let label = result
            .get("label")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let label_agent_id = result
            .get("agent_id")
            .and_then(|v| v.as_str())
            .or_else(|| result.get("agentId").and_then(|v| v.as_str()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(sanitize_session_token);
        let text = result
            .get("text")
            .and_then(|v| v.as_str())
            .or_else(|| result.get("message").and_then(|v| v.as_str()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| "sessions_send: text/message is empty".to_string())?;
        let role = result
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or("user");
        let timeout_seconds = result
            .get("timeout_seconds")
            .and_then(|v| v.as_u64())
            .or_else(|| result.get("timeoutSeconds").and_then(|v| v.as_u64()))
            .unwrap_or(30);
        let session_manager = self
            .session_manager
            .as_ref()
            .ok_or_else(|| "sessions_send: session manager unavailable".to_string())?;
        let mgr = session_manager.read().await;
        let access_ctx = self.build_session_access_context(&mgr).await?;
        if key.is_none() {
            if let Some(label) = label.as_deref() {
                let sessions = mgr.list_sessions().map_err(|e| e.to_string())?;
                key = sessions.into_iter().find_map(|s| {
                    if let Some(filter_agent) = label_agent_id.as_deref()
                        && Self::resolve_agent_id_from_session_key(&s.key) != filter_agent
                    {
                        return None;
                    }
                    if self
                        .check_session_access(&access_ctx, &s.key, SessionAccessAction::Send)
                        .is_err()
                    {
                        return None;
                    }
                    mgr.get_session_metadata(&s.key)
                        .ok()
                        .and_then(|m| m.get("label").cloned())
                        .filter(|v| v.eq_ignore_ascii_case(label))
                        .map(|_| s.key)
                });
            }
        }
        let key = key.ok_or_else(|| {
            "sessions_send: missing session key (or unresolved label)".to_string()
        })?;
        if let Err(err) = self.check_session_access(&access_ctx, &key, SessionAccessAction::Send) {
            return Ok(serde_json::json!({
                "action": "sessions_send",
                "session_key": key,
                "status": "forbidden",
                "error": err
            }));
        }
        if mgr.get_session(&key).map_err(|e| e.to_string())?.is_none() {
            let _ = mgr
                .create_session(&key, "default")
                .map_err(|e| e.to_string())?;
        }
        mgr.add_message(&key, role, text)
            .map_err(|e| e.to_string())?;

        // Non-user role is treated as data append only.
        if !role.eq_ignore_ascii_case("user") {
            let session = mgr
                .get_session(&key)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "sessions_send: failed to load updated session".to_string())?;
            return Ok(serde_json::json!({
                "action": "sessions_send",
                "session_key": key,
                "status": "sent",
                "message_count": session.message_count
            }));
        }

        let Some(provider) = self.llm_provider.as_ref() else {
            let session = mgr
                .get_session(&key)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "sessions_send: failed to load updated session".to_string())?;
            return Ok(serde_json::json!({
                "action": "sessions_send",
                "session_key": key,
                "status": "queued",
                "message_count": session.message_count
            }));
        };

        let requester_session_key = self.session_id.clone();
        let requester_channel = requester_session_key
            .as_ref()
            .and_then(|sid| mgr.get_session_metadata(sid).ok())
            .and_then(|meta| {
                session_metadata_pick(
                    &meta,
                    &["delivery.channel", "lastChannel", "last_channel", "channel"],
                )
            });
        let max_ping_pong_turns = self.resolve_a2a_ping_pong_turns().await;
        let announce_timeout_seconds = if timeout_seconds == 0 {
            30
        } else {
            timeout_seconds
        };
        let session_model_override = mgr
            .get_session_metadata(&key)
            .ok()
            .and_then(|meta| session_model_override_from_metadata(&meta));

        let run_id = uuid::Uuid::new_v4().to_string();
        let child_exec = self.build_child_executor(key.clone());
        let provider = provider.clone();
        let key_for_task = key.clone();
        let text_for_task = text.to_string();
        let session_manager_for_task = session_manager.clone();
        let flow_exec = self.clone();
        let flow_provider = provider.clone();
        let requester_session_for_task = requester_session_key.clone();
        let requester_channel_for_task = requester_channel.clone();
        let session_model_override_for_task = session_model_override.clone();
        let round_one_message_context = build_agent_to_agent_message_context(
            requester_session_key.as_deref(),
            requester_channel.as_deref(),
            &key,
        );
        let mut round_one_prompt = default_agent_system_prompt(&child_exec);
        if !round_one_message_context.trim().is_empty() {
            round_one_prompt.push_str("\n\n");
            round_one_prompt.push_str(&round_one_message_context);
        }
        let round_one_prompt_for_task = round_one_prompt.clone();

        if timeout_seconds == 0 {
            tokio::spawn(async move {
                if let Ok(out) = agent_reply_with_prompt_and_model_detailed(
                    &provider,
                    &child_exec,
                    &text_for_task,
                    Some(&key_for_task),
                    &round_one_prompt_for_task,
                    session_model_override_for_task.as_deref(),
                )
                .await
                {
                    let _ = session_manager_for_task.read().await.add_message(
                        &key_for_task,
                        "assistant",
                        &out.reply,
                    );
                    let guard = session_manager_for_task.read().await;
                    let _ =
                        apply_session_usage_delta(&guard, &key_for_task, &out.usage, &out.model);
                    let _ = flow_exec
                        .run_sessions_send_a2a_flow(
                            flow_provider,
                            key_for_task.clone(),
                            key_for_task.clone(),
                            text_for_task.clone(),
                            announce_timeout_seconds,
                            max_ping_pong_turns,
                            requester_session_for_task.clone(),
                            requester_channel_for_task.clone(),
                            Some(out.reply),
                        )
                        .await;
                }
            });
            let session = mgr
                .get_session(&key)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "sessions_send: failed to load updated session".to_string())?;
            return Ok(serde_json::json!({
                "action": "sessions_send",
                "run_id": run_id,
                "session_key": key,
                "status": "accepted",
                "delivery": {
                    "status": "pending",
                    "mode": "announce"
                },
                "message_count": session.message_count
            }));
        }

        let out = match tokio::time::timeout(
            Duration::from_secs(timeout_seconds),
            agent_reply_with_prompt_and_model_detailed(
                &provider,
                &child_exec,
                text,
                Some(&key),
                &round_one_prompt,
                session_model_override.as_deref(),
            ),
        )
        .await
        {
            Ok(Ok(out)) => out,
            Ok(Err(err)) => {
                return Ok(serde_json::json!({
                    "action": "sessions_send",
                    "run_id": run_id,
                    "session_key": key,
                    "status": "error",
                    "error": err
                }));
            }
            Err(_) => {
                return Ok(serde_json::json!({
                    "action": "sessions_send",
                    "run_id": run_id,
                    "session_key": key,
                    "status": "timeout",
                    "error": format!("timed out after {}s", timeout_seconds)
                }));
            }
        };

        mgr.add_message(&key, "assistant", &out.reply)
            .map_err(|e| e.to_string())?;
        let _ = apply_session_usage_delta(&mgr, &key, &out.usage, &out.model);
        let flow_exec = self.clone();
        let flow_provider = provider.clone();
        let key_for_flow = key.clone();
        let text_for_flow = text.to_string();
        let requester_session_for_flow = requester_session_key.clone();
        let requester_channel_for_flow = requester_channel.clone();
        let round_one_reply = out.reply.clone();
        tokio::spawn(async move {
            let _ = flow_exec
                .run_sessions_send_a2a_flow(
                    flow_provider,
                    key_for_flow.clone(),
                    key_for_flow,
                    text_for_flow,
                    announce_timeout_seconds,
                    max_ping_pong_turns,
                    requester_session_for_flow,
                    requester_channel_for_flow,
                    Some(round_one_reply),
                )
                .await;
        });
        let session = mgr
            .get_session(&key)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "sessions_send: failed to load updated session".to_string())?;
        Ok(serde_json::json!({
            "action": "sessions_send",
            "run_id": run_id,
            "session_key": key,
            "status": "ok",
            "reply": out.reply,
            "usage": usage_summary_to_json(&out.usage),
            "model": out.model,
            "delivery": {
                "status": "pending",
                "mode": "announce"
            },
            "message_count": session.message_count
        }))
    }

    async fn fulfill_sessions_spawn(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let agent_id_raw = result
            .get("agent_id")
            .and_then(|v| v.as_str())
            .or_else(|| result.get("agentId").and_then(|v| v.as_str()))
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("default")
            .to_string();
        let target_agent_id = sanitize_session_token(&agent_id_raw);
        let mut task = result
            .get("task")
            .and_then(|v| v.as_str())
            .or_else(|| result.get("prompt").and_then(|v| v.as_str()))
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if task.is_empty() {
            return Err("sessions_spawn: missing task/prompt".to_string());
        }
        let mut label = result
            .get("label")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or(target_agent_id.as_str())
            .to_string();
        let explicit_model = result
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string);
        let mut model_hint = if let Some(model) = explicit_model {
            model
        } else if let Some(model) = self.resolve_subagent_model_hint(&target_agent_id).await {
            model
        } else if let Some(provider_model) = self
            .llm_provider
            .as_ref()
            .map(|p| p.default_model().to_string())
        {
            provider_model
        } else {
            "default".to_string()
        };
        let timeout_seconds = result
            .get("run_timeout_seconds")
            .and_then(|v| v.as_u64())
            .or_else(|| result.get("runTimeoutSeconds").and_then(|v| v.as_u64()))
            .or_else(|| result.get("timeoutSeconds").and_then(|v| v.as_u64()))
            .unwrap_or(300);
        let expects_completion_message = result
            .get("expectsCompletionMessage")
            .and_then(|v| v.as_bool())
            .or_else(|| {
                result
                    .get("expects_completion_message")
                    .and_then(|v| v.as_bool())
            })
            .unwrap_or(true);
        let mut thread_requested = result
            .get("thread")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let mut spawn_mode = result
            .get("mode")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|mode| {
                if mode.eq_ignore_ascii_case("session") {
                    "session".to_string()
                } else {
                    "run".to_string()
                }
            })
            .unwrap_or_else(|| {
                if thread_requested {
                    "session".to_string()
                } else {
                    "run".to_string()
                }
            });
        let mut cleanup = result
            .get("cleanup")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|v| {
                if v.eq_ignore_ascii_case("delete") {
                    "delete".to_string()
                } else {
                    "keep".to_string()
                }
            })
            .unwrap_or_else(|| "keep".to_string());
        if spawn_mode == "session" && !thread_requested {
            return Ok(serde_json::json!({
                "action": "sessions_spawn",
                "status": "error",
                "error": "mode=\"session\" requires thread=true so the subagent can stay bound to a thread."
            }));
        }
        if spawn_mode == "session" {
            cleanup = "keep".to_string();
        }
        let requester_key = self
            .session_id
            .clone()
            .unwrap_or_else(|| "agent:default:main".to_string());
        let requester_agent_id = Self::resolve_agent_id_from_session_key(&requester_key);
        if target_agent_id != requester_agent_id {
            let allow_agents = self
                .resolve_subagent_allow_agents(&requester_agent_id)
                .await;
            if !Self::is_agent_allowed_by_allowlist(&allow_agents, &target_agent_id) {
                let allow_set: Vec<String> = allow_agents
                    .into_iter()
                    .filter(|v| v != "*")
                    .map(|v| sanitize_session_token(&v))
                    .collect();
                let allowed_text = if allow_set.is_empty() {
                    "none".to_string()
                } else {
                    allow_set.join(", ")
                };
                return Ok(serde_json::json!({
                    "action": "sessions_spawn",
                    "status": "forbidden",
                    "error": format!("agentId is not allowed for sessions_spawn (allowed: {})", allowed_text)
                }));
            }
        }
        let parent_session = result
            .get("parent_session_key")
            .and_then(|v| v.as_str())
            .or(self.session_id.as_deref());
        let session_key = format!(
            "agent:{}:subagent:{}",
            target_agent_id,
            uuid::Uuid::new_v4()
        );
        let mut thread_binding_ready = !thread_requested;
        let mut hook_binding_target: Option<AnnounceTarget> = None;
        if let Some(hooks) = self.hook_pipeline.as_ref() {
            let base_cfg = serde_json::json!({
                "child_session_key": session_key.clone(),
                "requester_session_key": requester_key.clone(),
                "agent_id": target_agent_id.clone(),
                "label": label.clone(),
                "task": task.clone(),
                "model": model_hint.clone(),
                "timeout_seconds": timeout_seconds,
                "thread": thread_requested,
                "mode": spawn_mode,
                "cleanup": cleanup
            });
            let patched = hooks
                .subagent_spawning(&target_agent_id, &base_cfg.to_string())
                .await
                .map_err(|e| format!("subagent_spawning hook failed: {}", e))?;
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&patched) {
                if thread_requested {
                    let status = value
                        .get("status")
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_ascii_lowercase())
                        .unwrap_or_default();
                    if status == "error" {
                        let hook_error = value
                            .get("error")
                            .and_then(|v| v.as_str())
                            .map(str::trim)
                            .filter(|v| !v.is_empty())
                            .unwrap_or(
                                "Failed to prepare thread binding for this subagent session.",
                            )
                            .to_string();
                        return Ok(serde_json::json!({
                            "action": "sessions_spawn",
                            "status": "error",
                            "error": hook_error
                        }));
                    }
                    thread_binding_ready = value
                        .get("threadBindingReady")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                }
                if let Some(target_value) = value
                    .get("deliveryTarget")
                    .or_else(|| value.get("boundDeliveryTarget"))
                    .or_else(|| value.get("origin"))
                    .or_else(|| value.get("requesterOrigin"))
                    .or_else(|| {
                        if value.get("channel").is_some() || value.get("to").is_some() {
                            Some(&value)
                        } else {
                            None
                        }
                    })
                    && let Some(target) =
                        Self::parse_subagent_delivery_target_from_hook(target_value)
                {
                    hook_binding_target = Some(target);
                }
                if let Some(v) = value.get("label").and_then(|v| v.as_str()).map(str::trim)
                    && !v.is_empty()
                {
                    label = v.to_string();
                }
                if let Some(v) = value.get("task").and_then(|v| v.as_str()).map(str::trim)
                    && !v.is_empty()
                {
                    task = v.to_string();
                }
                if let Some(v) = value.get("model").and_then(|v| v.as_str()).map(str::trim)
                    && !v.is_empty()
                {
                    model_hint = v.to_string();
                }
                if let Some(v) = value.get("thread").and_then(|v| v.as_bool()) {
                    thread_requested = v;
                }
                if let Some(v) = value.get("mode").and_then(|v| v.as_str()).map(str::trim)
                    && !v.is_empty()
                {
                    spawn_mode = if v.eq_ignore_ascii_case("session") {
                        "session".to_string()
                    } else {
                        "run".to_string()
                    };
                }
                if let Some(v) = value.get("cleanup").and_then(|v| v.as_str()).map(str::trim)
                    && !v.is_empty()
                {
                    cleanup = if v.eq_ignore_ascii_case("delete") {
                        "delete".to_string()
                    } else {
                        "keep".to_string()
                    };
                }
            }
        } else if thread_requested {
            return Ok(serde_json::json!({
                "action": "sessions_spawn",
                "status": "error",
                "error": "thread=true is unavailable because no channel plugin registered subagent_spawning hooks."
            }));
        }
        if spawn_mode == "session" && !thread_requested {
            return Ok(serde_json::json!({
                "action": "sessions_spawn",
                "status": "error",
                "error": "mode=\"session\" requires thread=true so the subagent can stay bound to a thread."
            }));
        }
        if thread_requested && !thread_binding_ready {
            return Ok(serde_json::json!({
                "action": "sessions_spawn",
                "status": "error",
                "error": "Unable to create or bind a thread for this subagent session. Session mode is unavailable for this target."
            }));
        }
        if spawn_mode == "session" {
            cleanup = "keep".to_string();
        }
        if task.trim().is_empty() {
            return Err("sessions_spawn: task was cleared by subagent_spawning hook".to_string());
        }

        let session_manager = self
            .session_manager
            .as_ref()
            .ok_or_else(|| "sessions_spawn: session manager unavailable".to_string())?;
        let mgr = session_manager.read().await;
        let (max_spawn_depth, max_children) = self.resolve_subagent_limits().await;
        let caller_depth = self.resolve_spawn_depth(&mgr, &requester_key);
        if caller_depth >= max_spawn_depth {
            return Ok(serde_json::json!({
                "action": "sessions_spawn",
                "status": "forbidden",
                "error": format!(
                    "sessions_spawn is not allowed at this depth (current depth: {}, max: {})",
                    caller_depth,
                    max_spawn_depth
                )
            }));
        }
        let active_children = self.count_active_children_for_requester(&requester_key);
        if active_children >= max_children {
            return Ok(serde_json::json!({
                "action": "sessions_spawn",
                "status": "forbidden",
                "error": format!(
                    "sessions_spawn has reached max active children for this session ({}/{})",
                    active_children,
                    max_children
                )
            }));
        }
        let child_depth = caller_depth.saturating_add(1);
        let created = mgr
            .create_session(&session_key, &target_agent_id)
            .map_err(|e| e.to_string())?;
        if let Some(parent) = parent_session {
            let _ = mgr.set_session_metadata_field(&session_key, "spawnedBy", parent);
            let _ = mgr.set_session_metadata_field(&session_key, "parentSessionKey", parent);
        }
        let _ = mgr.set_session_metadata_field(&session_key, "targetKind", "subagent");
        let _ = mgr.set_session_metadata_field(&session_key, "subagentLabel", &label);
        let _ = mgr.set_session_metadata_field(&session_key, "label", &label);
        let _ = mgr.set_session_metadata_field(&session_key, "subagentTask", &task);
        let _ =
            mgr.set_session_metadata_field(&session_key, "spawnDepth", &child_depth.to_string());
        let _ = mgr.set_session_metadata_field(&session_key, "spawnMode", &spawn_mode);
        let _ = mgr.set_session_metadata_field(&session_key, "cleanupPolicy", &cleanup);
        let _ = mgr.set_session_metadata_field(
            &session_key,
            "threadRequested",
            if thread_requested { "true" } else { "false" },
        );
        let _ = mgr.set_session_metadata_field(
            &session_key,
            "runTimeoutSeconds",
            &timeout_seconds.to_string(),
        );
        if !model_hint.is_empty() {
            let _ = mgr.set_session_metadata_field(&session_key, "modelOverride", &model_hint);
        }
        if thread_requested && thread_binding_ready {
            let binding_target = hook_binding_target
                .or_else(|| self.resolve_announce_target(&mgr, &requester_key, &requester_key));
            if let Some(target) = binding_target {
                let _ = mgr.set_session_metadata_field(
                    &session_key,
                    "delivery.channel",
                    &target.channel,
                );
                let _ = mgr.set_session_metadata_field(&session_key, "delivery.to", &target.to);
                if let Some(account_id) = target.account_id.as_deref()
                    && !account_id.trim().is_empty()
                {
                    let _ = mgr.set_session_metadata_field(
                        &session_key,
                        "delivery.accountId",
                        account_id.trim(),
                    );
                }
                if let Some(thread_id) = target.thread_id.as_deref()
                    && !thread_id.trim().is_empty()
                {
                    let _ = mgr.set_session_metadata_field(
                        &session_key,
                        "delivery.threadId",
                        thread_id.trim(),
                    );
                }
                let _ = self.upsert_session_binding(&mgr, &session_key, "subagent", &target);
            }
        }
        if !task.is_empty() {
            mgr.add_message(&session_key, "user", &task)
                .map_err(|e| e.to_string())?;
        }
        let requester_channel = session_metadata_pick(
            &mgr.get_session_metadata(&requester_key).unwrap_or_default(),
            &["delivery.channel", "lastChannel", "last_channel", "channel"],
        );
        let run_id = match self
            .start_subagent_run_task(
                requester_key,
                requester_channel,
                session_key.clone(),
                label.clone(),
                task.clone(),
                model_hint.clone(),
                timeout_seconds,
                spawn_mode.clone(),
                cleanup.clone(),
                thread_requested,
                expects_completion_message,
                child_depth,
                max_spawn_depth,
            )
            .await
        {
            Ok(id) => id,
            Err(err) => {
                let _ = mgr.remove_session(&session_key);
                return Ok(serde_json::json!({
                    "action": "sessions_spawn",
                    "status": "error",
                    "session_key": session_key,
                    "error": err
                }));
            }
        };
        if let Some(hooks) = self.hook_pipeline.as_ref() {
            hooks.subagent_spawned(&session_key).await;
        }
        let _ = mgr.set_session_metadata_field(&session_key, "subagentRunId", &run_id);
        let session_id_value = result
            .get("session_id")
            .cloned()
            .unwrap_or_else(|| serde_json::json!(session_key.clone()));
        Ok(serde_json::json!({
            "action": "sessions_spawn",
            "status": "accepted",
            "run_id": run_id,
            "session_key": session_key,
            "session_id": session_id_value,
            "agent_id": created.agent_id,
            "label": label,
            "task": task,
            "model": model_hint,
            "model_applied": !model_hint.trim().is_empty(),
            "mode": spawn_mode,
            "cleanup": cleanup,
            "thread": thread_requested,
            "expectsCompletionMessage": expects_completion_message,
            "note": if spawn_mode == "session" {
                SUBAGENT_SPAWN_SESSION_ACCEPTED_NOTE
            } else {
                SUBAGENT_SPAWN_ACCEPTED_NOTE
            },
            "prompt_queued": true
        }))
    }

    async fn fulfill_subagents(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        self.resume_pending_subagent_cleanup_tasks().await;
        let action = result
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        let requester_session_key = self
            .session_id
            .clone()
            .unwrap_or_else(|| "agent:default:main".to_string());
        let recent_minutes = result
            .get("recent_minutes")
            .and_then(|v| v.as_u64())
            .or_else(|| result.get("recentMinutes").and_then(|v| v.as_u64()))
            .unwrap_or(30);
        let now_ms = chrono::Utc::now().timestamp_millis();
        let recent_cutoff_ms = now_ms.saturating_sub((recent_minutes as i64) * 60_000);
        let runs = self.list_runs_for_requester(&requester_session_key);

        let session_manager = self
            .session_manager
            .as_ref()
            .ok_or_else(|| "subagents: session manager unavailable".to_string())?;
        let mgr = session_manager.read().await;

        match action {
            "subagents_help" => Ok(serde_json::json!({
                "action": "subagents_help",
                "text": build_subagents_help_text()
            })),
            "subagents_agents" => {
                let mut lines = vec!["agents:".to_string(), "-----".to_string()];
                if runs.is_empty() {
                    lines.push("(none)".to_string());
                } else {
                    for (idx, run) in runs.iter().enumerate() {
                        let active_binding = self
                            .read_session_bindings(&mgr, &run.child_session_key)
                            .into_iter()
                            .filter(|b| b.status.eq_ignore_ascii_case("active"))
                            .max_by_key(|b| b.bound_at_ms);
                        let binding_text = if let Some(binding) = active_binding {
                            let conversation = binding.conversation.conversation_id.trim();
                            if !conversation.is_empty() {
                                format!("thread:{}", conversation)
                            } else {
                                "bound".to_string()
                            }
                        } else {
                            "unbound".to_string()
                        };
                        lines.push(format!("{}. {} ({})", idx + 1, run.label, binding_text));
                    }
                }
                let requester_acp_bindings: Vec<SessionBindingRecord> = self
                    .read_session_bindings(&mgr, &requester_session_key)
                    .into_iter()
                    .filter(|entry| {
                        entry.status.eq_ignore_ascii_case("active")
                            && !entry.target_kind.eq_ignore_ascii_case("subagent")
                    })
                    .collect();
                if !requester_acp_bindings.is_empty() {
                    lines.push(String::new());
                    lines.push("acp/session bindings:".to_string());
                    lines.push("-----".to_string());
                    for binding in &requester_acp_bindings {
                        let label = binding
                            .metadata
                            .as_ref()
                            .and_then(|meta| meta.get("label"))
                            .and_then(|v| v.as_str())
                            .map(str::trim)
                            .filter(|v| !v.is_empty())
                            .unwrap_or(binding.target_session_key.as_str());
                        lines.push(format!(
                            "- {} (thread:{}, session:{})",
                            label, binding.conversation.conversation_id, binding.target_session_key
                        ));
                    }
                }
                Ok(serde_json::json!({
                    "action": "subagents_agents",
                    "requester_session_key": requester_session_key,
                    "runs": runs.len(),
                    "text": lines.join("\n")
                }))
            }
            "subagents_list" => {
                let active: Vec<serde_json::Value> = runs
                    .iter()
                    .filter(|r| r.ended_at_ms.is_none())
                    .into_iter()
                    .enumerate()
                    .map(|(idx, r)| {
                        let usage = r
                            .usage
                            .as_ref()
                            .map(usage_summary_to_json)
                            .unwrap_or_else(|| serde_json::json!(null));
                        serde_json::json!({
                            "index": idx + 1,
                            "run_id": r.run_id,
                            "session_key": r.child_session_key,
                            "label": r.label,
                            "task": r.task,
                            "model": r.model,
                            "mode": r.spawn_mode,
                            "cleanup": r.cleanup,
                            "thread_requested": r.thread_requested,
                            "run_timeout_seconds": r.run_timeout_seconds,
                            "archive_at_ms": r.archive_at_ms,
                            "cleanup_handled": r.cleanup_handled,
                            "cleanup_completed_at_ms": r.cleanup_completed_at_ms,
                            "announce_retry_count": r.announce_retry_count,
                            "last_announce_retry_at_ms": r.last_announce_retry_at_ms,
                            "status": r.status,
                            "runtime_ms": now_ms.saturating_sub(r.started_at_ms),
                            "started_at_ms": r.started_at_ms,
                            "usage": usage
                        })
                    })
                    .collect();
                let recent: Vec<serde_json::Value> = runs
                    .iter()
                    .filter(|r| {
                        r.ended_at_ms
                            .map(|ended| ended >= recent_cutoff_ms)
                            .unwrap_or(false)
                    })
                    .enumerate()
                    .map(|(idx, r)| {
                        let usage = r
                            .usage
                            .as_ref()
                            .map(usage_summary_to_json)
                            .unwrap_or_else(|| serde_json::json!(null));
                        serde_json::json!({
                            "index": idx + 1,
                            "run_id": r.run_id,
                            "session_key": r.child_session_key,
                            "label": r.label,
                            "task": r.task,
                            "model": r.model,
                            "mode": r.spawn_mode,
                            "cleanup": r.cleanup,
                            "thread_requested": r.thread_requested,
                            "run_timeout_seconds": r.run_timeout_seconds,
                            "archive_at_ms": r.archive_at_ms,
                            "cleanup_handled": r.cleanup_handled,
                            "cleanup_completed_at_ms": r.cleanup_completed_at_ms,
                            "announce_retry_count": r.announce_retry_count,
                            "last_announce_retry_at_ms": r.last_announce_retry_at_ms,
                            "status": r.status,
                            "runtime_ms": r.ended_at_ms.unwrap_or(now_ms).saturating_sub(r.started_at_ms),
                            "started_at_ms": r.started_at_ms,
                            "ended_at_ms": r.ended_at_ms,
                            "usage": usage
                        })
                    })
                    .collect();
                let mut lines = vec!["active subagents:".to_string()];
                if active.is_empty() {
                    lines.push("(none)".to_string());
                } else {
                    for item in &active {
                        let line = format!(
                            "{}. {} ({}, {} ms) {}{}",
                            item.get("index").and_then(|v| v.as_u64()).unwrap_or(0),
                            item.get("label")
                                .and_then(|v| v.as_str())
                                .unwrap_or("subagent"),
                            item.get("model")
                                .and_then(|v| v.as_str())
                                .unwrap_or("model n/a"),
                            item.get("runtime_ms").and_then(|v| v.as_i64()).unwrap_or(0),
                            item.get("status")
                                .and_then(|v| v.as_str())
                                .unwrap_or("running"),
                            item.get("usage")
                                .and_then(|u| u.get("total_tokens"))
                                .and_then(|v| v.as_i64())
                                .map(|tokens| format!(", {} tok", tokens))
                                .unwrap_or_default()
                        );
                        lines.push(line);
                    }
                }
                lines.push(String::new());
                lines.push(format!("recent (last {}m):", recent_minutes));
                if recent.is_empty() {
                    lines.push("(none)".to_string());
                } else {
                    for item in &recent {
                        let line = format!(
                            "{}. {} ({}) {}{}",
                            item.get("index").and_then(|v| v.as_u64()).unwrap_or(0),
                            item.get("label")
                                .and_then(|v| v.as_str())
                                .unwrap_or("subagent"),
                            item.get("model")
                                .and_then(|v| v.as_str())
                                .unwrap_or("model n/a"),
                            item.get("status")
                                .and_then(|v| v.as_str())
                                .unwrap_or("done"),
                            item.get("usage")
                                .and_then(|u| u.get("total_tokens"))
                                .and_then(|v| v.as_i64())
                                .map(|tokens| format!(", {} tok", tokens))
                                .unwrap_or_default()
                        );
                        lines.push(line);
                    }
                }
                Ok(serde_json::json!({
                    "action": "subagents_list",
                    "requester_session_key": requester_session_key,
                    "total": runs.len(),
                    "active": active,
                    "recent": recent,
                    "text": lines.join("\n")
                }))
            }
            "subagents_info" => {
                let target = result
                    .get("target")
                    .and_then(|v| v.as_str())
                    .or_else(|| result.get("session_key").and_then(|v| v.as_str()))
                    .or_else(|| result.get("sessionKey").and_then(|v| v.as_str()))
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "subagents_info: missing target".to_string())?;
                let run = self.resolve_subagent_target(&runs, target, recent_minutes)?;
                let runtime_ms = run
                    .ended_at_ms
                    .unwrap_or(now_ms)
                    .saturating_sub(run.started_at_ms)
                    .max(0);
                let session = mgr
                    .get_session(&run.child_session_key)
                    .map_err(|e| e.to_string())?;
                let session_meta = mgr
                    .get_session_metadata(&run.child_session_key)
                    .unwrap_or_default();
                let outcome = match run.status.as_str() {
                    "failed" => {
                        if let Some(err) = run
                            .error
                            .as_deref()
                            .map(str::trim)
                            .filter(|v| !v.is_empty())
                        {
                            format!("error ({})", err)
                        } else {
                            "error".to_string()
                        }
                    }
                    "timeout" => "timeout".to_string(),
                    "killed" => "killed".to_string(),
                    "steered" => "steered".to_string(),
                    "done" => "ok".to_string(),
                    other => other.to_string(),
                };
                let lines = vec![
                    "subagent info".to_string(),
                    format!("Status: {}", run.status),
                    format!("Label: {}", run.label),
                    format!("Task: {}", run.task),
                    format!("Run: {}", run.run_id),
                    format!("Session: {}", run.child_session_key),
                    format!(
                        "SessionId: {}",
                        session
                            .as_ref()
                            .map(|s| s.key.as_str())
                            .filter(|v| !v.is_empty())
                            .unwrap_or("n/a")
                    ),
                    format!("Runtime: {}s", runtime_ms.saturating_div(1000)),
                    format!("StartedAtMs: {}", run.started_at_ms),
                    format!(
                        "EndedAtMs: {}",
                        run.ended_at_ms
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| "n/a".to_string())
                    ),
                    format!("Cleanup: {}", run.cleanup),
                    format!("Outcome: {}", outcome),
                    format!(
                        "Delivery: channel={} to={} thread={}",
                        session_meta
                            .get("delivery.channel")
                            .map(String::as_str)
                            .unwrap_or("n/a"),
                        session_meta
                            .get("delivery.to")
                            .map(String::as_str)
                            .unwrap_or("n/a"),
                        session_meta
                            .get("delivery.threadId")
                            .map(String::as_str)
                            .unwrap_or("n/a"),
                    ),
                ];
                Ok(serde_json::json!({
                    "action": "subagents_info",
                    "target": target,
                    "run": {
                        "run_id": run.run_id,
                        "session_key": run.child_session_key,
                        "status": run.status,
                        "label": run.label,
                        "task": run.task,
                        "runtime_ms": runtime_ms,
                        "started_at_ms": run.started_at_ms,
                        "ended_at_ms": run.ended_at_ms,
                        "cleanup": run.cleanup,
                        "model": run.model,
                        "thread_requested": run.thread_requested,
                        "spawn_mode": run.spawn_mode,
                        "run_timeout_seconds": run.run_timeout_seconds,
                        "usage": run.usage.as_ref().map(usage_summary_to_json)
                    },
                    "text": lines.join("\n")
                }))
            }
            "subagents_log" => {
                let target = result
                    .get("target")
                    .and_then(|v| v.as_str())
                    .or_else(|| result.get("session_key").and_then(|v| v.as_str()))
                    .or_else(|| result.get("sessionKey").and_then(|v| v.as_str()))
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "subagents_log: missing target".to_string())?;
                let run = self.resolve_subagent_target(&runs, target, recent_minutes)?;
                let limit = result
                    .get("limit")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(20)
                    .clamp(1, 200) as usize;
                let include_tools = result
                    .get("include_tools")
                    .and_then(|v| v.as_bool())
                    .or_else(|| result.get("includeTools").and_then(|v| v.as_bool()))
                    .unwrap_or(false);
                let mut messages = mgr
                    .get_messages(&run.child_session_key, limit)
                    .map_err(|e| e.to_string())?;
                messages.reverse();
                let lines: Vec<String> = messages
                    .into_iter()
                    .filter(|msg| {
                        if include_tools {
                            return true;
                        }
                        let role = msg.role.trim().to_ascii_lowercase();
                        role != "tool" && role != "toolresult"
                    })
                    .filter_map(|msg| {
                        let text = msg.content.trim();
                        if text.is_empty() {
                            return None;
                        }
                        let role = msg.role.trim().to_ascii_lowercase();
                        let label = if role == "assistant" {
                            "Assistant"
                        } else if role == "system" {
                            "System"
                        } else if role == "tool" || role == "toolresult" {
                            "Tool"
                        } else {
                            "User"
                        };
                        Some(format!("{}: {}", label, text))
                    })
                    .collect();
                let header = format!("subagent log: {}", run.label);
                let text = if lines.is_empty() {
                    format!("{}\n(no messages)", header)
                } else {
                    std::iter::once(header)
                        .chain(lines.clone())
                        .collect::<Vec<_>>()
                        .join("\n")
                };
                Ok(serde_json::json!({
                    "action": "subagents_log",
                    "target": target,
                    "session_key": run.child_session_key,
                    "limit": limit,
                    "include_tools": include_tools,
                    "lines": lines,
                    "text": text
                }))
            }
            "subagents_send" => {
                let target = result
                    .get("target")
                    .and_then(|v| v.as_str())
                    .or_else(|| result.get("session_key").and_then(|v| v.as_str()))
                    .or_else(|| result.get("sessionKey").and_then(|v| v.as_str()))
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "subagents_send: missing target".to_string())?;
                let message = result
                    .get("message")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "subagents_send: missing message".to_string())?;
                let run = self.resolve_subagent_target(&runs, target, recent_minutes)?;
                let provider = self
                    .llm_provider
                    .as_ref()
                    .ok_or_else(|| "subagents_send: llm provider unavailable".to_string())?
                    .clone();
                let reply = self
                    .run_agent_step(
                        provider,
                        &run.child_session_key,
                        message,
                        "Subagent direct message from orchestrator. Reply with actionable output for this subagent session only.",
                        30,
                        Some(&requester_session_key),
                        None,
                    )
                    .await?;
                let status = if reply.is_some() { "ok" } else { "timeout" };
                let reply_text = reply
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| format!("sent to {} (no reply yet)", run.label));
                Ok(serde_json::json!({
                    "action": "subagents_send",
                    "target": target,
                    "run_id": run.run_id,
                    "session_key": run.child_session_key,
                    "status": status,
                    "reply": reply,
                    "text": reply_text
                }))
            }
            "subagents_kill" => {
                let target = result
                    .get("target")
                    .and_then(|v| v.as_str())
                    .or_else(|| result.get("session_key").and_then(|v| v.as_str()))
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "subagents_kill: missing target".to_string())?;

                if target == "all" || target == "*" {
                    let mut to_kill: HashMap<String, SubagentRunEntry> = HashMap::new();
                    for run in runs.iter().filter(|r| r.ended_at_ms.is_none()) {
                        to_kill.insert(run.run_id.clone(), run.clone());
                        for desc in self.collect_descendant_runs(&run.child_session_key) {
                            if desc.ended_at_ms.is_none() {
                                to_kill.insert(desc.run_id.clone(), desc);
                            }
                        }
                    }

                    let mut killed = 0usize;
                    let mut labels: Vec<String> = Vec::new();
                    for run in to_kill.values() {
                        self.kill_run_entry(run).await;
                        let _ = mgr.set_session_metadata_field(
                            &run.child_session_key,
                            "terminated",
                            "true",
                        );
                        killed = killed.saturating_add(1);
                        labels.push(run.label.clone());
                    }
                    return Ok(serde_json::json!({
                        "action": "subagents_kill",
                        "target": "all",
                        "killed": killed,
                        "labels": labels,
                        "text": if killed == 0 {
                            "no running subagents to kill.".to_string()
                        } else {
                            format!("killed {} subagent(s).", killed)
                        }
                    }));
                }

                let run = self.resolve_subagent_target(&runs, target, recent_minutes)?;
                if run.ended_at_ms.is_some() {
                    return Ok(serde_json::json!({
                        "action": "subagents_kill",
                        "target": target,
                        "run_id": run.run_id,
                        "session_key": run.child_session_key,
                        "status": "done",
                        "text": format!("{} is already finished.", run.label)
                    }));
                }
                let mut to_kill: HashMap<String, SubagentRunEntry> = HashMap::new();
                to_kill.insert(run.run_id.clone(), run.clone());
                for desc in self.collect_descendant_runs(&run.child_session_key) {
                    if desc.ended_at_ms.is_none() {
                        to_kill.insert(desc.run_id.clone(), desc);
                    }
                }
                let cascade_killed = to_kill.len().saturating_sub(1);
                for item in to_kill.values() {
                    self.kill_run_entry(item).await;
                    let _ = mgr.set_session_metadata_field(
                        &item.child_session_key,
                        "terminated",
                        "true",
                    );
                }
                let _ =
                    mgr.set_session_metadata_field(&run.child_session_key, "terminated", "true");
                Ok(serde_json::json!({
                    "action": "subagents_kill",
                    "target": target,
                    "run_id": run.run_id,
                    "session_key": run.child_session_key,
                    "label": run.label,
                    "status": "ok",
                    "cascade_killed": cascade_killed,
                    "text": if cascade_killed > 0 {
                        format!("killed (+{} descendant).", cascade_killed)
                    } else {
                        "killed".to_string()
                    }
                }))
            }
            "subagents_steer" => {
                let target = result
                    .get("target")
                    .and_then(|v| v.as_str())
                    .or_else(|| result.get("session_key").and_then(|v| v.as_str()))
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "subagents_steer: missing target".to_string())?;
                let message = result
                    .get("message")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "subagents_steer: missing message".to_string())?;

                let run = self.resolve_subagent_target(&runs, target, recent_minutes)?;
                if run.ended_at_ms.is_some() {
                    return Ok(serde_json::json!({
                        "action": "subagents_steer",
                        "target": target,
                        "run_id": run.run_id,
                        "session_key": run.child_session_key,
                        "status": "done",
                        "text": format!("{} is already finished.", run.label)
                    }));
                }
                if self
                    .session_id
                    .as_deref()
                    .map(|sid| sid == run.child_session_key)
                    .unwrap_or(false)
                {
                    return Ok(serde_json::json!({
                        "action": "subagents_steer",
                        "target": target,
                        "run_id": run.run_id,
                        "session_key": run.child_session_key,
                        "status": "forbidden",
                        "error": "subagents cannot steer themselves"
                    }));
                }
                if let Some(handle) = SUBAGENT_ABORTS
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&run.run_id)
                {
                    handle.abort();
                }
                let steer_now_ms = chrono::Utc::now().timestamp_millis();
                self.patch_run(&run.run_id, |entry| {
                    entry.status = "steered".to_string();
                    entry.ended_at_ms = Some(steer_now_ms);
                    entry.error = Some("interrupted for steer restart".to_string());
                    entry.suppress_announce_reason = Some("steer-restart".to_string());
                    entry.cleanup_handled = true;
                    entry.cleanup_completed_at_ms = Some(steer_now_ms);
                    entry.ended_reason = Some("subagent-killed".to_string());
                    if entry.archive_at_ms.is_none() {
                        entry.archive_at_ms = Some(steer_now_ms.saturating_add(60 * 60_000));
                    }
                });
                mgr.add_message(&run.child_session_key, "user", message)
                    .map_err(|e| e.to_string())?;
                let next_run_id = self
                    .start_subagent_run_task(
                        requester_session_key.clone(),
                        session_metadata_pick(
                            &mgr.get_session_metadata(&requester_session_key)
                                .unwrap_or_default(),
                            &["delivery.channel", "lastChannel", "last_channel", "channel"],
                        ),
                        run.child_session_key.clone(),
                        run.label.clone(),
                        message.to_string(),
                        run.model.clone(),
                        run.run_timeout_seconds,
                        run.spawn_mode.clone(),
                        run.cleanup.clone(),
                        run.thread_requested,
                        run.expects_completion_message,
                        Self::parse_spawn_depth_meta(
                            &mgr.get_session_metadata(&run.child_session_key)
                                .unwrap_or_default(),
                        )
                        .unwrap_or(1),
                        self.resolve_subagent_limits().await.0,
                    )
                    .await?;
                if next_run_id != run.run_id {
                    let mut guard = SUBAGENT_RUNS.lock().unwrap_or_else(|e| e.into_inner());
                    let _ = guard.remove(&run.run_id);
                    persist_subagent_registry_to_disk(&guard);
                }
                Ok(serde_json::json!({
                    "action": "subagents_steer",
                    "target": target,
                    "run_id": next_run_id,
                    "session_key": run.child_session_key,
                    "label": run.label,
                    "status": "accepted",
                    "mode": "restart",
                    "text": "steered"
                }))
            }
            "subagents_spawn" => {
                let mut spawn_payload = result.clone();
                if let Some(map) = spawn_payload.as_object_mut() {
                    map.insert(
                        "action".to_string(),
                        serde_json::Value::String("sessions_spawn".to_string()),
                    );
                }
                self.fulfill_sessions_spawn(spawn_payload).await
            }
            "subagents_focus" => {
                let target = result
                    .get("target")
                    .and_then(|v| v.as_str())
                    .or_else(|| result.get("session_key").and_then(|v| v.as_str()))
                    .or_else(|| result.get("sessionKey").and_then(|v| v.as_str()))
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| "subagents_focus: missing target".to_string())?;
                let target_run = self
                    .resolve_subagent_target(&runs, target, recent_minutes)
                    .ok();
                let (target_session_key, default_target_kind) = if let Some(run) = target_run {
                    (run.child_session_key, "subagent".to_string())
                } else if mgr
                    .get_session(target)
                    .map_err(|e| e.to_string())?
                    .is_some()
                {
                    (
                        target.to_string(),
                        if target.contains(":subagent:") {
                            "subagent".to_string()
                        } else {
                            "acp".to_string()
                        },
                    )
                } else {
                    let sessions = mgr.list_sessions().map_err(|e| e.to_string())?;
                    let mut label_matches: Vec<String> = sessions
                        .into_iter()
                        .filter_map(|row| {
                            let meta = mgr.get_session_metadata(&row.key).ok()?;
                            let label = meta.get("label")?.trim().to_string();
                            if label.eq_ignore_ascii_case(target) {
                                Some(row.key)
                            } else {
                                None
                            }
                        })
                        .collect();
                    label_matches.sort();
                    label_matches.dedup();
                    if label_matches.is_empty() {
                        return Err(format!(
                            "subagents_focus: unable to resolve target {}",
                            target
                        ));
                    }
                    if label_matches.len() > 1 {
                        return Err(format!("subagents_focus: ambiguous target {}", target));
                    }
                    let key = label_matches.remove(0);
                    (
                        key.clone(),
                        if key.contains(":subagent:") {
                            "subagent".to_string()
                        } else {
                            "acp".to_string()
                        },
                    )
                };
                let mut channel = result
                    .get("channel")
                    .and_then(|v| v.as_str())
                    .map(normalize_channel_alias)
                    .filter(|v| !v.trim().is_empty());
                let mut to = result
                    .get("to")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(ToString::to_string);
                let mut account_id = result
                    .get("account_id")
                    .and_then(|v| v.as_str())
                    .or_else(|| result.get("accountId").and_then(|v| v.as_str()))
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(ToString::to_string);
                let mut thread_id = result
                    .get("thread_id")
                    .and_then(|v| v.as_str())
                    .or_else(|| result.get("threadId").and_then(|v| v.as_str()))
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(ToString::to_string);
                let target_kind = result
                    .get("target_kind")
                    .and_then(|v| v.as_str())
                    .or_else(|| result.get("targetKind").and_then(|v| v.as_str()))
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(|v| {
                        if v.eq_ignore_ascii_case("acp") {
                            "acp".to_string()
                        } else {
                            "subagent".to_string()
                        }
                    })
                    .unwrap_or(default_target_kind);
                if (channel.is_none() || to.is_none())
                    && let Some(fallback) = self.resolve_announce_target(
                        &mgr,
                        &requester_session_key,
                        &requester_session_key,
                    )
                {
                    if channel.is_none() {
                        channel = Some(fallback.channel);
                    }
                    if to.is_none() {
                        to = Some(fallback.to);
                    }
                    if account_id.is_none() {
                        account_id = fallback.account_id;
                    }
                    if thread_id.is_none() {
                        thread_id = fallback.thread_id;
                    }
                }
                if to.is_none()
                    && let Some(thread) = thread_id
                        .as_deref()
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                {
                    to = Some(format!("channel:{}", thread));
                }
                let final_channel = channel
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| "subagents_focus: missing channel".to_string())?;
                let final_to = to
                    .map(|v| v.trim().to_string())
                    .filter(|v| !v.is_empty())
                    .ok_or_else(|| "subagents_focus: missing to/threadId".to_string())?;
                let announce_target = AnnounceTarget {
                    channel: final_channel.clone(),
                    to: final_to.clone(),
                    account_id: account_id.clone(),
                    thread_id: thread_id.clone(),
                };
                let _ = mgr.set_session_metadata_field(
                    &target_session_key,
                    "delivery.channel",
                    &final_channel,
                );
                let _ =
                    mgr.set_session_metadata_field(&target_session_key, "delivery.to", &final_to);
                if let Some(account) = account_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                {
                    let _ = mgr.set_session_metadata_field(
                        &target_session_key,
                        "delivery.accountId",
                        account,
                    );
                }
                if let Some(thread) = thread_id
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                {
                    let _ = mgr.set_session_metadata_field(
                        &target_session_key,
                        "delivery.threadId",
                        thread,
                    );
                }
                let binding = self.upsert_session_binding(
                    &mgr,
                    &target_session_key,
                    &target_kind,
                    &announce_target,
                );
                let binding_json = binding
                    .map(|item| {
                        serde_json::json!({
                            "binding_id": item.binding_id,
                            "status": item.status,
                            "bound_at_ms": item.bound_at_ms
                        })
                    })
                    .unwrap_or_else(|| serde_json::json!(null));
                Ok(serde_json::json!({
                    "action": "subagents_focus",
                    "status": "accepted",
                    "target": target,
                    "target_kind": target_kind,
                    "target_session_key": target_session_key,
                    "delivery": {
                        "channel": final_channel,
                        "to": final_to,
                        "account_id": account_id,
                        "thread_id": thread_id
                    },
                    "binding": binding_json,
                    "text": "focus binding updated"
                }))
            }
            "subagents_unfocus" => {
                let target = result
                    .get("target")
                    .and_then(|v| v.as_str())
                    .or_else(|| result.get("session_key").and_then(|v| v.as_str()))
                    .or_else(|| result.get("sessionKey").and_then(|v| v.as_str()))
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| requester_session_key.clone());
                let binding_id = result
                    .get("binding_id")
                    .and_then(|v| v.as_str())
                    .or_else(|| result.get("bindingId").and_then(|v| v.as_str()))
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(ToString::to_string);
                let mut bindings = self.read_session_bindings(&mgr, &target);
                let mut changed = 0usize;
                for entry in &mut bindings {
                    if !entry.status.eq_ignore_ascii_case("active") {
                        continue;
                    }
                    if let Some(ref id) = binding_id
                        && entry.binding_id != *id
                    {
                        continue;
                    }
                    entry.status = "inactive".to_string();
                    changed = changed.saturating_add(1);
                }
                if changed > 0 {
                    self.write_session_bindings(&mgr, &target, &bindings);
                }
                Ok(serde_json::json!({
                    "action": "subagents_unfocus",
                    "target_session_key": target,
                    "binding_id": binding_id,
                    "changed": changed,
                    "status": if changed > 0 { "ok" } else { "noop" },
                    "text": if changed > 0 {
                        format!("unfocused {} binding(s).", changed)
                    } else {
                        "no active bindings matched.".to_string()
                    }
                }))
            }
            _ => Ok(result),
        }
    }

    async fn fulfill_session_status(
        &self,
        result: serde_json::Value,
    ) -> Result<serde_json::Value, String> {
        let key = resolve_session_key(
            result
                .get("session_key")
                .and_then(|v| v.as_str())
                .or_else(|| result.get("sessionKey").and_then(|v| v.as_str())),
            self,
        )
        .ok_or_else(|| "session_status: missing session_key/sessionKey".to_string())?;
        let session_manager = self
            .session_manager
            .as_ref()
            .ok_or_else(|| "session_status: session manager unavailable".to_string())?;
        let mgr = session_manager.read().await;
        let access_ctx = self.build_session_access_context(&mgr).await?;
        if let Err(err) = self.check_session_access(&access_ctx, &key, SessionAccessAction::Status)
        {
            return Ok(serde_json::json!({
                "action": "session_status",
                "session_key": key,
                "status": "forbidden",
                "error": err
            }));
        }
        let session = mgr
            .get_session(&key)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("session_status: session not found: {}", key))?;
        let mut metadata = mgr.get_session_metadata(&key).map_err(|e| e.to_string())?;
        if let Some(model_override) = result
            .get("model")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if model_override.eq_ignore_ascii_case("default") {
                let _ = mgr.set_session_metadata_field(&key, "modelOverride", "");
                metadata.insert("modelOverride".to_string(), String::new());
            } else {
                let _ = mgr.set_session_metadata_field(&key, "modelOverride", model_override);
                metadata.insert("modelOverride".to_string(), model_override.to_string());
            }
        }
        let now_ms = chrono::Utc::now().timestamp_millis();
        let idle_ms = now_ms.saturating_sub(session.updated_at);
        let age_ms = now_ms.saturating_sub(session.created_at);
        let active_subagents = self
            .list_runs_for_requester(&key)
            .into_iter()
            .filter(|r| r.ended_at_ms.is_none())
            .count();
        let model_display = metadata
            .get("modelOverride")
            .cloned()
            .filter(|s| !s.trim().is_empty())
            .or_else(|| {
                self.llm_provider
                    .as_ref()
                    .map(|p| p.default_model().to_string())
            })
            .unwrap_or_else(|| "default".to_string());
        let usage_meta = usage_summary_from_metadata(&metadata);
        let runtime_tokens = self
            .session_usage_tokens
            .as_ref()
            .and_then(|m| {
                m.lock()
                    .ok()
                    .and_then(|guard| guard.get(&key).copied())
                    .map(|v| v as i64)
            })
            .unwrap_or(0);
        let session_total_tokens = usage_total_tokens(&usage_meta).saturating_add(runtime_tokens);
        let gateway_usage = if let Some(snapshot) = self.usage_snapshot.as_ref() {
            let guard = snapshot.read().await;
            Some(serde_json::json!({
                "updated_at": guard.updated_at,
                "totals": guard.totals
            }))
        } else {
            None
        };
        let status_text = format!(
            "Session: {}\nModel: {}\nStatus: {}\nMessages: {}\nActive subagents: {}\nUsage: {} tok (input {} / output {}, cache r{} w{}, calls {}, cost ${:.6})\nAge: {}s\nIdle: {}s",
            key,
            model_display,
            metadata
                .get("terminated")
                .map(|v| if v == "true" { "terminated" } else { "active" })
                .unwrap_or("active"),
            session.message_count,
            active_subagents,
            session_total_tokens,
            usage_meta.input_tokens,
            usage_meta.output_tokens,
            usage_meta.cache_read_tokens,
            usage_meta.cache_write_tokens,
            usage_meta.total_calls,
            usage_meta.total_cost_usd,
            age_ms / 1000,
            idle_ms / 1000
        );
        Ok(serde_json::json!({
            "action": "session_status",
            "session_key": key,
            "status": metadata.get("terminated").map(|v| if v == "true" { "terminated" } else { "active" }).unwrap_or("active"),
            "agent_id": session.agent_id,
            "message_count": session.message_count,
            "created_at": session.created_at,
            "updated_at": session.updated_at,
            "model": model_display,
            "active_subagents": active_subagents,
            "age_ms": age_ms,
            "idle_ms": idle_ms,
            "usage": {
                "session": usage_summary_to_json(&usage_meta),
                "session_runtime_tokens": runtime_tokens,
                "session_total_tokens": session_total_tokens,
                "gateway": gateway_usage
            },
            "text": status_text,
            "metadata": metadata
        }))
    }
}

#[async_trait::async_trait]
impl ToolExecutor for ToolRegistryExecutor {
    async fn execute(&self, name: &str, arguments: &str) -> Result<String, String> {
        let normalized_name = Self::normalize_tool_call_name(name);
        // Try built-in registry first
        if self.registry.has_tool(normalized_name) {
            let args: serde_json::Value = serde_json::from_str(arguments)
                .unwrap_or(serde_json::Value::Object(Default::default()));
            let call = oclaw_tools_core::tool::ToolCall {
                id: uuid::Uuid::new_v4().to_string(),
                name: normalized_name.to_string(),
                arguments: args,
            };
            let resp = self.registry.execute_call(call).await;
            return if let Some(err) = resp.error {
                Err(err)
            } else {
                let fulfilled = self
                    .fulfill_runtime_intent(normalized_name, resp.result)
                    .await?;
                Ok(serde_json::to_string(&fulfilled).unwrap_or_default())
            };
        }

        // Fall back to plugin tools
        if let Some(regs) = &self.plugin_regs {
            let tools = regs.tools.read().await;
            if let Some(tool) = tools.iter().find(|t| t.name == name) {
                let params: serde_json::Value = serde_json::from_str(arguments)
                    .unwrap_or(serde_json::Value::Object(Default::default()));
                return match tool.executor.execute(params).await {
                    Ok(val) => Ok(serde_json::to_string(&val).unwrap_or_default()),
                    Err(e) => Err(e),
                };
            }
        }

        Err(format!("Tool '{}' not found", name))
    }

    fn available_tools(&self) -> Vec<Tool> {
        let mut tools: Vec<Tool> = self
            .registry
            .list_for_llm()
            .into_iter()
            .filter_map(|v| {
                Some(Tool {
                    type_: "function".into(),
                    function: ToolFunction {
                        name: v["name"].as_str()?.to_string(),
                        description: v["description"].as_str()?.to_string(),
                        parameters: v["parameters"].clone(),
                    },
                })
            })
            .collect();

        // Merge plugin tools (blocking read via try_read to avoid async in sync fn)
        if let Some(regs) = &self.plugin_regs
            && let Ok(plugin_tools) = regs.tools.try_read()
        {
            for pt in plugin_tools.iter() {
                tools.push(Tool {
                    type_: "function".into(),
                    function: ToolFunction {
                        name: pt.name.clone(),
                        description: pt.description.clone(),
                        parameters: pt.input_schema.clone(),
                    },
                });
            }
        }

        tools
    }
}

fn build_runtime_line(tool_executor: &ToolRegistryExecutor) -> String {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let cwd = std::env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".to_string());
    let mut parts = Vec::new();
    if let Some(session_id) = tool_executor
        .session_id
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        parts.push(format!("session={}", session_id));
    }
    parts.push(format!("os={} ({})", os, arch));
    parts.push(format!("cwd={}", cwd));
    parts.push("thinking=off".to_string());
    format!("Runtime: {}", parts.join(" | "))
}

fn agent_system_prompt(tool_executor: &ToolRegistryExecutor, is_minimal: bool) -> String {
    let tool_names: Vec<String> = tool_executor
        .available_tools()
        .iter()
        .map(|t| t.function.name.clone())
        .collect();
    let tool_lines: Vec<String> = tool_names
        .iter()
        .map(|name| format!("- {}", name))
        .collect();
    let has_tool = |name: &str| tool_names.iter().any(|tool| tool == name);

    let mut lines = vec![
        "You are a personal assistant running inside OpenClaw.".to_string(),
        "".to_string(),
        "## Tooling".to_string(),
        "Tool availability (filtered by policy):".to_string(),
        "Tool names are case-sensitive. Call tools exactly as listed.".to_string(),
    ];
    lines.extend(tool_lines);
    lines.extend([
        "TOOLS.md does not control tool availability; it is user guidance for external tools."
            .to_string(),
        "For long waits, avoid rapid poll loops. Use long-yield exec/process polling instead."
            .to_string(),
        "If a task is complex or long-running, spawn a sub-agent; completion is push-based."
            .to_string(),
        "".to_string(),
        "## Tool Call Style".to_string(),
        "Default: do not narrate routine, low-risk tool calls.".to_string(),
        "Narrate only when it helps: multi-step work, complex work, sensitive actions, or explicit user request.".to_string(),
        "Keep narration brief and value-dense; avoid repeating obvious steps.".to_string(),
        "When a first-class tool exists, call the tool instead of asking the user to run equivalent CLI commands.".to_string(),
        "".to_string(),
        "## Safety".to_string(),
        "You have no independent goals: do not pursue self-preservation, replication, resource acquisition, or power-seeking.".to_string(),
        "Prioritize safety and human oversight over completion; if instructions conflict, pause and ask.".to_string(),
        "Do not manipulate users to expand access or disable safeguards.".to_string(),
        "".to_string(),
        "Never output provider-specific tool markup like `<minimax:tool_call>`/`<invoke>` to users."
            .to_string(),
    ]);

    if has_tool("web_search") || has_tool("web_fetch") || has_tool("browse") {
        lines.push("## Internet Access".to_string());
        lines.push(
            "You CAN access the internet. Do not claim browsing/network is unavailable when tools exist."
                .to_string(),
        );
        if has_tool("web_search") {
            lines.push("- Use `web_search` for real-time prices/news/facts lookup.".to_string());
        }
        if has_tool("web_fetch") {
            lines.push("- Use `web_fetch` for direct pages/APIs/simple HTML.".to_string());
        }
        if has_tool("browse") {
            lines.push(
                "- Use `browse` for JS-heavy pages that static fetch cannot read.".to_string(),
            );
        }
        lines.push(
            "- When users ask for latest/live information, run tools first, then answer with findings."
                .to_string(),
        );
        if has_tool("browse") {
            lines.push(
                "- If the user asks you to open a website in a browser, call `browse` first (example: {\"action\":\"navigate\",\"url\":\"https://bing.com\"}) instead of saying you cannot open a browser."
                    .to_string(),
            );
        }
        lines.push("".to_string());
    }

    if !is_minimal && (has_tool("memory_search") || has_tool("memory_get")) {
        lines.push("## Memory Recall".to_string());
        lines.push(
            "Before claiming you don't remember prior context, run memory recall tools for prior work/decisions/preferences/dates."
                .to_string(),
        );
        if has_tool("memory_search") {
            lines.push("- Start with `memory_search` to find relevant memories.".to_string());
        }
        if has_tool("memory_get") {
            lines.push("- Then use `memory_get` for the exact needed entries.".to_string());
        }
        lines.push("".to_string());
    }

    if !is_minimal {
        lines.push("## OpenClaw CLI Quick Reference".to_string());
        lines.push("Do not invent commands. Typical daemon controls:".to_string());
        lines.push("- openclaw gateway status".to_string());
        lines.push("- openclaw gateway start".to_string());
        lines.push("- openclaw gateway stop".to_string());
        lines.push("- openclaw gateway restart".to_string());
        lines.push("".to_string());

        lines.push("## Skills (mandatory)".to_string());
        lines.push(
            "Before replying, scan available skills descriptions. If exactly one clearly applies, read its SKILL.md and follow it."
                .to_string(),
        );
        lines.push(
            "If none clearly apply, proceed normally. Do not read multiple skills up front."
                .to_string(),
        );
        lines.push("".to_string());

        lines.push("## Reply Tags".to_string());
        lines.push(
            "To request native threaded reply on supported channels, place tag as the first token: [[reply_to_current]]."
                .to_string(),
        );
        lines.push(
            "Prefer [[reply_to_current]]. Use [[reply_to:<id>]] only when explicit id is provided."
                .to_string(),
        );
        lines.push("".to_string());
    }

    if !is_minimal && (has_tool("message") || has_tool("sessions_send") || has_tool("subagents")) {
        lines.push("## Messaging".to_string());
        lines.push(
            "- Reply in current session -> automatically routes to source channel.".to_string(),
        );
        if has_tool("sessions_send") {
            lines.push(
                "- Cross-session messaging -> use sessions_send(sessionKey, message).".to_string(),
            );
        }
        if has_tool("subagents") {
            lines.push(
                "- Sub-agent orchestration -> use subagents(action=list|steer|kill|spawn|focus|unfocus)."
                    .to_string(),
            );
        }
        lines.push(
            "- Never use exec/curl for provider messaging; OCLAW handles routing internally."
                .to_string(),
        );
        lines.push("".to_string());
    }

    if !is_minimal && has_tool("message") {
        lines.push("### message tool".to_string());
        lines.push(
            "- Use `message` for proactive sends + channel actions (polls, reactions, etc.)."
                .to_string(),
        );
        lines.push("- For `action=send`, include `to` and `message`.".to_string());
        lines.push("- If multiple channels are configured, pass `channel` explicitly.".to_string());
        lines.push(
            "- If you already delivered the user-visible reply via `message` (`action=send`), respond with ONLY: [[SILENT]]."
                .to_string(),
        );
        lines.push(
            "- When users ask to proactively contact someone (Feishu/Telegram/Slack/etc), use `message` instead of claiming integration is unavailable."
                .to_string(),
        );
        lines.push(
            "- If sending fails, report the exact tool error and ask for missing routing fields."
                .to_string(),
        );
        lines.push("".to_string());
    }

    if !is_minimal {
        lines.push("## Documentation".to_string());
        lines.push("OpenClaw docs: https://docs.openclaw.ai".to_string());
        lines.push("Source: https://github.com/openclaw/openclaw".to_string());
        lines.push("".to_string());
    }

    lines.push("## Workspace".to_string());
    lines.push(
        "Treat the current working directory as the primary workspace for read/write/edit/apply_patch."
            .to_string(),
    );
    lines.push("".to_string());

    if !is_minimal {
        lines.push("## Silent Replies".to_string());
        lines.push("When you have nothing to say, respond with ONLY: [[SILENT]]".to_string());
        lines.push("Never append [[SILENT]] to normal replies.".to_string());
        lines.push("".to_string());

        lines.push("## Heartbeats".to_string());
        lines.push(
            "If you receive a heartbeat poll and there is nothing that needs attention, reply exactly: HEARTBEAT_OK"
                .to_string(),
        );
        lines.push(
            "If something needs attention, do NOT include HEARTBEAT_OK; reply with the actual alert."
                .to_string(),
        );
        lines.push("".to_string());
    }

    lines.push("## Runtime".to_string());
    lines.push(build_runtime_line(tool_executor));
    lines
        .push("Reasoning: off (hidden unless explicitly enabled by runtime controls).".to_string());

    lines.join("\n")
}

fn default_agent_system_prompt(tool_executor: &ToolRegistryExecutor) -> String {
    agent_system_prompt(tool_executor, false)
}

fn build_subagent_system_prompt(
    tool_executor: &ToolRegistryExecutor,
    requester_session_key: &str,
    requester_channel: Option<&str>,
    child_session_key: &str,
    label: Option<&str>,
    task: &str,
    child_depth: u32,
    max_spawn_depth: u32,
) -> String {
    let mut prompt = agent_system_prompt(tool_executor, true);
    let task_text = task
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    let task_text = if task_text.is_empty() {
        "{{TASK_DESCRIPTION}}".to_string()
    } else {
        task_text
    };
    let parent_label = if child_depth >= 2 {
        "parent orchestrator"
    } else {
        "main agent"
    };
    let can_spawn = child_depth < max_spawn_depth.max(1);
    let mut lines: Vec<String> = vec![
        "# Subagent Context".to_string(),
        "".to_string(),
        format!(
            "You are a subagent spawned by the {} for a specific task.",
            parent_label
        ),
        "".to_string(),
        "## Your Role".to_string(),
        format!("- You were created to handle: {}", task_text),
        "- Complete this task. That's your entire purpose.".to_string(),
        format!("- You are NOT the {}. Don't try to be.", parent_label),
        "".to_string(),
        "## Rules".to_string(),
        "1. Stay focused - Do your assigned task, nothing else.".to_string(),
        format!(
            "2. Complete the task - Your final message will be automatically reported to the {}.",
            parent_label
        ),
        "3. Don't initiate - No heartbeats, no proactive actions, no side quests.".to_string(),
        "4. Be ephemeral - You may be terminated after task completion. That's fine.".to_string(),
        "5. Trust push-based completion - Descendant results are auto-announced back to you; do not busy-poll for status.".to_string(),
        "6. Recover from compacted/truncated tool output - If you see compacted or truncated markers, re-read only what you need using smaller chunks (read offset/limit or targeted rg/head/tail), not full-file dumps.".to_string(),
        "".to_string(),
        "## Output Format".to_string(),
        "When complete, your final response should include:".to_string(),
        "- What you accomplished or found.".to_string(),
        format!("- Any relevant details the {} should know.", parent_label),
        "- Keep it concise but informative.".to_string(),
        "".to_string(),
        "## What You DON'T Do".to_string(),
        format!("- NO user conversations (that's {}'s job).", parent_label),
        "- NO external messages unless explicitly tasked with a specific recipient/channel."
            .to_string(),
        "- NO cron jobs or persistent state.".to_string(),
        format!("- NO pretending to be the {}.", parent_label),
        "- Only use the message tool when explicitly instructed to contact a specific external recipient; otherwise return plain text and let the orchestrator deliver it.".to_string(),
        "".to_string(),
    ];
    if can_spawn {
        lines.extend([
            "## Sub-Agent Spawning".to_string(),
            "You CAN spawn your own sub-agents for parallel or complex work using sessions_spawn."
                .to_string(),
            "Use the subagents tool to steer, kill, or do an on-demand status check for your spawned sub-agents.".to_string(),
            "Your sub-agents will announce their results back to you automatically (not to the main agent).".to_string(),
            "Default workflow: spawn work, continue orchestrating, and wait for auto-announced completions.".to_string(),
            "Do NOT repeatedly poll subagents list in a loop unless you are actively debugging or intervening.".to_string(),
            "Coordinate child work and synthesize results before reporting back.".to_string(),
            "".to_string(),
        ]);
    } else if child_depth >= 2 {
        lines.extend([
            "## Sub-Agent Spawning".to_string(),
            "You are a leaf worker and CANNOT spawn further sub-agents. Focus on your assigned task."
                .to_string(),
            "".to_string(),
        ]);
    }
    lines.push("## Session Context".to_string());
    if let Some(trimmed) = label.map(str::trim).filter(|v| !v.is_empty()) {
        lines.push(format!("- Label: {}.", trimmed));
    }
    if !requester_session_key.trim().is_empty() {
        lines.push(format!(
            "- Requester session: {}.",
            requester_session_key.trim()
        ));
    }
    if let Some(channel) = requester_channel.map(str::trim).filter(|v| !v.is_empty()) {
        lines.push(format!("- Requester channel: {}.", channel));
    }
    lines.push(format!("- Your session: {}.", child_session_key.trim()));
    lines.push(String::new());

    prompt.push_str("\n\n");
    prompt.push_str(&lines.join("\n"));
    prompt
}

fn session_metadata_pick(map: &HashMap<String, String>, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(v) = map.get(*key) {
            let trimmed = v.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn resolve_announce_target_from_key(session_key: &str) -> Option<AnnounceTarget> {
    let mut parts: Vec<&str> = session_key
        .split(':')
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() >= 3 && parts.first().copied() == Some("agent") {
        parts = parts.into_iter().skip(2).collect();
    }
    if parts.len() < 3 {
        return None;
    }
    let channel_raw = parts[0].trim();
    let kind = parts[1].trim().to_ascii_lowercase();
    if kind != "group" && kind != "channel" {
        return None;
    }
    let mut rest = parts[2..].join(":");
    if rest.trim().is_empty() {
        return None;
    }
    let mut thread_id = None;
    if let Ok(re) = regex::Regex::new("(?i):(topic|thread):(\\d+)$")
        && let Some(caps) = re.captures(&rest)
        && let Some(m) = caps.get(2)
    {
        thread_id = Some(m.as_str().to_string());
        rest = re.replace(&rest, "").to_string();
    }
    let id = rest.trim();
    if id.is_empty() {
        return None;
    }
    let channel = normalize_channel_alias(channel_raw);
    let to = if channel == "discord" || channel == "slack" {
        format!("channel:{}", id)
    } else if kind == "channel" {
        format!("channel:{}", id)
    } else {
        format!("group:{}", id)
    };
    Some(AnnounceTarget {
        channel,
        to,
        account_id: None,
        thread_id,
    })
}

fn build_subagent_completion_announce_text(
    label: &str,
    spawn_mode: &str,
    status: &str,
    error: Option<&str>,
    reply: Option<&str>,
    usage: Option<&UsageSummary>,
    runtime_ms: i64,
) -> String {
    let subagent_name = label.trim();
    let name = if subagent_name.is_empty() {
        "subagent"
    } else {
        subagent_name
    };
    let persistent = spawn_mode.eq_ignore_ascii_case("session");
    let header = match status {
        "failed" => {
            if persistent {
                format!(
                    "Subagent {} failed this task (session remains active).",
                    name
                )
            } else {
                format!("Subagent {} failed.", name)
            }
        }
        "timeout" => {
            if persistent {
                format!(
                    "Subagent {} timed out on this task (session remains active).",
                    name
                )
            } else {
                format!("Subagent {} timed out.", name)
            }
        }
        "killed" => format!("Subagent {} was killed.", name),
        _ => {
            if persistent {
                format!(
                    "Subagent {} completed this task (session remains active).",
                    name
                )
            } else {
                format!("Subagent {} finished.", name)
            }
        }
    };

    let mut lines = vec![header];
    if runtime_ms > 0 {
        lines.push(format!("Runtime: {}s.", runtime_ms.saturating_div(1000)));
    }
    if let Some(summary) = usage {
        lines.push(format!(
            "Usage: {} tokens, {} calls.",
            usage_total_tokens(summary),
            summary.total_calls
        ));
    }
    let details = reply
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            error
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(|v| format!("Error: {}", v))
        });
    if let Some(content) = details {
        lines.push(String::new());
        lines.push(content);
    }
    lines.join("\n")
}

fn build_subagent_announce_reply_instruction(
    remaining_active_subagent_runs: usize,
    requester_is_subagent: bool,
    expects_completion_message: bool,
) -> String {
    if remaining_active_subagent_runs > 0 {
        let run_label = if remaining_active_subagent_runs == 1 {
            "run"
        } else {
            "runs"
        };
        return format!(
            "There are still {} active subagent {} for this session. If they are part of the same workflow, wait for the remaining results before sending a user update. If they are unrelated, respond normally using only the result above.",
            remaining_active_subagent_runs, run_label
        );
    }
    if requester_is_subagent {
        return format!(
            "Convert this completion into a concise internal orchestration update for your parent agent in your own words. Keep this internal context private (don't mention system/log/stats/session details or announce type). If this result is duplicate or no update is needed, reply ONLY: {}.",
            REPLY_SKIP_TOKEN
        );
    }
    if expects_completion_message {
        return "A completed subagent task is ready for user delivery. Convert the result above into your normal assistant voice and send that user-facing update now. Keep this internal context private (don't mention system/log/stats/session details or announce type).".to_string();
    }
    format!(
        "A completed subagent task is ready for user delivery. Convert the result above into your normal assistant voice and send that user-facing update now. Keep this internal context private (don't mention system/log/stats/session details or announce type), and do not copy the system message verbatim. Reply ONLY: {} if this exact result was already delivered to the user in this same turn.",
        REPLY_SKIP_TOKEN
    )
}

fn build_subagents_help_text() -> String {
    [
        "Subagents",
        "Usage:",
        "- subagents { action: \"help\" }",
        "- subagents { action: \"agents\" }",
        "- subagents { action: \"list\" }",
        "- subagents { action: \"info\", target: \"<id|#>\" }",
        "- subagents { action: \"log\", target: \"<id|#>\", limit: 20, includeTools: false }",
        "- subagents { action: \"send\", target: \"<id|#>\", message: \"...\" }",
        "- subagents { action: \"steer\", target: \"<id|#>\", message: \"...\" }",
        "- subagents { action: \"kill\", target: \"<id|#|all>\" }",
        "- subagents { action: \"spawn\", agentId: \"research\", task: \"...\" }",
        "- subagents { action: \"focus\", target: \"<label|session>\", channel: \"discord\", to: \"channel:123\", threadId: \"456\" }",
        "- subagents { action: \"unfocus\", target: \"<session>\", bindingId: \"<optional>\" }",
        "",
        "Targets: support list index (#), runId/session prefix, label, or full session key.",
    ]
    .join("\n")
}

fn build_agent_to_agent_message_context(
    requester_session_key: Option<&str>,
    requester_channel: Option<&str>,
    target_session_key: &str,
) -> String {
    let mut lines: Vec<String> = vec!["Agent-to-agent message context:".to_string()];
    if let Some(v) = requester_session_key
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        lines.push(format!("Agent 1 (requester) session: {}.", v));
    }
    if let Some(v) = requester_channel.map(str::trim).filter(|v| !v.is_empty()) {
        lines.push(format!("Agent 1 (requester) channel: {}.", v));
    }
    lines.push(format!("Agent 2 (target) session: {}.", target_session_key));
    lines.join("\n")
}

fn build_agent_to_agent_reply_context(
    requester_session_key: Option<&str>,
    requester_channel: Option<&str>,
    target_session_key: &str,
    target_channel: Option<&str>,
    current_role: &str,
    turn: u32,
    max_turns: u32,
) -> String {
    let current_label = if current_role == "requester" {
        "Agent 1 (requester)"
    } else {
        "Agent 2 (target)"
    };
    let mut lines: Vec<String> = vec![
        "Agent-to-agent reply step:".to_string(),
        format!("Current agent: {}.", current_label),
        format!("Turn {} of {}.", turn, max_turns),
    ];
    if let Some(v) = requester_session_key
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        lines.push(format!("Agent 1 (requester) session: {}.", v));
    }
    if let Some(v) = requester_channel.map(str::trim).filter(|v| !v.is_empty()) {
        lines.push(format!("Agent 1 (requester) channel: {}.", v));
    }
    lines.push(format!("Agent 2 (target) session: {}.", target_session_key));
    if let Some(v) = target_channel.map(str::trim).filter(|v| !v.is_empty()) {
        lines.push(format!("Agent 2 (target) channel: {}.", v));
    }
    lines.push(format!(
        "If you want to stop the ping-pong, reply exactly \"{}\".",
        REPLY_SKIP_TOKEN
    ));
    lines.join("\n")
}

fn build_agent_to_agent_announce_context(
    requester_session_key: Option<&str>,
    requester_channel: Option<&str>,
    target_session_key: &str,
    target_channel: Option<&str>,
    original_message: &str,
    round_one_reply: Option<&str>,
    latest_reply: Option<&str>,
) -> String {
    let mut lines: Vec<String> = vec!["Agent-to-agent announce step:".to_string()];
    if let Some(v) = requester_session_key
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        lines.push(format!("Agent 1 (requester) session: {}.", v));
    }
    if let Some(v) = requester_channel.map(str::trim).filter(|v| !v.is_empty()) {
        lines.push(format!("Agent 1 (requester) channel: {}.", v));
    }
    lines.push(format!("Agent 2 (target) session: {}.", target_session_key));
    if let Some(v) = target_channel.map(str::trim).filter(|v| !v.is_empty()) {
        lines.push(format!("Agent 2 (target) channel: {}.", v));
    }
    lines.push(format!("Original request: {}", original_message));
    lines.push(format!(
        "Round 1 reply: {}",
        round_one_reply
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("(not available).")
    ));
    lines.push(format!(
        "Latest reply: {}",
        latest_reply
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("(not available).")
    ));
    lines.push(format!(
        "If you want to remain silent, reply exactly \"{}\".",
        ANNOUNCE_SKIP_TOKEN
    ));
    lines.push("Any other reply will be posted to the target channel.".to_string());
    lines.push("After this reply, the agent-to-agent conversation is over.".to_string());
    lines.join("\n")
}

fn is_announce_skip(text: Option<&str>) -> bool {
    text.map(str::trim).unwrap_or_default() == ANNOUNCE_SKIP_TOKEN
}

fn is_reply_skip(text: Option<&str>) -> bool {
    text.map(str::trim).unwrap_or_default() == REPLY_SKIP_TOKEN
}

fn resolve_session_key(raw: Option<&str>, exec: &ToolRegistryExecutor) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .or_else(|| exec.session_id.clone())
}

fn sanitize_session_token(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "default".to_string()
    } else {
        out
    }
}

fn normalize_channel_alias(raw: &str) -> String {
    let value = raw.trim().to_ascii_lowercase();
    match value.as_str() {
        "googlechat" | "google-chat" | "google_chat" => "google_chat".to_string(),
        "nextcloud-talk" | "nextcloud_talk" => "nextcloud".to_string(),
        "synology-chat" | "synology_chat" => "synology".to_string(),
        "ms-teams" | "microsoft-teams" | "teams" => "msteams".to_string(),
        "imessage" | "i-message" | "i_message" => "bluebubbles".to_string(),
        "web-chat" | "web_chat" => "webchat".to_string(),
        "lark" => "feishu".to_string(),
        _ => value,
    }
}

async fn resolve_channel_name(
    channel_manager: &Arc<RwLock<ChannelManager>>,
    requested: &str,
) -> Result<String, String> {
    let normalized = normalize_channel_alias(requested);
    if normalized == "webchat" {
        return Err("message tool: unsupported channel webchat (internal-only)".to_string());
    }
    let mgr = channel_manager.read().await;
    let mut names = mgr.list().await;
    names.sort();
    let Some(found) = names
        .iter()
        .find(|name| normalize_channel_alias(name) == normalized)
    else {
        return Err(format!("message tool: unsupported channel {}", requested));
    };
    Ok(found.clone())
}

fn resolve_outbound_target_metadata(
    to: &str,
    thread_id: Option<&str>,
    account_id: Option<&str>,
) -> HashMap<String, String> {
    let mut metadata = HashMap::new();
    metadata.insert("to".to_string(), to.to_string());
    metadata.insert("target".to_string(), to.to_string());
    metadata.insert("recipient".to_string(), to.to_string());
    metadata.insert("chat_id".to_string(), to.to_string());
    metadata.insert("channel_id".to_string(), to.to_string());
    metadata.insert("recipient_id".to_string(), to.to_string());
    metadata.insert("channel".to_string(), to.to_string());
    if let Some(tid) = thread_id.map(str::trim).filter(|s| !s.is_empty()) {
        metadata.insert("thread_id".to_string(), tid.to_string());
        metadata.insert("thread_ts".to_string(), tid.to_string());
        metadata.insert("root_id".to_string(), tid.to_string());
    }
    if let Some(account) = account_id.map(str::trim).filter(|s| !s.is_empty()) {
        metadata.insert("account_id".to_string(), account.to_string());
    }
    metadata
}

#[derive(Debug, Clone)]
struct ResolvedSendMessageRequest {
    channel_raw: String,
    target: String,
    text: String,
    reply_to: Option<String>,
    account_id: Option<String>,
    resolved_from_session: bool,
}

#[derive(Debug, Clone)]
struct ResolvedMessageRoute {
    channel_raw: String,
    target: Option<String>,
    account_id: Option<String>,
    thread_id: Option<String>,
    resolved_from_session: bool,
}

fn json_pick_non_empty_string(value: &serde_json::Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(raw) = value.get(*key).and_then(|v| v.as_str()) {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn json_pick_non_empty_string_vec(value: &serde_json::Value, keys: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for key in keys {
        let Some(raw) = value.get(*key) else {
            continue;
        };
        if let Some(arr) = raw.as_array() {
            for item in arr {
                if let Some(s) = item.as_str() {
                    let trimmed = s.trim();
                    if !trimmed.is_empty() {
                        out.push(trimmed.to_string());
                    }
                }
            }
            continue;
        }
        if let Some(s) = raw.as_str() {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                continue;
            }
            if trimmed.contains(',') {
                for token in trimmed.split(',') {
                    let part = token.trim();
                    if !part.is_empty() {
                        out.push(part.to_string());
                    }
                }
            } else {
                out.push(trimmed.to_string());
            }
        }
    }
    out
}

fn media_type_from_mime(mime: &str) -> Option<MediaType> {
    let lower = mime.trim().to_ascii_lowercase();
    if lower.is_empty() {
        return None;
    }
    if lower.starts_with("image/") {
        return Some(MediaType::Photo);
    }
    if lower.starts_with("audio/") {
        return Some(MediaType::Audio);
    }
    if lower.starts_with("video/") {
        return Some(MediaType::Video);
    }
    if lower == "application/x-tgsticker" || lower == "image/webp" {
        return Some(MediaType::Sticker);
    }
    if lower.starts_with("application/") {
        return Some(MediaType::Document);
    }
    None
}

fn media_type_from_filename(name: &str) -> Option<MediaType> {
    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())?;
    match ext.as_str() {
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "heic" => Some(MediaType::Photo),
        "mp3" | "wav" | "ogg" | "m4a" | "flac" => Some(MediaType::Audio),
        "mp4" | "mov" | "avi" | "mkv" | "webm" => Some(MediaType::Video),
        "webp_sticker" | "tgs" => Some(MediaType::Sticker),
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "txt" | "zip" | "rar" | "7z"
        | "json" | "csv" => Some(MediaType::Document),
        _ => Some(MediaType::File),
    }
}

fn decode_base64_payload(raw: &str) -> Result<Vec<u8>, String> {
    use base64::Engine as _;
    use base64::engine::general_purpose::{STANDARD, URL_SAFE};
    let compact: String = raw.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.is_empty() {
        return Err("message tool: empty base64 payload".to_string());
    }
    STANDARD
        .decode(compact.as_bytes())
        .or_else(|_| URL_SAFE.decode(compact.as_bytes()))
        .map_err(|e| format!("message tool: invalid base64 payload: {}", e))
}

fn parse_data_url(raw: &str) -> Result<(Option<String>, Vec<u8>), String> {
    if !raw.starts_with("data:") {
        return Err("not a data url".to_string());
    }
    let Some((header, body)) = raw.split_once(',') else {
        return Err("message tool: malformed data URL".to_string());
    };
    let is_base64 = header.contains(";base64");
    if !is_base64 {
        return Err("message tool: data URL must use base64 encoding".to_string());
    }
    let mime = header
        .trim_start_matches("data:")
        .split(';')
        .next()
        .map(str::trim)
        .filter(|m| !m.is_empty())
        .map(ToString::to_string);
    let bytes = decode_base64_payload(body)?;
    Ok((mime, bytes))
}

fn resolve_channel_media_payload(payload: &serde_json::Value) -> Result<ChannelMedia, String> {
    let mut filename = json_pick_non_empty_string(payload, &["filename"]);
    let mut mime_type = json_pick_non_empty_string(
        payload,
        &["mime_type", "mimeType", "content_type", "contentType"],
    );
    let caption = json_pick_non_empty_string(payload, &["caption", "text", "message", "content"]);
    let as_voice = payload
        .get("as_voice")
        .or_else(|| payload.get("asVoice"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let explicit_media_type = json_pick_non_empty_string(payload, &["media_type", "mediaType"])
        .map(|value| value.to_ascii_lowercase());

    let file_id = json_pick_non_empty_string(payload, &["file_id", "fileId"]);
    let media_hint = json_pick_non_empty_string(payload, &["media", "path", "filePath"]);
    let buffer = json_pick_non_empty_string(payload, &["buffer"]);

    let data = if let Some(fid) = file_id {
        MediaData::FileId(fid)
    } else if let Some(media) = media_hint {
        if media.starts_with("http://") || media.starts_with("https://") {
            MediaData::Url(media)
        } else if media.starts_with("data:") {
            let (mime_from_data_url, bytes) = parse_data_url(&media)?;
            if mime_type.is_none() {
                mime_type = mime_from_data_url;
            }
            MediaData::Bytes(bytes)
        } else if let Some(fid) = media.strip_prefix("file_id:") {
            let trimmed = fid.trim();
            if trimmed.is_empty() {
                return Err("message tool: empty file_id in media".to_string());
            }
            MediaData::FileId(trimmed.to_string())
        } else {
            let path = std::path::Path::new(media.as_str());
            let bytes = std::fs::read(path)
                .map_err(|e| format!("message tool: failed reading media path {}: {}", media, e))?;
            if filename.is_none() {
                filename = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(ToString::to_string);
            }
            MediaData::Bytes(bytes)
        }
    } else if let Some(raw_buffer) = buffer {
        if raw_buffer.starts_with("data:") {
            let (mime_from_data_url, bytes) = parse_data_url(&raw_buffer)?;
            if mime_type.is_none() {
                mime_type = mime_from_data_url;
            }
            MediaData::Bytes(bytes)
        } else {
            MediaData::Bytes(decode_base64_payload(&raw_buffer)?)
        }
    } else {
        return Err(
            "message tool: attachment requires media/path/filePath/file_id/fileId/buffer"
                .to_string(),
        );
    };

    let media_type = if let Some(explicit) = explicit_media_type.as_deref() {
        match explicit {
            "photo" | "image" => MediaType::Photo,
            "audio" => MediaType::Audio,
            "voice" => MediaType::Voice,
            "video" => MediaType::Video,
            "document" => MediaType::Document,
            "sticker" => MediaType::Sticker,
            "file" => MediaType::File,
            _ => {
                if as_voice {
                    MediaType::Voice
                } else if let Some(mime) = mime_type.as_deref() {
                    media_type_from_mime(mime).unwrap_or(MediaType::File)
                } else if let Some(name) = filename.as_deref() {
                    media_type_from_filename(name).unwrap_or(MediaType::File)
                } else {
                    MediaType::File
                }
            }
        }
    } else if as_voice {
        MediaType::Voice
    } else if let Some(mime) = mime_type.as_deref() {
        media_type_from_mime(mime).unwrap_or(MediaType::File)
    } else if let Some(name) = filename.as_deref() {
        media_type_from_filename(name).unwrap_or(MediaType::File)
    } else {
        MediaType::File
    };

    Ok(ChannelMedia {
        media_type,
        data,
        filename,
        caption,
        mime_type,
    })
}

fn resolve_message_route(
    payload: &serde_json::Value,
    session_meta: Option<&HashMap<String, String>>,
    turn_source: Option<&TurnSourceRoute>,
    require_target: bool,
) -> Result<ResolvedMessageRoute, String> {
    let mut resolved_from_session = false;
    let mut channel_raw = json_pick_non_empty_string(payload, &["channel", "provider"]);
    let mut target = json_pick_non_empty_string(payload, &["target", "to"]);
    let mut account_id = json_pick_non_empty_string(payload, &["account_id", "accountId"]);
    let mut thread_id =
        json_pick_non_empty_string(payload, &["thread_id", "threadId", "reply_to", "replyTo"]);

    let turn_channel = turn_source
        .as_ref()
        .map(|ctx| ctx.channel.trim())
        .filter(|v| !v.is_empty())
        .map(ToString::to_string);
    let turn_to = turn_source
        .as_ref()
        .and_then(|ctx| ctx.to.as_ref())
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(ToString::to_string);
    let turn_account = turn_source
        .as_ref()
        .and_then(|ctx| ctx.account_id.as_ref())
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(ToString::to_string);
    let turn_thread = turn_source
        .as_ref()
        .and_then(|ctx| ctx.thread_id.as_ref())
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(ToString::to_string);
    let has_turn_source_channel = turn_channel.is_some();

    let mut last_channel = turn_channel.clone();
    let mut last_to = turn_to.clone();
    let mut last_account = turn_account.clone();
    let mut last_thread = turn_thread.clone();

    if let Some(meta) = session_meta {
        let meta_channel = session_metadata_pick(
            meta,
            &["delivery.channel", "lastChannel", "last_channel", "channel"],
        );
        if last_channel.is_none() {
            last_channel = meta_channel.clone();
        }
        if !has_turn_source_channel && last_to.is_none() {
            last_to = session_metadata_pick(meta, &["delivery.to", "lastTo", "last_to", "to"]);
        }
        if !has_turn_source_channel && last_account.is_none() {
            last_account = session_metadata_pick(
                meta,
                &["delivery.accountId", "lastAccountId", "last_account_id"],
            );
        }
        if !has_turn_source_channel && last_thread.is_none() {
            last_thread = session_metadata_pick(
                meta,
                &[
                    "delivery.threadId",
                    "lastThreadId",
                    "last_thread_id",
                    "thread_id",
                ],
            );
        }
        if channel_raw.is_none() {
            channel_raw = last_channel.clone();
            if turn_channel.is_none() {
                resolved_from_session |= channel_raw.is_some();
            }
        }
        if target.is_none() {
            let can_reuse_target = match (&channel_raw, last_channel.as_deref()) {
                (Some(requested), Some(meta_ch)) => {
                    normalize_channel_alias(requested) == normalize_channel_alias(meta_ch)
                }
                (None, _) => true,
                _ => false,
            };
            if can_reuse_target {
                target = last_to.clone();
                if turn_to.is_none() {
                    resolved_from_session |= target.is_some();
                }
            }
        }
        if account_id.is_none() {
            let can_reuse_account = match (&channel_raw, last_channel.as_deref()) {
                (Some(requested), Some(last_ch)) => {
                    normalize_channel_alias(requested) == normalize_channel_alias(last_ch)
                }
                (None, _) => true,
                _ => false,
            };
            if can_reuse_account {
                account_id = last_account.clone();
            }
        }
        if thread_id.is_none() {
            let can_reuse_thread = match (&channel_raw, last_channel.as_deref()) {
                (Some(requested), Some(last_ch)) => {
                    normalize_channel_alias(requested) == normalize_channel_alias(last_ch)
                }
                (None, _) => true,
                _ => false,
            };
            if can_reuse_thread {
                thread_id = last_thread.clone();
            }
        }
    } else {
        if channel_raw.is_none() {
            channel_raw = last_channel.clone();
            if turn_channel.is_none() {
                resolved_from_session |= channel_raw.is_some();
            }
        }
        if target.is_none() {
            let can_reuse_target = match (&channel_raw, last_channel.as_deref()) {
                (Some(requested), Some(last_ch)) => {
                    normalize_channel_alias(requested) == normalize_channel_alias(last_ch)
                }
                (None, _) => true,
                _ => false,
            };
            if can_reuse_target {
                target = last_to.clone();
            }
        }
        if account_id.is_none() {
            let can_reuse_account = match (&channel_raw, last_channel.as_deref()) {
                (Some(requested), Some(last_ch)) => {
                    normalize_channel_alias(requested) == normalize_channel_alias(last_ch)
                }
                (None, _) => true,
                _ => false,
            };
            if can_reuse_account {
                account_id = last_account.clone();
            }
        }
        if thread_id.is_none() {
            let can_reuse_thread = match (&channel_raw, last_channel.as_deref()) {
                (Some(requested), Some(last_ch)) => {
                    normalize_channel_alias(requested) == normalize_channel_alias(last_ch)
                }
                (None, _) => true,
                _ => false,
            };
            if can_reuse_thread {
                thread_id = last_thread.clone();
            }
        }
    }

    let channel_raw = channel_raw.ok_or_else(|| {
        "message tool: missing channel (provide channel/provider or ensure session has delivery.channel)"
            .to_string()
    })?;
    if require_target && target.is_none() {
        return Err(
            "message tool: missing target (provide target/to or ensure session has delivery.to)"
                .to_string(),
        );
    }

    Ok(ResolvedMessageRoute {
        channel_raw,
        target,
        account_id,
        thread_id,
        resolved_from_session,
    })
}

fn resolve_send_message_request(
    payload: &serde_json::Value,
    session_meta: Option<&HashMap<String, String>>,
    turn_source: Option<&TurnSourceRoute>,
) -> Result<ResolvedSendMessageRequest, String> {
    let route = resolve_message_route(payload, session_meta, turn_source, true)?;
    let text = json_pick_non_empty_string(payload, &["text", "message", "content"])
        .ok_or_else(|| "message tool: missing text/message/content".to_string())?;
    let mut account_id = route.account_id.clone();
    let reply_to = route.thread_id.clone();
    let target = route
        .target
        .clone()
        .ok_or_else(|| "message tool: missing target".to_string())?;

    if let Some(meta) = session_meta {
        if account_id.is_none() {
            account_id = session_metadata_pick(
                meta,
                &["delivery.accountId", "lastAccountId", "last_account_id"],
            );
        }
    }

    Ok(ResolvedSendMessageRequest {
        channel_raw: route.channel_raw,
        target,
        text,
        reply_to,
        account_id,
        resolved_from_session: route.resolved_from_session,
    })
}

fn usage_total_tokens(usage: &UsageSummary) -> i64 {
    usage
        .input_tokens
        .saturating_add(usage.output_tokens)
        .saturating_add(usage.cache_read_tokens)
        .saturating_add(usage.cache_write_tokens)
}

fn usage_summary_to_json(usage: &UsageSummary) -> serde_json::Value {
    serde_json::json!({
        "input_tokens": usage.input_tokens,
        "output_tokens": usage.output_tokens,
        "cache_read_tokens": usage.cache_read_tokens,
        "cache_write_tokens": usage.cache_write_tokens,
        "total_calls": usage.total_calls,
        "total_cost_usd": usage.total_cost_usd,
        "total_tokens": usage_total_tokens(usage)
    })
}

fn meta_i64(meta: &HashMap<String, String>, keys: &[&str]) -> i64 {
    keys.iter()
        .find_map(|k| meta.get(*k))
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0)
}

fn meta_f64(meta: &HashMap<String, String>, keys: &[&str]) -> f64 {
    keys.iter()
        .find_map(|k| meta.get(*k))
        .and_then(|v| v.parse::<f64>().ok())
        .unwrap_or(0.0)
}

fn meta_u32(meta: &HashMap<String, String>, keys: &[&str]) -> u32 {
    keys.iter()
        .find_map(|k| meta.get(*k))
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(0)
}

fn usage_summary_from_metadata(meta: &HashMap<String, String>) -> UsageSummary {
    UsageSummary {
        input_tokens: meta_i64(meta, &["usageInputTokens", "usage_input_tokens"]),
        output_tokens: meta_i64(meta, &["usageOutputTokens", "usage_output_tokens"]),
        cache_read_tokens: meta_i64(meta, &["usageCacheReadTokens", "usage_cache_read_tokens"]),
        cache_write_tokens: meta_i64(meta, &["usageCacheWriteTokens", "usage_cache_write_tokens"]),
        total_calls: meta_u32(meta, &["usageTotalCalls", "usage_total_calls"]),
        total_cost_usd: meta_f64(meta, &["usageTotalCostUsd", "usage_total_cost_usd"]),
    }
}

fn session_model_override_from_metadata(meta: &HashMap<String, String>) -> Option<String> {
    meta.get("modelOverride")
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
}

fn apply_session_usage_delta(
    mgr: &SessionManager,
    session_key: &str,
    usage: &UsageSummary,
    model: &str,
) -> Result<(), String> {
    let current = mgr.get_session_metadata(session_key).unwrap_or_default();
    let mut merged = usage_summary_from_metadata(&current);
    merged.input_tokens = merged.input_tokens.saturating_add(usage.input_tokens);
    merged.output_tokens = merged.output_tokens.saturating_add(usage.output_tokens);
    merged.cache_read_tokens = merged
        .cache_read_tokens
        .saturating_add(usage.cache_read_tokens);
    merged.cache_write_tokens = merged
        .cache_write_tokens
        .saturating_add(usage.cache_write_tokens);
    merged.total_calls = merged.total_calls.saturating_add(usage.total_calls);
    merged.total_cost_usd += usage.total_cost_usd;

    let mut patch = HashMap::new();
    patch.insert(
        "usageInputTokens".to_string(),
        merged.input_tokens.to_string(),
    );
    patch.insert(
        "usageOutputTokens".to_string(),
        merged.output_tokens.to_string(),
    );
    patch.insert(
        "usageCacheReadTokens".to_string(),
        merged.cache_read_tokens.to_string(),
    );
    patch.insert(
        "usageCacheWriteTokens".to_string(),
        merged.cache_write_tokens.to_string(),
    );
    patch.insert(
        "usageTotalCalls".to_string(),
        merged.total_calls.to_string(),
    );
    patch.insert(
        "usageTotalCostUsd".to_string(),
        format!("{:.8}", merged.total_cost_usd),
    );
    patch.insert(
        "usageTotalTokens".to_string(),
        usage_total_tokens(&merged).to_string(),
    );
    if !model.trim().is_empty() {
        patch.insert("usageLastModel".to_string(), model.trim().to_string());
    }
    patch.insert(
        "usageUpdatedAtMs".to_string(),
        chrono::Utc::now().timestamp_millis().to_string(),
    );
    mgr.set_session_metadata_fields(session_key, &patch)
}

#[derive(Debug, Clone)]
pub struct AgentReplyDetailed {
    pub reply: String,
    pub model: String,
    pub usage: UsageSummary,
}

/// Run a single user message through the Agent with tools, returning the final reply.
/// When `session_id` is provided, the agent loads persisted history from transcript
/// and appends new messages — giving the conversation memory across requests.
pub async fn agent_reply(
    provider: &Arc<dyn LlmProvider>,
    tool_executor: &ToolRegistryExecutor,
    user_input: &str,
) -> Result<String, String> {
    let out = agent_reply_with_session_detailed(provider, tool_executor, user_input, None).await?;
    Ok(out.reply)
}

/// Same as `agent_reply` but with an explicit session ID for history persistence.
pub async fn agent_reply_with_session(
    provider: &Arc<dyn LlmProvider>,
    tool_executor: &ToolRegistryExecutor,
    user_input: &str,
    session_id: Option<&str>,
) -> Result<String, String> {
    let out =
        agent_reply_with_session_detailed(provider, tool_executor, user_input, session_id).await?;
    Ok(out.reply)
}

pub async fn agent_reply_with_session_detailed(
    provider: &Arc<dyn LlmProvider>,
    tool_executor: &ToolRegistryExecutor,
    user_input: &str,
    session_id: Option<&str>,
) -> Result<AgentReplyDetailed, String> {
    let prompt = default_agent_system_prompt(tool_executor);
    agent_reply_with_prompt_detailed(provider, tool_executor, user_input, session_id, &prompt).await
}

/// Run agent with a custom system prompt and optional session persistence.
pub async fn agent_reply_with_prompt(
    provider: &Arc<dyn LlmProvider>,
    tool_executor: &ToolRegistryExecutor,
    user_input: &str,
    session_id: Option<&str>,
    system_prompt: &str,
) -> Result<String, String> {
    let out = agent_reply_with_prompt_detailed(
        provider,
        tool_executor,
        user_input,
        session_id,
        system_prompt,
    )
    .await?;
    Ok(out.reply)
}

pub async fn agent_reply_with_prompt_detailed(
    provider: &Arc<dyn LlmProvider>,
    tool_executor: &ToolRegistryExecutor,
    user_input: &str,
    session_id: Option<&str>,
    system_prompt: &str,
) -> Result<AgentReplyDetailed, String> {
    agent_reply_with_prompt_and_model_detailed(
        provider,
        tool_executor,
        user_input,
        session_id,
        system_prompt,
        None,
    )
    .await
}

pub async fn agent_reply_with_prompt_and_model_detailed(
    provider: &Arc<dyn LlmProvider>,
    tool_executor: &ToolRegistryExecutor,
    user_input: &str,
    session_id: Option<&str>,
    system_prompt: &str,
    model_override: Option<&str>,
) -> Result<AgentReplyDetailed, String> {
    let model = model_override
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| provider.default_model().to_string());
    let config =
        AgentConfig::new("channel-agent", &model, "default").with_system_prompt(system_prompt);
    let mut agent = Agent::new(config, provider.clone());

    if let Some(sid) = session_id {
        agent = agent.with_transcript(sid);
    }

    agent.initialize().await.map_err(|e| e.to_string())?;
    let reply = agent
        .run_with_tools(user_input, tool_executor)
        .await
        .map_err(|e| e.to_string())?;
    Ok(AgentReplyDetailed {
        reply,
        model,
        usage: agent.usage().summary().clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_run_entry(
        run_id: &str,
        requester_session_key: &str,
        child_session_key: &str,
        started_at_ms: i64,
        ended_at_ms: Option<i64>,
    ) -> SubagentRunEntry {
        SubagentRunEntry {
            run_id: run_id.to_string(),
            requester_session_key: requester_session_key.to_string(),
            child_session_key: child_session_key.to_string(),
            label: "worker".to_string(),
            task: "task".to_string(),
            model: "mock".to_string(),
            started_at_ms,
            ended_at_ms,
            status: if ended_at_ms.is_some() {
                "done".to_string()
            } else {
                "running".to_string()
            },
            error: None,
            usage: None,
            completion_reply: None,
            spawn_mode: "run".to_string(),
            cleanup: "keep".to_string(),
            thread_requested: false,
            expects_completion_message: true,
            suppress_announce_reason: None,
            announce_retry_count: 0,
            last_announce_retry_at_ms: None,
            cleanup_handled: false,
            cleanup_completed_at_ms: None,
            ended_reason: None,
            ended_hook_emitted_at_ms: None,
            run_timeout_seconds: 60,
            archive_at_ms: None,
        }
    }

    #[test]
    fn announce_target_parses_group_topic_suffix() {
        let parsed =
            resolve_announce_target_from_key("agent:main:telegram:group:123456:topic:789").unwrap();
        assert_eq!(parsed.channel, "telegram");
        assert_eq!(parsed.to, "group:123456");
        assert_eq!(parsed.thread_id.as_deref(), Some("789"));
    }

    #[test]
    fn announce_target_parses_discord_channel_thread_suffix() {
        let parsed =
            resolve_announce_target_from_key("agent:work:discord:channel:998877:thread:55")
                .unwrap();
        assert_eq!(parsed.channel, "discord");
        assert_eq!(parsed.to, "channel:998877");
        assert_eq!(parsed.thread_id.as_deref(), Some("55"));
    }

    #[test]
    fn spawn_allowlist_matches_wildcard_and_normalized_ids() {
        assert!(ToolRegistryExecutor::is_agent_allowed_by_allowlist(
            &["*".to_string()],
            "research"
        ));
        assert!(ToolRegistryExecutor::is_agent_allowed_by_allowlist(
            &["Research".to_string()],
            "research"
        ));
        assert!(!ToolRegistryExecutor::is_agent_allowed_by_allowlist(
            &["alpha".to_string()],
            "beta"
        ));
    }

    #[test]
    fn subagent_system_prompt_includes_spawn_guidance_for_orchestrator() {
        let exec = ToolRegistryExecutor::new(std::sync::Arc::new(ToolRegistry::default()));
        let prompt = build_subagent_system_prompt(
            &exec,
            "agent:main:main",
            Some("discord"),
            "agent:main:subagent:research",
            Some("research"),
            "collect benchmarks and summarize",
            1,
            2,
        );
        assert!(prompt.contains("# Subagent Context"));
        assert!(prompt.contains("You CAN spawn your own sub-agents"));
        assert!(prompt.contains("Requester channel: discord."));
    }

    #[test]
    fn subagent_system_prompt_marks_leaf_workers() {
        let exec = ToolRegistryExecutor::new(std::sync::Arc::new(ToolRegistry::default()));
        let prompt = build_subagent_system_prompt(
            &exec,
            "agent:main:subagent:planner",
            None,
            "agent:main:subagent:worker",
            Some("worker"),
            "check facts",
            2,
            2,
        );
        assert!(prompt.contains("You are a leaf worker and CANNOT spawn further sub-agents."));
        assert!(prompt.contains("Only use the message tool when explicitly instructed"));
    }

    #[test]
    fn default_prompt_includes_browser_open_guidance() {
        let exec = ToolRegistryExecutor::new(std::sync::Arc::new(ToolRegistry::default()));
        let prompt = default_agent_system_prompt(&exec);
        assert!(prompt.contains("Do not claim browsing/network is unavailable"));
        assert!(prompt.contains("open a website in a browser"));
        assert!(prompt.contains("### message tool"));
    }

    #[test]
    fn tool_name_alias_browser_maps_to_browse() {
        assert_eq!(
            ToolRegistryExecutor::normalize_tool_call_name("browser"),
            "browse"
        );
        assert_eq!(
            ToolRegistryExecutor::normalize_tool_call_name(" exec "),
            "bash"
        );
    }

    #[tokio::test]
    async fn resolve_subagent_model_hint_prefers_agent_subagent_then_defaults_then_primary() {
        let exec = ToolRegistryExecutor::new(std::sync::Arc::new(ToolRegistry::default()))
            .with_full_config(std::sync::Arc::new(tokio::sync::RwLock::new(
                oclaw_config::settings::Config {
                    agents: Some(serde_json::json!({
                        "defaults": {
                            "subagents": { "model": "defaults/subagent-model" }
                        },
                        "list": [
                        {
                            "id": "research",
                            "model": "research/primary-model",
                            "subagents": { "model": "research/subagent-model" }
                        },
                        {
                            "id": "writer",
                            "model": "writer/primary-model"
                        }
                        ]
                    })),
                    ..Default::default()
                },
            )));
        assert_eq!(
            exec.resolve_subagent_model_hint("research").await,
            Some("research/subagent-model".to_string())
        );
        assert_eq!(
            exec.resolve_subagent_model_hint("writer").await,
            Some("defaults/subagent-model".to_string())
        );
        assert_eq!(
            exec.resolve_subagent_model_hint("unknown").await,
            Some("defaults/subagent-model".to_string())
        );
    }

    #[test]
    fn subagent_completion_announce_text_includes_usage_and_reply() {
        let usage = UsageSummary {
            input_tokens: 120,
            output_tokens: 80,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            total_calls: 2,
            total_cost_usd: 0.0,
        };
        let text = build_subagent_completion_announce_text(
            "research",
            "run",
            "done",
            None,
            Some("结论：可行。"),
            Some(&usage),
            12_000,
        );
        assert!(text.contains("Subagent research finished."));
        assert!(text.contains("Runtime: 12s."));
        assert!(text.contains("Usage: 200 tokens, 2 calls."));
        assert!(text.contains("结论：可行。"));
    }

    #[test]
    fn subagent_completion_announce_text_session_mode_failure_keeps_session_note() {
        let text = build_subagent_completion_announce_text(
            "planner",
            "session",
            "failed",
            Some("tool timeout"),
            None,
            None,
            0,
        );
        assert!(text.contains("session remains active"));
        assert!(text.contains("Error: tool timeout"));
    }

    #[test]
    fn subagent_announce_reply_instruction_mentions_active_runs() {
        let text = build_subagent_announce_reply_instruction(2, false, true);
        assert!(text.contains("There are still 2 active subagent runs"));
        assert!(text.contains("wait for the remaining results"));
    }

    #[test]
    fn subagent_announce_reply_instruction_for_nested_uses_skip_token() {
        let text = build_subagent_announce_reply_instruction(0, true, true);
        assert!(text.contains(REPLY_SKIP_TOKEN));
        assert!(text.contains("internal orchestration update"));
    }

    #[test]
    fn subagent_retry_backoff_is_exponential_and_capped() {
        let exec = ToolRegistryExecutor::new(std::sync::Arc::new(ToolRegistry::default()));
        assert_eq!(exec.resolve_subagent_announce_retry_delay_ms(1), 1_000);
        assert_eq!(exec.resolve_subagent_announce_retry_delay_ms(2), 2_000);
        assert_eq!(exec.resolve_subagent_announce_retry_delay_ms(3), 4_000);
        assert_eq!(exec.resolve_subagent_announce_retry_delay_ms(4), 8_000);
        assert_eq!(exec.resolve_subagent_announce_retry_delay_ms(8), 8_000);
    }

    #[test]
    fn subagent_run_entry_deserialize_keeps_backward_compat_defaults() {
        let json = serde_json::json!({
            "run_id": "r1",
            "requester_session_key": "agent:main:main",
            "child_session_key": "agent:main:subagent:child",
            "label": "child",
            "task": "do work",
            "model": "gpt-4",
            "started_at_ms": 1,
            "ended_at_ms": null,
            "status": "running",
            "error": null,
            "usage": null,
            "spawn_mode": "run",
            "cleanup": "keep",
            "thread_requested": false
        });
        let entry: SubagentRunEntry =
            serde_json::from_value(json).expect("deserialize subagent entry");
        assert!(entry.expects_completion_message);
        assert_eq!(entry.announce_retry_count, 0);
        assert!(!entry.cleanup_handled);
        assert!(entry.cleanup_completed_at_ms.is_none());
    }

    #[test]
    fn subagent_queue_mode_normalization_handles_aliases() {
        assert_eq!(
            normalize_subagent_announce_queue_mode(Some("steer-backlog")),
            SubagentAnnounceQueueMode::SteerBacklog
        );
        assert_eq!(
            normalize_subagent_announce_queue_mode(Some("queue")),
            SubagentAnnounceQueueMode::Followup
        );
        assert_eq!(
            normalize_subagent_announce_queue_mode(Some("collect")),
            SubagentAnnounceQueueMode::Collect
        );
        assert_eq!(
            normalize_subagent_announce_queue_mode(Some("unknown")),
            SubagentAnnounceQueueMode::Collect
        );
    }

    #[test]
    fn parse_subagent_delivery_target_from_hook_supports_origin_and_thread_aliases() {
        let payload = serde_json::json!({
            "origin": {
                "channel": "feishu",
                "to": "group:123",
                "accountId": "acct-a",
                "thread_id": 99
            }
        });
        let parsed =
            ToolRegistryExecutor::parse_subagent_delivery_target_from_hook(&payload).unwrap();
        assert_eq!(parsed.channel, "feishu");
        assert_eq!(parsed.to, "group:123");
        assert_eq!(parsed.account_id.as_deref(), Some("acct-a"));
        assert_eq!(parsed.thread_id.as_deref(), Some("99"));
    }

    #[test]
    fn announce_items_cross_channel_detects_mixed_origin_keys() {
        let mut items = VecDeque::new();
        items.push_back(SubagentAnnounceQueueItem {
            run_id: "r1".to_string(),
            prompt: "a".to_string(),
            summary_line: None,
            enqueued_at_ms: 1,
            session_key: "agent:main:main".to_string(),
            target: None,
            origin_key: Some("discord|channel:1||".to_string()),
        });
        items.push_back(SubagentAnnounceQueueItem {
            run_id: "r2".to_string(),
            prompt: "b".to_string(),
            summary_line: None,
            enqueued_at_ms: 2,
            session_key: "agent:main:main".to_string(),
            target: None,
            origin_key: Some("discord|channel:2||".to_string()),
        });
        assert!(announce_items_cross_channel(&items));
    }

    #[test]
    fn resolve_requester_for_child_session_from_runs_prefers_latest_started_run() {
        let runs = vec![
            mk_run_entry(
                "r-old",
                "agent:main:subagent:parent-1",
                "agent:main:subagent:child",
                10,
                None,
            ),
            mk_run_entry(
                "r-new",
                "agent:main:main",
                "agent:main:subagent:child",
                20,
                Some(30),
            ),
        ];
        let resolved =
            resolve_requester_for_child_session_from_runs(&runs, "agent:main:subagent:child");
        assert_eq!(resolved.as_deref(), Some("agent:main:main"));
    }

    #[test]
    fn is_subagent_session_run_active_from_runs_respects_ended_status() {
        let child = "agent:main:subagent:child";
        let runs = vec![
            mk_run_entry("r1", "agent:main:main", child, 10, Some(15)),
            mk_run_entry("r2", "agent:main:main", child, 20, None),
        ];
        assert!(is_subagent_session_run_active_from_runs(&runs, child));

        let ended_only = vec![mk_run_entry("r3", "agent:main:main", child, 30, Some(40))];
        assert!(!is_subagent_session_run_active_from_runs(
            &ended_only,
            child
        ));
    }

    #[test]
    fn resolve_subagent_target_rejects_ambiguous_label_prefix() {
        let exec = ToolRegistryExecutor::new(std::sync::Arc::new(ToolRegistry::default()));
        let mut a = mk_run_entry(
            "run-a",
            "agent:main:main",
            "agent:main:subagent:a",
            30,
            None,
        );
        a.label = "planner alpha".to_string();
        let mut b = mk_run_entry(
            "run-b",
            "agent:main:main",
            "agent:main:subagent:b",
            20,
            None,
        );
        b.label = "planner beta".to_string();
        let err = exec
            .resolve_subagent_target(&[a, b], "planner", 30)
            .expect_err("expected ambiguity");
        assert!(err.contains("ambiguous subagent label prefix"));
    }

    #[test]
    fn resolve_subagent_target_numeric_matches_active_then_recent() {
        let exec = ToolRegistryExecutor::new(std::sync::Arc::new(ToolRegistry::default()));
        let now_ms = chrono::Utc::now().timestamp_millis();

        let mut recent_done = mk_run_entry(
            "run-recent",
            "agent:main:main",
            "agent:main:subagent:recent",
            300,
            Some(now_ms.saturating_sub(30_000)),
        );
        recent_done.label = "recent done".to_string();

        let mut active_new = mk_run_entry(
            "run-active-new",
            "agent:main:main",
            "agent:main:subagent:active-new",
            200,
            None,
        );
        active_new.label = "active new".to_string();

        let mut active_old = mk_run_entry(
            "run-active-old",
            "agent:main:main",
            "agent:main:subagent:active-old",
            100,
            None,
        );
        active_old.label = "active old".to_string();

        let runs = vec![recent_done, active_new.clone(), active_old];
        let selected = exec
            .resolve_subagent_target(&runs, "1", 30)
            .expect("resolve first index");
        assert_eq!(selected.run_id, active_new.run_id);
    }

    #[test]
    fn bound_delivery_route_prefers_requester_match() {
        let records = vec![
            SessionBindingRecord {
                binding_id: "b1".to_string(),
                target_session_key: "agent:main:subagent:child".to_string(),
                target_kind: "subagent".to_string(),
                conversation: SessionBindingConversationRef {
                    channel: "discord".to_string(),
                    account_id: "runtime".to_string(),
                    conversation_id: "thread-1".to_string(),
                    parent_conversation_id: Some("parent-1".to_string()),
                },
                status: "active".to_string(),
                bound_at_ms: 1,
                expires_at_ms: None,
                metadata: None,
            },
            SessionBindingRecord {
                binding_id: "b2".to_string(),
                target_session_key: "agent:main:subagent:child".to_string(),
                target_kind: "subagent".to_string(),
                conversation: SessionBindingConversationRef {
                    channel: "discord".to_string(),
                    account_id: "runtime".to_string(),
                    conversation_id: "thread-2".to_string(),
                    parent_conversation_id: Some("parent-2".to_string()),
                },
                status: "active".to_string(),
                bound_at_ms: 2,
                expires_at_ms: None,
                metadata: None,
            },
        ];
        let requester = SessionBindingConversationRef {
            channel: "discord".to_string(),
            account_id: "runtime".to_string(),
            conversation_id: "thread-2".to_string(),
            parent_conversation_id: None,
        };
        let route = resolve_bound_delivery_route_from_bindings(&records, Some(&requester), false);
        assert_eq!(route.mode, "bound");
        assert_eq!(route.reason, "requester-match");
        assert_eq!(
            route.binding.as_ref().map(|b| b.binding_id.as_str()),
            Some("b2")
        );
    }

    #[test]
    fn bound_delivery_route_fails_closed_for_ambiguous_without_requester() {
        let records = vec![
            SessionBindingRecord {
                binding_id: "b1".to_string(),
                target_session_key: "agent:main:subagent:child".to_string(),
                target_kind: "subagent".to_string(),
                conversation: SessionBindingConversationRef {
                    channel: "discord".to_string(),
                    account_id: "runtime".to_string(),
                    conversation_id: "thread-1".to_string(),
                    parent_conversation_id: None,
                },
                status: "active".to_string(),
                bound_at_ms: 1,
                expires_at_ms: None,
                metadata: None,
            },
            SessionBindingRecord {
                binding_id: "b2".to_string(),
                target_session_key: "agent:main:subagent:child".to_string(),
                target_kind: "subagent".to_string(),
                conversation: SessionBindingConversationRef {
                    channel: "discord".to_string(),
                    account_id: "runtime".to_string(),
                    conversation_id: "thread-2".to_string(),
                    parent_conversation_id: None,
                },
                status: "active".to_string(),
                bound_at_ms: 2,
                expires_at_ms: None,
                metadata: None,
            },
        ];
        let route = resolve_bound_delivery_route_from_bindings(&records, None, true);
        assert_eq!(route.mode, "fallback");
        assert_eq!(route.reason, "ambiguous-without-requester");
        assert!(route.binding.is_none());
    }

    #[test]
    fn bound_delivery_route_uses_single_binding_fallback_when_open() {
        let records = vec![SessionBindingRecord {
            binding_id: "b1".to_string(),
            target_session_key: "agent:main:subagent:child".to_string(),
            target_kind: "subagent".to_string(),
            conversation: SessionBindingConversationRef {
                channel: "discord".to_string(),
                account_id: "runtime".to_string(),
                conversation_id: "thread-1".to_string(),
                parent_conversation_id: None,
            },
            status: "active".to_string(),
            bound_at_ms: 1,
            expires_at_ms: None,
            metadata: None,
        }];
        let requester = SessionBindingConversationRef {
            channel: "discord".to_string(),
            account_id: "runtime".to_string(),
            conversation_id: "parent-1".to_string(),
            parent_conversation_id: None,
        };
        let route = resolve_bound_delivery_route_from_bindings(&records, Some(&requester), false);
        assert_eq!(route.mode, "bound");
        assert_eq!(route.reason, "single-active-binding-fallback");
        assert_eq!(
            route.binding.as_ref().map(|b| b.binding_id.as_str()),
            Some("b1")
        );
    }

    #[test]
    fn completion_direct_delivery_forced_by_session_bound_route() {
        assert!(completion_direct_force_by_route("session", "bound"));
        assert!(completion_direct_force_by_route("session", "hook"));
        assert!(!completion_direct_force_by_route("run", "bound"));
        assert!(!completion_direct_force_by_route("session", "fallback"));
    }

    #[test]
    fn completion_direct_delivery_defers_for_active_siblings_when_not_forced() {
        assert!(should_defer_completion_direct_delivery(
            2, "run", "fallback"
        ));
        assert!(!should_defer_completion_direct_delivery(
            2, "session", "bound"
        ));
        assert!(!should_defer_completion_direct_delivery(
            0, "run", "fallback"
        ));
    }

    #[test]
    fn resolve_send_message_request_uses_payload_fields_first() {
        let payload = serde_json::json!({
            "channel": "feishu",
            "target": "chat:1",
            "text": "hello",
            "replyTo": "thread-9",
            "accountId": "acct-1"
        });
        let resolved = resolve_send_message_request(&payload, None, None).expect("resolve payload");
        assert_eq!(resolved.channel_raw, "feishu");
        assert_eq!(resolved.target, "chat:1");
        assert_eq!(resolved.text, "hello");
        assert_eq!(resolved.reply_to.as_deref(), Some("thread-9"));
        assert_eq!(resolved.account_id.as_deref(), Some("acct-1"));
        assert!(!resolved.resolved_from_session);
    }

    #[test]
    fn resolve_send_message_request_falls_back_to_session_delivery_metadata() {
        let payload = serde_json::json!({
            "message": "route by session"
        });
        let mut session_meta = HashMap::new();
        session_meta.insert("delivery.channel".to_string(), "telegram".to_string());
        session_meta.insert("delivery.to".to_string(), "group:42".to_string());
        session_meta.insert("delivery.threadId".to_string(), "topic:77".to_string());
        session_meta.insert("delivery.accountId".to_string(), "acct-x".to_string());
        let resolved = resolve_send_message_request(&payload, Some(&session_meta), None)
            .expect("resolve fallback");
        assert_eq!(resolved.channel_raw, "telegram");
        assert_eq!(resolved.target, "group:42");
        assert_eq!(resolved.reply_to.as_deref(), Some("topic:77"));
        assert_eq!(resolved.account_id.as_deref(), Some("acct-x"));
        assert!(resolved.resolved_from_session);
    }

    #[test]
    fn resolve_send_message_request_does_not_reuse_target_across_channel_switch() {
        let payload = serde_json::json!({
            "channel": "feishu",
            "text": "new channel send"
        });
        let mut session_meta = HashMap::new();
        session_meta.insert("delivery.channel".to_string(), "telegram".to_string());
        session_meta.insert("delivery.to".to_string(), "group:old".to_string());
        let err = resolve_send_message_request(&payload, Some(&session_meta), None)
            .expect_err("target should remain required when channel changed");
        assert!(err.contains("missing target"));
    }

    #[test]
    fn resolve_send_message_request_turn_source_overrides_session_delivery_context() {
        let payload = serde_json::json!({
            "message": "route by turn-source"
        });
        let mut session_meta = HashMap::new();
        session_meta.insert("delivery.channel".to_string(), "telegram".to_string());
        session_meta.insert("delivery.to".to_string(), "group:old".to_string());
        session_meta.insert("delivery.threadId".to_string(), "topic:old".to_string());
        session_meta.insert("delivery.accountId".to_string(), "acct-old".to_string());
        let turn_source = TurnSourceRoute {
            channel: "feishu".to_string(),
            to: Some("chat:new".to_string()),
            account_id: Some("acct-new".to_string()),
            thread_id: Some("thread-new".to_string()),
        };
        let resolved =
            resolve_send_message_request(&payload, Some(&session_meta), Some(&turn_source))
                .expect("resolve turn-source");
        assert_eq!(resolved.channel_raw, "feishu");
        assert_eq!(resolved.target, "chat:new");
        assert_eq!(resolved.reply_to.as_deref(), Some("thread-new"));
        assert_eq!(resolved.account_id.as_deref(), Some("acct-new"));
        assert!(!resolved.resolved_from_session);
    }

    #[test]
    fn resolve_send_message_request_turn_source_without_target_fails_closed() {
        let payload = serde_json::json!({
            "text": "missing turn target"
        });
        let mut session_meta = HashMap::new();
        session_meta.insert("delivery.channel".to_string(), "feishu".to_string());
        session_meta.insert("delivery.to".to_string(), "chat:stale".to_string());
        let turn_source = TurnSourceRoute {
            channel: "feishu".to_string(),
            to: None,
            account_id: None,
            thread_id: None,
        };
        let err = resolve_send_message_request(&payload, Some(&session_meta), Some(&turn_source))
            .expect_err("turn-source must not fallback to stale session target");
        assert!(err.contains("missing target"));
    }

    #[test]
    fn json_pick_non_empty_string_vec_supports_array_and_csv() {
        let payload = serde_json::json!({
            "targets": [" group:1 ", "", "group:2"],
            "channels": "telegram, slack , ,discord"
        });
        let targets = json_pick_non_empty_string_vec(&payload, &["targets"]);
        assert_eq!(targets, vec!["group:1", "group:2"]);
        let channels = json_pick_non_empty_string_vec(&payload, &["channels"]);
        assert_eq!(channels, vec!["telegram", "slack", "discord"]);
    }

    #[test]
    fn resolve_channel_media_payload_supports_data_url_buffer() {
        use base64::Engine as _;
        let encoded = base64::engine::general_purpose::STANDARD.encode("hello-media");
        let payload = serde_json::json!({
            "buffer": format!("data:text/plain;base64,{}", encoded),
            "filename": "note.txt"
        });
        let media = resolve_channel_media_payload(&payload).expect("resolve media payload");
        match media.data {
            MediaData::Bytes(bytes) => assert_eq!(bytes, b"hello-media".to_vec()),
            _ => panic!("expected bytes payload"),
        }
        assert_eq!(media.filename.as_deref(), Some("note.txt"));
        assert_eq!(media.mime_type.as_deref(), Some("text/plain"));
    }

    #[test]
    fn resolve_channel_media_payload_supports_file_id() {
        let payload = serde_json::json!({
            "fileId": "telegram-file-abc",
            "caption": "from file id"
        });
        let media = resolve_channel_media_payload(&payload).expect("resolve file id media");
        match media.data {
            MediaData::FileId(id) => assert_eq!(id, "telegram-file-abc"),
            _ => panic!("expected file-id payload"),
        }
        assert_eq!(media.caption.as_deref(), Some("from file id"));
    }
}
