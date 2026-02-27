use axum::{
    Json, Router,
    extract::{ConnectInfo, DefaultBodyLimit, State, WebSocketUpgrade},
    http::{Method, StatusCode},
    middleware as axum_mw,
    response::{IntoResponse, Response},
    routing::{any, delete, get, post, put},
};
use futures_util::{SinkExt, StreamExt};
use oclaw_config::settings::Gateway;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio::process::Command;
use tokio::sync::RwLock;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use tracing::{error, info, warn};

use oclaw_agent_core::{
    EchoTracker, SubagentConfig, SubagentRegistry, TaskGraph, TaskGraphResult, TaskGraphRunner,
    TaskNode, ToolExecutor,
};
use oclaw_browser_core::{BrowserManager, CdpConnection, CdpDomain, Page, build_method};
use oclaw_channel_core::ChannelManager;
use oclaw_channel_core::group_gate::GroupActivation;
use oclaw_doctor_core::{HealthChecker, SystemHealthCheck};
use oclaw_llm_core::providers::LlmProvider;
use oclaw_media_understanding::providers::{
    anthropic::AnthropicMediaProvider, deepgram::DeepgramMediaProvider,
    google::GoogleMediaProvider, openai::OpenAiMediaProvider,
};
use oclaw_media_understanding::{MediaAttachment, MediaConfig, MediaPipeline};
use oclaw_memory_core::{AutoCaptureConfig, MemoryManager};
use oclaw_pairing::PairingStore;
use oclaw_plugin_core::HookPipeline;
use oclaw_plugin_core::PluginRegistrations;
use oclaw_skills_core::SkillRegistry;
use oclaw_tools_core::ApprovalGate;
use oclaw_tools_core::approval::ApprovalDecision;
use oclaw_tools_core::tool::ToolRegistry;
use oclaw_tts_core::prepare::prepare_for_tts;
use oclaw_tts_core::providers::TtsProviderBackend;
use oclaw_tts_core::providers::{
    edge::{EdgeTts, EdgeTtsOptions},
    elevenlabs::ElevenLabsTts,
    openai::OpenAiTts,
};
use oclaw_tts_core::types::TtsProvider;

use crate::auth::AuthState;
use crate::error::{GatewayError, GatewayResult};
use crate::message::{MessageHandler, SessionManager};
use crate::server::GatewayServer;
use oclaw_protocol::frames::{
    ErrorDetails, EventFrame, GatewayFrame, HelloOk, Policy, ServerFeatures, ServerInfo,
};
use oclaw_protocol::snapshot::{AuthMode, Snapshot, StateVersion};

pub mod agent_bridge;
pub mod auth;
pub mod cron_executor;
pub mod metrics;
pub mod middleware;
pub mod rate_limit;
pub mod routes;
pub mod webhooks;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WizardStep {
    id: String,
    title: String,
    description: String,
    answer: Option<serde_json::Value>,
}

#[derive(Debug, Clone)]
pub struct WizardSessionState {
    created_at_ms: u64,
    updated_at_ms: u64,
    status: String,
    current_step: usize,
    steps: Vec<WizardStep>,
    error: Option<String>,
}

impl WizardSessionState {
    fn now_ms() -> u64 {
        chrono::Utc::now().timestamp_millis().max(0) as u64
    }

    fn new() -> Self {
        let now = Self::now_ms();
        Self {
            created_at_ms: now,
            updated_at_ms: now,
            status: "running".to_string(),
            current_step: 0,
            steps: vec![
                WizardStep {
                    id: "gateway.mode".to_string(),
                    title: "Gateway Mode".to_string(),
                    description: "选择网关模式：local 或 remote".to_string(),
                    answer: None,
                },
                WizardStep {
                    id: "gateway.port".to_string(),
                    title: "Gateway Port".to_string(),
                    description: "设置网关端口（默认 18789）".to_string(),
                    answer: None,
                },
                WizardStep {
                    id: "auth".to_string(),
                    title: "Auth".to_string(),
                    description: "配置鉴权 token/password".to_string(),
                    answer: None,
                },
                WizardStep {
                    id: "models".to_string(),
                    title: "Model Provider".to_string(),
                    description: "配置默认模型提供商与 API Key".to_string(),
                    answer: None,
                },
                WizardStep {
                    id: "channels".to_string(),
                    title: "Channels".to_string(),
                    description: "启用消息渠道并完成必要凭证".to_string(),
                    answer: None,
                },
            ],
            error: None,
        }
    }
}

const SYSTEM_PRESENCE_SELF_KEY: &str = "gateway:self";
const SYSTEM_PRESENCE_TTL_MS: i64 = 5 * 60 * 1000;
const SYSTEM_PRESENCE_MAX_ENTRIES: usize = 200;
const MAIN_SYSTEM_SESSION_KEY: &str = "main";
const MAX_SYSTEM_EVENTS: usize = 20;
const DEFAULT_WAKE_REASON: &str = "requested";

static GLOBAL_HEARTBEATS_ENABLED: AtomicBool = AtomicBool::new(true);

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct SystemPresenceEntry {
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    platform: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model_identifier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last_input_seconds: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    device_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    roles: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scopes: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    instance_id: Option<String>,
    text: String,
    ts: i64,
}

static SYSTEM_PRESENCE: Lazy<std::sync::Mutex<HashMap<String, SystemPresenceEntry>>> =
    Lazy::new(|| {
        let mut map = HashMap::new();
        map.insert(
            SYSTEM_PRESENCE_SELF_KEY.to_string(),
            build_self_system_presence_entry(),
        );
        std::sync::Mutex::new(map)
    });

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SystemEventEntry {
    text: String,
    ts: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    context_key: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct SessionSystemEventQueue {
    queue: VecDeque<SystemEventEntry>,
    last_text: Option<String>,
    last_context_key: Option<String>,
}

static SYSTEM_EVENT_QUEUES: Lazy<std::sync::Mutex<HashMap<String, SessionSystemEventQueue>>> =
    Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

#[derive(Debug, Clone)]
struct PendingHeartbeatWake {
    reason: String,
    priority: i32,
    requested_at: i64,
    agent_id: Option<String>,
    session_key: Option<String>,
}

static PENDING_HEARTBEAT_WAKES: Lazy<std::sync::Mutex<HashMap<String, PendingHeartbeatWake>>> =
    Lazy::new(|| std::sync::Mutex::new(HashMap::new()));

static GLOBAL_LAST_HEARTBEAT_EVENT: Lazy<std::sync::Mutex<Option<serde_json::Value>>> =
    Lazy::new(|| std::sync::Mutex::new(None));

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TtsRuntimeState {
    enabled: bool,
    provider: TtsProvider,
    voice: Option<String>,
}

impl Default for TtsRuntimeState {
    fn default() -> Self {
        Self {
            enabled: false,
            provider: TtsProvider::Edge,
            voice: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecApprovalsFileSnapshot {
    version: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    socket: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    defaults: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    agents: Option<serde_json::Value>,
}

impl Default for ExecApprovalsFileSnapshot {
    fn default() -> Self {
        Self {
            version: 1,
            socket: Some(serde_json::json!({
                "path": "~/.oclaw/exec-approvals.sock",
            })),
            defaults: Some(serde_json::json!({
                "security": "deny",
                "ask": "on-miss",
                "askFallback": "deny",
                "autoAllowSkills": false,
            })),
            agents: Some(serde_json::json!({})),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeInvokeRecord {
    id: String,
    node_id: String,
    command: String,
    params: serde_json::Value,
    created_at_ms: u64,
    status: String,
    result: Option<serde_json::Value>,
    error: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodePairRecord {
    request_id: String,
    node_id: String,
    display_name: Option<String>,
    platform: Option<String>,
    version: Option<String>,
    core_version: Option<String>,
    ui_version: Option<String>,
    remote_ip: Option<String>,
    caps: Option<Vec<String>>,
    commands: Option<Vec<String>>,
    is_repair: bool,
    ts: u64,
    approved: bool,
    token: Option<String>,
    approved_at_ms: Option<u64>,
    last_connected_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentRunRecord {
    run_id: String,
    status: String,
    started_at_ms: u64,
    ended_at_ms: Option<u64>,
    error: Option<String>,
    result: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcTaskGraphNodeInput {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    task: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    max_iterations: Option<i32>,
    #[serde(default)]
    timeout_seconds: Option<i64>,
    #[serde(default)]
    capabilities: Option<Vec<String>>,
    #[serde(default)]
    depends_on: Option<Vec<String>>,
    #[serde(default)]
    on_success: Option<Vec<String>>,
    #[serde(default)]
    on_failure: Option<Vec<String>>,
    #[serde(default)]
    input_from: Option<String>,
    #[serde(default)]
    input_template: Option<String>,
    #[serde(default)]
    base_input: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RpcTaskGraphInput {
    #[serde(default)]
    input: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    provider: Option<String>,
    #[serde(default)]
    system_prompt: Option<String>,
    #[serde(default)]
    max_concurrent: Option<u64>,
    #[serde(default)]
    session_key: Option<String>,
    #[serde(default)]
    nodes: Vec<RpcTaskGraphNodeInput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatRunRecord {
    run_id: String,
    session_key: String,
    status: String,
    started_at_ms: u64,
    ended_at_ms: Option<u64>,
    error: Option<String>,
    result: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TalkModeRuntimeState {
    enabled: bool,
    phase: Option<String>,
    ts: u64,
}

impl Default for TalkModeRuntimeState {
    fn default() -> Self {
        Self {
            enabled: false,
            phase: None,
            ts: now_epoch_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GatewayUsageTotals {
    input: i64,
    output: i64,
    cache_read: i64,
    cache_write: i64,
    total_tokens: i64,
    total_cost: f64,
    input_cost: f64,
    output_cost: f64,
    cache_read_cost: f64,
    cache_write_cost: f64,
    missing_cost_entries: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct GatewayUsageSnapshot {
    updated_at: u64,
    totals: GatewayUsageTotals,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePairPendingRecord {
    request_id: String,
    device_id: String,
    public_key: Option<String>,
    display_name: Option<String>,
    platform: Option<String>,
    client_id: Option<String>,
    client_mode: Option<String>,
    role: Option<String>,
    roles: Option<Vec<String>>,
    scopes: Option<Vec<String>>,
    remote_ip: Option<String>,
    silent: bool,
    is_repair: bool,
    ts: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAuthTokenRecord {
    role: String,
    token: String,
    scopes: Vec<String>,
    created_at_ms: u64,
    rotated_at_ms: Option<u64>,
    revoked_at_ms: Option<u64>,
    last_used_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePairedRecord {
    device_id: String,
    public_key: Option<String>,
    display_name: Option<String>,
    platform: Option<String>,
    client_id: Option<String>,
    client_mode: Option<String>,
    role: Option<String>,
    roles: Option<Vec<String>>,
    scopes: Option<Vec<String>>,
    approved_scopes: Option<Vec<String>>,
    remote_ip: Option<String>,
    tokens: HashMap<String, DeviceAuthTokenRecord>,
    created_at_ms: u64,
    approved_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct DevicePairingSnapshot {
    pending_by_id: HashMap<String, DevicePairPendingRecord>,
    paired_by_device_id: HashMap<String, DevicePairedRecord>,
}

fn device_pairing_snapshot_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("oclaw")
        .join("pairing")
        .join("devices.json")
}

fn load_device_pairing_snapshot() -> DevicePairingSnapshot {
    let path = device_pairing_snapshot_path();
    let content = match std::fs::read_to_string(&path) {
        Ok(v) => v,
        Err(_) => return DevicePairingSnapshot::default(),
    };
    serde_json::from_str::<DevicePairingSnapshot>(&content).unwrap_or_default()
}

fn persist_device_pairing_snapshot(snapshot: &DevicePairingSnapshot) -> Result<(), String> {
    let path = device_pairing_snapshot_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create dir failed: {}", e))?;
    }
    let json = serde_json::to_string_pretty(snapshot)
        .map_err(|e| format!("serialize device pairing failed: {}", e))?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("write device pairing failed: {}", e))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("rename device pairing failed: {}", e))?;
    Ok(())
}

fn build_device_pending_index(
    pending: &HashMap<String, DevicePairPendingRecord>,
) -> HashMap<String, String> {
    let mut index = HashMap::new();
    for (request_id, rec) in pending {
        index.insert(rec.device_id.clone(), request_id.clone());
    }
    index
}

fn load_voicewake_triggers_from_config(
    cfg: Option<&Arc<RwLock<oclaw_config::settings::Config>>>,
) -> Vec<String> {
    let Some(cfg) = cfg else {
        return Vec::new();
    };
    let Ok(guard) = cfg.try_read() else {
        return Vec::new();
    };
    let raw = guard
        .commands
        .as_ref()
        .and_then(|v| v.pointer("/voicewake/triggers"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    raw.iter()
        .filter_map(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub struct HttpServer {
    addr: SocketAddr,
    gateway: Arc<Gateway>,
    auth_state: Arc<RwLock<AuthState>>,
    gateway_server: Arc<GatewayServer>,
    tls_config: Option<Arc<rustls::ServerConfig>>,
    static_files_path: Option<PathBuf>,
    llm_provider: Option<Arc<dyn LlmProvider>>,
    hook_pipeline: Option<Arc<HookPipeline>>,
    channel_manager: Option<Arc<RwLock<ChannelManager>>>,
    tool_registry: Option<Arc<ToolRegistry>>,
    skill_registry: Option<Arc<SkillRegistry>>,
    approval_gate: Option<Arc<ApprovalGate>>,
    plugin_registrations: Option<Arc<PluginRegistrations>>,
    cron_service: Option<Arc<oclaw_cron_core::CronService>>,
    memory_manager: Option<Arc<MemoryManager>>,
    workspace: Option<Arc<oclaw_workspace_core::files::Workspace>>,
    full_config: Option<Arc<RwLock<oclaw_config::settings::Config>>>,
    config_path: Option<PathBuf>,
    needs_hatching: Arc<std::sync::atomic::AtomicBool>,
    dm_scope: crate::session_key::DmScope,
    identity_links: Option<Arc<crate::session_key::IdentityLinks>>,
}

impl HttpServer {
    pub fn new(
        addr: SocketAddr,
        gateway: Arc<Gateway>,
        gateway_server: Arc<GatewayServer>,
    ) -> Self {
        let auth_state = Arc::new(RwLock::new(AuthState::new(gateway.auth.clone())));
        Self {
            addr,
            gateway,
            auth_state,
            gateway_server,
            tls_config: None,
            static_files_path: None,
            llm_provider: None,
            hook_pipeline: None,
            channel_manager: None,
            tool_registry: None,
            skill_registry: None,
            approval_gate: None,
            plugin_registrations: None,
            cron_service: None,
            memory_manager: None,
            workspace: None,
            full_config: None,
            config_path: None,
            needs_hatching: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            dm_scope: crate::session_key::DmScope::default(),
            identity_links: None,
        }
    }

    pub fn with_static_files(mut self, path: PathBuf) -> Self {
        self.static_files_path = Some(path);
        self
    }

    pub fn with_tls(mut self, tls_config: Arc<rustls::ServerConfig>) -> Self {
        self.tls_config = Some(tls_config);
        self
    }

    pub fn with_llm_provider(mut self, provider: Arc<dyn LlmProvider>) -> Self {
        self.llm_provider = Some(provider);
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

    pub fn with_tool_registry(mut self, registry: Arc<ToolRegistry>) -> Self {
        self.tool_registry = Some(registry);
        self
    }

    pub fn with_skill_registry(mut self, registry: Arc<SkillRegistry>) -> Self {
        self.skill_registry = Some(registry);
        self
    }

    pub fn with_approval_gate(mut self, gate: Arc<ApprovalGate>) -> Self {
        self.approval_gate = Some(gate);
        self
    }

    pub fn with_plugin_registrations(mut self, regs: Arc<PluginRegistrations>) -> Self {
        self.plugin_registrations = Some(regs);
        self
    }

    pub fn with_cron_service(mut self, svc: Arc<oclaw_cron_core::CronService>) -> Self {
        self.cron_service = Some(svc);
        self
    }

    pub fn with_memory_manager(mut self, manager: Arc<MemoryManager>) -> Self {
        self.memory_manager = Some(manager);
        self
    }

    pub fn with_workspace(
        mut self,
        workspace: Arc<oclaw_workspace_core::files::Workspace>,
    ) -> Self {
        self.workspace = Some(workspace);
        self
    }

    pub fn with_full_config(
        mut self,
        config: oclaw_config::settings::Config,
        path: PathBuf,
    ) -> Self {
        self.full_config = Some(Arc::new(RwLock::new(config)));
        self.config_path = Some(path);
        self
    }

    pub fn with_needs_hatching(mut self, flag: Arc<std::sync::atomic::AtomicBool>) -> Self {
        self.needs_hatching = flag;
        self
    }

    pub fn with_dm_scope(mut self, scope: crate::session_key::DmScope) -> Self {
        self.dm_scope = scope;
        self
    }

    pub fn with_identity_links(mut self, links: Arc<crate::session_key::IdentityLinks>) -> Self {
        self.identity_links = Some(links);
        self
    }

    pub fn into_router(self) -> Router {
        let cors = self.build_cors_layer();
        let mut hc = HealthChecker::new();
        hc.register(Box::new(SystemHealthCheck::new()));

        // Build cron scheduler if cron_service + llm_provider are available
        let (cron_scheduler, cron_events, cron_run_log) = self.build_cron_scheduler();

        let device_pairing_snapshot = load_device_pairing_snapshot();
        let device_pending_index =
            build_device_pending_index(&device_pairing_snapshot.pending_by_id);
        let voicewake_triggers = load_voicewake_triggers_from_config(self.full_config.as_ref());
        let (event_tx, _) = tokio::sync::broadcast::channel(512);

        let state = Arc::new(HttpState {
            auth_state: self.auth_state.clone(),
            gateway_server: self.gateway_server.clone(),
            _gateway: self.gateway.clone(),
            llm_provider: self.llm_provider.clone(),
            hook_pipeline: self.hook_pipeline.clone(),
            channel_manager: self.channel_manager.clone(),
            tool_registry: self.tool_registry.clone(),
            skill_registry: self.skill_registry.clone(),
            approval_gate: self.approval_gate.clone(),
            plugin_registrations: self.plugin_registrations.clone(),
            cron_service: self.cron_service.clone(),
            cron_scheduler: cron_scheduler.clone(),
            cron_events,
            cron_run_log,
            memory_manager: self.memory_manager.clone(),
            workspace: self.workspace.clone(),
            metrics: Arc::new(metrics::AppMetrics::new()),
            health_checker: Arc::new(hc),
            full_config: self.full_config.clone(),
            config_path: self.config_path.clone(),
            echo_tracker: Arc::new(tokio::sync::Mutex::new(EchoTracker::default())),
            group_activation: GroupActivation::default(),
            dm_scope: self.dm_scope,
            identity_links: self.identity_links.clone(),
            needs_hatching: self.needs_hatching.clone(),
            pipeline_config: Arc::new(crate::pipeline::PipelineConfig::default()),
            flush_tracker: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            session_usage_tokens: Arc::new(std::sync::Mutex::new(HashMap::new())),
            session_turn_counts: Arc::new(std::sync::Mutex::new(HashMap::new())),
            session_rate_limiter: Arc::new(oclaw_acp::SessionRateLimiter::default_session_limiter()),
            session_queues: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            session_run_locks: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            auto_capture_config: Arc::new(AutoCaptureConfig::default()),
            auto_capture_counts: Arc::new(std::sync::Mutex::new(HashMap::new())),
            skill_overrides: Arc::new(RwLock::new(HashMap::new())),
            wizard_sessions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            tts_runtime: Arc::new(RwLock::new(TtsRuntimeState::default())),
            exec_approvals_snapshot: Arc::new(RwLock::new(ExecApprovalsFileSnapshot::default())),
            node_pairing_store: Arc::new(tokio::sync::Mutex::new(PairingStore::default())),
            node_pairs: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            node_pair_index: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            node_exec_approvals: Arc::new(RwLock::new(HashMap::new())),
            node_invocations: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            node_connected: Arc::new(tokio::sync::Mutex::new(HashSet::new())),
            agent_runs: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            agent_idempotency: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            agent_idempotency_gates: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            chat_runs: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            chat_abort_handles: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            chat_dedupe: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            chat_idempotency_gates: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            send_dedupe: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            send_idempotency_gates: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            voicewake_triggers: Arc::new(RwLock::new(voicewake_triggers)),
            talk_mode: Arc::new(RwLock::new(TalkModeRuntimeState::default())),
            heartbeats_enabled: Arc::new(RwLock::new(true)),
            last_heartbeat_event: Arc::new(RwLock::new(None)),
            usage_snapshot: Arc::new(RwLock::new(GatewayUsageSnapshot {
                updated_at: now_epoch_ms(),
                totals: GatewayUsageTotals::default(),
            })),
            event_tx,
            device_pair_pending: Arc::new(tokio::sync::Mutex::new(
                device_pairing_snapshot.pending_by_id,
            )),
            device_pair_pending_index: Arc::new(tokio::sync::Mutex::new(device_pending_index)),
            device_paired: Arc::new(tokio::sync::Mutex::new(
                device_pairing_snapshot.paired_by_device_id,
            )),
        });

        // Start the cron scheduler background loop
        if let Some(ref sched) = cron_scheduler {
            sched.clone().start();
            info!("Cron scheduler started");
        }

        // Webhook routes skip auth middleware (they use their own verification)
        let webhook_routes = Router::new()
            .route("/webhooks/telegram", post(webhooks::telegram_webhook))
            .route("/webhooks/slack", post(webhooks::slack_webhook))
            .route("/webhooks/discord", post(webhooks::discord_webhook))
            .route("/webhooks/feishu", post(webhooks::feishu_webhook))
            .route("/webhooks/whatsapp", post(webhooks::whatsapp_webhook))
            .route("/webhooks/{channel}", post(webhooks::generic_webhook))
            .with_state(state.clone());

        // Config UI routes skip auth (local admin use)
        let config_ui_routes = Router::new()
            .route("/api/config/full", get(routes::config_full_get_handler))
            .route("/api/config/full", put(routes::config_full_put_handler))
            .route("/ui/config", get(routes::config_ui_handler))
            .route("/ui/chat", get(routes::webchat_ui_handler))
            .route("/ui/canvas", get(routes::canvas_ui_handler))
            .with_state(state.clone());

        // Webchat WebSocket routes (skip auth, local use)
        let webchat_routes = crate::webchat::create_webchat_router(state.clone());

        let mut router = Router::new()
            .route("/health", get(health_handler))
            .route("/ready", get(readiness_handler))
            .route(
                "/v1/chat/completions",
                post(routes::chat_completions_handler),
            )
            .route("/v1/responses", post(routes::responses_handler))
            .route("/ws", get(ws_handler))
            .route("/agent/status", get(routes::agent_status_handler))
            .route(
                "/transcript/{session_key}",
                get(routes::transcript_history_handler),
            )
            .route("/sessions", get(routes::sessions_list_handler))
            .route("/sessions/{key}", delete(routes::sessions_delete_handler))
            .route("/config", get(routes::config_get_handler))
            .route("/config/reload", post(routes::config_reload_handler))
            .route("/models", get(routes::models_list_handler))
            .route("/cron/jobs", get(routes::cron_list_handler))
            .route("/cron/jobs", post(routes::cron_create_handler))
            .route("/cron/jobs/{id}", delete(routes::cron_delete_handler))
            .route(
                "/cron/jobs/{id}/trigger",
                post(routes::cron_trigger_handler),
            )
            .route("/cron/jobs/{id}/logs", get(routes::cron_logs_handler))
            .route("/cron/status", get(routes::cron_status_handler))
            .route("/api/approval/pending", get(approval_pending_handler))
            .route("/api/approval/{id}/approve", post(approval_approve_handler))
            .route("/api/approval/{id}/reject", post(approval_reject_handler))
            .route("/metrics", get(metrics::metrics_handler))
            .route("/", any(root_handler))
            .layer(axum_mw::from_fn(middleware::security_headers_middleware))
            .layer(axum_mw::from_fn_with_state(
                state.clone(),
                middleware::hook_middleware,
            ))
            .layer(axum_mw::from_fn_with_state(
                state.clone(),
                middleware::auth_middleware,
            ))
            .layer(axum_mw::from_fn_with_state(
                state.clone(),
                middleware::request_id_middleware,
            ))
            .layer(cors)
            .layer(TraceLayer::new_for_http())
            .layer(rate_limit::RateLimitLayer::new(100, 60))
            .layer(TimeoutLayer::with_status_code(
                StatusCode::GATEWAY_TIMEOUT,
                std::time::Duration::from_secs(30),
            ))
            .layer(ServiceBuilder::new().layer(DefaultBodyLimit::max(10 * 1024 * 1024)))
            .with_state(state.clone())
            .merge(webhook_routes)
            .merge(config_ui_routes)
            .nest("/webchat", webchat_routes);

        // Plugin HTTP routes
        if let Some(ref _regs) = self.plugin_registrations {
            let plugin_routes = Router::new()
                .route("/plugins/{plugin_id}/{*rest}", any(plugin_route_handler))
                .with_state(state);
            router = router.merge(plugin_routes);
        }

        if let Some(ref static_path) = self.static_files_path
            && static_path.exists()
        {
            let serve_dir = ServeDir::new(static_path);
            router = Router::new()
                .nest_service("/static", serve_dir.clone())
                .fallback_service(serve_dir)
                .merge(router);
        }

        router
    }

    pub async fn start(self) -> GatewayResult<()> {
        let addr = self.addr;
        let auth_state = self.auth_state.clone();
        let session_mgr = self.gateway_server.session_manager.clone();

        // Extract heartbeat fields before into_router() consumes self
        let hb_provider = self.llm_provider.clone();
        let hb_channel_mgr = self.channel_manager.clone();
        let hb_workspace = self.workspace.clone();

        let router = self.into_router();

        let listener = {
            let sock = tokio::net::TcpSocket::new_v4().map_err(|e| {
                GatewayError::ServerError(format!("Failed to create socket: {}", e))
            })?;
            sock.set_reuseaddr(true).ok();
            #[cfg(windows)]
            {
                use std::os::windows::io::AsRawSocket;
                unsafe {
                    unsafe extern "system" {
                        fn SetHandleInformation(h: usize, mask: u32, flags: u32) -> i32;
                    }
                    SetHandleInformation(sock.as_raw_socket() as usize, 1, 0);
                }
            }
            sock.bind(addr).map_err(|e| {
                GatewayError::ServerError(format!("Failed to bind to {}: {}", addr, e))
            })?;
            sock.listen(1024)
                .map_err(|e| GatewayError::ServerError(format!("Failed to listen: {}", e)))?
        };

        info!("HTTP server listening on {}", addr);

        // Periodic cleanup every 5 minutes: expired tokens + stale sessions
        let (cleanup_stop_tx, mut cleanup_stop_rx) = tokio::sync::oneshot::channel::<()>();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(300));
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        auth_state.read().await.cleanup_expired_tokens().await;
                        let removed = session_mgr.read().await.cleanup_stale(24 * 60 * 60 * 1000).unwrap_or(0);
                        if removed > 0 {
                            info!("Cleaned up {} stale sessions", removed);
                        }
                    }
                    _ = &mut cleanup_stop_rx => break,
                }
            }
        });

        // Heartbeat background loop (if workspace + LLM provider available)
        let (hb_stop_tx, mut hb_stop_rx) = tokio::sync::oneshot::channel::<()>();
        if let (Some(provider), Some(workspace)) = (hb_provider, hb_workspace) {
            let delivery = Arc::new(crate::heartbeat_runner::GatewayHeartbeatDelivery::new(
                provider,
                hb_channel_mgr,
            ));
            let hb_config = oclaw_workspace_core::heartbeat::HeartbeatConfig::default();
            let mut runner =
                crate::heartbeat_runner::HeartbeatRunner::new(hb_config, workspace, delivery);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            if !GLOBAL_HEARTBEATS_ENABLED.load(Ordering::Relaxed) {
                                continue;
                            }

                            let pending_wakes = match take_pending_heartbeat_wakes() {
                                Ok(v) => v,
                                Err(e) => {
                                    tracing::warn!("Heartbeat wake queue error: {}", e);
                                    Vec::new()
                                }
                            };

                            if !pending_wakes.is_empty() {
                                for wake in pending_wakes {
                                    let session_key = wake
                                        .session_key
                                        .as_deref()
                                        .unwrap_or(MAIN_SYSTEM_SESSION_KEY);
                                    let events = match drain_system_event_entries(session_key) {
                                        Ok(v) => v
                                            .iter()
                                            .map(|entry| entry.text.trim().to_string())
                                            .filter(|entry| !entry.is_empty())
                                            .collect::<Vec<String>>(),
                                        Err(e) => {
                                            tracing::warn!("System event drain error: {}", e);
                                            Vec::new()
                                        }
                                    };
                                    if let Err(e) = runner
                                        .tick_with_options(true, Some(&wake.reason), &events)
                                        .await
                                    {
                                        tracing::warn!("Heartbeat wake tick error: {}", e);
                                        let _ = queue_pending_heartbeat_wake(
                                            Some("retry"),
                                            wake.agent_id.as_deref(),
                                            wake.session_key.as_deref(),
                                        );
                                    }
                                }
                                continue;
                            }

                            let now_ms = chrono::Utc::now().timestamp_millis() as u64;
                            if runner.should_tick(now_ms) {
                                let events = match drain_system_event_entries(MAIN_SYSTEM_SESSION_KEY) {
                                    Ok(v) => v
                                        .iter()
                                        .map(|entry| entry.text.trim().to_string())
                                        .filter(|entry| !entry.is_empty())
                                        .collect::<Vec<String>>(),
                                    Err(e) => {
                                        tracing::warn!("System event drain error: {}", e);
                                        Vec::new()
                                    }
                                };
                                if let Err(e) = runner
                                    .tick_with_options(false, Some("interval"), &events)
                                    .await
                                {
                                    tracing::warn!("Heartbeat tick error: {}", e);
                                }
                            }
                        }
                        _ = &mut hb_stop_rx => break,
                    }
                }
            });
            info!("Heartbeat background loop started");
        }

        let shutdown = async {
            tokio::signal::ctrl_c().await.ok();
            info!("Shutdown signal received, draining connections...");
        };

        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown)
            .await
            .map_err(|e: std::io::Error| {
                GatewayError::ServerError(format!("HTTP server error: {}", e))
            })?;

        drop(cleanup_stop_tx);
        drop(hb_stop_tx);
        info!("HTTP server stopped");
        Ok(())
    }

    fn build_cors_layer(&self) -> CorsLayer {
        let mut cors = CorsLayer::new()
            .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
            .allow_headers(Any)
            .allow_origin(Any);

        if let Some(control_ui) = &self.gateway.control_ui
            && let Some(origins) = &control_ui.allowed_origins
            && !origins.is_empty()
        {
            let origins: Vec<axum::http::HeaderValue> =
                origins.iter().filter_map(|o| o.parse().ok()).collect();
            if !origins.is_empty() {
                cors = CorsLayer::new()
                    .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE])
                    .allow_headers(Any)
                    .allow_origin(origins);
            }
        }

        cors
    }

    fn build_cron_scheduler(
        &self,
    ) -> (
        Option<Arc<oclaw_cron_core::scheduler::CronScheduler>>,
        Option<oclaw_cron_core::events::CronEventSender>,
        Option<Arc<oclaw_cron_core::run_log::RunLog>>,
    ) {
        let (Some(cron_svc), Some(provider)) = (&self.cron_service, &self.llm_provider) else {
            return (None, None, None);
        };

        let mut exec = cron_executor::GatewayCronExecutor::new(provider.clone());
        if let Some(ref reg) = self.tool_registry {
            exec = exec.with_tool_registry(reg.clone());
        }
        if let Some(ref regs) = self.plugin_registrations {
            exec = exec.with_plugin_registrations(regs.clone());
        }
        if let Some(ref hooks) = self.hook_pipeline {
            exec = exec.with_hook_pipeline(hooks.clone());
        }
        if let Some(ref cm) = self.channel_manager {
            exec = exec.with_channel_manager(cm.clone());
        }
        exec = exec.with_session_manager(self.gateway_server.session_manager.clone());
        if let Some(ref cfg) = self.full_config {
            exec = exec.with_full_config(cfg.clone());
        }

        let log_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("oclaw")
            .join("cron")
            .join("logs");
        let run_log = Arc::new(oclaw_cron_core::run_log::RunLog::new(log_dir));
        let (events_tx, _) = oclaw_cron_core::events::event_channel();

        let scheduler = Arc::new(oclaw_cron_core::scheduler::CronScheduler::new(
            cron_svc.clone(),
            Arc::new(exec),
            run_log.clone(),
            events_tx.clone(),
        ));

        (Some(scheduler), Some(events_tx), Some(run_log))
    }
}

#[derive(Clone)]
pub struct HttpState {
    pub auth_state: Arc<RwLock<AuthState>>,
    pub gateway_server: Arc<GatewayServer>,
    pub _gateway: Arc<Gateway>,
    pub llm_provider: Option<Arc<dyn LlmProvider>>,
    pub hook_pipeline: Option<Arc<HookPipeline>>,
    pub channel_manager: Option<Arc<RwLock<ChannelManager>>>,
    pub tool_registry: Option<Arc<ToolRegistry>>,
    pub skill_registry: Option<Arc<SkillRegistry>>,
    pub approval_gate: Option<Arc<ApprovalGate>>,
    pub plugin_registrations: Option<Arc<PluginRegistrations>>,
    pub cron_service: Option<Arc<oclaw_cron_core::CronService>>,
    pub cron_scheduler: Option<Arc<oclaw_cron_core::scheduler::CronScheduler>>,
    pub cron_events: Option<oclaw_cron_core::events::CronEventSender>,
    pub cron_run_log: Option<Arc<oclaw_cron_core::run_log::RunLog>>,
    pub memory_manager: Option<Arc<MemoryManager>>,
    pub workspace: Option<Arc<oclaw_workspace_core::files::Workspace>>,
    pub metrics: Arc<metrics::AppMetrics>,
    pub health_checker: Arc<HealthChecker>,
    pub full_config: Option<Arc<RwLock<oclaw_config::settings::Config>>>,
    pub config_path: Option<PathBuf>,
    pub echo_tracker: Arc<tokio::sync::Mutex<EchoTracker>>,
    pub group_activation: GroupActivation,
    pub dm_scope: crate::session_key::DmScope,
    pub identity_links: Option<Arc<crate::session_key::IdentityLinks>>,
    pub needs_hatching: Arc<std::sync::atomic::AtomicBool>,
    pub pipeline_config: Arc<crate::pipeline::PipelineConfig>,
    /// Per-session compaction count at which flush last ran. u64::MAX = never flushed.
    pub flush_tracker: Arc<std::sync::Mutex<std::collections::HashMap<String, u64>>>,
    /// Approximate cumulative LLM token usage per session.
    pub session_usage_tokens: Arc<std::sync::Mutex<std::collections::HashMap<String, u64>>>,
    /// Monotonic per-session turn counter used as flush round marker.
    pub session_turn_counts: Arc<std::sync::Mutex<std::collections::HashMap<String, u64>>>,
    pub session_rate_limiter: Arc<oclaw_acp::SessionRateLimiter>,
    pub session_queues: Arc<
        tokio::sync::Mutex<
            HashMap<String, Arc<tokio::sync::Mutex<oclaw_auto_reply::MessageQueue>>>,
        >,
    >,
    pub session_run_locks: Arc<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
    pub auto_capture_config: Arc<AutoCaptureConfig>,
    pub auto_capture_counts: Arc<std::sync::Mutex<HashMap<String, usize>>>,
    pub skill_overrides: Arc<RwLock<HashMap<String, serde_json::Value>>>,
    pub wizard_sessions: Arc<tokio::sync::Mutex<HashMap<String, WizardSessionState>>>,
    pub tts_runtime: Arc<RwLock<TtsRuntimeState>>,
    pub exec_approvals_snapshot: Arc<RwLock<ExecApprovalsFileSnapshot>>,
    pub node_pairing_store: Arc<tokio::sync::Mutex<PairingStore>>,
    pub node_pairs: Arc<tokio::sync::Mutex<HashMap<String, NodePairRecord>>>,
    pub node_pair_index: Arc<tokio::sync::Mutex<HashMap<String, String>>>,
    pub node_exec_approvals: Arc<RwLock<HashMap<String, ExecApprovalsFileSnapshot>>>,
    pub node_invocations: Arc<tokio::sync::Mutex<HashMap<String, NodeInvokeRecord>>>,
    pub node_connected: Arc<tokio::sync::Mutex<HashSet<String>>>,
    pub agent_runs: Arc<tokio::sync::Mutex<HashMap<String, AgentRunRecord>>>,
    pub agent_idempotency: Arc<tokio::sync::Mutex<HashMap<String, String>>>,
    pub agent_idempotency_gates:
        Arc<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Semaphore>>>>,
    pub chat_runs: Arc<tokio::sync::Mutex<HashMap<String, ChatRunRecord>>>,
    pub chat_abort_handles: Arc<tokio::sync::Mutex<HashMap<String, tokio::task::AbortHandle>>>,
    pub chat_dedupe: Arc<tokio::sync::Mutex<HashMap<String, serde_json::Value>>>,
    pub chat_idempotency_gates:
        Arc<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Semaphore>>>>,
    pub send_dedupe: Arc<tokio::sync::Mutex<HashMap<String, serde_json::Value>>>,
    pub send_idempotency_gates:
        Arc<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Semaphore>>>>,
    pub voicewake_triggers: Arc<RwLock<Vec<String>>>,
    pub talk_mode: Arc<RwLock<TalkModeRuntimeState>>,
    pub heartbeats_enabled: Arc<RwLock<bool>>,
    pub last_heartbeat_event: Arc<RwLock<Option<serde_json::Value>>>,
    pub usage_snapshot: Arc<RwLock<GatewayUsageSnapshot>>,
    pub event_tx: tokio::sync::broadcast::Sender<EventFrame>,
    pub device_pair_pending: Arc<tokio::sync::Mutex<HashMap<String, DevicePairPendingRecord>>>,
    pub device_pair_pending_index: Arc<tokio::sync::Mutex<HashMap<String, String>>>,
    pub device_paired: Arc<tokio::sync::Mutex<HashMap<String, DevicePairedRecord>>>,
}

impl HttpState {
    pub fn emit_event(&self, event: &str, payload: serde_json::Value) {
        let frame = MessageHandler::new_event(event, Some(payload));
        let _ = self.event_tx.send(frame);
    }

    /// Returns the compaction count at which memory flush last ran for this session.
    /// Returns u64::MAX if the session has never been flushed.
    pub fn last_flush_compaction_count(&self, session_id: &str) -> u64 {
        self.flush_tracker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(session_id)
            .copied()
            .unwrap_or(u64::MAX)
    }

    /// Record that a memory flush ran for `session_id` at `compaction_count`.
    pub fn set_last_flush_compaction_count(&self, session_id: &str, compaction_count: u64) {
        if let Ok(mut map) = self.flush_tracker.lock() {
            map.insert(session_id.to_string(), compaction_count);
        }
    }

    /// Add tokens to a session usage total and return the new cumulative value.
    pub fn add_session_usage_tokens(&self, session_id: &str, tokens: u64) -> u64 {
        let mut map = self
            .session_usage_tokens
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let next = map
            .get(session_id)
            .copied()
            .unwrap_or(0)
            .saturating_add(tokens);
        map.insert(session_id.to_string(), next);
        next
    }

    /// Increment and return per-session turn count.
    pub fn bump_session_turn_count(&self, session_id: &str) -> u64 {
        let mut map = self
            .session_turn_counts
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let next = map.get(session_id).copied().unwrap_or(0).saturating_add(1);
        map.insert(session_id.to_string(), next);
        next
    }

    /// Remove all in-memory counters for a session.
    pub fn clear_session_counters(&self, session_id: &str) {
        if let Ok(mut map) = self.flush_tracker.lock() {
            map.remove(session_id);
        }
        if let Ok(mut map) = self.session_usage_tokens.lock() {
            map.remove(session_id);
        }
        if let Ok(mut map) = self.session_turn_counts.lock() {
            map.remove(session_id);
        }
        let mut auto = self
            .auto_capture_counts
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        auto.remove(session_id);
    }

    pub async fn get_or_create_queue(
        &self,
        session_id: &str,
        mode: oclaw_auto_reply::QueueMode,
    ) -> Arc<tokio::sync::Mutex<oclaw_auto_reply::MessageQueue>> {
        let queue = {
            let mut queues = self.session_queues.lock().await;
            queues
                .entry(session_id.to_string())
                .or_insert_with(|| {
                    Arc::new(tokio::sync::Mutex::new(
                        oclaw_auto_reply::MessageQueue::new(mode),
                    ))
                })
                .clone()
        };
        {
            let mut q = queue.lock().await;
            if q.mode() != mode {
                q.set_mode(mode);
            }
        }
        queue
    }

    pub async fn get_or_create_run_lock(&self, session_id: &str) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self.session_run_locks.lock().await;
        locks
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    pub fn bump_auto_capture_count(&self, session_id: &str) -> usize {
        let mut map = self
            .auto_capture_counts
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let next = map.get(session_id).copied().unwrap_or(0).saturating_add(1);
        map.insert(session_id.to_string(), next);
        next
    }

    pub async fn get_or_create_send_idempotency_gate(
        &self,
        idempotency_key: &str,
    ) -> Arc<tokio::sync::Semaphore> {
        let mut map = self.send_idempotency_gates.lock().await;
        map.entry(idempotency_key.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(1)))
            .clone()
    }

    pub async fn get_or_create_agent_idempotency_gate(
        &self,
        idempotency_key: &str,
    ) -> Arc<tokio::sync::Semaphore> {
        let mut map = self.agent_idempotency_gates.lock().await;
        map.entry(idempotency_key.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(1)))
            .clone()
    }

    pub async fn get_or_create_chat_idempotency_gate(
        &self,
        idempotency_key: &str,
    ) -> Arc<tokio::sync::Semaphore> {
        let mut map = self.chat_idempotency_gates.lock().await;
        map.entry(idempotency_key.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Semaphore::new(1)))
            .clone()
    }
}

async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

pub async fn readiness_handler(State(state): State<Arc<HttpState>>) -> Response {
    let report = state.health_checker.check_all();
    let status = if report.is_healthy() {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(serde_json::to_value(&report).unwrap_or_default()),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
struct ApprovalRejectBody {
    reason: Option<String>,
}

async fn approval_pending_handler(State(state): State<Arc<HttpState>>) -> Response {
    let Some(ref gate) = state.approval_gate else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"enabled": false, "pending": []})),
        )
            .into_response();
    };
    let pending = gate.pending_requests().await;
    (
        StatusCode::OK,
        Json(serde_json::json!({"enabled": true, "pending": pending})),
    )
        .into_response()
}

async fn approval_approve_handler(
    State(state): State<Arc<HttpState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    let Some(ref gate) = state.approval_gate else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "approval gate not configured"})),
        )
            .into_response();
    };
    if gate.approve(&id).await {
        (
            StatusCode::OK,
            Json(serde_json::json!({"approved": true, "id": id})),
        )
            .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("request not found: {}", id)})),
        )
            .into_response()
    }
}

async fn approval_reject_handler(
    State(state): State<Arc<HttpState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    body: Option<Json<ApprovalRejectBody>>,
) -> Response {
    let Some(ref gate) = state.approval_gate else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "approval gate not configured"})),
        )
            .into_response();
    };
    let reason = body
        .map(|b| b.0.reason.unwrap_or_else(|| "rejected by user".to_string()))
        .unwrap_or_else(|| "rejected by user".to_string());
    if gate.deny(&id).await {
        (
            StatusCode::OK,
            Json(serde_json::json!({"rejected": true, "id": id, "reason": reason})),
        )
            .into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": format!("request not found: {}", id)})),
        )
            .into_response()
    }
}

async fn plugin_route_handler(
    State(state): State<Arc<HttpState>>,
    axum::extract::Path((plugin_id, rest)): axum::extract::Path<(String, String)>,
    method: axum::http::Method,
    body: axum::body::Bytes,
) -> Response {
    let Some(ref regs) = state.plugin_registrations else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "no plugins loaded"})),
        )
            .into_response();
    };
    let routes = regs.http_routes.read().await;
    let path = format!("/{}", rest);
    let route = routes
        .iter()
        .find(|r| r.plugin_id == plugin_id && path.starts_with(&r.path));
    match route {
        Some(r) => match r.handler.handle(method.as_str(), &body).await {
            Ok((status, body)) => {
                let sc = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
                (sc, body).into_response()
            }
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": e})),
            )
                .into_response(),
        },
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "plugin route not found"})),
        )
            .into_response(),
    }
}

async fn root_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "service": "oclaw-gateway",
        "version": env!("CARGO_PKG_VERSION"),
        "endpoints": [
            "/health",
            "/ready",
            "/ws",
            "/v1/chat/completions",
            "/v1/responses",
            "/agent/status",
            "/sessions",
            "/config",
            "/config/reload",
            "/models",
            "/api/config/full",
            "/ui/config",
            "/ui/chat",
            "/webchat/ws",
            "/metrics",
            "/webhooks/telegram",
            "/webhooks/slack",
            "/webhooks/discord",
            "/webhooks/{channel}"
        ]
    }))
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<HttpState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
) -> Response {
    if state
        .session_rate_limiter
        .check_with_key(&addr.ip().to_string())
        .is_err()
    {
        return (StatusCode::TOO_MANY_REQUESTS, "Too many session creations").into_response();
    }

    let auth_state = state.auth_state.read().await;
    let is_allowed = auth_state.should_allow_connection(&addr.ip()).await;
    drop(auth_state);

    if !is_allowed {
        return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
    }

    let state_clone = state.clone();

    ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_ws(socket, addr, state_clone).await {
            error!("WebSocket error: {}", e);
        }
    })
}

async fn handle_ws(
    socket: axum::extract::ws::WebSocket,
    addr: SocketAddr,
    state: Arc<HttpState>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (mut write, mut read) = socket.split();
    let mut event_rx = state.event_tx.subscribe();

    let hello = HelloOk {
        frame_type: oclaw_protocol::frames::HelloOkType::HelloOk,
        protocol: 1,
        server: ServerInfo {
            version: "0.1.0".to_string(),
            commit: None,
            host: None,
            conn_id: uuid::Uuid::new_v4().to_string(),
        },
        features: ServerFeatures {
            methods: vec![
                "session.create".to_string(),
                "session.list".to_string(),
                "session.get".to_string(),
                "session.delete".to_string(),
                "chat.send".to_string(),
                "chat.history".to_string(),
                "send".to_string(),
                "agent".to_string(),
                "agent.identity.get".to_string(),
                "agent.wait".to_string(),
                "taskgraph.run".to_string(),
                "config.get".to_string(),
                "config.set".to_string(),
                "models.list".to_string(),
                "channels.status".to_string(),
                "cron.list".to_string(),
                "cron.create".to_string(),
                "cron.delete".to_string(),
                "cron.trigger".to_string(),
                "cron.logs".to_string(),
                "cron.status".to_string(),
                "last-heartbeat".to_string(),
                "set-heartbeats".to_string(),
                "wake".to_string(),
                "system-presence".to_string(),
                "system-event".to_string(),
                "talk.config".to_string(),
                "talk.mode".to_string(),
                "voicewake.get".to_string(),
                "voicewake.set".to_string(),
                "update.run".to_string(),
                "browser.request".to_string(),
                "device.pair.list".to_string(),
                "device.pair.approve".to_string(),
                "device.pair.reject".to_string(),
                "device.pair.remove".to_string(),
                "device.token.rotate".to_string(),
                "device.token.revoke".to_string(),
            ],
            events: vec![
                "connect.challenge".to_string(),
                "agent".to_string(),
                "chat".to_string(),
                "presence".to_string(),
                "tick".to_string(),
                "talk.mode".to_string(),
                "shutdown".to_string(),
                "health".to_string(),
                "heartbeat".to_string(),
                "cron".to_string(),
                "node.pair.requested".to_string(),
                "node.pair.resolved".to_string(),
                "node.invoke.request".to_string(),
                "device.pair.requested".to_string(),
                "device.pair.resolved".to_string(),
                "voicewake.changed".to_string(),
                "exec.approval.requested".to_string(),
                "exec.approval.resolved".to_string(),
                "session.start".to_string(),
                "session.end".to_string(),
            ],
        },
        snapshot: Snapshot {
            presence: vec![],
            health: serde_json::json!({}),
            state_version: StateVersion {
                presence: 0,
                health: 0,
            },
            uptime_ms: 0,
            config_path: None,
            state_dir: None,
            session_defaults: None,
            auth_mode: Some(AuthMode::None),
            update_available: None,
        },
        canvas_host_url: Some("/ui/canvas".to_string()),
        auth: None,
        policy: Policy {
            max_payload: 1024 * 1024,
            max_buffered_bytes: 1024 * 1024,
            tick_interval_ms: 5000,
        },
    };

    let hello_json = serde_json::to_vec(&hello)?;
    write
        .send(axum::extract::ws::Message::Binary(hello_json.into()))
        .await?;

    loop {
        tokio::select! {
            msg = read.next() => {
                let frame_bytes = match msg {
                    Some(Ok(axum::extract::ws::Message::Binary(data))) => Some(data.to_vec()),
                    Some(Ok(axum::extract::ws::Message::Text(text))) => Some(text.as_bytes().to_vec()),
                    Some(Ok(axum::extract::ws::Message::Close(_))) => {
                        info!("Client {} disconnected", addr);
                        break;
                    }
                    Some(Err(e)) => {
                        error!("WebSocket error: {}", e);
                        break;
                    }
                    None => break,
                    _ => None,
                };

                if let Some(data) = frame_bytes {
                    let frame: GatewayFrame = serde_json::from_slice(&data)?;
                    if let Some(resp) = handle_frame(frame, &state).await? {
                        let json = serde_json::to_vec(&resp)?;
                        write.send(axum::extract::ws::Message::Binary(json.into())).await?;
                    }
                }
            }
            evt = event_rx.recv() => {
                match evt {
                    Ok(event) => {
                        let json = serde_json::to_vec(&event)?;
                        write.send(axum::extract::ws::Message::Binary(json.into())).await?;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        error!("WS client {} lagged, skipped {} events", addr, skipped);
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        }
    }

    Ok(())
}

async fn handle_frame(
    frame: GatewayFrame,
    state: &Arc<HttpState>,
) -> Result<Option<oclaw_protocol::frames::ResponseFrame>, Box<dyn std::error::Error + Send + Sync>>
{
    match frame {
        GatewayFrame::Request(req) => {
            let response = dispatch_rpc(&req.id, &req.method, req.params, state).await;
            Ok(Some(response))
        }
        _ => Ok(None),
    }
}

/// Unified RPC dispatch — maps method strings to handler logic.
async fn dispatch_rpc(
    id: &str,
    method: &str,
    params: Option<serde_json::Value>,
    state: &Arc<HttpState>,
) -> oclaw_protocol::frames::ResponseFrame {
    let p = params.unwrap_or(serde_json::Value::Null);
    let result = match method {
        // ── Session RPCs ──
        "session.create" => rpc_session_create(&p, state).await,
        "session.list" => rpc_session_list(state).await,
        "session.get" => rpc_session_get(&p, state).await,
        "session.delete" => rpc_session_delete(&p, state).await,
        "session.preview" => rpc_session_preview(&p, state).await,
        "session.resolve" => rpc_session_resolve(&p, state).await,
        "session.patch" => rpc_session_patch(&p, state).await,
        "session.reset" => rpc_session_reset(&p, state).await,
        "session.compact" => rpc_session_compact(&p, state).await,
        "session.history" => rpc_session_history(&p, state).await,
        "session.send" => rpc_session_send(&p, state).await,
        // Node-compatible aliases
        "sessions.list" => rpc_session_list(state).await,
        "sessions.delete" => rpc_session_delete(&p, state).await,
        "sessions.preview" => rpc_session_preview(&p, state).await,
        "sessions.patch" => rpc_session_patch(&p, state).await,
        "sessions.reset" => rpc_session_reset(&p, state).await,
        "sessions.compact" => rpc_session_compact(&p, state).await,

        // ── Chat RPCs ──
        "chat.send" => rpc_chat_send(&p, state).await,
        "chat.history" => rpc_chat_history(&p, state).await,
        "chat.abort" => rpc_chat_abort(&p, state).await,
        "send" => rpc_send(&p, state).await,
        "agent" => rpc_agent(&p, state).await,
        "agent.identity.get" => rpc_agent_identity_get(&p, state).await,
        "agent.wait" => rpc_agent_wait(&p, state).await,
        "taskgraph.run" | "agent.taskgraph.run" => rpc_taskgraph_run(&p, state).await,

        // ── Agents RPCs ──
        "agents.list" => rpc_agents_list(state).await,
        "agents.create" => rpc_agents_create(&p, state).await,
        "agents.update" => rpc_agents_update(&p, state).await,
        "agents.delete" => rpc_agents_delete(&p, state).await,
        "agents.files.list" => rpc_agents_files_list(&p, state).await,
        "agents.files.get" => rpc_agents_files_get(&p, state).await,
        "agents.files.set" => rpc_agents_files_set(&p, state).await,

        // ── System RPCs ──
        "system.health" => rpc_system_health(state).await,
        "system.status" => rpc_system_status(state).await,
        "system.heartbeat" => rpc_system_heartbeat(state).await,
        "system.presence" => rpc_system_presence(state).await,
        // Node-compatible aliases
        "health" => rpc_system_health(state).await,
        "status" => rpc_system_status(state).await,
        "system-presence" => rpc_system_presence(state).await,
        "system-event" => rpc_system_event(&p, state).await,
        "last-heartbeat" => rpc_last_heartbeat(state).await,
        "set-heartbeats" => rpc_set_heartbeats(&p, state).await,
        "wake" => rpc_wake(&p, state).await,
        "usage.tokens" => rpc_usage_tokens(&p, state).await,
        "usage.status" => rpc_usage_status(&p, state).await,
        "usage.cost" => rpc_usage_cost(&p, state).await,

        // ── Config RPCs ──
        "config.get" => rpc_config_get(state).await,
        "config.set" => rpc_config_set(&p, state).await,
        "config.patch" => rpc_config_patch(&p, state).await,
        "config.apply" => rpc_config_apply(&p, state).await,
        "config.schema" => rpc_config_schema(state).await,

        // ── Models RPCs ──
        "models.list" => rpc_models_list(state).await,

        // ── Channel RPCs ──
        "channels.status" => rpc_channels_status(state).await,
        "channels.logout" => rpc_channels_logout(&p, state).await,

        // ── Cron RPCs ──
        "cron.list" => rpc_cron_list(state).await,
        "cron.create" => rpc_cron_create(&p, state).await,
        "cron.delete" => rpc_cron_delete(&p, state).await,
        "cron.trigger" => rpc_cron_trigger(&p, state).await,
        "cron.logs" => rpc_cron_logs(&p, state).await,
        "cron.status" => rpc_cron_status(state).await,
        // Node-compatible aliases
        "cron.add" => rpc_cron_create(&p, state).await,
        "cron.update" => rpc_cron_update(&p, state).await,
        "cron.remove" => rpc_cron_delete(&p, state).await,
        "cron.run" => rpc_cron_run(&p, state).await,
        "cron.runs" => rpc_cron_logs(&p, state).await,

        // ── Skills RPCs ──
        "skills.status" => rpc_skills_status(&p, state).await,
        "skills.install" => rpc_skills_install(&p, state).await,
        "skills.update" => rpc_skills_update(&p, state).await,
        "skills.bins" => rpc_skills_bins(state).await,

        // ── Wizard RPCs ──
        "wizard.start" => rpc_wizard_start(&p, state).await,
        "wizard.next" => rpc_wizard_next(&p, state).await,
        "wizard.cancel" => rpc_wizard_cancel(&p, state).await,
        "wizard.status" => rpc_wizard_status(state).await,

        // ── Logs RPCs ──
        "logs.tail" => rpc_logs_tail(&p, state).await,

        // ── Exec Approval RPCs ──
        "exec.approvals.list" => rpc_exec_approvals_list(state).await,
        "exec.approvals.approve" => rpc_exec_approvals_approve(&p, state).await,
        "exec.approvals.reject" => rpc_exec_approvals_reject(&p, state).await,
        "exec.approvals.get" => rpc_exec_approvals_get(state).await,
        "exec.approvals.set" => rpc_exec_approvals_set(&p, state).await,
        "exec.approval.request" => rpc_exec_approval_request(&p, state).await,
        "exec.approval.waitDecision" => rpc_exec_approval_wait_decision(&p, state).await,
        "exec.approval.resolve" => rpc_exec_approval_resolve(&p, state).await,
        "exec.approvals.node.get" => rpc_exec_approvals_node_get(&p, state).await,
        "exec.approvals.node.set" => rpc_exec_approvals_node_set(&p, state).await,

        // ── TTS RPCs ──
        "tts.status" => rpc_tts_status(state).await,
        "tts.enable" => rpc_tts_enable(state).await,
        "tts.disable" => rpc_tts_disable(state).await,
        "tts.convert" => rpc_tts_convert(&p, state).await,
        "tts.setProvider" => rpc_tts_set_provider(&p, state).await,
        "tts.providers" => rpc_tts_providers(state).await,
        "talk.config" => rpc_talk_config(&p, state).await,
        "talk.mode" => rpc_talk_mode(&p, state).await,
        "voicewake.get" => rpc_voicewake_get(state).await,
        "voicewake.set" => rpc_voicewake_set(&p, state).await,
        "update.run" => rpc_update_run(&p, state).await,
        "browser.request" => rpc_browser_request(&p, state).await,

        // ── Node RPCs ──
        "node.list" => rpc_node_list(state).await,
        "node.describe" => rpc_node_describe(&p, state).await,
        "node.pair.request" => rpc_node_pair_request(&p, state).await,
        "node.pair.list" => rpc_node_pair_list(&p, state).await,
        "node.pair.approve" => rpc_node_pair_approve(&p, state).await,
        "node.pair.reject" => rpc_node_pair_reject(&p, state).await,
        "node.pair.verify" => rpc_node_pair_verify(&p, state).await,
        "node.rename" => rpc_node_rename(&p, state).await,
        "node.invoke" => rpc_node_invoke(&p, state).await,
        "node.invoke.result" => rpc_node_invoke_result(&p, state).await,
        "node.event" => rpc_node_event(&p, state).await,
        "device.pair.list" => rpc_device_pair_list(&p, state).await,
        "device.pair.approve" => rpc_device_pair_approve(&p, state).await,
        "device.pair.reject" => rpc_device_pair_reject(&p, state).await,
        "device.pair.remove" => rpc_device_pair_remove(&p, state).await,
        "device.token.rotate" => rpc_device_token_rotate(&p, state).await,
        "device.token.revoke" => rpc_device_token_revoke(&p, state).await,

        _ => Err(rpc_error(
            "METHOD_NOT_FOUND",
            &format!("Unknown method: {}", method),
        )),
    };

    match result {
        Ok(val) => MessageHandler::new_response(id, true, Some(val), None),
        Err(err) => MessageHandler::new_response(id, false, None, Some(err)),
    }
}

fn rpc_error(code: &str, message: &str) -> ErrorDetails {
    ErrorDetails {
        code: code.to_string(),
        message: message.to_string(),
        details: None,
        retryable: Some(false),
        retry_after_ms: None,
    }
}

type RpcResult = Result<serde_json::Value, ErrorDetails>;

// ── Session RPCs ────────────────────────────────────────────────────────

async fn rpc_session_create(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let manager = state.gateway_server.session_manager.read().await;
    let key = p["key"].as_str().unwrap_or("default");
    let agent_id = p["agentId"].as_str().unwrap_or("default");
    let session = manager
        .create_session(key, agent_id)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    serde_json::to_value(&session).map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_session_list(state: &HttpState) -> RpcResult {
    let manager = state.gateway_server.session_manager.read().await;
    let sessions = manager
        .list_sessions()
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    serde_json::to_value(&sessions).map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_session_get(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let key = p["key"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'key' parameter"))?;
    let manager = state.gateway_server.session_manager.read().await;
    let session = manager
        .get_session(key)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    serde_json::to_value(&session).map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_session_delete(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let key = p["key"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'key' parameter"))?;
    let manager = state.gateway_server.session_manager.read().await;
    manager
        .remove_session(key)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    state.clear_session_counters(key);
    Ok(serde_json::json!({"deleted": true}))
}

async fn rpc_session_preview(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let manager = state.gateway_server.session_manager.read().await;
    let limit = p["limit"].as_u64().unwrap_or(20) as usize;
    let sessions = manager
        .list_sessions()
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    let previews: Vec<serde_json::Value> = sessions
        .iter()
        .take(limit)
        .map(|s| {
            serde_json::json!({
                "key": s.key,
                "agent_id": s.agent_id,
                "created_at": s.created_at,
                "updated_at": s.updated_at,
                "message_count": s.message_count,
            })
        })
        .collect();
    Ok(serde_json::json!({"sessions": previews}))
}

async fn rpc_session_resolve(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let key = p["key"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'key'"))?;
    let manager = state.gateway_server.session_manager.read().await;
    let session = manager
        .get_session(key)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    match session {
        Some(s) => {
            let val =
                serde_json::to_value(&s).map_err(|e| rpc_error("INTERNAL", &e.to_string()))?;
            Ok(serde_json::json!({"found": true, "session": val}))
        }
        None => Ok(serde_json::json!({"found": false, "session": null})),
    }
}

async fn rpc_session_patch(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let key = p["key"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'key'"))?;
    let manager = state.gateway_server.session_manager.read().await;
    // Verify session exists
    manager
        .get_session(key)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?
        .ok_or_else(|| rpc_error("NOT_FOUND", "Session not found"))?;
    if let Some(agent_id) = p["agentId"].as_str() {
        manager
            .update_agent_id(key, agent_id)
            .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    } else {
        manager
            .touch_session(key)
            .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    }
    Ok(serde_json::json!({"patched": true}))
}

async fn rpc_session_reset(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let key = p["key"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'key'"))?;
    let manager = state.gateway_server.session_manager.read().await;
    manager
        .get_session(key)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?
        .ok_or_else(|| rpc_error("NOT_FOUND", "Session not found"))?;
    manager
        .clear_messages(key)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    Ok(serde_json::json!({"reset": true}))
}

async fn rpc_session_compact(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let key = p["key"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'key'"))?;
    let max_messages = p["maxMessages"].as_u64().unwrap_or(50) as usize;
    let manager = state.gateway_server.session_manager.read().await;
    manager
        .get_session(key)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?
        .ok_or_else(|| rpc_error("NOT_FOUND", "Session not found"))?;
    let (original_count, new_count) = manager
        .compact_messages(key, max_messages)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    Ok(serde_json::json!({
        "compacted": true,
        "original_count": original_count,
        "new_count": new_count,
    }))
}

async fn rpc_session_history(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let key = p["key"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'key'"))?;
    let limit = p["limit"].as_u64().unwrap_or(50) as usize;
    let manager = state.gateway_server.session_manager.read().await;
    let session = manager
        .get_session(key)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?
        .ok_or_else(|| rpc_error("NOT_FOUND", "Session not found"))?;
    let messages = manager
        .get_messages(key, limit)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    let val = serde_json::to_value(&messages).map_err(|e| rpc_error("INTERNAL", &e.to_string()))?;
    Ok(serde_json::json!({"messages": val, "total": session.message_count}))
}

async fn rpc_session_send(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let key = p["key"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'key'"))?;
    let text = p["text"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'text'"))?;
    let role = p["role"].as_str().unwrap_or("user");
    let manager = state.gateway_server.session_manager.read().await;
    manager
        .get_session(key)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?
        .ok_or_else(|| rpc_error("NOT_FOUND", "Session not found"))?;
    manager
        .add_message(key, role, text)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    let session = manager
        .get_session(key)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?
        .ok_or_else(|| rpc_error("NOT_FOUND", "Session not found"))?;
    Ok(serde_json::json!({"sent": true, "message_count": session.message_count}))
}

// ── Chat RPCs ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct ChatReplyOutput {
    reply: String,
    model: String,
    usage: Option<oclaw_llm_core::chat::Usage>,
}

fn estimate_usage_cost_usd(model: &str, usage: &oclaw_llm_core::chat::Usage) -> Option<(f64, f64)> {
    let catalog = oclaw_llm_core::catalog::ModelCatalog::builtin();
    let info = catalog.lookup(model)?;
    let in_price = info.cost_per_1k_input?;
    let out_price = info.cost_per_1k_output?;
    let input_cost = (usage.prompt_tokens as f64 / 1000.0) * in_price;
    let output_cost = (usage.completion_tokens as f64 / 1000.0) * out_price;
    Some((input_cost, output_cost))
}

fn apply_usage_snapshot(
    snapshot: &mut GatewayUsageSnapshot,
    model: &str,
    usage: Option<&oclaw_llm_core::chat::Usage>,
) {
    let Some(usage) = usage else {
        return;
    };
    snapshot.totals.input = snapshot
        .totals
        .input
        .saturating_add(usage.prompt_tokens as i64);
    snapshot.totals.output = snapshot
        .totals
        .output
        .saturating_add(usage.completion_tokens as i64);
    snapshot.totals.total_tokens = snapshot
        .totals
        .total_tokens
        .saturating_add(usage.total_tokens as i64);
    if let Some((input_cost, output_cost)) = estimate_usage_cost_usd(model, usage) {
        snapshot.totals.input_cost += input_cost;
        snapshot.totals.output_cost += output_cost;
        snapshot.totals.total_cost += input_cost + output_cost;
    } else {
        snapshot.totals.missing_cost_entries =
            snapshot.totals.missing_cost_entries.saturating_add(1);
    }
    snapshot.updated_at = now_epoch_ms();
}

async fn record_usage_snapshot(
    snapshot: Arc<RwLock<GatewayUsageSnapshot>>,
    model: String,
    usage: Option<oclaw_llm_core::chat::Usage>,
) {
    let mut guard = snapshot.write().await;
    apply_usage_snapshot(&mut guard, &model, usage.as_ref());
}

async fn generate_chat_reply(
    provider: &Arc<dyn LlmProvider>,
    tool_registry: Option<Arc<ToolRegistry>>,
    plugin_registrations: Option<Arc<PluginRegistrations>>,
    hook_pipeline: Option<Arc<HookPipeline>>,
    channel_manager: Option<Arc<RwLock<ChannelManager>>>,
    session_manager: Option<Arc<RwLock<SessionManager>>>,
    full_config: Option<Arc<RwLock<oclaw_config::settings::Config>>>,
    session_usage_tokens: Option<Arc<std::sync::Mutex<std::collections::HashMap<String, u64>>>>,
    usage_snapshot: Option<Arc<RwLock<GatewayUsageSnapshot>>>,
    message: &str,
    session_id: Option<&str>,
) -> Result<ChatReplyOutput, String> {
    if let Some(registry) = tool_registry {
        let mut executor =
            agent_bridge::ToolRegistryExecutor::new(registry).with_llm_provider(provider.clone());
        if let Some(regs) = plugin_registrations {
            executor = executor.with_plugin_registrations(regs);
        }
        if let Some(hooks) = hook_pipeline {
            executor = executor.with_hook_pipeline(hooks);
        }
        if let Some(cm) = channel_manager {
            executor = executor.with_channel_manager(cm);
        }
        if let Some(sm) = session_manager {
            executor = executor.with_session_manager(sm);
        }
        if let Some(cfg) = full_config {
            executor = executor.with_full_config(cfg);
        }
        if let Some(usage_tokens) = session_usage_tokens {
            executor = executor.with_session_usage_tokens(usage_tokens);
        }
        if let Some(snapshot) = usage_snapshot {
            executor = executor.with_usage_snapshot(snapshot);
        }
        if let Some(sid) = session_id {
            executor = executor.with_session_id(sid.to_string());
        }
        let reply =
            agent_bridge::agent_reply_with_session(provider, &executor, message, session_id)
                .await?;
        return Ok(ChatReplyOutput {
            reply,
            model: provider.default_model().to_string(),
            usage: None,
        });
    }

    let request = oclaw_llm_core::chat::ChatRequest {
        model: provider.default_model().to_string(),
        messages: vec![oclaw_llm_core::chat::ChatMessage {
            role: oclaw_llm_core::chat::MessageRole::User,
            content: message.to_string(),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        temperature: None,
        top_p: None,
        max_tokens: None,
        stop: None,
        tools: None,
        tool_choice: None,
        stream: None,
        response_format: None,
    };
    provider
        .chat(request)
        .await
        .map(|c| ChatReplyOutput {
            reply: c
                .choices
                .first()
                .map(|ch| ch.message.content.clone())
                .unwrap_or_default(),
            model: c.model,
            usage: c.usage,
        })
        .map_err(|e| e.to_string())
}

const BARE_SESSION_RESET_PROMPT: &str = "A new session was started via /new or /reset. Execute your Session Startup sequence now - read the required files before responding to the user. Then greet the user in your configured persona, if one is provided. Be yourself - use your defined voice, mannerisms, and mood. Keep it to 1-3 sentences and ask what they want to do. If the runtime model differs from default_model in the system prompt, mention the default model. Do not mention internal steps, files, tools, or reasoning.";
const CHAT_HISTORY_OVERSIZED_PLACEHOLDER: &str = "[chat.history omitted: message too large]";
const CHAT_HISTORY_TRUNCATED_PLACEHOLDER: &str =
    "[chat.history omitted: history truncated by size budget]";
const CHAT_HISTORY_MAX_TOTAL_BYTES: usize = 1_000_000;
const CHAT_HISTORY_MAX_SINGLE_MESSAGE_BYTES: usize = 128_000;
const CHAT_ATTACHMENT_MAX_BYTES: usize = 5_000_000;

fn parse_session_reset_command(message: &str) -> Option<(String, Option<String>)> {
    let trimmed = message.trim();
    if !trimmed.starts_with('/') {
        return None;
    }
    let body = &trimmed[1..];
    let mut parts = body.splitn(2, char::is_whitespace);
    let cmd = parts.next().unwrap_or("").trim().to_ascii_lowercase();
    if cmd != "new" && cmd != "reset" {
        return None;
    }
    let rest = parts.next().map(str::trim).filter(|v| !v.is_empty());
    Some((cmd, rest.map(ToString::to_string)))
}

fn session_send_policy_denied(metadata: &HashMap<String, String>) -> bool {
    let value = metadata
        .get("sendPolicy")
        .or_else(|| metadata.get("send_policy"))
        .or_else(|| metadata.get("policy.send"))
        .map(|v| v.trim().to_ascii_lowercase());
    matches!(value.as_deref(), Some("deny" | "blocked" | "disabled"))
}

fn strip_disallowed_chat_control_chars(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    for ch in message.chars() {
        let code = ch as u32;
        if code == 9 || code == 10 || code == 13 || (code >= 32 && code != 127) {
            out.push(ch);
        }
    }
    out
}

fn sanitize_chat_send_message_input(message: &str) -> Result<String, ErrorDetails> {
    if message.contains('\0') {
        return Err(rpc_error(
            "INVALID_PARAMS",
            "message must not contain null bytes",
        ));
    }
    Ok(strip_disallowed_chat_control_chars(message))
}

#[derive(Debug, Clone)]
struct NormalizedRpcAttachment {
    label: String,
    kind: Option<String>,
    mime_type: Option<String>,
    content_base64: String,
}

#[derive(Debug, Clone)]
struct ParsedImageAttachment {
    label: String,
    mime_type: String,
    data: String,
}

#[derive(Debug, Clone, Default)]
struct ParsedRpcAttachmentSet {
    normalized_count: usize,
    normalized: Vec<NormalizedRpcAttachment>,
    images: Vec<ParsedImageAttachment>,
}

fn build_message_with_image_attachments(message: &str, images: &[ParsedImageAttachment]) -> String {
    if images.is_empty() {
        return message.to_string();
    }
    let blocks = images
        .iter()
        .map(|image| {
            let safe_label = if image.label.trim().is_empty() {
                image.mime_type.clone()
            } else {
                image.label.replace(char::is_whitespace, "_")
            };
            format!(
                "![{}](data:{};base64,{})",
                safe_label, image.mime_type, image.data
            )
        })
        .collect::<Vec<_>>();
    let separator = if message.trim().is_empty() {
        ""
    } else {
        "\n\n"
    };
    format!("{}{}{}", message, separator, blocks.join("\n\n"))
}

fn normalize_chat_attachment_mime(mime: Option<&str>) -> Option<String> {
    let raw = mime?.trim();
    if raw.is_empty() {
        return None;
    }
    let cleaned = raw
        .split(';')
        .next()
        .unwrap_or(raw)
        .trim()
        .to_ascii_lowercase();
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
}

fn is_image_mime(mime: Option<&str>) -> bool {
    mime.map(|m| m.starts_with("image/")).unwrap_or(false)
}

fn extract_data_url_base64(content: &str) -> (String, Option<String>) {
    let trimmed = content.trim();
    if !trimmed.to_ascii_lowercase().starts_with("data:") {
        return (trimmed.to_string(), None);
    }
    let Some((meta, payload)) = trimmed.split_once(',') else {
        return (trimmed.to_string(), None);
    };
    if !meta.to_ascii_lowercase().contains(";base64") {
        return (trimmed.to_string(), None);
    }
    let mime = meta
        .strip_prefix("data:")
        .and_then(|v| v.split(';').next())
        .and_then(|v| normalize_chat_attachment_mime(Some(v)));
    (payload.to_string(), mime)
}

fn normalize_rpc_attachment_content(content: &serde_json::Value) -> Option<String> {
    if let Some(text) = content.as_str() {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }
        return Some(trimmed.to_string());
    }
    let arr = content.as_array()?;
    if arr.is_empty() {
        return None;
    }
    let mut bytes = Vec::with_capacity(arr.len());
    for value in arr {
        let byte = value.as_u64().and_then(|n| u8::try_from(n).ok())?;
        bytes.push(byte);
    }
    Some(base64::Engine::encode(
        &base64::engine::general_purpose::STANDARD,
        bytes,
    ))
}

fn normalize_rpc_attachments_to_chat_inputs(
    raw: Option<&serde_json::Value>,
) -> Vec<NormalizedRpcAttachment> {
    let Some(arr) = raw.and_then(|v| v.as_array()) else {
        return Vec::new();
    };

    arr.iter()
        .enumerate()
        .filter_map(|(idx, value)| {
            let obj = value.as_object()?;
            let content_base64 = normalize_rpc_attachment_content(obj.get("content")?)?;
            let file_name = obj.get("fileName").and_then(|v| v.as_str()).map(str::trim);
            let kind = obj.get("type").and_then(|v| v.as_str()).map(str::trim);
            let label = file_name
                .filter(|v| !v.is_empty())
                .map(ToString::to_string)
                .or_else(|| kind.filter(|v| !v.is_empty()).map(ToString::to_string))
                .unwrap_or_else(|| format!("attachment-{}", idx + 1));
            let mime_type = obj
                .get("mimeType")
                .and_then(|v| v.as_str())
                .and_then(|v| normalize_chat_attachment_mime(Some(v)));
            Some(NormalizedRpcAttachment {
                label,
                kind: kind.filter(|v| !v.is_empty()).map(ToString::to_string),
                mime_type,
                content_base64,
            })
        })
        .collect()
}

fn is_valid_base64_payload(value: &str) -> bool {
    if value.is_empty() || value.len() % 4 != 0 {
        return false;
    }
    let bytes = value.as_bytes();
    let mut seen_padding = false;
    let mut padding_count = 0usize;
    for (idx, ch) in bytes.iter().enumerate() {
        match ch {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' => {
                if seen_padding {
                    return false;
                }
            }
            b'=' => {
                seen_padding = true;
                padding_count = padding_count.saturating_add(1);
                if idx + 2 < bytes.len() {
                    return false;
                }
            }
            _ => return false,
        }
    }
    padding_count <= 2
}

fn estimate_base64_decoded_bytes(value: &str) -> Option<usize> {
    if value.is_empty() || value.len() % 4 != 0 {
        return None;
    }
    let padding = if value.ends_with("==") {
        2usize
    } else if value.ends_with('=') {
        1usize
    } else {
        0usize
    };
    value
        .len()
        .checked_div(4)?
        .checked_mul(3)?
        .checked_sub(padding)
}

fn sniff_known_non_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"%PDF-") {
        return Some("application/pdf");
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return Some("application/zip");
    }
    if bytes.starts_with(&[0x1F, 0x8B]) {
        return Some("application/gzip");
    }
    if bytes.starts_with(b"ID3") {
        return Some("audio/mpeg");
    }
    None
}

fn parse_rpc_attachments_for_chat(
    raw: Option<&serde_json::Value>,
    max_bytes: usize,
) -> Result<ParsedRpcAttachmentSet, ErrorDetails> {
    let normalized = normalize_rpc_attachments_to_chat_inputs(raw);
    if normalized.is_empty() {
        return Ok(ParsedRpcAttachmentSet::default());
    }

    let mut images = Vec::new();
    for attachment in &normalized {
        let (base64_payload, data_url_mime) = extract_data_url_base64(&attachment.content_base64);
        if !is_valid_base64_payload(&base64_payload) {
            return Err(rpc_error(
                "INVALID_PARAMS",
                &format!("attachment {}: invalid base64 content", attachment.label),
            ));
        }
        let size_bytes = estimate_base64_decoded_bytes(&base64_payload).ok_or_else(|| {
            rpc_error(
                "INVALID_PARAMS",
                &format!("attachment {}: invalid base64 content", attachment.label),
            )
        })?;
        if size_bytes == 0 || size_bytes > max_bytes {
            return Err(rpc_error(
                "INVALID_PARAMS",
                &format!(
                    "attachment {}: exceeds size limit ({} > {} bytes)",
                    attachment.label, size_bytes, max_bytes
                ),
            ));
        }
        let decoded =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &base64_payload)
                .map_err(|_| {
                    rpc_error(
                        "INVALID_PARAMS",
                        &format!("attachment {}: invalid base64 content", attachment.label),
                    )
                })?;

        let provided_mime = attachment.mime_type.clone().or(data_url_mime);
        let sniffed_mime = image::guess_format(&decoded)
            .ok()
            .map(|fmt| fmt.to_mime_type().to_string())
            .or_else(|| sniff_known_non_image_mime(&decoded).map(ToString::to_string));

        if let Some(sniffed) = sniffed_mime.as_deref()
            && !is_image_mime(Some(sniffed))
        {
            warn!(
                "attachment {}: detected non-image ({}), dropping",
                attachment.label, sniffed
            );
            continue;
        }
        if sniffed_mime.is_none() && !is_image_mime(provided_mime.as_deref()) {
            warn!(
                "attachment {}: unable to detect image mime type, dropping",
                attachment.label
            );
            continue;
        }

        let final_mime = if let Some(sniffed) = sniffed_mime {
            if let Some(provided) = provided_mime.as_deref()
                && !provided.eq_ignore_ascii_case(&sniffed)
            {
                warn!(
                    "attachment {}: mime mismatch ({} -> {}), using sniffed",
                    attachment.label, provided, sniffed
                );
            }
            sniffed
        } else {
            provided_mime.unwrap_or_else(|| "image/unknown".to_string())
        };

        images.push(ParsedImageAttachment {
            label: attachment.label.clone(),
            mime_type: final_mime,
            data: base64_payload,
        });
    }

    Ok(ParsedRpcAttachmentSet {
        normalized_count: normalized.len(),
        normalized,
        images,
    })
}

#[derive(Debug, Clone, Default)]
struct PreparedMediaAttachmentSet {
    attachments: Vec<MediaAttachment>,
    temp_paths: Vec<PathBuf>,
}

fn infer_mime_from_attachment_kind(kind: Option<&str>) -> Option<String> {
    let normalized = kind?.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "image" => Some("image/unknown".to_string()),
        "audio" => Some("audio/unknown".to_string()),
        "video" => Some("video/unknown".to_string()),
        _ => None,
    }
}

fn sniff_additional_media_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"OggS") {
        return Some("audio/ogg");
    }
    if bytes.starts_with(b"fLaC") {
        return Some("audio/flac");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && bytes[8..12].eq(b"WAVE") {
        return Some("audio/wav");
    }
    if bytes.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some("video/webm");
    }
    if bytes.len() >= 12 && bytes[4..8].eq(b"ftyp") {
        let brand = &bytes[8..12];
        if brand.eq(b"M4A ") || brand.eq(b"M4B ") || brand.eq(b"M4P ") {
            return Some("audio/mp4");
        }
        return Some("video/mp4");
    }
    None
}

fn sniff_media_mime(bytes: &[u8]) -> Option<String> {
    image::guess_format(bytes)
        .ok()
        .map(|fmt| fmt.to_mime_type().to_string())
        .or_else(|| sniff_known_non_image_mime(bytes).map(ToString::to_string))
        .or_else(|| sniff_additional_media_mime(bytes).map(ToString::to_string))
}

fn media_temp_file_extension(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/wav" => "wav",
        "audio/ogg" => "ogg",
        "audio/flac" => "flac",
        "audio/mp4" => "m4a",
        "video/mp4" => "mp4",
        "video/webm" => "webm",
        _ if mime.starts_with("image/") => "img",
        _ if mime.starts_with("audio/") => "aud",
        _ if mime.starts_with("video/") => "vid",
        _ => "bin",
    }
}

fn env_first_non_empty(keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        std::env::var(key)
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    })
}

fn env_csv_non_empty(keys: &[&str]) -> Option<Vec<String>> {
    env_first_non_empty(keys).map(|raw| {
        raw.split(',')
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToString::to_string)
            .collect::<Vec<_>>()
    })
}

fn env_bool(keys: &[&str]) -> Option<bool> {
    env_first_non_empty(keys).and_then(|raw| {
        let normalized = raw.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        }
    })
}

fn resolve_media_understanding_config_from_env() -> MediaConfig {
    let mut cfg = MediaConfig::default();
    if let Some(value) = env_bool(&["OCLAW_MEDIA_IMAGE_ENABLED", "OCLAWS_MEDIA_IMAGE_ENABLED"]) {
        cfg.image_enabled = value;
    }
    if let Some(value) = env_bool(&["OCLAW_MEDIA_AUDIO_ENABLED", "OCLAWS_MEDIA_AUDIO_ENABLED"]) {
        cfg.audio_enabled = value;
    }
    if let Some(value) = env_bool(&["OCLAW_MEDIA_VIDEO_ENABLED", "OCLAWS_MEDIA_VIDEO_ENABLED"]) {
        cfg.video_enabled = value;
    }
    if let Some(value) = env_first_non_empty(&[
        "OCLAW_MEDIA_DEFAULT_IMAGE_PROVIDER",
        "OCLAWS_MEDIA_DEFAULT_IMAGE_PROVIDER",
    ]) {
        cfg.default_image_provider = value;
    }
    if let Some(value) = env_first_non_empty(&[
        "OCLAW_MEDIA_DEFAULT_AUDIO_PROVIDER",
        "OCLAWS_MEDIA_DEFAULT_AUDIO_PROVIDER",
    ]) {
        cfg.default_audio_provider = value;
    }
    if let Some(value) = env_first_non_empty(&[
        "OCLAW_MEDIA_DEFAULT_VIDEO_PROVIDER",
        "OCLAWS_MEDIA_DEFAULT_VIDEO_PROVIDER",
    ]) {
        cfg.default_video_provider = value;
    }
    if let Some(values) = env_csv_non_empty(&[
        "OCLAW_MEDIA_IMAGE_FALLBACK_PROVIDERS",
        "OCLAWS_MEDIA_IMAGE_FALLBACK_PROVIDERS",
    ]) {
        cfg.image_fallback_providers = values;
    }
    if let Some(values) = env_csv_non_empty(&[
        "OCLAW_MEDIA_AUDIO_FALLBACK_PROVIDERS",
        "OCLAWS_MEDIA_AUDIO_FALLBACK_PROVIDERS",
    ]) {
        cfg.audio_fallback_providers = values;
    }
    if let Some(values) = env_csv_non_empty(&[
        "OCLAW_MEDIA_VIDEO_FALLBACK_PROVIDERS",
        "OCLAWS_MEDIA_VIDEO_FALLBACK_PROVIDERS",
    ]) {
        cfg.video_fallback_providers = values;
    }
    if let Some(raw) = env_first_non_empty(&[
        "OCLAW_MEDIA_MAX_IMAGE_SIZE_BYTES",
        "OCLAWS_MEDIA_MAX_IMAGE_SIZE_BYTES",
    ]) && let Ok(value) = raw.parse::<u64>()
    {
        cfg.max_image_size_bytes = value.max(1);
    }
    cfg
}

fn build_media_pipeline_from_env() -> (MediaPipeline, Vec<String>) {
    let mut pipeline = MediaPipeline::new(resolve_media_understanding_config_from_env());
    let mut providers = Vec::new();

    if let Some(api_key) =
        env_first_non_empty(&["OPENAI_API_KEY", "OCLAWS_PROVIDER_OPENAI_API_KEY"])
    {
        let mut provider = OpenAiMediaProvider::new(api_key);
        if let Some(base_url) = env_first_non_empty(&["OPENAI_BASE_URL"]) {
            provider = provider.with_base_url(base_url);
        }
        if let Some(model) = env_first_non_empty(&["OPENAI_VISION_MODEL", "OPENAI_IMAGE_MODEL"]) {
            provider = provider.with_vision_model(model);
        }
        pipeline.add_provider(Box::new(provider));
        providers.push("openai".to_string());
    }

    if let Some(api_key) =
        env_first_non_empty(&["ANTHROPIC_API_KEY", "OCLAWS_PROVIDER_ANTHROPIC_API_KEY"])
    {
        let mut provider = AnthropicMediaProvider::new(api_key);
        if let Some(base_url) = env_first_non_empty(&["ANTHROPIC_BASE_URL"]) {
            provider = provider.with_base_url(base_url);
        }
        if let Some(model) =
            env_first_non_empty(&["ANTHROPIC_VISION_MODEL", "ANTHROPIC_IMAGE_MODEL"])
        {
            provider = provider.with_model(model);
        }
        pipeline.add_provider(Box::new(provider));
        providers.push("anthropic".to_string());
    }

    if let Some(api_key) = env_first_non_empty(&["GOOGLE_API_KEY", "GEMINI_API_KEY"]) {
        let mut provider = GoogleMediaProvider::new(api_key);
        if let Some(base_url) = env_first_non_empty(&["GOOGLE_BASE_URL", "GEMINI_BASE_URL"]) {
            provider = provider.with_base_url(base_url);
        }
        if let Some(model) =
            env_first_non_empty(&["GOOGLE_MEDIA_MODEL", "GOOGLE_VISION_MODEL", "GEMINI_MODEL"])
        {
            provider = provider.with_model(model);
        }
        pipeline.add_provider(Box::new(provider));
        providers.push("google".to_string());
    }

    if let Some(api_key) =
        env_first_non_empty(&["DEEPGRAM_API_KEY", "OCLAWS_PROVIDER_DEEPGRAM_API_KEY"])
    {
        let mut provider = DeepgramMediaProvider::new(api_key);
        if let Some(base_url) = env_first_non_empty(&["DEEPGRAM_BASE_URL"]) {
            provider = provider.with_base_url(base_url);
        }
        pipeline.add_provider(Box::new(provider));
        providers.push("deepgram".to_string());
    }

    (pipeline, providers)
}

async fn prepare_media_attachments_for_understanding(
    normalized: &[NormalizedRpcAttachment],
    max_bytes: usize,
) -> Result<PreparedMediaAttachmentSet, ErrorDetails> {
    let mut prepared = PreparedMediaAttachmentSet::default();

    for (idx, attachment) in normalized.iter().enumerate() {
        let (base64_payload, data_url_mime) = extract_data_url_base64(&attachment.content_base64);
        if !is_valid_base64_payload(&base64_payload) {
            return Err(rpc_error(
                "INVALID_PARAMS",
                &format!("attachment {}: invalid base64 content", attachment.label),
            ));
        }
        let size_bytes = estimate_base64_decoded_bytes(&base64_payload).ok_or_else(|| {
            rpc_error(
                "INVALID_PARAMS",
                &format!("attachment {}: invalid base64 content", attachment.label),
            )
        })?;
        if size_bytes == 0 || size_bytes > max_bytes {
            return Err(rpc_error(
                "INVALID_PARAMS",
                &format!(
                    "attachment {}: exceeds size limit ({} > {} bytes)",
                    attachment.label, size_bytes, max_bytes
                ),
            ));
        }

        let decoded =
            base64::Engine::decode(&base64::engine::general_purpose::STANDARD, &base64_payload)
                .map_err(|_| {
                    rpc_error(
                        "INVALID_PARAMS",
                        &format!("attachment {}: invalid base64 content", attachment.label),
                    )
                })?;

        let provided_mime = attachment.mime_type.clone().or(data_url_mime);
        let sniffed_mime = sniff_media_mime(&decoded);
        if let (Some(provided), Some(sniffed)) = (provided_mime.as_deref(), sniffed_mime.as_deref())
            && !provided.eq_ignore_ascii_case(sniffed)
        {
            warn!(
                "attachment {}: mime mismatch ({} -> {}), using sniffed for media-understanding",
                attachment.label, provided, sniffed
            );
        }

        let final_mime = sniffed_mime
            .or(provided_mime)
            .or_else(|| infer_mime_from_attachment_kind(attachment.kind.as_deref()));
        let Some(final_mime) = final_mime else {
            warn!(
                "attachment {}: missing media mime, skipping media-understanding",
                attachment.label
            );
            continue;
        };
        if !final_mime.starts_with("image/")
            && !final_mime.starts_with("audio/")
            && !final_mime.starts_with("video/")
        {
            warn!(
                "attachment {}: unsupported media mime {}, skipping media-understanding",
                attachment.label, final_mime
            );
            continue;
        }

        let ext = media_temp_file_extension(&final_mime);
        let file_name = format!(
            "oclaw-media-{}-{}-{}-{}.{}",
            std::process::id(),
            now_epoch_ms(),
            idx + 1,
            uuid::Uuid::new_v4(),
            ext
        );
        let path = std::env::temp_dir().join(file_name);
        tokio::fs::write(&path, &decoded).await.map_err(|e| {
            rpc_error(
                "INTERNAL_ERROR",
                &format!("failed to stage attachment {}: {}", attachment.label, e),
            )
        })?;

        prepared.temp_paths.push(path.clone());
        prepared.attachments.push(MediaAttachment {
            path: path.to_string_lossy().to_string(),
            mime_type: final_mime,
            size_bytes: size_bytes as u64,
            original_name: Some(attachment.label.clone()),
        });
    }

    Ok(prepared)
}

async fn cleanup_prepared_media_files(paths: &[PathBuf]) {
    for path in paths {
        if let Err(err) = tokio::fs::remove_file(path).await
            && err.kind() != std::io::ErrorKind::NotFound
        {
            warn!(
                "failed to remove staged media file {}: {}",
                path.display(),
                err
            );
        }
    }
}

async fn collect_media_understanding_debug_payload(
    normalized: Vec<NormalizedRpcAttachment>,
    max_bytes: usize,
    timeout_ms: u64,
) -> Option<serde_json::Value> {
    if normalized.is_empty() {
        return None;
    }

    let prepared = match prepare_media_attachments_for_understanding(&normalized, max_bytes).await {
        Ok(prepared) => prepared,
        Err(err) => {
            return Some(serde_json::json!({
                "decisions": [],
                "outputs": [],
                "errors": [err.message],
            }));
        }
    };

    if prepared.attachments.is_empty() {
        let (pipeline, providers) = build_media_pipeline_from_env();
        let (_, decisions) = pipeline.process_with_decisions(&[]).await;
        cleanup_prepared_media_files(&prepared.temp_paths).await;
        return Some(serde_json::json!({
            "providers": providers,
            "decisions": decisions,
            "outputs": [],
            "errors": [],
        }));
    }

    let (pipeline, providers) = build_media_pipeline_from_env();
    let result = tokio::time::timeout(
        Duration::from_millis(timeout_ms.clamp(1_000, 120_000)),
        pipeline.process_with_decisions(&prepared.attachments),
    )
    .await;
    cleanup_prepared_media_files(&prepared.temp_paths).await;

    match result {
        Ok((results, decisions)) => {
            let outputs = results
                .iter()
                .filter_map(|entry| entry.as_ref().ok())
                .cloned()
                .collect::<Vec<_>>();
            let errors = results
                .iter()
                .filter_map(|entry| entry.as_ref().err())
                .filter(|err| {
                    !matches!(
                        err,
                        oclaw_media_understanding::providers::MediaProviderError::Unsupported(_)
                    )
                })
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            Some(serde_json::json!({
                "providers": providers,
                "decisions": decisions,
                "outputs": outputs,
                "errors": errors,
            }))
        }
        Err(_) => Some(serde_json::json!({
            "providers": providers,
            "decisions": [],
            "outputs": [],
            "errors": [format!("media-understanding timed out after {}ms", timeout_ms.clamp(1_000, 120_000))],
        })),
    }
}

async fn await_media_understanding_debug_task(
    task: Option<tokio::task::JoinHandle<Option<serde_json::Value>>>,
) -> Option<serde_json::Value> {
    let Some(task) = task else {
        return None;
    };
    match task.await {
        Ok(value) => value,
        Err(err) => {
            warn!("media-understanding task failed: {}", err);
            None
        }
    }
}

fn attach_media_debug_to_payload(
    payload: &mut serde_json::Value,
    media_debug: Option<&serde_json::Value>,
) {
    if let Some(media) = media_debug
        && let Some(obj) = payload.as_object_mut()
    {
        obj.insert("media".to_string(), media.clone());
    }
}

fn augment_message_with_media_outputs(
    message: &str,
    media_debug: Option<&serde_json::Value>,
) -> String {
    let Some(media) = media_debug else {
        return message.to_string();
    };
    let Some(outputs) = media.get("outputs").and_then(|v| v.as_array()) else {
        return message.to_string();
    };
    let mut notes = Vec::new();
    for item in outputs {
        let text = item
            .get("text")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty());
        let Some(text) = text else {
            continue;
        };
        let kind = item
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("media.output");
        let idx = item
            .get("attachmentIndex")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        notes.push(format!("[Media {} #{}]\n{}", kind, idx, text));
    }
    if notes.is_empty() {
        return message.to_string();
    }
    let separator = if message.trim().is_empty() {
        ""
    } else {
        "\n\n"
    };
    format!("{}{}{}", message, separator, notes.join("\n\n"))
}

fn is_chat_stop_command_text(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.eq_ignore_ascii_case("/stop") {
        return true;
    }
    trimmed.eq_ignore_ascii_case("/abort") || trimmed.eq_ignore_ascii_case("/cancel")
}

fn emit_chat_lifecycle_event_with_tx(
    event_tx: &tokio::sync::broadcast::Sender<EventFrame>,
    run_id: &str,
    session_key: &str,
    status: &str,
    reply: Option<&str>,
    error: Option<&str>,
) {
    let mut payload = serde_json::Map::new();
    payload.insert(
        "runId".to_string(),
        serde_json::Value::String(run_id.to_string()),
    );
    payload.insert(
        "sessionKey".to_string(),
        serde_json::Value::String(session_key.to_string()),
    );
    payload.insert(
        "state".to_string(),
        serde_json::Value::String(status.to_string()),
    );
    if let Some(text) = reply {
        payload.insert(
            "reply".to_string(),
            serde_json::Value::String(text.to_string()),
        );
    }
    if let Some(err) = error {
        payload.insert(
            "errorMessage".to_string(),
            serde_json::Value::String(err.to_string()),
        );
    }
    let frame = MessageHandler::new_event("chat", Some(serde_json::Value::Object(payload)));
    let _ = event_tx.send(frame);
}

fn emit_chat_lifecycle_event(
    state: &HttpState,
    run_id: &str,
    session_key: &str,
    status: &str,
    reply: Option<&str>,
    error: Option<&str>,
) {
    emit_chat_lifecycle_event_with_tx(&state.event_tx, run_id, session_key, status, reply, error);
}

fn parse_skill_invocation_for_chat(text: &str) -> Option<(String, String)> {
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

async fn try_execute_chat_skill_command(
    state: &HttpState,
    session_id: &str,
    command_text: &str,
) -> Result<Option<String>, ErrorDetails> {
    let Some((requested, raw_args)) = parse_skill_invocation_for_chat(command_text) else {
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
                    params.insert(
                        "command".to_string(),
                        serde_json::Value::String(raw_args.to_string()),
                    );
                }
            }
        } else {
            params.insert(
                "command".to_string(),
                serde_json::Value::String(raw_args.to_string()),
            );
        }
    }

    let input = oclaw_skills_core::SkillInput {
        name: def.name.clone(),
        description: def.description.clone(),
        parameters: params,
        context: Some(oclaw_skills_core::SkillContext {
            user_id: None,
            session_id: Some(session_id.to_string()),
            request_id: Some(uuid::Uuid::new_v4().to_string()),
            metadata: HashMap::new(),
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

async fn set_chat_run_state(
    state: &HttpState,
    run_id: &str,
    status: &str,
    error: Option<String>,
    result: Option<serde_json::Value>,
) {
    set_chat_run_state_in_map(&state.chat_runs, run_id, status, error, result).await;
}

async fn set_chat_run_state_in_map(
    runs_map: &Arc<tokio::sync::Mutex<HashMap<String, ChatRunRecord>>>,
    run_id: &str,
    status: &str,
    error: Option<String>,
    result: Option<serde_json::Value>,
) {
    let mut runs = runs_map.lock().await;
    if let Some(run) = runs.get_mut(run_id) {
        run.status = status.to_string();
        run.ended_at_ms = Some(now_epoch_ms());
        run.error = error;
        run.result = result;
    }
}

async fn rpc_chat_send(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let idempotency_key = p
        .get("idempotencyKey")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let mut chat_gate_permit: Option<tokio::sync::OwnedSemaphorePermit> = None;
    if let Some(idem) = idempotency_key.as_ref() {
        let gate = state.get_or_create_chat_idempotency_gate(idem).await;
        let permit = gate
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| rpc_error("UNAVAILABLE", "idempotency gate unavailable"))?;
        chat_gate_permit = Some(permit);
    }
    if let Some(idem) = idempotency_key.as_ref()
        && let Some(cached) = state.chat_dedupe.lock().await.get(idem).cloned()
    {
        return Ok(cached);
    }

    let session_key = p
        .get("sessionKey")
        .or_else(|| p.get("sessionId"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("main")
        .to_string();
    let raw_message = p.get("message").and_then(|v| v.as_str()).unwrap_or("");
    let mut message = sanitize_chat_send_message_input(raw_message)?;
    if is_chat_stop_command_text(&message) {
        let abort_params = serde_json::json!({
            "sessionKey": session_key,
        });
        let payload = rpc_chat_abort(&abort_params, state).await?;
        if let Some(idem) = idempotency_key.as_ref() {
            state
                .chat_dedupe
                .lock()
                .await
                .insert(idem.to_string(), payload.clone());
        }
        drop(chat_gate_permit);
        return Ok(payload);
    }

    let mut parsed_attachments =
        parse_rpc_attachments_for_chat(p.get("attachments"), CHAT_ATTACHMENT_MAX_BYTES)?;
    if message.trim().is_empty() && parsed_attachments.normalized_count == 0 {
        return Err(rpc_error(
            "INVALID_PARAMS",
            "invalid chat.send params: text or media is required",
        ));
    }
    message = build_message_with_image_attachments(&message, &parsed_attachments.images);
    let media_normalized_attachments = std::mem::take(&mut parsed_attachments.normalized);

    {
        let manager = state.gateway_server.session_manager.read().await;
        if manager
            .get_session(&session_key)
            .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?
            .is_none()
        {
            manager
                .create_session(&session_key, "default")
                .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
        }
        let metadata = manager
            .get_session_metadata(&session_key)
            .unwrap_or_default();
        if session_send_policy_denied(&metadata) {
            return Err(rpc_error(
                "INVALID_PARAMS",
                "send blocked by session policy",
            ));
        }
    }

    if let Some((_reason, followup)) = parse_session_reset_command(&message) {
        let manager = state.gateway_server.session_manager.read().await;
        manager
            .clear_messages(&session_key)
            .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
        state.clear_session_counters(&session_key);
        message = followup.unwrap_or_else(|| BARE_SESSION_RESET_PROMPT.to_string());
    }

    let run_id = idempotency_key
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if let Some(existing) = state.chat_runs.lock().await.get(&run_id).cloned()
        && existing.status == "running"
    {
        let payload = serde_json::json!({
            "runId": run_id,
            "status": "in_flight",
            "sessionKey": session_key,
        });
        drop(chat_gate_permit);
        return Ok(payload);
    }

    {
        let manager = state.gateway_server.session_manager.read().await;
        manager
            .add_message(&session_key, "user", &message)
            .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    }

    let timeout_ms = p
        .get("timeoutMs")
        .or_else(|| p.get("timeout"))
        .and_then(|v| v.as_u64())
        .unwrap_or(300_000)
        .clamp(1_000, 900_000);
    let media_debug_task = if media_normalized_attachments.is_empty() {
        None
    } else {
        Some(tokio::spawn(collect_media_understanding_debug_payload(
            media_normalized_attachments,
            CHAT_ATTACHMENT_MAX_BYTES,
            timeout_ms.min(120_000),
        )))
    };

    if message.trim().starts_with('/')
        && let Some(command_reply) =
            try_execute_chat_skill_command(state, &session_key, &message).await?
    {
        {
            let manager = state.gateway_server.session_manager.read().await;
            manager
                .add_message(&session_key, "assistant", &command_reply)
                .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
        }
        let media_debug = await_media_understanding_debug_task(media_debug_task).await;
        let mut payload = serde_json::json!({
            "runId": run_id.clone(),
            "status": "ok",
            "sessionKey": session_key.clone(),
            "reply": command_reply.clone(),
            "command": true,
        });
        attach_media_debug_to_payload(&mut payload, media_debug.as_ref());
        emit_chat_lifecycle_event(
            state,
            &run_id,
            &session_key,
            "final",
            Some(&command_reply),
            None,
        );
        if let Some(idem) = idempotency_key.as_ref() {
            state
                .chat_dedupe
                .lock()
                .await
                .insert(idem.to_string(), payload.clone());
        }
        drop(chat_gate_permit);
        return Ok(payload);
    }

    let provider = state
        .llm_provider
        .clone()
        .ok_or_else(|| rpc_error("NO_PROVIDER", "No LLM provider configured"))?;
    state.chat_runs.lock().await.insert(
        run_id.clone(),
        ChatRunRecord {
            run_id: run_id.clone(),
            session_key: session_key.clone(),
            status: "running".to_string(),
            started_at_ms: now_epoch_ms(),
            ended_at_ms: None,
            error: None,
            result: None,
        },
    );
    emit_chat_lifecycle_event(state, &run_id, &session_key, "started", None, None);

    let tool_registry = state.tool_registry.clone();
    let plugin_registrations = state.plugin_registrations.clone();
    let hook_pipeline = state.hook_pipeline.clone();
    let full_config = state.full_config.clone();
    let session_usage_tokens = state.session_usage_tokens.clone();
    let run_message = message.clone();
    let run_session_key = session_key.clone();
    let run_id_for_task = run_id.clone();
    let session_key_for_task = session_key.clone();
    let idempotency_key_for_task = idempotency_key.clone();
    let chat_abort_handles = state.chat_abort_handles.clone();
    let chat_runs = state.chat_runs.clone();
    let chat_dedupe = state.chat_dedupe.clone();
    let usage_snapshot = state.usage_snapshot.clone();
    let session_manager = state.gateway_server.session_manager.clone();
    let channel_manager = state.channel_manager.clone();
    let event_tx = state.event_tx.clone();
    let media_debug_task_for_task = media_debug_task;

    tokio::spawn(async move {
        let run_session_manager = session_manager.clone();
        let usage_snapshot_for_run = usage_snapshot.clone();
        let run_task = tokio::spawn(async move {
            let media_debug = await_media_understanding_debug_task(media_debug_task_for_task).await;
            let model_message =
                augment_message_with_media_outputs(&run_message, media_debug.as_ref());
            let run_result = generate_chat_reply(
                &provider,
                tool_registry,
                plugin_registrations,
                hook_pipeline,
                channel_manager,
                Some(run_session_manager),
                full_config,
                Some(session_usage_tokens),
                Some(usage_snapshot_for_run),
                &model_message,
                Some(&run_session_key),
            )
            .await
            .map_err(|e| e.to_string());
            (run_result, media_debug)
        });
        let abort_handle = run_task.abort_handle();
        chat_abort_handles
            .lock()
            .await
            .insert(run_id_for_task.clone(), abort_handle.clone());

        let final_payload = match tokio::time::timeout(Duration::from_millis(timeout_ms), run_task)
            .await
        {
            Ok(joined) => match joined {
                Ok((run_result, media_debug)) => match run_result {
                    Ok(out) => {
                        record_usage_snapshot(usage_snapshot, out.model.clone(), out.usage.clone())
                            .await;
                        let mut persist_error: Option<String> = None;
                        if let Err(err) = session_manager.read().await.add_message(
                            &session_key_for_task,
                            "assistant",
                            &out.reply,
                        ) {
                            persist_error = Some(err.to_string());
                        }
                        if let Some(err_msg) = persist_error {
                            let mut payload = serde_json::json!({
                                "runId": run_id_for_task,
                                "status": "error",
                                "sessionKey": session_key_for_task,
                                "error": err_msg,
                            });
                            attach_media_debug_to_payload(&mut payload, media_debug.as_ref());
                            set_chat_run_state_in_map(
                                &chat_runs,
                                &run_id_for_task,
                                "error",
                                payload
                                    .get("error")
                                    .and_then(|v| v.as_str())
                                    .map(ToString::to_string),
                                Some(payload.clone()),
                            )
                            .await;
                            emit_chat_lifecycle_event_with_tx(
                                &event_tx,
                                &run_id_for_task,
                                &session_key_for_task,
                                "error",
                                None,
                                payload.get("error").and_then(|v| v.as_str()),
                            );
                            payload
                        } else {
                            let mut payload = serde_json::json!({
                                "runId": run_id_for_task,
                                "status": "ok",
                                "sessionKey": session_key_for_task,
                                "reply": out.reply,
                            });
                            attach_media_debug_to_payload(&mut payload, media_debug.as_ref());
                            set_chat_run_state_in_map(
                                &chat_runs,
                                &run_id_for_task,
                                "ok",
                                None,
                                Some(payload.clone()),
                            )
                            .await;
                            emit_chat_lifecycle_event_with_tx(
                                &event_tx,
                                &run_id_for_task,
                                &session_key_for_task,
                                "final",
                                payload.get("reply").and_then(|v| v.as_str()),
                                None,
                            );
                            payload
                        }
                    }
                    Err(err_msg) => {
                        let mut payload = serde_json::json!({
                            "runId": run_id_for_task,
                            "status": "error",
                            "sessionKey": session_key_for_task,
                            "error": err_msg,
                        });
                        attach_media_debug_to_payload(&mut payload, media_debug.as_ref());
                        set_chat_run_state_in_map(
                            &chat_runs,
                            &run_id_for_task,
                            "error",
                            payload
                                .get("error")
                                .and_then(|v| v.as_str())
                                .map(ToString::to_string),
                            Some(payload.clone()),
                        )
                        .await;
                        emit_chat_lifecycle_event_with_tx(
                            &event_tx,
                            &run_id_for_task,
                            &session_key_for_task,
                            "error",
                            None,
                            payload.get("error").and_then(|v| v.as_str()),
                        );
                        payload
                    }
                },
                Err(join_err) if join_err.is_cancelled() => {
                    let payload = serde_json::json!({
                        "runId": run_id_for_task,
                        "status": "aborted",
                        "sessionKey": session_key_for_task,
                        "error": "chat run aborted",
                    });
                    set_chat_run_state_in_map(
                        &chat_runs,
                        &run_id_for_task,
                        "aborted",
                        Some("chat run aborted".to_string()),
                        Some(payload.clone()),
                    )
                    .await;
                    emit_chat_lifecycle_event_with_tx(
                        &event_tx,
                        &run_id_for_task,
                        &session_key_for_task,
                        "error",
                        None,
                        Some("chat run aborted"),
                    );
                    payload
                }
                Err(join_err) => {
                    let err_msg = join_err.to_string();
                    let payload = serde_json::json!({
                        "runId": run_id_for_task,
                        "status": "error",
                        "sessionKey": session_key_for_task,
                        "error": err_msg,
                    });
                    set_chat_run_state_in_map(
                        &chat_runs,
                        &run_id_for_task,
                        "error",
                        payload
                            .get("error")
                            .and_then(|v| v.as_str())
                            .map(ToString::to_string),
                        Some(payload.clone()),
                    )
                    .await;
                    emit_chat_lifecycle_event_with_tx(
                        &event_tx,
                        &run_id_for_task,
                        &session_key_for_task,
                        "error",
                        None,
                        payload.get("error").and_then(|v| v.as_str()),
                    );
                    payload
                }
            },
            Err(_) => {
                abort_handle.abort();
                let timeout_error = format!("chat.send timed out after {}ms", timeout_ms);
                let payload = serde_json::json!({
                    "runId": run_id_for_task,
                    "status": "timeout",
                    "sessionKey": session_key_for_task,
                    "error": timeout_error,
                });
                set_chat_run_state_in_map(
                    &chat_runs,
                    &run_id_for_task,
                    "timeout",
                    payload
                        .get("error")
                        .and_then(|v| v.as_str())
                        .map(ToString::to_string),
                    Some(payload.clone()),
                )
                .await;
                emit_chat_lifecycle_event_with_tx(
                    &event_tx,
                    &run_id_for_task,
                    &session_key_for_task,
                    "error",
                    None,
                    payload.get("error").and_then(|v| v.as_str()),
                );
                payload
            }
        };
        let final_status = final_payload
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("error")
            .to_string();
        let final_error = final_payload
            .get("error")
            .and_then(|v| v.as_str())
            .map(ToString::to_string);
        set_chat_run_state_in_map(
            &chat_runs,
            &run_id_for_task,
            &final_status,
            final_error,
            Some(final_payload.clone()),
        )
        .await;

        chat_abort_handles.lock().await.remove(&run_id_for_task);
        if let Some(idem) = idempotency_key_for_task.as_ref() {
            chat_dedupe
                .lock()
                .await
                .insert(idem.to_string(), final_payload);
        }
    });

    let payload = serde_json::json!({
        "runId": run_id,
        "status": "started",
    });
    drop(chat_gate_permit);
    Ok(payload)
}

async fn rpc_chat_history(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let session_id = p
        .get("sessionKey")
        .or_else(|| p.get("sessionId"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'sessionKey' parameter"))?;
    let limit = p
        .get("limit")
        .and_then(|v| v.as_u64())
        .unwrap_or(200)
        .clamp(1, 1000) as usize;
    let manager = state.gateway_server.session_manager.read().await;
    let session = manager
        .get_session(session_id)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?
        .ok_or_else(|| rpc_error("NOT_FOUND", "Session not found"))?;
    let metadata = manager.get_session_metadata(session_id).unwrap_or_default();
    let mut messages = manager
        .get_messages(session_id, limit)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    messages.reverse();

    let mut placeholder_count = 0usize;
    for msg in &mut messages {
        if msg.content.as_bytes().len() > CHAT_HISTORY_MAX_SINGLE_MESSAGE_BYTES {
            msg.content = CHAT_HISTORY_OVERSIZED_PLACEHOLDER.to_string();
            placeholder_count = placeholder_count.saturating_add(1);
        }
    }

    let json_len = |items: &Vec<crate::message::SessionMessage>| -> usize {
        serde_json::to_vec(items).map(|b| b.len()).unwrap_or(0)
    };
    let mut truncated = false;
    while json_len(&messages) > CHAT_HISTORY_MAX_TOTAL_BYTES && !messages.is_empty() {
        messages.remove(0);
        truncated = true;
    }
    if truncated {
        messages.insert(
            0,
            crate::message::SessionMessage {
                role: "system".to_string(),
                content: CHAT_HISTORY_TRUNCATED_PLACEHOLDER.to_string(),
                timestamp: chrono::Utc::now().timestamp_millis(),
            },
        );
        placeholder_count = placeholder_count.saturating_add(1);
    }

    let thinking_level = metadata
        .get("thinkingLevel")
        .or_else(|| metadata.get("thinking_level"))
        .or_else(|| metadata.get("thinking"))
        .cloned();
    let verbose_level = metadata
        .get("verboseLevel")
        .or_else(|| metadata.get("verbose_level"))
        .or_else(|| metadata.get("verbose"))
        .cloned();
    let session_id_value = session.key.clone();
    Ok(serde_json::json!({
        "sessionKey": session_id,
        "sessionId": session_id_value,
        "session": session,
        "messages": messages,
        "total": session.message_count,
        "thinkingLevel": thinking_level,
        "verboseLevel": verbose_level,
        "omittedPlaceholders": placeholder_count,
    }))
}

async fn rpc_chat_abort(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let session_key = p
        .get("sessionKey")
        .or_else(|| p.get("sessionId"))
        .and_then(|v| v.as_str());
    let run_id = p
        .get("runId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);

    if let Some(run_id) = run_id {
        let run = state.chat_runs.lock().await.get(&run_id).cloned();
        let Some(run) = run else {
            return Ok(serde_json::json!({
                "ok": true,
                "aborted": false,
                "runIds": [],
            }));
        };
        if let Some(session_key) = session_key.map(str::trim).filter(|s| !s.is_empty())
            && !run.session_key.eq_ignore_ascii_case(session_key)
        {
            return Err(rpc_error(
                "INVALID_PARAMS",
                "runId does not match sessionKey",
            ));
        }
        if run.status != "running" {
            return Ok(serde_json::json!({
                "ok": true,
                "aborted": false,
                "runIds": [],
            }));
        }
        let aborted = if let Some(handle) = state.chat_abort_handles.lock().await.remove(&run_id) {
            handle.abort();
            true
        } else {
            false
        };
        if aborted {
            set_chat_run_state(
                state,
                &run_id,
                "aborted",
                Some("chat run aborted by rpc".to_string()),
                Some(serde_json::json!({
                    "runId": run_id,
                    "status": "aborted",
                    "sessionKey": run.session_key,
                })),
            )
            .await;
            emit_chat_lifecycle_event(
                state,
                &run_id,
                &run.session_key,
                "error",
                None,
                Some("chat run aborted by rpc"),
            );
        }
        return Ok(serde_json::json!({
            "ok": true,
            "aborted": aborted,
            "runIds": if aborted { vec![run_id] } else { Vec::<String>::new() },
        }));
    }

    let session_key = session_key
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'sessionKey'"))?;

    let run_ids: Vec<String> = {
        let runs = state.chat_runs.lock().await;
        runs.iter()
            .filter_map(|(run_id, run)| {
                if run.status == "running" && run.session_key.eq_ignore_ascii_case(session_key) {
                    Some(run_id.clone())
                } else {
                    None
                }
            })
            .collect()
    };
    if run_ids.is_empty() {
        return Ok(serde_json::json!({
            "ok": true,
            "aborted": false,
            "runIds": [],
        }));
    }

    let mut aborted_ids = Vec::new();
    for run_id in &run_ids {
        if let Some(handle) = state.chat_abort_handles.lock().await.remove(run_id) {
            handle.abort();
            aborted_ids.push(run_id.clone());
        }
    }
    for run_id in &aborted_ids {
        set_chat_run_state(
            state,
            run_id,
            "aborted",
            Some("chat run aborted by rpc".to_string()),
            Some(serde_json::json!({
                "runId": run_id,
                "status": "aborted",
                "sessionKey": session_key,
            })),
        )
        .await;
        emit_chat_lifecycle_event(
            state,
            run_id,
            session_key,
            "error",
            None,
            Some("chat run aborted by rpc"),
        );
    }

    Ok(serde_json::json!({
        "ok": true,
        "aborted": !aborted_ids.is_empty(),
        "runIds": aborted_ids,
    }))
}

#[derive(Debug, Clone, Default)]
struct SessionDeliveryContext {
    channel: Option<String>,
    to: Option<String>,
    account_id: Option<String>,
    thread_id: Option<String>,
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

async fn load_session_delivery_context(
    state: &HttpState,
    session_key: &str,
) -> Option<SessionDeliveryContext> {
    let manager = state.gateway_server.session_manager.read().await;
    let metadata = manager.get_session_metadata(session_key).ok()?;
    Some(SessionDeliveryContext {
        channel: session_metadata_pick(
            &metadata,
            &["delivery.channel", "lastChannel", "last_channel"],
        ),
        to: session_metadata_pick(&metadata, &["delivery.to", "lastTo", "last_to"]),
        account_id: session_metadata_pick(
            &metadata,
            &["delivery.accountId", "lastAccountId", "last_account_id"],
        ),
        thread_id: session_metadata_pick(
            &metadata,
            &["delivery.threadId", "lastThreadId", "last_thread_id"],
        ),
    })
}

fn build_session_delivery_fields(
    channel: Option<&str>,
    to: Option<&str>,
    account_id: Option<&str>,
    thread_id: Option<&str>,
) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    if let Some(channel) = channel.map(str::trim).filter(|s| !s.is_empty()) {
        fields.insert("delivery.channel".to_string(), channel.to_string());
        fields.insert("lastChannel".to_string(), channel.to_string());
    }
    if let Some(to) = to.map(str::trim).filter(|s| !s.is_empty()) {
        fields.insert("delivery.to".to_string(), to.to_string());
        fields.insert("lastTo".to_string(), to.to_string());
    }
    if let Some(account_id) = account_id.map(str::trim).filter(|s| !s.is_empty()) {
        fields.insert("delivery.accountId".to_string(), account_id.to_string());
        fields.insert("lastAccountId".to_string(), account_id.to_string());
    }
    if let Some(thread_id) = thread_id.map(str::trim).filter(|s| !s.is_empty()) {
        fields.insert("delivery.threadId".to_string(), thread_id.to_string());
        fields.insert("lastThreadId".to_string(), thread_id.to_string());
    }
    fields
}

async fn persist_session_delivery_context(
    state: &HttpState,
    session_key: &str,
    channel: Option<&str>,
    to: Option<&str>,
    account_id: Option<&str>,
    thread_id: Option<&str>,
) -> Result<(), ErrorDetails> {
    let fields = build_session_delivery_fields(channel, to, account_id, thread_id);
    if fields.is_empty() {
        return Ok(());
    }
    let manager = state.gateway_server.session_manager.read().await;
    manager
        .set_session_metadata_fields(session_key, &fields)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))
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

async fn resolve_channel_name_for_send(
    state: &HttpState,
    requested: Option<&str>,
) -> Result<String, ErrorDetails> {
    let Some(cm) = state.channel_manager.as_ref() else {
        return Err(rpc_error("UNAVAILABLE", "No channel manager configured"));
    };
    let mgr = cm.read().await;
    let mut names = mgr.list().await;
    names.sort();
    if names.is_empty() {
        return Err(rpc_error("UNAVAILABLE", "No channels configured"));
    }
    if let Some(raw) = requested.map(str::trim).filter(|s| !s.is_empty()) {
        let normalized = normalize_channel_alias(raw);
        if normalized == "webchat" {
            return Err(rpc_error(
                "INVALID_PARAMS",
                "unsupported channel: webchat (internal-only). Use `chat.send` for WebChat UI messages or choose a deliverable channel.",
            ));
        }
        if let Some(found) = names
            .iter()
            .find(|name| normalize_channel_alias(name) == normalized)
        {
            return Ok(found.clone());
        }
        return Err(rpc_error(
            "INVALID_PARAMS",
            &format!("unsupported channel: {}", raw),
        ));
    }
    if let Some(non_webchat) = names
        .iter()
        .find(|name| !name.eq_ignore_ascii_case("webchat"))
    {
        return Ok(non_webchat.clone());
    }
    Err(rpc_error(
        "INVALID_PARAMS",
        "No deliverable channels configured (only webchat is available).",
    ))
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

async fn rpc_send(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let session_key = p
        .get("sessionKey")
        .or_else(|| p.get("sessionId"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let session_delivery = if let Some(key) = session_key.as_deref() {
        load_session_delivery_context(state, key).await
    } else {
        None
    };
    let to = p
        .get("to")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'to'"))?;
    let message = p
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    let media_url = p
        .get("mediaUrl")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let media_urls: Vec<String> = p
        .get("mediaUrls")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if message.is_empty() && media_url.is_none() && media_urls.is_empty() {
        return Err(rpc_error(
            "INVALID_PARAMS",
            "send requires message or mediaUrl/mediaUrls",
        ));
    }

    let idempotency_key = p
        .get("idempotencyKey")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let mut send_gate_permit: Option<tokio::sync::OwnedSemaphorePermit> = None;
    if let Some(idem) = idempotency_key {
        let gate = state.get_or_create_send_idempotency_gate(idem).await;
        let permit = gate
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| rpc_error("UNAVAILABLE", "idempotency gate unavailable"))?;
        send_gate_permit = Some(permit);
    }
    if let Some(idem) = idempotency_key
        && let Some(cached) = state.send_dedupe.lock().await.get(idem).cloned()
    {
        return Ok(cached);
    }

    let requested_channel = p.get("channel").and_then(|v| v.as_str()).or_else(|| {
        session_delivery
            .as_ref()
            .and_then(|ctx| ctx.channel.as_deref())
    });
    let channel = resolve_channel_name_for_send(state, requested_channel).await?;
    let run_id = idempotency_key
        .map(ToString::to_string)
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let thread_id = p
        .get("threadId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            session_delivery
                .as_ref()
                .and_then(|ctx| ctx.thread_id.clone())
        });
    let account_id = p
        .get("accountId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            session_delivery
                .as_ref()
                .and_then(|ctx| ctx.account_id.clone())
        });

    let mut body = String::new();
    if !message.is_empty() {
        body.push_str(message);
    }
    if let Some(url) = media_url {
        if !body.is_empty() {
            body.push('\n');
        }
        body.push_str(url);
    }
    for url in &media_urls {
        if !body.is_empty() {
            body.push('\n');
        }
        body.push_str(url);
    }

    let msg = oclaw_channel_core::traits::ChannelMessage {
        id: uuid::Uuid::new_v4().to_string(),
        channel: channel.clone(),
        sender: "rpc-send".to_string(),
        content: body,
        timestamp: chrono::Utc::now().timestamp_millis(),
        metadata: resolve_outbound_target_metadata(to, thread_id.as_deref(), account_id.as_deref()),
    };
    let sent_id = {
        let cm = state
            .channel_manager
            .as_ref()
            .ok_or_else(|| rpc_error("UNAVAILABLE", "No channel manager configured"))?;
        let mgr = cm.read().await;
        mgr.send_to_channel(&channel, &msg)
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &e.to_string()))?
    };
    let payload = serde_json::json!({
        "runId": run_id,
        "channel": channel,
        "messageId": sent_id,
        "to": to,
        "threadId": thread_id,
        "accountId": account_id,
    });
    if let Some(idem) = idempotency_key {
        state
            .send_dedupe
            .lock()
            .await
            .insert(idem.to_string(), payload.clone());
    }
    if let Some(key) = session_key.as_deref() {
        let _ = persist_session_delivery_context(
            state,
            key,
            Some(&channel),
            Some(to),
            account_id.as_deref(),
            thread_id.as_deref(),
        )
        .await;
    }
    drop(send_gate_permit);
    Ok(payload)
}

fn normalize_non_empty_owned(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
}

fn default_taskgraph_system_prompt() -> &'static str {
    "You are a specialized subagent worker. Complete the assigned task accurately and concisely. Return practical output only."
}

fn sanitize_string_list(values: Option<Vec<String>>) -> Vec<String> {
    values
        .unwrap_or_default()
        .into_iter()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

fn build_task_graph_from_rpc(
    params: &RpcTaskGraphInput,
    provider: &Arc<dyn LlmProvider>,
) -> Result<TaskGraph, ErrorDetails> {
    if params.nodes.is_empty() {
        return Err(rpc_error(
            "INVALID_PARAMS",
            "taskgraph.run requires non-empty nodes",
        ));
    }

    let global_model = normalize_non_empty_owned(params.model.as_deref())
        .unwrap_or_else(|| provider.default_model().to_string());
    let global_provider =
        normalize_non_empty_owned(params.provider.as_deref()).unwrap_or_else(|| "default".into());
    let global_system_prompt = normalize_non_empty_owned(params.system_prompt.as_deref())
        .unwrap_or_else(|| default_taskgraph_system_prompt().to_string());

    let mut graph = TaskGraph::new();
    for node in &params.nodes {
        let node_id = normalize_non_empty_owned(Some(node.id.as_str()))
            .ok_or_else(|| rpc_error("INVALID_PARAMS", "taskgraph node id cannot be empty"))?;
        let node_name = normalize_non_empty_owned(node.name.as_deref()).unwrap_or(node_id.clone());
        let node_task = normalize_non_empty_owned(node.task.as_deref());
        let mut system_prompt = normalize_non_empty_owned(node.system_prompt.as_deref())
            .or_else(|| normalize_non_empty_owned(node.prompt.as_deref()))
            .unwrap_or_else(|| global_system_prompt.clone());
        if let Some(task_text) = node_task.as_deref() {
            system_prompt.push_str("\n\nAssigned task:\n");
            system_prompt.push_str(task_text);
        }
        let model =
            normalize_non_empty_owned(node.model.as_deref()).unwrap_or(global_model.clone());
        let provider_name =
            normalize_non_empty_owned(node.provider.as_deref()).unwrap_or(global_provider.clone());
        let description = normalize_non_empty_owned(node.description.as_deref());
        let capabilities = sanitize_string_list(node.capabilities.clone());
        let depends_on = sanitize_string_list(node.depends_on.clone());
        let on_success = sanitize_string_list(node.on_success.clone());
        let on_failure = sanitize_string_list(node.on_failure.clone());
        let input_from = normalize_non_empty_owned(node.input_from.as_deref());
        let input_template = normalize_non_empty_owned(node.input_template.as_deref());
        let base_input =
            normalize_non_empty_owned(node.base_input.as_deref()).or_else(|| node_task.clone());

        let config = SubagentConfig {
            name: node_name,
            description,
            system_prompt,
            model,
            provider: provider_name,
            max_iterations: node.max_iterations,
            timeout_seconds: node.timeout_seconds,
            capabilities,
        };

        graph = graph.with_node(TaskNode {
            id: node_id,
            config,
            depends_on,
            on_success,
            on_failure,
            input_from,
            input_template,
            base_input,
        });
    }
    Ok(graph)
}

async fn rpc_taskgraph_run(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let provider = state
        .llm_provider
        .clone()
        .ok_or_else(|| rpc_error("NO_PROVIDER", "No LLM provider configured"))?;
    let params: RpcTaskGraphInput = serde_json::from_value(p.clone()).map_err(|e| {
        rpc_error(
            "INVALID_PARAMS",
            &format!("invalid taskgraph params: {}", e),
        )
    })?;
    let graph = build_task_graph_from_rpc(&params, &provider)?;
    let initial_input = params.input.as_deref().unwrap_or("").to_string();
    let max_concurrent = params.max_concurrent.unwrap_or(3).clamp(1, 32) as usize;
    let session_key = normalize_non_empty_owned(params.session_key.as_deref())
        .unwrap_or_else(|| format!("taskgraph:{}", uuid::Uuid::new_v4()));

    let tool_executor: Option<Arc<dyn ToolExecutor>> =
        if let Some(ref registry) = state.tool_registry {
            let mut executor = agent_bridge::ToolRegistryExecutor::new(registry.clone())
                .with_session_manager(state.gateway_server.session_manager.clone())
                .with_session_id(session_key)
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
            if let Some(ref regs) = state.plugin_registrations {
                executor = executor.with_plugin_registrations(regs.clone());
            }
            Some(Arc::new(executor) as Arc<dyn ToolExecutor>)
        } else {
            None
        };

    let runner =
        TaskGraphRunner::new(Arc::new(SubagentRegistry::new())).with_max_concurrent(max_concurrent);
    let result = runner
        .run(graph, &initial_input, provider, tool_executor)
        .await
        .map_err(|e| rpc_error("EXECUTION_ERROR", &format!("taskgraph run failed: {}", e)))?;

    match result {
        TaskGraphResult::AllSucceeded { outputs } => Ok(serde_json::json!({
            "status": "ok",
            "maxConcurrent": max_concurrent,
            "outputs": outputs,
        })),
        TaskGraphResult::PartialSuccess { outputs, failed } => Ok(serde_json::json!({
            "status": "partial",
            "maxConcurrent": max_concurrent,
            "outputs": outputs,
            "failed": failed,
        })),
    }
}

async fn rpc_agent(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let raw_message = p.get("message").and_then(|v| v.as_str()).unwrap_or("");
    let mut message = sanitize_chat_send_message_input(raw_message)?;
    let idempotency_key = p
        .get("idempotencyKey")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let mut agent_gate_permit: Option<tokio::sync::OwnedSemaphorePermit> = None;
    if let Some(idem) = idempotency_key.as_ref() {
        let gate = state.get_or_create_agent_idempotency_gate(idem).await;
        let permit = gate
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| rpc_error("UNAVAILABLE", "idempotency gate unavailable"))?;
        agent_gate_permit = Some(permit);
    }
    if let Some(idem) = idempotency_key.as_ref()
        && let Some(existing_run_id) = state.agent_idempotency.lock().await.get(idem).cloned()
    {
        if let Some(run) = state.agent_runs.lock().await.get(&existing_run_id).cloned() {
            if run.status != "running" {
                return Ok(serde_json::json!({
                    "runId": run.run_id,
                    "status": run.status,
                    "startedAt": run.started_at_ms,
                    "endedAt": run.ended_at_ms,
                    "error": run.error,
                    "result": run.result,
                    "cached": true,
                }));
            }
        }
        return Ok(serde_json::json!({
            "runId": existing_run_id,
            "status": "accepted",
            "acceptedAt": now_epoch_ms(),
            "cached": true,
        }));
    }

    let mut parsed_attachments =
        parse_rpc_attachments_for_chat(p.get("attachments"), CHAT_ATTACHMENT_MAX_BYTES)?;
    if message.trim().is_empty() && parsed_attachments.normalized_count == 0 {
        return Err(rpc_error(
            "INVALID_PARAMS",
            "invalid agent params: message or attachment required",
        ));
    }

    let mut session_id = p
        .get("sessionKey")
        .or_else(|| p.get("sessionId"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    if let Some(session_key) = session_id.as_deref()
        && let Some((_reason, maybe_prompt)) = parse_session_reset_command(&message)
    {
        let manager = state.gateway_server.session_manager.read().await;
        if manager
            .get_session(session_key)
            .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?
            .is_none()
        {
            manager
                .create_session(session_key, "default")
                .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
        }
        manager
            .clear_messages(session_key)
            .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
        state.clear_session_counters(session_key);
        message = maybe_prompt.unwrap_or_else(|| BARE_SESSION_RESET_PROMPT.to_string());
        session_id = Some(session_key.to_string());
    }
    message = build_message_with_image_attachments(&message, &parsed_attachments.images);
    let media_normalized_attachments = std::mem::take(&mut parsed_attachments.normalized);
    let session_delivery = if let Some(key) = session_id.as_deref() {
        let metadata = state
            .gateway_server
            .session_manager
            .read()
            .await
            .get_session_metadata(key)
            .unwrap_or_default();
        if session_send_policy_denied(&metadata) {
            return Err(rpc_error(
                "INVALID_PARAMS",
                "send blocked by session policy",
            ));
        }
        load_session_delivery_context(state, key).await
    } else {
        None
    };

    let provider = state
        .llm_provider
        .clone()
        .ok_or_else(|| rpc_error("NO_PROVIDER", "No LLM provider configured"))?;
    let timeout_ms = p
        .get("timeoutMs")
        .or_else(|| p.get("timeout"))
        .and_then(|v| v.as_u64())
        .unwrap_or(300_000)
        .clamp(1_000, 900_000);
    let media_debug_task = if media_normalized_attachments.is_empty() {
        None
    } else {
        Some(tokio::spawn(collect_media_understanding_debug_payload(
            media_normalized_attachments,
            CHAT_ATTACHMENT_MAX_BYTES,
            timeout_ms.min(120_000),
        )))
    };

    let run_id = idempotency_key
        .clone()
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if let Some(idem) = idempotency_key.clone() {
        state
            .agent_idempotency
            .lock()
            .await
            .insert(idem, run_id.clone());
    }

    state.agent_runs.lock().await.insert(
        run_id.clone(),
        AgentRunRecord {
            run_id: run_id.clone(),
            status: "running".to_string(),
            started_at_ms: now_epoch_ms(),
            ended_at_ms: None,
            error: None,
            result: None,
        },
    );

    let run_id_for_task = run_id.clone();
    let tool_registry = state.tool_registry.clone();
    let plugin_registrations = state.plugin_registrations.clone();
    let hook_pipeline = state.hook_pipeline.clone();
    let full_config = state.full_config.clone();
    let session_usage_tokens = state.session_usage_tokens.clone();
    let channel_manager = state.channel_manager.clone();
    let session_manager = state.gateway_server.session_manager.clone();
    let deliver = p.get("deliver").and_then(|v| v.as_bool()).unwrap_or(false);
    let channel_hint = p
        .get("replyChannel")
        .or_else(|| p.get("channel"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            session_delivery
                .as_ref()
                .and_then(|ctx| ctx.channel.clone())
        });
    let to_hint = p
        .get("replyTo")
        .or_else(|| p.get("to"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .or_else(|| session_delivery.as_ref().and_then(|ctx| ctx.to.clone()));
    let thread_id = p
        .get("threadId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            session_delivery
                .as_ref()
                .and_then(|ctx| ctx.thread_id.clone())
        });
    let account_id = p
        .get("replyAccountId")
        .or_else(|| p.get("accountId"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            session_delivery
                .as_ref()
                .and_then(|ctx| ctx.account_id.clone())
        });
    let media_debug_task_for_task = media_debug_task;
    let runs_map = state.agent_runs.clone();
    let usage_snapshot = state.usage_snapshot.clone();
    let session_key_for_delivery = session_id.clone();
    let validated_channel = if deliver {
        let channel_name = if let Some(channel_name) = channel_hint.clone() {
            channel_name
        } else {
            resolve_channel_name_for_send(state, None).await?
        };
        let Some(target) = to_hint.clone() else {
            return Err(rpc_error(
                "INVALID_PARAMS",
                "deliver=true requires to/replyTo",
            ));
        };
        let _ = target;
        resolve_channel_name_for_send(state, Some(&channel_name)).await?
    } else if let Some(channel_name) = channel_hint.clone() {
        resolve_channel_name_for_send(state, Some(&channel_name)).await?
    } else {
        String::new()
    };

    tokio::spawn(async move {
        let media_debug = await_media_understanding_debug_task(media_debug_task_for_task).await;
        let model_message = augment_message_with_media_outputs(&message, media_debug.as_ref());
        let result = tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            generate_chat_reply(
                &provider,
                tool_registry.clone(),
                plugin_registrations.clone(),
                hook_pipeline.clone(),
                channel_manager.clone(),
                Some(session_manager.clone()),
                full_config.clone(),
                Some(session_usage_tokens.clone()),
                Some(usage_snapshot.clone()),
                &model_message,
                session_id.as_deref(),
            ),
        )
        .await
        .map_err(|_| format!("agent run timed out after {}ms", timeout_ms))
        .and_then(|v| v);

        let mut final_status = "ok".to_string();
        let mut final_error: Option<String> = None;
        let mut final_result: Option<serde_json::Value> = None;

        match result {
            Ok(out) => {
                record_usage_snapshot(usage_snapshot.clone(), out.model.clone(), out.usage.clone())
                    .await;
                let mut payload = serde_json::json!({ "reply": out.reply });
                if deliver
                    && let (Some(cm), Some(channel_raw), Some(to_raw)) = (
                        channel_manager.as_ref(),
                        Some(validated_channel.as_str()),
                        to_hint.as_deref(),
                    )
                {
                    let msg = oclaw_channel_core::traits::ChannelMessage {
                        id: uuid::Uuid::new_v4().to_string(),
                        channel: channel_raw.to_string(),
                        sender: "rpc-agent".to_string(),
                        content: payload["reply"].as_str().unwrap_or_default().to_string(),
                        timestamp: chrono::Utc::now().timestamp_millis(),
                        metadata: resolve_outbound_target_metadata(
                            to_raw,
                            thread_id.as_deref(),
                            account_id.as_deref(),
                        ),
                    };
                    match cm
                        .read()
                        .await
                        .send_to_channel(channel_raw, &msg)
                        .await
                        .map_err(|e| e.to_string())
                    {
                        Ok(message_id) => {
                            payload["delivery"] = serde_json::json!({
                                "channel": channel_raw,
                                "to": to_raw,
                                "threadId": thread_id.clone(),
                                "accountId": account_id.clone(),
                                "messageId": message_id,
                            });
                            if let Some(session_key) = session_key_for_delivery.as_deref() {
                                let fields = build_session_delivery_fields(
                                    Some(channel_raw),
                                    Some(to_raw),
                                    account_id.as_deref(),
                                    thread_id.as_deref(),
                                );
                                if !fields.is_empty() {
                                    let _ = session_manager
                                        .read()
                                        .await
                                        .set_session_metadata_fields(session_key, &fields);
                                }
                            }
                        }
                        Err(e) => {
                            final_status = "error".to_string();
                            final_error = Some(format!("delivery failed: {}", e));
                            payload["delivery"] = serde_json::json!({
                                "channel": channel_raw,
                                "to": to_raw,
                                "error": e,
                            });
                        }
                    }
                }
                final_result = Some(payload);
            }
            Err(err) => {
                final_status = "error".to_string();
                final_error = Some(err);
            }
        }
        if let Some(media) = media_debug {
            if let Some(payload) = final_result.as_mut() {
                attach_media_debug_to_payload(payload, Some(&media));
            } else {
                final_result = Some(serde_json::json!({
                    "media": media,
                }));
            }
        }

        if let Some(run) = runs_map.lock().await.get_mut(&run_id_for_task) {
            run.status = final_status;
            run.ended_at_ms = Some(now_epoch_ms());
            run.error = final_error;
            run.result = final_result;
        }
    });

    drop(agent_gate_permit);
    Ok(serde_json::json!({
        "runId": run_id,
        "status": "accepted",
        "acceptedAt": now_epoch_ms(),
    }))
}

async fn rpc_agent_wait(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let run_id = p
        .get("runId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'runId'"))?
        .to_string();
    let timeout_ms = p
        .get("timeoutMs")
        .and_then(|v| v.as_u64())
        .unwrap_or(30_000)
        .min(600_000);
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);

    loop {
        let snapshot = state.agent_runs.lock().await.get(&run_id).cloned();
        if let Some(run) = snapshot {
            if run.status != "running" {
                return Ok(serde_json::json!({
                    "runId": run.run_id,
                    "status": run.status,
                    "startedAt": run.started_at_ms,
                    "endedAt": run.ended_at_ms,
                    "error": run.error,
                    "result": run.result,
                }));
            }
        } else {
            return Err(rpc_error(
                "NOT_FOUND",
                &format!("run not found: {}", run_id),
            ));
        }

        if std::time::Instant::now() >= deadline {
            return Ok(serde_json::json!({
                "runId": run_id,
                "status": "timeout",
            }));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn rpc_agent_identity_get(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let requested_agent_id = p
        .get("agentId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let session_key = p
        .get("sessionKey")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let resolved_agent_id = requested_agent_id.or_else(|| {
        session_key.and_then(|raw| {
            raw.split([':', '/', '_'])
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty() && *s != "main")
                .map(ToString::to_string)
        })
    });

    let (name, avatar, emoji, creature, vibe, theme) = if let Some(ws) = state.workspace.as_ref() {
        match oclaw_workspace_core::identity::AgentIdentity::load(ws).await {
            Ok(Some(identity)) => (
                identity.name,
                identity.avatar,
                identity.emoji,
                identity.creature,
                identity.vibe,
                identity.theme,
            ),
            _ => (None, None, None, None, None, None),
        }
    } else {
        (None, None, None, None, None, None)
    };

    let (cfg_name, cfg_avatar) = if let Some(cfg) = state.full_config.as_ref() {
        let guard = cfg.read().await;
        (
            guard
                .ui
                .as_ref()
                .and_then(|ui| ui.assistant.as_ref())
                .and_then(|assistant| assistant.name.clone()),
            guard
                .ui
                .as_ref()
                .and_then(|ui| ui.assistant.as_ref())
                .and_then(|assistant| assistant.avatar.clone()),
        )
    } else {
        (None, None)
    };

    Ok(serde_json::json!({
        "agentId": resolved_agent_id,
        "name": name.or(cfg_name).unwrap_or_else(|| "Assistant".to_string()),
        "avatar": avatar.or(cfg_avatar),
        "emoji": emoji,
        "creature": creature,
        "vibe": vibe,
        "theme": theme,
    }))
}

// ── Agents RPCs ──────────────────────────────────────────────────────────

async fn rpc_agents_list(state: &HttpState) -> RpcResult {
    let manager = state.gateway_server.session_manager.read().await;
    let agents = manager.list_agents().unwrap_or_default();
    serde_json::to_value(&agents).map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_agents_create(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let id = p["id"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    let name = p["name"].as_str().unwrap_or(id);
    let model = p["model"].as_str().unwrap_or("default");
    let system_prompt = p["systemPrompt"].as_str().unwrap_or("");

    let manager = state.gateway_server.session_manager.read().await;
    manager
        .create_agent(id, name, model, system_prompt)
        .map_err(|e| rpc_error("AGENT_ERROR", &e.to_string()))?;
    Ok(serde_json::json!({"created": true, "id": id}))
}

async fn rpc_agents_update(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let id = p["id"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    let manager = state.gateway_server.session_manager.read().await;
    manager
        .update_agent(id, p)
        .map_err(|e| rpc_error("AGENT_ERROR", &e.to_string()))?;
    Ok(serde_json::json!({"updated": true, "id": id}))
}

async fn rpc_agents_delete(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let id = p["id"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    let manager = state.gateway_server.session_manager.read().await;
    manager
        .delete_agent(id)
        .map_err(|e| rpc_error("AGENT_ERROR", &e.to_string()))?;
    Ok(serde_json::json!({"deleted": true, "id": id}))
}

fn agent_files_root(agent_id: &str) -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".oclaw")
        .join("agents")
        .join(agent_id)
        .join("files")
}

fn sanitize_agent_file_name(name: &str) -> Result<String, ErrorDetails> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(rpc_error("INVALID_PARAMS", "File name is empty"));
    }
    if trimmed.starts_with('/') || trimmed.contains("..") || trimmed.contains('\\') {
        return Err(rpc_error("INVALID_PARAMS", "Unsafe file path"));
    }
    Ok(trimmed.to_string())
}

fn collect_files_recursive(root: &Path) -> Result<Vec<serde_json::Value>, ErrorDetails> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let rd = std::fs::read_dir(&dir)
            .map_err(|e| rpc_error("INTERNAL", &format!("Read dir failed: {}", e)))?;
        for entry in rd {
            let entry = entry.map_err(|e| rpc_error("INTERNAL", &e.to_string()))?;
            let path = entry.path();
            let meta = entry
                .metadata()
                .map_err(|e| rpc_error("INTERNAL", &format!("Metadata failed: {}", e)))?;
            if meta.is_dir() {
                stack.push(path);
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push(serde_json::json!({
                "name": rel,
                "size": meta.len(),
                "modifiedAtMs": meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_millis() as u64),
            }));
        }
    }
    out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    Ok(out)
}

async fn rpc_agents_files_list(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let id = p["id"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    let root = agent_files_root(id);
    if !root.exists() {
        return Ok(serde_json::json!({"agentId": id, "files": []}));
    }
    let files = collect_files_recursive(&root)?;
    Ok(serde_json::json!({"agentId": id, "root": root, "files": files}))
}

async fn rpc_agents_files_get(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let id = p["id"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    let name = p["name"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'name'"))?;
    let safe_name = sanitize_agent_file_name(name)?;
    let path = agent_files_root(id).join(&safe_name);
    if !path.exists() {
        return Err(rpc_error(
            "NOT_FOUND",
            &format!("Agent file not found: {}/{}", id, safe_name),
        ));
    }
    let bytes = std::fs::read(&path)
        .map_err(|e| rpc_error("INTERNAL", &format!("Failed to read file: {}", e)))?;
    let encoding = p["encoding"].as_str().unwrap_or("utf8");
    if encoding.eq_ignore_ascii_case("base64") {
        return Ok(serde_json::json!({
            "agentId": id,
            "name": safe_name,
            "encoding": "base64",
            "content": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes),
        }));
    }
    match String::from_utf8(bytes.clone()) {
        Ok(text) => Ok(serde_json::json!({
            "agentId": id,
            "name": safe_name,
            "encoding": "utf8",
            "content": text,
        })),
        Err(_) => Ok(serde_json::json!({
            "agentId": id,
            "name": safe_name,
            "encoding": "base64",
            "content": base64::Engine::encode(&base64::engine::general_purpose::STANDARD, bytes),
        })),
    }
}

async fn rpc_agents_files_set(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let id = p["id"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    let name = p["name"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'name'"))?;
    let content = p["content"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'content'"))?;
    let encoding = p["encoding"].as_str().unwrap_or("utf8");
    let overwrite = p["overwrite"].as_bool().unwrap_or(true);
    let safe_name = sanitize_agent_file_name(name)?;
    let path = agent_files_root(id).join(&safe_name);
    if !overwrite && path.exists() {
        return Err(rpc_error(
            "CONFLICT",
            "File already exists and overwrite=false",
        ));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| rpc_error("INTERNAL", &format!("Create dir failed: {}", e)))?;
    }
    let data = if encoding.eq_ignore_ascii_case("base64") {
        base64::Engine::decode(&base64::engine::general_purpose::STANDARD, content)
            .map_err(|e| rpc_error("INVALID_PARAMS", &format!("Invalid base64 content: {}", e)))?
    } else {
        content.as_bytes().to_vec()
    };
    std::fs::write(&path, &data)
        .map_err(|e| rpc_error("INTERNAL", &format!("Failed to write file: {}", e)))?;
    Ok(serde_json::json!({
        "ok": true,
        "agentId": id,
        "name": safe_name,
        "bytes": data.len(),
    }))
}

// ── System RPCs ──────────────────────────────────────────────────────────

fn normalize_system_session_key(raw: Option<&str>) -> String {
    let trimmed = raw.map(str::trim).unwrap_or_default();
    if trimmed.is_empty() {
        MAIN_SYSTEM_SESSION_KEY.to_string()
    } else {
        trimmed.to_string()
    }
}

fn normalize_context_key(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
}

fn normalize_wake_target(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

fn normalize_wake_reason(raw: Option<&str>) -> String {
    let trimmed = raw.map(str::trim).unwrap_or_default();
    if trimmed.is_empty() {
        DEFAULT_WAKE_REASON.to_string()
    } else {
        trimmed.to_string()
    }
}

fn resolve_wake_reason_kind(reason: &str) -> &'static str {
    let trimmed = reason.trim();
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

fn is_action_wake_reason(reason: &str) -> bool {
    matches!(
        resolve_wake_reason_kind(reason),
        "manual" | "exec-event" | "hook"
    )
}

fn resolve_wake_reason_priority(reason: &str) -> i32 {
    match resolve_wake_reason_kind(reason) {
        "retry" => 0,
        "interval" => 1,
        _ if is_action_wake_reason(reason) => 3,
        _ => 2,
    }
}

fn wake_target_key(agent_id: Option<&str>, session_key: Option<&str>) -> String {
    format!(
        "{}::{}",
        agent_id.unwrap_or_default(),
        session_key.unwrap_or_default()
    )
}

fn is_system_event_context_changed(
    session_key: &str,
    context_key: Option<&str>,
) -> Result<bool, String> {
    let normalized_session = normalize_system_session_key(Some(session_key));
    let normalized_context = normalize_context_key(context_key);
    let queues = SYSTEM_EVENT_QUEUES
        .lock()
        .map_err(|_| "system event queue lock poisoned".to_string())?;
    let previous = queues
        .get(&normalized_session)
        .and_then(|q| q.last_context_key.clone());
    Ok(normalized_context != previous)
}

fn enqueue_system_event_entry(
    session_key: &str,
    text: &str,
    context_key: Option<&str>,
) -> Result<(), String> {
    let normalized_session = normalize_system_session_key(Some(session_key));
    let cleaned = text.trim();
    if cleaned.is_empty() {
        return Ok(());
    }
    let normalized_context = normalize_context_key(context_key);
    let mut queues = SYSTEM_EVENT_QUEUES
        .lock()
        .map_err(|_| "system event queue lock poisoned".to_string())?;
    let queue = queues
        .entry(normalized_session)
        .or_insert_with(SessionSystemEventQueue::default);
    queue.last_context_key = normalized_context.clone();
    if queue.last_text.as_deref() == Some(cleaned) {
        return Ok(());
    }
    queue.last_text = Some(cleaned.to_string());
    queue.queue.push_back(SystemEventEntry {
        text: cleaned.to_string(),
        ts: chrono::Utc::now().timestamp_millis(),
        context_key: normalized_context,
    });
    while queue.queue.len() > MAX_SYSTEM_EVENTS {
        let _ = queue.queue.pop_front();
    }
    Ok(())
}

fn drain_system_event_entries(session_key: &str) -> Result<Vec<SystemEventEntry>, String> {
    let normalized_session = normalize_system_session_key(Some(session_key));
    let mut queues = SYSTEM_EVENT_QUEUES
        .lock()
        .map_err(|_| "system event queue lock poisoned".to_string())?;
    let Some(entry) = queues.remove(&normalized_session) else {
        return Ok(Vec::new());
    };
    Ok(entry.queue.into_iter().collect())
}

fn queue_pending_heartbeat_wake(
    reason: Option<&str>,
    agent_id: Option<&str>,
    session_key: Option<&str>,
) -> Result<(), String> {
    let normalized_reason = normalize_wake_reason(reason);
    let normalized_agent = normalize_wake_target(agent_id);
    let normalized_session = normalize_wake_target(session_key);
    let key = wake_target_key(normalized_agent.as_deref(), normalized_session.as_deref());
    let now_ms = chrono::Utc::now().timestamp_millis();
    let next = PendingHeartbeatWake {
        reason: normalized_reason.clone(),
        priority: resolve_wake_reason_priority(&normalized_reason),
        requested_at: now_ms,
        agent_id: normalized_agent,
        session_key: normalized_session,
    };

    let mut wakes = PENDING_HEARTBEAT_WAKES
        .lock()
        .map_err(|_| "heartbeat wake queue lock poisoned".to_string())?;
    if let Some(previous) = wakes.get(&key) {
        if next.priority > previous.priority
            || (next.priority == previous.priority && next.requested_at >= previous.requested_at)
        {
            wakes.insert(key, next);
        }
    } else {
        wakes.insert(key, next);
    }
    Ok(())
}

fn take_pending_heartbeat_wakes() -> Result<Vec<PendingHeartbeatWake>, String> {
    let mut wakes = PENDING_HEARTBEAT_WAKES
        .lock()
        .map_err(|_| "heartbeat wake queue lock poisoned".to_string())?;
    let mut out: Vec<PendingHeartbeatWake> = wakes.values().cloned().collect();
    wakes.clear();
    out.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then(a.requested_at.cmp(&b.requested_at))
    });
    Ok(out)
}

fn set_global_last_heartbeat_event(value: serde_json::Value) {
    if let Ok(mut guard) = GLOBAL_LAST_HEARTBEAT_EVENT.lock() {
        *guard = Some(value);
    }
}

fn get_global_last_heartbeat_event() -> Option<serde_json::Value> {
    GLOBAL_LAST_HEARTBEAT_EVENT
        .lock()
        .ok()
        .and_then(|guard| guard.clone())
}

async fn rpc_system_health(state: &HttpState) -> RpcResult {
    let report = state.health_checker.check_all();
    serde_json::to_value(&report).map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_system_status(state: &HttpState) -> RpcResult {
    let session_count = state
        .gateway_server
        .session_manager
        .read()
        .await
        .list_sessions()
        .map(|s| s.len())
        .unwrap_or(0);
    let has_llm = state.llm_provider.is_some();
    let has_channels = state.channel_manager.is_some();
    let has_cron = state.cron_service.is_some();
    Ok(serde_json::json!({
        "session_count": session_count,
        "llm_configured": has_llm,
        "channels_configured": has_channels,
        "cron_configured": has_cron,
        "version": env!("CARGO_PKG_VERSION"),
    }))
}

async fn rpc_system_heartbeat(state: &HttpState) -> RpcResult {
    let enabled = GLOBAL_HEARTBEATS_ENABLED.load(Ordering::Relaxed);
    *state.heartbeats_enabled.write().await = enabled;
    let event = serde_json::json!({
        "timestamp": chrono::Utc::now().timestamp_millis(),
        "alive": true,
        "enabled": enabled,
    });
    set_global_last_heartbeat_event(event.clone());
    *state.last_heartbeat_event.write().await = Some(event.clone());
    Ok(event)
}

fn build_self_system_presence_entry() -> SystemPresenceEntry {
    let host = std::env::var("COMPUTERNAME")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .or_else(|| {
            std::env::var("HOSTNAME")
                .ok()
                .filter(|v| !v.trim().is_empty())
        })
        .unwrap_or_else(|| "localhost".to_string());
    let version = env!("CARGO_PKG_VERSION").to_string();
    let os_name = std::env::consts::OS.to_string();
    let arch = std::env::consts::ARCH.to_string();
    let platform = format!("{} {}", os_name, arch);
    let device_family = match os_name.as_str() {
        "macos" => "Mac",
        "windows" => "Windows",
        "linux" => "Linux",
        _ => "Unknown",
    }
    .to_string();
    let text = format!(
        "Gateway: {} · app {} · mode gateway · reason self",
        host, version
    );
    SystemPresenceEntry {
        host: Some(host),
        ip: None,
        version: Some(version),
        platform: Some(platform),
        device_family: Some(device_family),
        model_identifier: Some(arch),
        last_input_seconds: None,
        mode: Some("gateway".to_string()),
        reason: Some("self".to_string()),
        device_id: None,
        roles: None,
        scopes: None,
        instance_id: None,
        text,
        ts: now_epoch_ms() as i64,
    }
}

fn normalize_presence_key(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_ascii_lowercase())
}

fn read_presence_string(p: &serde_json::Value, key: &str) -> Option<String> {
    p.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

fn read_presence_i64(p: &serde_json::Value, key: &str) -> Option<i64> {
    p.get(key)
        .and_then(|v| v.as_i64())
        .or_else(|| p.get(key).and_then(|v| v.as_u64()).map(|v| v as i64))
}

fn read_presence_string_list(p: &serde_json::Value, key: &str) -> Option<Vec<String>> {
    let values: Vec<String> = p
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|item| item.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn merge_presence_string_list(
    existing: Option<Vec<String>>,
    incoming: Option<Vec<String>>,
) -> Option<Vec<String>> {
    let mut merged: Vec<String> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for source in [existing.unwrap_or_default(), incoming.unwrap_or_default()] {
        for item in source {
            let key = item.to_ascii_lowercase();
            if seen.insert(key) {
                merged.push(item);
            }
        }
    }
    if merged.is_empty() {
        None
    } else {
        Some(merged)
    }
}

fn ensure_self_presence_locked(entries: &mut HashMap<String, SystemPresenceEntry>) {
    if !entries.contains_key(SYSTEM_PRESENCE_SELF_KEY) {
        entries.insert(
            SYSTEM_PRESENCE_SELF_KEY.to_string(),
            build_self_system_presence_entry(),
        );
    }
}

fn touch_self_presence_locked(entries: &mut HashMap<String, SystemPresenceEntry>) {
    if let Some(self_entry) = entries.get_mut(SYSTEM_PRESENCE_SELF_KEY) {
        self_entry.ts = now_epoch_ms() as i64;
    } else {
        entries.insert(
            SYSTEM_PRESENCE_SELF_KEY.to_string(),
            build_self_system_presence_entry(),
        );
    }
}

fn prune_system_presence_locked(entries: &mut HashMap<String, SystemPresenceEntry>) {
    let now = now_epoch_ms() as i64;
    entries.retain(|key, value| {
        if key == SYSTEM_PRESENCE_SELF_KEY {
            return true;
        }
        now.saturating_sub(value.ts) <= SYSTEM_PRESENCE_TTL_MS
    });
    if entries.len() <= SYSTEM_PRESENCE_MAX_ENTRIES {
        return;
    }
    let mut keyed: Vec<(String, i64)> = entries
        .iter()
        .map(|(k, v)| (k.clone(), v.ts))
        .filter(|(k, _)| k != SYSTEM_PRESENCE_SELF_KEY)
        .collect();
    keyed.sort_by(|a, b| a.1.cmp(&b.1));
    let excess = entries
        .len()
        .saturating_sub(SYSTEM_PRESENCE_MAX_ENTRIES)
        .min(keyed.len());
    for (key, _) in keyed.into_iter().take(excess) {
        entries.remove(&key);
    }
}

fn list_system_presence_entries() -> Result<Vec<SystemPresenceEntry>, ErrorDetails> {
    let mut guard = SYSTEM_PRESENCE
        .lock()
        .map_err(|_| rpc_error("INTERNAL", "system presence lock poisoned"))?;
    ensure_self_presence_locked(&mut guard);
    prune_system_presence_locked(&mut guard);
    touch_self_presence_locked(&mut guard);
    let mut entries: Vec<SystemPresenceEntry> = guard.values().cloned().collect();
    entries.sort_by(|a, b| b.ts.cmp(&a.ts));
    Ok(entries)
}

#[derive(Debug, Clone)]
struct SystemPresenceUpdate {
    key: String,
    next: SystemPresenceEntry,
    changed_keys: Vec<String>,
}

fn update_system_presence_entry(
    p: &serde_json::Value,
) -> Result<SystemPresenceUpdate, ErrorDetails> {
    let text = p
        .get("text")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "text required"))?
        .to_string();
    let host = read_presence_string(p, "host");
    let ip = read_presence_string(p, "ip");
    let version = read_presence_string(p, "version");
    let platform = read_presence_string(p, "platform");
    let device_family = read_presence_string(p, "deviceFamily");
    let model_identifier = read_presence_string(p, "modelIdentifier");
    let mode = read_presence_string(p, "mode");
    let reason = read_presence_string(p, "reason");
    let device_id = read_presence_string(p, "deviceId");
    let instance_id = read_presence_string(p, "instanceId");
    let last_input_seconds = read_presence_i64(p, "lastInputSeconds");
    let roles = read_presence_string_list(p, "roles");
    let scopes = read_presence_string_list(p, "scopes");

    let key = normalize_presence_key(device_id.as_deref())
        .or_else(|| normalize_presence_key(instance_id.as_deref()))
        .or_else(|| normalize_presence_key(host.as_deref()))
        .or_else(|| normalize_presence_key(ip.as_deref()))
        .unwrap_or_else(|| {
            text.chars()
                .take(64)
                .collect::<String>()
                .to_ascii_lowercase()
        });

    let mut guard = SYSTEM_PRESENCE
        .lock()
        .map_err(|_| rpc_error("INTERNAL", "system presence lock poisoned"))?;
    ensure_self_presence_locked(&mut guard);
    let existing = guard.get(&key).cloned().unwrap_or_default();
    let previous = existing.clone();
    let next = SystemPresenceEntry {
        host: host.or(existing.host),
        ip: ip.or(existing.ip),
        version: version.or(existing.version),
        platform: platform.or(existing.platform),
        device_family: device_family.or(existing.device_family),
        model_identifier: model_identifier.or(existing.model_identifier),
        last_input_seconds: last_input_seconds.or(existing.last_input_seconds),
        mode: mode.or(existing.mode),
        reason: reason.or(existing.reason),
        device_id: device_id.or(existing.device_id),
        roles: merge_presence_string_list(existing.roles, roles),
        scopes: merge_presence_string_list(existing.scopes, scopes),
        instance_id: instance_id.or(existing.instance_id),
        text,
        ts: now_epoch_ms() as i64,
    };
    let mut changed_keys = Vec::new();
    if previous.host != next.host {
        changed_keys.push("host".to_string());
    }
    if previous.ip != next.ip {
        changed_keys.push("ip".to_string());
    }
    if previous.version != next.version {
        changed_keys.push("version".to_string());
    }
    if previous.mode != next.mode {
        changed_keys.push("mode".to_string());
    }
    if previous.reason != next.reason {
        changed_keys.push("reason".to_string());
    }
    guard.insert(key.clone(), next.clone());
    prune_system_presence_locked(&mut guard);
    Ok(SystemPresenceUpdate {
        key,
        next,
        changed_keys,
    })
}

async fn rpc_system_presence(_state: &HttpState) -> RpcResult {
    let entries = list_system_presence_entries()?;
    serde_json::to_value(entries).map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_system_event(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    if let Some(raw_text) = p.get("text").and_then(|v| v.as_str()) {
        let text = raw_text.trim();
        if text.is_empty() {
            return Err(rpc_error("INVALID_PARAMS", "text required"));
        }
        let session_key =
            normalize_system_session_key(p.get("sessionKey").and_then(|v| v.as_str()));
        let presence_update = update_system_presence_entry(p)?;
        let reason_value = presence_update
            .next
            .reason
            .clone()
            .or_else(|| read_presence_string(p, "reason"));
        let normalized_reason = reason_value
            .as_deref()
            .map(|v| v.trim().to_ascii_lowercase())
            .unwrap_or_default();
        if normalized_reason.starts_with("periodic") || normalized_reason == "heartbeat" {
            let heartbeat_event = serde_json::json!({
                "timestamp": chrono::Utc::now().timestamp_millis(),
                "alive": true,
                "reason": reason_value.clone().unwrap_or_else(|| "heartbeat".to_string()),
                "sessionKey": session_key,
                "source": "system-event",
            });
            set_global_last_heartbeat_event(heartbeat_event.clone());
            *state.last_heartbeat_event.write().await = Some(heartbeat_event);
        }
        if text.starts_with("Node:") {
            let changed: HashSet<String> = presence_update.changed_keys.iter().cloned().collect();
            let ignore_reason =
                normalized_reason.starts_with("periodic") || normalized_reason == "heartbeat";
            let host_changed = changed.contains("host");
            let ip_changed = changed.contains("ip");
            let version_changed = changed.contains("version");
            let mode_changed = changed.contains("mode");
            let reason_changed = changed.contains("reason") && !ignore_reason;
            let has_changes =
                host_changed || ip_changed || version_changed || mode_changed || reason_changed;
            if has_changes {
                let context_changed =
                    is_system_event_context_changed(&session_key, Some(&presence_update.key))
                        .map_err(|e| rpc_error("INTERNAL", &e))?;
                let mut parts: Vec<String> = Vec::new();
                if context_changed || host_changed || ip_changed {
                    let host_label = presence_update
                        .next
                        .host
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .unwrap_or("Unknown");
                    let ip_label = presence_update
                        .next
                        .ip
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty());
                    parts.push(format!(
                        "Node: {}{}",
                        host_label,
                        ip_label.map(|ip| format!(" ({})", ip)).unwrap_or_default()
                    ));
                }
                if version_changed {
                    let version = presence_update
                        .next
                        .version
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .unwrap_or("unknown");
                    parts.push(format!("app {}", version));
                }
                if mode_changed {
                    let mode = presence_update
                        .next
                        .mode
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .unwrap_or("unknown");
                    parts.push(format!("mode {}", mode));
                }
                if reason_changed {
                    let reason = reason_value
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .unwrap_or("event");
                    parts.push(format!("reason {}", reason));
                }
                let delta_text = parts.join(" · ");
                if !delta_text.is_empty() {
                    enqueue_system_event_entry(
                        &session_key,
                        &delta_text,
                        Some(&presence_update.key),
                    )
                    .map_err(|e| rpc_error("INTERNAL", &e))?;
                }
            }
        } else {
            enqueue_system_event_entry(&session_key, text, None)
                .map_err(|e| rpc_error("INTERNAL", &e))?;
        }
        return Ok(serde_json::json!({ "ok": true }));
    }

    let event = p["event"].as_str().unwrap_or("unknown");
    let payload = p.get("payload").cloned().unwrap_or(serde_json::Value::Null);
    let accepted = serde_json::json!({
        "accepted": true,
        "event": event,
        "payload": payload,
        "timestamp": chrono::Utc::now().timestamp_millis(),
    });

    let heartbeat_like = event.eq_ignore_ascii_case("heartbeat")
        || event.eq_ignore_ascii_case("last-heartbeat")
        || event.to_ascii_lowercase().contains("heartbeat");
    if heartbeat_like {
        set_global_last_heartbeat_event(accepted.clone());
        *state.last_heartbeat_event.write().await = Some(accepted.clone());
    }

    Ok(accepted)
}

async fn rpc_last_heartbeat(state: &HttpState) -> RpcResult {
    if let Some(v) = get_global_last_heartbeat_event() {
        return Ok(v);
    }
    if let Some(v) = state.last_heartbeat_event.read().await.clone() {
        return Ok(v);
    }
    Ok(serde_json::json!({
        "timestamp": chrono::Utc::now().timestamp_millis(),
        "alive": false,
        "seen": false,
    }))
}

async fn rpc_set_heartbeats(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let enabled = p
        .get("enabled")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "enabled(boolean) required"))?;
    GLOBAL_HEARTBEATS_ENABLED.store(enabled, Ordering::Relaxed);
    *state.heartbeats_enabled.write().await = enabled;
    Ok(serde_json::json!({
        "ok": true,
        "enabled": enabled,
    }))
}

fn parse_wake_mode(raw: &str) -> Option<&'static str> {
    let mode = raw.trim();
    if mode.eq_ignore_ascii_case("now") {
        return Some("now");
    }
    if mode.eq_ignore_ascii_case("next-heartbeat") {
        return Some("next-heartbeat");
    }
    None
}

async fn rpc_wake(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let mode = p
        .get("mode")
        .and_then(|v| v.as_str())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "mode(now|next-heartbeat) required"))?;
    let mode = parse_wake_mode(mode)
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "mode must be now or next-heartbeat"))?
        .to_string();
    let text = p
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "text(non-empty string) required"))?
        .trim()
        .to_string();
    if text.is_empty() {
        return Err(rpc_error(
            "INVALID_PARAMS",
            "text(non-empty string) required",
        ));
    }
    let session_key = normalize_system_session_key(
        p.get("sessionKey")
            .or_else(|| p.get("key"))
            .and_then(|v| v.as_str()),
    );
    let reason = p
        .get("reason")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("wake")
        .to_string();
    let context_key = p.get("contextKey").and_then(|v| v.as_str());
    enqueue_system_event_entry(&session_key, &text, context_key)
        .map_err(|e| rpc_error("INTERNAL", &e))?;
    if mode.eq_ignore_ascii_case("now") {
        let agent_id = p
            .get("agentId")
            .or_else(|| p.get("agent"))
            .and_then(|v| v.as_str());
        queue_pending_heartbeat_wake(Some(&reason), agent_id, Some(&session_key))
            .map_err(|e| rpc_error("INTERNAL", &e))?;
    }

    let payload = serde_json::json!({
        "ok": true,
        "mode": mode,
        "reason": reason,
        "text": text,
        "sessionKey": session_key,
        "ts": now_epoch_ms(),
    });
    set_global_last_heartbeat_event(payload.clone());
    *state.last_heartbeat_event.write().await = Some(payload.clone());
    Ok(payload)
}

async fn rpc_usage_tokens(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let session_key = p["sessionKey"].as_str();
    let total_requests = state
        .metrics
        .request_count
        .load(std::sync::atomic::Ordering::Relaxed);
    let total_errors = state
        .metrics
        .error_count
        .load(std::sync::atomic::Ordering::Relaxed);
    let avg_latency_ms = state.metrics.average_response_time();
    Ok(serde_json::json!({
        "session_key": session_key,
        "total_requests": total_requests,
        "total_errors": total_errors,
        "avg_latency_ms": avg_latency_ms,
        "source": "gateway_metrics",
    }))
}

async fn rpc_usage_status(_p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let requests = state
        .metrics
        .request_count
        .load(std::sync::atomic::Ordering::Relaxed);
    let errors = state
        .metrics
        .error_count
        .load(std::sync::atomic::Ordering::Relaxed);
    let avg_latency_ms = state.metrics.average_response_time();
    let usage = state.usage_snapshot.read().await.clone();
    Ok(serde_json::json!({
        "requests": requests,
        "errors": errors,
        "avgLatencyMs": avg_latency_ms,
        "errorRate": if requests == 0 { 0.0 } else { errors as f64 / requests as f64 },
        "usage": usage,
    }))
}

async fn rpc_usage_cost(_p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let usage = state.usage_snapshot.read().await.clone();
    Ok(serde_json::json!({
        "updatedAt": usage.updated_at,
        "totals": usage.totals,
        "source": "gateway_runtime_usage",
    }))
}

// ── Config RPCs ─────────────────────────────────────────────────────────

async fn rpc_config_get(state: &HttpState) -> RpcResult {
    let Some(ref cfg) = state.full_config else {
        return Err(rpc_error("NO_CONFIG", "No configuration loaded"));
    };
    let config = cfg.read().await;
    serde_json::to_value(&*config).map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_config_set(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let Some(ref cfg) = state.full_config else {
        return Err(rpc_error("NO_CONFIG", "No configuration loaded"));
    };
    let new_config: oclaw_config::settings::Config = serde_json::from_value(p.clone())
        .map_err(|e| rpc_error("INVALID_PARAMS", &format!("Invalid config: {}", e)))?;

    {
        let mut config = cfg.write().await;
        *config = new_config;
    }

    // Persist to disk if path is available
    if let Some(ref path) = state.config_path {
        let config = cfg.read().await;
        let json = serde_json::to_string_pretty(&*config)
            .map_err(|e| rpc_error("INTERNAL", &e.to_string()))?;
        std::fs::write(path, &json)
            .map_err(|e| rpc_error("INTERNAL", &format!("Failed to write config: {}", e)))?;
    }

    Ok(serde_json::json!({"updated": true}))
}

fn deep_merge_json(base: &mut serde_json::Value, patch: &serde_json::Value) {
    match (base, patch) {
        (serde_json::Value::Object(base_map), serde_json::Value::Object(patch_map)) => {
            for (k, v) in patch_map {
                if let Some(existing) = base_map.get_mut(k) {
                    deep_merge_json(existing, v);
                } else {
                    base_map.insert(k.clone(), v.clone());
                }
            }
        }
        (base_slot, patch_val) => {
            *base_slot = patch_val.clone();
        }
    }
}

async fn persist_full_config(state: &HttpState) -> Result<(), ErrorDetails> {
    let (Some(cfg), Some(path)) = (state.full_config.as_ref(), state.config_path.as_ref()) else {
        return Ok(());
    };
    let config = cfg.read().await;
    let json = serde_json::to_string_pretty(&*config)
        .map_err(|e| rpc_error("INTERNAL", &e.to_string()))?;
    std::fs::write(path, &json)
        .map_err(|e| rpc_error("INTERNAL", &format!("Failed to write config: {}", e)))?;
    Ok(())
}

async fn rpc_config_patch(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let Some(cfg) = state.full_config.as_ref() else {
        return Err(rpc_error("NO_CONFIG", "No configuration loaded"));
    };

    let patch = if let Some(v) = p.get("patch") { v } else { p };
    if !patch.is_object() {
        return Err(rpc_error(
            "INVALID_PARAMS",
            "config.patch expects an object or { patch: object }",
        ));
    }

    let mut guard = cfg.write().await;
    let mut current_json =
        serde_json::to_value(&*guard).map_err(|e| rpc_error("INTERNAL", &e.to_string()))?;
    deep_merge_json(&mut current_json, patch);
    let next: oclaw_config::settings::Config = serde_json::from_value(current_json.clone())
        .map_err(|e| rpc_error("INVALID_PARAMS", &format!("Patch invalid: {}", e)))?;
    *guard = next;
    drop(guard);
    persist_full_config(state).await?;

    Ok(serde_json::json!({
        "updated": true,
        "method": "patch",
        "config": current_json,
    }))
}

async fn rpc_config_apply(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    // Node semantics: config.apply behaves like patching a concrete object payload.
    rpc_config_patch(p, state).await
}

async fn rpc_config_schema(_state: &HttpState) -> RpcResult {
    let defaults = serde_json::to_value(oclaw_config::settings::Config::default())
        .map_err(|e| rpc_error("INTERNAL", &e.to_string()))?;
    Ok(serde_json::json!({
        "schemaVersion": 1,
        "topLevelKeys": [
            "meta","env","wizard","diagnostics","logging","update","browser","ui","auth",
            "models","nodeHost","agents","tools","bindings","broadcast","audio","media","messages",
            "commands","approvals","session","cron","hooks","web","channels","discovery",
            "canvasHost","talk","gateway","plugins","memory"
        ],
        "defaults": defaults,
    }))
}

// ── Models RPCs ─────────────────────────────────────────────────────────

async fn rpc_models_list(state: &HttpState) -> RpcResult {
    let Some(ref provider) = state.llm_provider else {
        return Ok(serde_json::json!({"models": []}));
    };
    let models = provider
        .list_models()
        .await
        .unwrap_or_else(|_| provider.supported_models());
    Ok(serde_json::json!({
        "models": models,
        "default": provider.default_model(),
    }))
}

// ── Channel RPCs ────────────────────────────────────────────────────────

async fn rpc_channels_status(state: &HttpState) -> RpcResult {
    let Some(ref cm) = state.channel_manager else {
        return Ok(serde_json::json!({"channels": []}));
    };
    let mgr = cm.read().await;
    let names = mgr.list().await;
    let status_map = mgr.get_status().await;
    let mut channels = Vec::new();
    for name in &names {
        if let Some(ch) = mgr.get(name).await {
            let ch = ch.read().await;
            let status = status_map.get(name).copied().unwrap_or_else(|| ch.status());
            channels.push(serde_json::json!({
                "name": name,
                "type": ch.channel_type(),
                "status": format!("{:?}", status),
            }));
        }
    }
    Ok(serde_json::json!({"channels": channels}))
}

async fn rpc_channels_logout(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let channel = p["channel"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'channel'"))?;
    let Some(ref cm) = state.channel_manager else {
        return Err(rpc_error("NO_CHANNELS", "Channel manager not configured"));
    };
    let mgr = cm.read().await;
    let ch = mgr
        .get(channel)
        .await
        .ok_or_else(|| rpc_error("NOT_FOUND", &format!("Channel not found: {}", channel)))?;
    let mut ch = ch.write().await;
    ch.disconnect()
        .await
        .map_err(|e| rpc_error("CHANNEL_ERROR", &e.to_string()))?;
    Ok(serde_json::json!({
        "channel": channel,
        "cleared": true,
        "status": format!("{:?}", ch.status()),
    }))
}

// ── Cron RPCs ───────────────────────────────────────────────────────────

async fn rpc_cron_list(state: &HttpState) -> RpcResult {
    let Some(ref svc) = state.cron_service else {
        return Ok(serde_json::json!({"jobs": []}));
    };
    let jobs = svc.list().await;
    serde_json::to_value(&jobs).map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_cron_create(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let svc = state
        .cron_service
        .as_ref()
        .ok_or_else(|| rpc_error("NO_CRON", "Cron service not configured"))?;

    // Accept a full CronJob JSON or build one from simple params
    let job: oclaw_cron_core::CronJob = serde_json::from_value(p.clone())
        .map_err(|e| rpc_error("INVALID_PARAMS", &format!("Invalid job: {}", e)))?;

    let created = svc
        .add(job)
        .await
        .map_err(|e| rpc_error("CRON_ERROR", &e.to_string()))?;
    serde_json::to_value(&created).map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_cron_update(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let svc = state
        .cron_service
        .as_ref()
        .ok_or_else(|| rpc_error("NO_CRON", "Cron service not configured"))?;
    let id = p["id"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    let patch = oclaw_cron_core::service::CronJobPatch {
        name: p
            .get("name")
            .and_then(|v| v.as_str())
            .map(ToString::to_string),
        enabled: p.get("enabled").and_then(|v| v.as_bool()),
        schedule: p
            .get("schedule")
            .cloned()
            .map(serde_json::from_value)
            .transpose()
            .map_err(|e| rpc_error("INVALID_PARAMS", &format!("Invalid schedule: {}", e)))?,
    };
    let updated = svc
        .update(id, patch)
        .await
        .map_err(|e| rpc_error("CRON_ERROR", &e.to_string()))?;
    serde_json::to_value(&updated).map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_cron_delete(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let svc = state
        .cron_service
        .as_ref()
        .ok_or_else(|| rpc_error("NO_CRON", "Cron service not configured"))?;
    let id = p["id"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    svc.remove(id)
        .await
        .map_err(|e| rpc_error("CRON_ERROR", &e.to_string()))?;
    Ok(serde_json::json!({"deleted": true}))
}

async fn rpc_cron_trigger(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let svc = state
        .cron_service
        .as_ref()
        .ok_or_else(|| rpc_error("NO_CRON", "Cron service not configured"))?;
    let id = p["id"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    svc.trigger(id)
        .await
        .map_err(|e| rpc_error("CRON_ERROR", &e.to_string()))?;
    Ok(serde_json::json!({"triggered": true}))
}

async fn rpc_cron_run(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let id = p["id"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    let mode = p["mode"].as_str().unwrap_or("force");
    if mode.eq_ignore_ascii_case("force")
        && let Some(ref scheduler) = state.cron_scheduler
    {
        scheduler
            .run_once(id)
            .await
            .map_err(|e| rpc_error("CRON_ERROR", &e.to_string()))?;
    } else if let Some(ref svc) = state.cron_service {
        svc.trigger(id)
            .await
            .map_err(|e| rpc_error("CRON_ERROR", &e.to_string()))?;
    } else {
        return Err(rpc_error("NO_CRON", "Cron service not configured"));
    }

    let latest = if let Some(ref run_log) = state.cron_run_log {
        run_log.read(id, 1).await.ok().and_then(|mut v| v.pop())
    } else {
        None
    };

    Ok(serde_json::json!({
        "ok": true,
        "id": id,
        "mode": mode,
        "latestRun": latest,
    }))
}

async fn rpc_cron_logs(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let run_log = state
        .cron_run_log
        .as_ref()
        .ok_or_else(|| rpc_error("NO_CRON", "Cron run log not available"))?;
    let id = p["id"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    let limit = p["limit"].as_u64().unwrap_or(50) as usize;
    let entries = run_log
        .read(id, limit)
        .await
        .map_err(|e| rpc_error("CRON_ERROR", &e.to_string()))?;
    serde_json::to_value(&entries).map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_cron_status(state: &HttpState) -> RpcResult {
    let scheduler_running = state
        .cron_scheduler
        .as_ref()
        .map(|s| s.is_running())
        .unwrap_or(false);
    let job_count = match &state.cron_service {
        Some(svc) => svc.list().await.len(),
        None => 0,
    };
    Ok(serde_json::json!({
        "scheduler_running": scheduler_running,
        "job_count": job_count,
    }))
}

// ── Skills RPCs ─────────────────────────────────────────────────────────

fn normalize_agent_id(raw: &str) -> String {
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return "main".to_string();
    }
    let mut out = String::with_capacity(trimmed.len());
    let mut prev_dash = false;
    for ch in trimmed.chars() {
        let valid = ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-';
        if valid {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "main".to_string()
    } else {
        out.chars().take(64).collect()
    }
}

fn resolve_user_path(input: &str) -> PathBuf {
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

fn resolve_state_dir_from_env() -> PathBuf {
    let override_dir = std::env::var("OCLAWS_STATE_DIR")
        .ok()
        .or_else(|| std::env::var("OPENCLAW_STATE_DIR").ok())
        .or_else(|| std::env::var("CLAWDBOT_STATE_DIR").ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    if let Some(dir) = override_dir {
        return resolve_user_path(&dir);
    }
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".oclaw")
}

fn resolve_default_agent_workspace_dir_from_env() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let profile = std::env::var("OCLAWS_PROFILE")
        .ok()
        .or_else(|| std::env::var("OPENCLAW_PROFILE").ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    if let Some(profile) = profile
        && !profile.eq_ignore_ascii_case("default")
    {
        return home.join(".oclaw").join(format!("workspace-{}", profile));
    }
    home.join(".oclaw").join("workspace")
}

fn resolve_default_agent_id_from_config(cfg: &oclaw_config::settings::Config) -> String {
    let list = cfg
        .agents
        .as_ref()
        .and_then(|v| v.get("list"))
        .and_then(|v| v.as_array());
    let Some(list) = list else {
        return "main".to_string();
    };

    let mut first: Option<String> = None;
    for entry in list {
        let Some(obj) = entry.as_object() else {
            continue;
        };
        let raw_id = obj.get("id").and_then(|v| v.as_str()).unwrap_or("");
        let id = normalize_agent_id(raw_id);
        if id == "main" && raw_id.trim().is_empty() {
            continue;
        }
        if first.is_none() {
            first = Some(id.clone());
        }
        if obj
            .get("default")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return id;
        }
    }
    first.unwrap_or_else(|| "main".to_string())
}

fn resolve_agent_workspace_dir_from_config(
    cfg: &oclaw_config::settings::Config,
    agent_id: &str,
    workspace_hint: Option<&Path>,
) -> PathBuf {
    let normalized_id = normalize_agent_id(agent_id);
    if let Some(list) = cfg
        .agents
        .as_ref()
        .and_then(|v| v.get("list"))
        .and_then(|v| v.as_array())
    {
        for entry in list {
            let Some(obj) = entry.as_object() else {
                continue;
            };
            let entry_id = normalize_agent_id(obj.get("id").and_then(|v| v.as_str()).unwrap_or(""));
            if entry_id != normalized_id {
                continue;
            }
            if let Some(workspace) = obj
                .get("workspace")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                return resolve_user_path(workspace);
            }
        }
    }

    let default_agent_id = resolve_default_agent_id_from_config(cfg);
    if normalized_id == default_agent_id {
        if let Some(fallback) = cfg
            .agents
            .as_ref()
            .and_then(|v| v.pointer("/defaults/workspace"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            return resolve_user_path(fallback);
        }
        if let Some(root) = workspace_hint {
            return root.to_path_buf();
        }
        return resolve_default_agent_workspace_dir_from_env();
    }

    resolve_state_dir_from_env().join(format!("workspace-{}", normalized_id))
}

async fn resolve_skills_workspace_for_agent(
    state: &HttpState,
    requested_agent_id: Option<&str>,
) -> (String, Option<PathBuf>) {
    let normalized_requested = requested_agent_id.map(normalize_agent_id);
    if let Some(cfg) = state.full_config.as_ref() {
        let guard = cfg.read().await;
        let effective_agent_id =
            normalized_requested.unwrap_or_else(|| resolve_default_agent_id_from_config(&guard));
        let workspace = resolve_agent_workspace_dir_from_config(
            &guard,
            &effective_agent_id,
            state.workspace.as_ref().map(|w| w.root()),
        );
        return (effective_agent_id, Some(workspace));
    }
    (
        normalized_requested.unwrap_or_else(|| "main".to_string()),
        state
            .workspace
            .as_ref()
            .map(|w| w.root().to_path_buf())
            .or_else(|| std::env::current_dir().ok()),
    )
}

fn collect_agent_workspace_dirs_from_config(
    cfg: &oclaw_config::settings::Config,
    workspace_hint: Option<&Path>,
) -> Vec<PathBuf> {
    let mut out = std::collections::BTreeSet::new();
    for agent_id in collect_agent_ids_from_config(cfg) {
        let dir = resolve_agent_workspace_dir_from_config(cfg, &agent_id, workspace_hint);
        out.insert(dir.to_string_lossy().to_string());
    }
    out.into_iter().map(PathBuf::from).collect()
}

fn json_value_truthy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Null => false,
        serde_json::Value::Bool(v) => *v,
        serde_json::Value::Number(n) => n
            .as_i64()
            .map(|v| v != 0)
            .or_else(|| n.as_u64().map(|v| v != 0))
            .or_else(|| n.as_f64().map(|v| v != 0.0))
            .unwrap_or(false),
        serde_json::Value::String(s) => !s.trim().is_empty(),
        serde_json::Value::Array(arr) => !arr.is_empty(),
        serde_json::Value::Object(obj) => !obj.is_empty(),
    }
}

fn config_path_truthy(root: &serde_json::Value, path: &str) -> bool {
    let mut cur = root;
    for raw in path.split('.') {
        let seg = raw.trim();
        if seg.is_empty() {
            continue;
        }
        let Some(obj) = cur.as_object() else {
            return false;
        };
        let Some(next) = obj.get(seg) else {
            return false;
        };
        cur = next;
    }
    json_value_truthy(cur)
}

async fn load_config_truthy_snapshot(state: &HttpState) -> Option<serde_json::Value> {
    let cfg = state.full_config.as_ref()?;
    let guard = cfg.read().await;
    serde_json::to_value(&*guard).ok()
}

fn skill_tier_str(tier: oclaw_skills_core::discovery::SkillTier) -> &'static str {
    match tier {
        oclaw_skills_core::discovery::SkillTier::Workspace => "workspace",
        oclaw_skills_core::discovery::SkillTier::User => "user",
        oclaw_skills_core::discovery::SkillTier::Bundled => "bundled",
    }
}

fn resolve_skill_install_id(
    spec: &oclaw_skills_core::manifest::InstallSpec,
    index: usize,
) -> String {
    let explicit = spec.id.as_deref().map(str::trim).filter(|v| !v.is_empty());
    explicit
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("{}-{}", spec.kind, index))
}

fn current_platform_id_for_skills() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

fn binary_exists(bin: &str) -> bool {
    let trimmed = bin.trim();
    if trimmed.is_empty() {
        return false;
    }
    let candidate = std::path::Path::new(trimmed);
    if candidate.components().count() > 1 {
        return candidate.exists() && candidate.is_file();
    }
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    #[cfg(target_os = "windows")]
    let exts: Vec<String> = std::env::var("PATHEXT")
        .ok()
        .map(|v| {
            v.split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| s.to_ascii_lowercase())
                .collect()
        })
        .unwrap_or_else(|| vec![".exe".to_string(), ".cmd".to_string(), ".bat".to_string()]);
    for dir in std::env::split_paths(&path) {
        let plain = dir.join(trimmed);
        if plain.exists() && plain.is_file() {
            return true;
        }
        #[cfg(target_os = "windows")]
        {
            for ext in &exts {
                let with_ext = dir.join(format!("{}{}", trimmed, ext));
                if with_ext.exists() && with_ext.is_file() {
                    return true;
                }
            }
        }
    }
    false
}

fn install_spec_matches_platform(spec: &oclaw_skills_core::manifest::InstallSpec) -> bool {
    if spec.os.is_empty() {
        return true;
    }
    let current = current_platform_id_for_skills();
    let current_raw = std::env::consts::OS;
    spec.os.iter().any(|os| {
        os.eq_ignore_ascii_case(current)
            || os.eq_ignore_ascii_case(current_raw)
            || (current == "darwin" && os.eq_ignore_ascii_case("macos"))
    })
}

fn build_install_label(
    spec: &oclaw_skills_core::manifest::InstallSpec,
    node_manager: &str,
) -> String {
    if let Some(label) = spec
        .label
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return label.to_string();
    }
    match spec.kind.as_str() {
        "brew" => spec
            .formula
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|f| format!("Install {} (brew)", f))
            .unwrap_or_else(|| "Run installer".to_string()),
        "node" => spec
            .package
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|pkg| format!("Install {} ({})", pkg, node_manager))
            .unwrap_or_else(|| "Run installer".to_string()),
        "go" => spec
            .module
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|m| format!("Install {} (go)", m))
            .unwrap_or_else(|| "Run installer".to_string()),
        "uv" => spec
            .package
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|pkg| format!("Install {} (uv)", pkg))
            .unwrap_or_else(|| "Run installer".to_string()),
        "download" => spec
            .url
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|url| {
                let last = url.rsplit('/').next().unwrap_or(url);
                format!("Download {}", last)
            })
            .unwrap_or_else(|| "Run installer".to_string()),
        _ => "Run installer".to_string(),
    }
}

fn normalize_skill_install_options(
    specs: &[oclaw_skills_core::manifest::InstallSpec],
    prefer_brew: bool,
    node_manager: &str,
) -> Vec<serde_json::Value> {
    if specs.is_empty() {
        return Vec::new();
    }
    let filtered: Vec<(usize, &oclaw_skills_core::manifest::InstallSpec)> = specs
        .iter()
        .enumerate()
        .filter(|(_, spec)| install_spec_matches_platform(spec))
        .collect();
    if filtered.is_empty() {
        return Vec::new();
    }

    let all_download = filtered.iter().all(|(_, spec)| spec.kind == "download");
    let selected: Vec<(usize, &oclaw_skills_core::manifest::InstallSpec)> = if all_download {
        filtered
    } else {
        let brew_available = binary_exists("brew");
        let uv_available = binary_exists("uv");
        let go_available = binary_exists("go");
        let node_available = binary_exists(node_manager);

        let prefer = |kind: &str, required_available: bool| {
            if !required_available {
                return None;
            }
            filtered
                .iter()
                .find(|(_, spec)| spec.kind.eq_ignore_ascii_case(kind))
                .copied()
        };

        let chosen = (if prefer_brew {
            prefer("brew", brew_available)
        } else {
            None
        })
        .or_else(|| prefer("uv", uv_available))
        .or_else(|| prefer("node", node_available))
        .or_else(|| prefer("brew", brew_available))
        .or_else(|| prefer("go", go_available))
        .or_else(|| {
            filtered
                .iter()
                .find(|(_, spec)| spec.kind == "download")
                .copied()
        })
        .or_else(|| filtered.first().copied());
        chosen.into_iter().collect()
    };

    selected
        .into_iter()
        .map(|(idx, spec)| {
            serde_json::json!({
                "id": resolve_skill_install_id(spec, idx),
                "kind": spec.kind,
                "label": build_install_label(spec, node_manager),
                "bins": spec.bins,
            })
        })
        .collect()
}

fn resolve_skills_install_preferences(snapshot: Option<&serde_json::Value>) -> (bool, String) {
    let prefer_brew = snapshot
        .and_then(|root| root.pointer("/skills/install/preferBrew"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let configured_manager = snapshot
        .and_then(|root| root.pointer("/skills/install/nodeManager"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_ascii_lowercase());
    let manager = configured_manager
        .filter(|m| ["npm", "pnpm", "yarn", "bun"].iter().any(|x| x == m))
        .and_then(|m| if binary_exists(&m) { Some(m) } else { None })
        .or_else(|| {
            ["pnpm", "yarn", "bun", "npm"]
                .iter()
                .find(|m| binary_exists(m))
                .map(|m| (*m).to_string())
        })
        .unwrap_or_else(|| "npm".to_string());
    (prefer_brew, manager)
}

fn resolve_skill_disabled_from_config_snapshot(
    snapshot: Option<&serde_json::Value>,
    skill_key: &str,
    fallback_name: &str,
) -> Option<bool> {
    let entries = snapshot
        .and_then(|root| root.get("skills"))
        .and_then(|v| v.get("entries"))
        .and_then(|v| v.as_object())?;
    let entry = entries
        .get(skill_key)
        .or_else(|| entries.get(fallback_name))
        .and_then(|v| v.as_object())?;
    entry
        .get("enabled")
        .and_then(|v| v.as_bool())
        .map(|enabled| !enabled)
}

fn resolve_skill_disabled_from_overrides(
    overrides: &HashMap<String, serde_json::Value>,
    skill_key: &str,
    fallback_name: &str,
) -> Option<bool> {
    let entry = overrides
        .get(skill_key)
        .or_else(|| overrides.get(fallback_name))
        .and_then(|v| v.as_object())?;
    entry
        .get("enabled")
        .and_then(|v| v.as_bool())
        .map(|enabled| !enabled)
}

fn collect_agent_ids_from_config(
    cfg: &oclaw_config::settings::Config,
) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    out.insert("main".to_string());
    out.insert(resolve_default_agent_id_from_config(cfg));

    if let Some(list) = cfg
        .agents
        .as_ref()
        .and_then(|v| v.get("list"))
        .and_then(|v| v.as_array())
    {
        for entry in list {
            if let Some(id) = entry
                .get("id")
                .and_then(|v| v.as_str())
                .map(normalize_agent_id)
            {
                out.insert(id);
            }
        }
    }

    if let Some(agents) = cfg.agents.as_ref()
        && let Some(obj) = agents.as_object()
    {
        let reserved = ["list", "defaults", "entries"];
        for (key, value) in obj {
            let trimmed = key.trim();
            if trimmed.is_empty() || reserved.iter().any(|r| r.eq_ignore_ascii_case(trimmed)) {
                continue;
            }
            if value.is_object() {
                out.insert(normalize_agent_id(trimmed));
            }
        }
        if let Some(entries) = obj.get("entries").and_then(|v| v.as_object()) {
            for key in entries.keys() {
                let trimmed = key.trim();
                if !trimmed.is_empty() {
                    out.insert(normalize_agent_id(trimmed));
                }
            }
        }
    }
    out
}

async fn rpc_skills_status(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let requested_agent_id = p
        .get("agentId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(normalize_agent_id);
    if let Some(agent_id) = requested_agent_id.as_ref() {
        let mut known = std::collections::BTreeSet::new();
        if let Some(cfg) = state.full_config.as_ref() {
            let guard = cfg.read().await;
            known.extend(collect_agent_ids_from_config(&guard));
        }
        let manager_agents = state
            .gateway_server
            .session_manager
            .read()
            .await
            .list_agents()
            .unwrap_or_default();
        for agent in manager_agents {
            let trimmed = agent.id.trim();
            if !trimmed.is_empty() {
                known.insert(normalize_agent_id(trimmed));
            }
        }
        if !known.is_empty() && !known.contains(agent_id) {
            return Err(rpc_error(
                "INVALID_PARAMS",
                &format!("unknown agent id \"{}\"", agent_id),
            ));
        }
    }

    let (effective_agent_id, workspace) =
        resolve_skills_workspace_for_agent(state, requested_agent_id.as_deref()).await;
    let config_snapshot = load_config_truthy_snapshot(state).await;
    let (prefer_brew, node_manager) = resolve_skills_install_preferences(config_snapshot.as_ref());
    let config_snapshot_for_lookup = config_snapshot.clone();
    let loader = oclaw_skills_core::WorkspaceSkillLoader::new(workspace.as_deref())
        .with_config_lookup(move |path| {
            config_snapshot_for_lookup
                .as_ref()
                .map(|root| config_path_truthy(root, path))
                .unwrap_or(false)
        });
    let all = loader.load_all_with_gates().await;
    let overrides = state.skill_overrides.read().await.clone();
    let discovered: Vec<serde_json::Value> = all
        .iter()
        .map(|s| {
            let metadata = s
                .skill
                .manifest
                .metadata
                .as_ref()
                .and_then(|m| m.openclaw.as_ref());
            let source = match s.skill.tier {
                oclaw_skills_core::discovery::SkillTier::Workspace => "workspace",
                oclaw_skills_core::discovery::SkillTier::User => "user",
                oclaw_skills_core::discovery::SkillTier::Bundled => "openclaw-bundled",
            };
            let bundled = matches!(
                s.skill.tier,
                oclaw_skills_core::discovery::SkillTier::Bundled
            );
            let skill_key = metadata
                .as_ref()
                .and_then(|oc| oc.skill_key.as_ref())
                .map(|v| v.trim())
                .filter(|v| !v.is_empty())
                .map(ToString::to_string)
                .unwrap_or_else(|| s.skill.manifest.name.clone());
            let always = metadata.as_ref().and_then(|oc| oc.always).unwrap_or(false);
            let disabled = resolve_skill_disabled_from_overrides(
                &overrides,
                &skill_key,
                &s.skill.manifest.name,
            )
            .or_else(|| {
                resolve_skill_disabled_from_config_snapshot(
                    config_snapshot.as_ref(),
                    &skill_key,
                    &s.skill.manifest.name,
                )
            })
            .unwrap_or(false);
            let blocked_by_allowlist = false;
            let required_bins = metadata
                .as_ref()
                .and_then(|oc| oc.requires.as_ref())
                .map(|req| req.bins.clone())
                .unwrap_or_default();
            let required_env = metadata
                .as_ref()
                .and_then(|oc| oc.requires.as_ref())
                .map(|req| req.env.clone())
                .unwrap_or_default();
            let required_config = metadata
                .as_ref()
                .and_then(|oc| oc.requires.as_ref())
                .map(|req| req.config.clone())
                .unwrap_or_default();
            let config_checks: Vec<serde_json::Value> = required_config
                .iter()
                .map(|path| {
                    let missing = s.gate_result.missing_config.iter().any(|m| m == path);
                    serde_json::json!({
                        "path": path,
                        "satisfied": !missing,
                    })
                })
                .collect();
            let install = metadata
                .as_ref()
                .map(|oc| normalize_skill_install_options(&oc.install, prefer_brew, &node_manager))
                .unwrap_or_default();
            let gate_passed = s.gate_result.passed;
            let final_eligible = gate_passed && !disabled && !blocked_by_allowlist;
            serde_json::json!({
                "name": s.skill.manifest.name,
                "description": s.skill.manifest.description,
                "source": source,
                "bundled": bundled,
                "filePath": std::path::Path::new(&s.skill.manifest.source_dir).join("SKILL.md"),
                "baseDir": s.skill.manifest.source_dir,
                "skillKey": skill_key,
                "primaryEnv": metadata.as_ref().and_then(|oc| oc.primary_env.clone()),
                "emoji": metadata.as_ref().and_then(|oc| oc.emoji.clone()),
                "homepage": s.skill.manifest.homepage,
                "always": always,
                "disabled": disabled,
                "blockedByAllowlist": blocked_by_allowlist,
                "tier": skill_tier_str(s.skill.tier),
                "eligible": final_eligible,
                "requirements": {
                    "bins": required_bins,
                    "env": required_env,
                    "config": required_config,
                },
                "missing": {
                    "bins": s.gate_result.missing_bins,
                    "env": s.gate_result.missing_env,
                    "config": s.gate_result.missing_config,
                },
                "configChecks": config_checks,
                "install": install,
                "missingBins": s.gate_result.missing_bins,
                "missingEnv": s.gate_result.missing_env,
                "missingConfig": s.gate_result.missing_config,
                "osMismatch": s.gate_result.os_mismatch,
            })
        })
        .collect();
    let eligible = discovered
        .iter()
        .filter(|entry| entry.get("eligible").and_then(|v| v.as_bool()) == Some(true))
        .count();

    let installed = if let Some(ref registry) = state.skill_registry {
        registry.list().await
    } else {
        Vec::new()
    };

    let discovered_count = discovered.len();

    Ok(serde_json::json!({
        "agentId": effective_agent_id,
        "workspaceDir": workspace,
        "managedSkillsDir": dirs::home_dir()
            .map(|h| h.join(".oclaw").join("skills").to_string_lossy().to_string()),
        "skills": discovered.clone(),
        "installed": installed,
        "available": discovered_count,
        "eligible": eligible,
        "discovered": discovered,
        "runtimeOverrides": overrides,
    }))
}

async fn rpc_skills_install(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let name = p["name"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'name'"))?;
    let install_id = p
        .get("installId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'installId'"))?
        .to_string();
    let timeout = p["timeout"].as_u64().unwrap_or(300);
    let requested_agent_id = p
        .get("agentId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(normalize_agent_id);
    if let Some(agent_id) = requested_agent_id.as_ref() {
        let mut known = std::collections::BTreeSet::new();
        if let Some(cfg) = state.full_config.as_ref() {
            let guard = cfg.read().await;
            known.extend(collect_agent_ids_from_config(&guard));
        }
        if !known.is_empty() && !known.contains(agent_id) {
            return Err(rpc_error(
                "INVALID_PARAMS",
                &format!("unknown agent id \"{}\"", agent_id),
            ));
        }
    }
    let (effective_agent_id, workspace) =
        resolve_skills_workspace_for_agent(state, requested_agent_id.as_deref()).await;
    let config_snapshot = load_config_truthy_snapshot(state).await;
    let loader = oclaw_skills_core::WorkspaceSkillLoader::new(workspace.as_deref())
        .with_config_lookup(move |path| {
            config_snapshot
                .as_ref()
                .map(|root| config_path_truthy(root, path))
                .unwrap_or(false)
        });
    let skill = loader
        .load_all_with_gates()
        .await
        .into_iter()
        .find(|s| s.skill.manifest.name == name)
        .ok_or_else(|| rpc_error("NOT_FOUND", &format!("Skill not found: {}", name)))?;

    if !skill.gate_result.passed {
        return Err(rpc_error(
            "GATE_FAILED",
            &format!(
                "Skill '{}' failed gates (missing bins: {}, missing env: {}, missing config: {}, os mismatch: {})",
                name,
                skill.gate_result.missing_bins.join(","),
                skill.gate_result.missing_env.join(","),
                skill.gate_result.missing_config.join(","),
                skill.gate_result.os_mismatch
            ),
        ));
    }

    let specs = skill
        .skill
        .manifest
        .metadata
        .as_ref()
        .and_then(|m| m.openclaw.as_ref())
        .map(|oc| oc.install.clone())
        .unwrap_or_default();
    if specs.is_empty() {
        return Ok(serde_json::json!({
            "installed": false,
            "name": name,
            "installId": install_id,
            "agentId": effective_agent_id,
            "workspaceDir": workspace,
            "note": "No install specs declared in SKILL.md",
            "results": [],
        }));
    }

    let (matched_idx, matched_spec) = specs
        .iter()
        .enumerate()
        .find(|(idx, spec)| resolve_skill_install_id(spec, *idx) == install_id)
        .ok_or_else(|| rpc_error("NOT_FOUND", &format!("Installer not found: {}", install_id)))?;
    let r = oclaw_skills_core::installer::run_install(matched_spec, timeout).await;

    Ok(serde_json::json!({
        "installed": r.ok,
        "name": name,
        "installId": install_id,
        "agentId": effective_agent_id,
        "workspaceDir": workspace,
        "results": [serde_json::json!({
            "id": resolve_skill_install_id(matched_spec, matched_idx),
            "kind": matched_spec.kind,
            "ok": r.ok,
            "message": r.message,
            "code": r.code,
            "stderr": r.stderr,
        })],
    }))
}

async fn rpc_skills_update(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let id = p["id"]
        .as_str()
        .or_else(|| p["skillKey"].as_str())
        .or_else(|| p["name"].as_str())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing skill id/name"))?;

    let mut update = p.clone();
    if let Some(obj) = update.as_object_mut() {
        obj.remove("id");
        obj.remove("skillKey");
        obj.remove("name");
    }

    let mut persisted = false;
    if let Some(cfg) = state.full_config.as_ref() {
        let mut guard = cfg.write().await;
        let skills_root = guard
            .skills
            .get_or_insert_with(|| serde_json::json!({ "entries": {} }));
        if !skills_root.is_object() {
            *skills_root = serde_json::json!({ "entries": {} });
        }
        let root_obj = skills_root
            .as_object_mut()
            .ok_or_else(|| rpc_error("INTERNAL", "skills root must be object"))?;
        let entries_value = root_obj
            .entry("entries".to_string())
            .or_insert_with(|| serde_json::json!({}));
        if !entries_value.is_object() {
            *entries_value = serde_json::json!({});
        }
        let entries = entries_value
            .as_object_mut()
            .ok_or_else(|| rpc_error("INTERNAL", "skills.entries must be object"))?;
        let mut current = entries
            .get(id)
            .and_then(|v| v.as_object())
            .cloned()
            .unwrap_or_default();

        if let Some(enabled) = p.get("enabled").and_then(|v| v.as_bool()) {
            current.insert("enabled".to_string(), serde_json::Value::Bool(enabled));
        }
        if let Some(api_key) = p.get("apiKey").and_then(|v| v.as_str()) {
            let trimmed = api_key.trim();
            if trimmed.is_empty() {
                current.remove("apiKey");
            } else {
                current.insert(
                    "apiKey".to_string(),
                    serde_json::Value::String(trimmed.to_string()),
                );
            }
        }
        if let Some(env) = p.get("env").and_then(|v| v.as_object()) {
            let mut next_env = current
                .get("env")
                .and_then(|v| v.as_object())
                .cloned()
                .unwrap_or_default();
            for (key, value) in env {
                let trimmed_key = key.trim();
                if trimmed_key.is_empty() {
                    continue;
                }
                let trimmed_value = value.as_str().unwrap_or("").trim();
                if trimmed_value.is_empty() {
                    next_env.remove(trimmed_key);
                } else {
                    next_env.insert(
                        trimmed_key.to_string(),
                        serde_json::Value::String(trimmed_value.to_string()),
                    );
                }
            }
            current.insert("env".to_string(), serde_json::Value::Object(next_env));
        }

        entries.insert(id.to_string(), serde_json::Value::Object(current.clone()));
        persisted = true;
        update = serde_json::Value::Object(current);
    }

    state
        .skill_overrides
        .write()
        .await
        .insert(id.to_string(), update.clone());

    if persisted {
        persist_full_config(state).await?;
    }

    Ok(serde_json::json!({
        "updated": true,
        "id": id,
        "persisted": persisted,
        "config": update,
    }))
}

async fn rpc_skills_bins(state: &HttpState) -> RpcResult {
    let mut workspace_dirs = if let Some(cfg) = state.full_config.as_ref() {
        let guard = cfg.read().await;
        collect_agent_workspace_dirs_from_config(&guard, state.workspace.as_ref().map(|w| w.root()))
    } else {
        Vec::new()
    };
    if workspace_dirs.is_empty()
        && let Some(root) = state.workspace.as_ref().map(|w| w.root().to_path_buf())
    {
        workspace_dirs.push(root);
    }
    if workspace_dirs.is_empty()
        && let Ok(cwd) = std::env::current_dir()
    {
        workspace_dirs.push(cwd);
    }

    let mut bins = std::collections::BTreeSet::new();
    for workspace_dir in workspace_dirs {
        let skills = oclaw_skills_core::discovery::discover_skills(Some(&workspace_dir)).await;
        for s in &skills {
            if let Some(meta) = &s.manifest.metadata
                && let Some(oc) = &meta.openclaw
            {
                if let Some(req) = &oc.requires {
                    for bin in &req.bins {
                        let trimmed = bin.trim();
                        if !trimmed.is_empty() {
                            bins.insert(trimmed.to_string());
                        }
                    }
                    for bin in &req.any_bins {
                        let trimmed = bin.trim();
                        if !trimmed.is_empty() {
                            bins.insert(trimmed.to_string());
                        }
                    }
                }
                for install in &oc.install {
                    for bin in &install.bins {
                        let trimmed = bin.trim();
                        if !trimmed.is_empty() {
                            bins.insert(trimmed.to_string());
                        }
                    }
                }
            }
        }
    }
    Ok(serde_json::json!({
        "bins": bins.into_iter().collect::<Vec<_>>(),
    }))
}

const ADMIN_SCOPE: &str = "operator.admin";
const TALK_SECRETS_SCOPE: &str = "operator.talk.secrets";

fn merge_scopes_from_value(
    out: &mut std::collections::BTreeSet<String>,
    value: Option<&serde_json::Value>,
) {
    let Some(scopes) = value.and_then(|v| v.as_array()) else {
        return;
    };
    for scope in scopes {
        if let Some(raw) = scope.as_str() {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                out.insert(trimmed.to_string());
            }
        }
    }
}

fn request_scopes(p: &serde_json::Value) -> Vec<String> {
    let mut scopes = std::collections::BTreeSet::new();
    merge_scopes_from_value(&mut scopes, p.get("scopes"));
    merge_scopes_from_value(&mut scopes, p.get("connect").and_then(|v| v.get("scopes")));
    merge_scopes_from_value(&mut scopes, p.get("auth").and_then(|v| v.get("scopes")));
    scopes.into_iter().collect()
}

fn can_read_talk_secrets(scopes: &[String]) -> bool {
    scopes
        .iter()
        .any(|scope| scope == ADMIN_SCOPE || scope == TALK_SECRETS_SCOPE || scope == "*")
}

async fn rpc_talk_config(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let include_secrets = p
        .get("includeSecrets")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if include_secrets {
        let auth_enforced = state.auth_state.read().await.has_auth_config();
        let scopes = request_scopes(p);
        if auth_enforced && !can_read_talk_secrets(&scopes) {
            return Err(rpc_error(
                "INVALID_PARAMS",
                &format!("missing scope: {}", TALK_SECRETS_SCOPE),
            ));
        }
    }
    let Some(cfg) = state.full_config.as_ref() else {
        return Ok(serde_json::json!({ "config": {} }));
    };
    let guard = cfg.read().await;
    let mut payload = serde_json::Map::new();

    if let Some(talk) = guard.talk.as_ref() {
        let mut talk_value =
            serde_json::to_value(talk).map_err(|e| rpc_error("INTERNAL", &e.to_string()))?;
        if !include_secrets && let Some(obj) = talk_value.as_object_mut() {
            obj.remove("apiKey");
        }
        payload.insert("talk".to_string(), talk_value);
    }

    let session_main = if matches!(state.dm_scope, crate::session_key::DmScope::Main) {
        Some("main".to_string())
    } else {
        None
    };
    payload.insert(
        "session".to_string(),
        serde_json::json!({
            "mainKey": session_main,
            "dmScope": guard
                .session
                .as_ref()
                .and_then(|s| s.dm_scope.clone()),
        }),
    );
    if let Some(seam_color) = guard.ui.as_ref().and_then(|ui| ui.seam_color.clone()) {
        payload.insert(
            "ui".to_string(),
            serde_json::json!({ "seamColor": seam_color }),
        );
    }
    Ok(serde_json::json!({
        "config": payload,
    }))
}

async fn rpc_talk_mode(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let enabled = p
        .get("enabled")
        .and_then(|v| v.as_bool())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "enabled(boolean) required"))?;
    let phase = p
        .get("phase")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let payload = TalkModeRuntimeState {
        enabled,
        phase,
        ts: now_epoch_ms(),
    };
    *state.talk_mode.write().await = payload.clone();
    state.emit_event(
        "talk.mode",
        serde_json::to_value(&payload).map_err(|e| rpc_error("INTERNAL", &e.to_string()))?,
    );
    serde_json::to_value(payload).map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_voicewake_get(state: &HttpState) -> RpcResult {
    let triggers = state.voicewake_triggers.read().await.clone();
    Ok(serde_json::json!({
        "triggers": triggers,
    }))
}

fn normalize_voicewake_triggers(raw: &[serde_json::Value]) -> Vec<String> {
    raw.iter()
        .filter_map(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}

async fn rpc_voicewake_set(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let triggers_raw = p
        .get("triggers")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            rpc_error(
                "INVALID_PARAMS",
                "voicewake.set requires triggers: string[]",
            )
        })?;
    let triggers = normalize_voicewake_triggers(triggers_raw);
    *state.voicewake_triggers.write().await = triggers.clone();

    if let Some(cfg) = state.full_config.as_ref() {
        let mut guard = cfg.write().await;
        let commands = guard.commands.get_or_insert_with(|| serde_json::json!({}));
        if !commands.is_object() {
            *commands = serde_json::json!({});
        }
        if let Some(obj) = commands.as_object_mut() {
            let voicewake = obj
                .entry("voicewake".to_string())
                .or_insert_with(|| serde_json::json!({}));
            if !voicewake.is_object() {
                *voicewake = serde_json::json!({});
            }
            if let Some(voicewake_obj) = voicewake.as_object_mut() {
                voicewake_obj.insert(
                    "triggers".to_string(),
                    serde_json::to_value(&triggers).unwrap_or_default(),
                );
            }
        }
    }
    persist_full_config(state).await?;
    state.emit_event(
        "voicewake.changed",
        serde_json::json!({
            "triggers": triggers.clone(),
        }),
    );

    Ok(serde_json::json!({
        "triggers": triggers,
    }))
}

fn command_output_tail(bytes: &[u8], max_len: usize) -> String {
    let text = String::from_utf8_lossy(bytes);
    if text.len() <= max_len {
        return text.to_string();
    }
    text[text.len() - max_len..].to_string()
}

async fn run_update_step(
    name: &str,
    cmd: &str,
    args: &[&str],
    cwd: &Path,
    timeout_ms: u64,
) -> serde_json::Value {
    let started = std::time::Instant::now();
    let mut command = Command::new(cmd);
    command.args(args).current_dir(cwd);
    let out = tokio::time::timeout(Duration::from_millis(timeout_ms), command.output()).await;
    match out {
        Ok(Ok(output)) => {
            let ok = output.status.success();
            serde_json::json!({
                "name": name,
                "command": format!("{} {}", cmd, args.join(" ")),
                "cwd": cwd,
                "ok": ok,
                "durationMs": started.elapsed().as_millis() as u64,
                "exitCode": output.status.code(),
                "stdoutTail": command_output_tail(&output.stdout, 4000),
                "stderrTail": command_output_tail(&output.stderr, 4000),
            })
        }
        Ok(Err(e)) => serde_json::json!({
            "name": name,
            "command": format!("{} {}", cmd, args.join(" ")),
            "cwd": cwd,
            "ok": false,
            "durationMs": started.elapsed().as_millis() as u64,
            "error": e.to_string(),
        }),
        Err(_) => serde_json::json!({
            "name": name,
            "command": format!("{} {}", cmd, args.join(" ")),
            "cwd": cwd,
            "ok": false,
            "durationMs": started.elapsed().as_millis() as u64,
            "error": format!("timeout after {}ms", timeout_ms),
        }),
    }
}

fn restart_sentinel_path() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("oclaw")
        .join("restart-sentinel.json")
}

fn write_restart_sentinel(payload: &serde_json::Value) -> Result<PathBuf, ErrorDetails> {
    let path = restart_sentinel_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| rpc_error("INTERNAL", &format!("create sentinel dir failed: {}", e)))?;
    }
    let json = serde_json::to_string_pretty(payload)
        .map_err(|e| rpc_error("INTERNAL", &format!("serialize sentinel failed: {}", e)))?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, json)
        .map_err(|e| rpc_error("INTERNAL", &format!("write sentinel failed: {}", e)))?;
    std::fs::rename(&tmp, &path)
        .map_err(|e| rpc_error("INTERNAL", &format!("rename sentinel failed: {}", e)))?;
    Ok(path)
}

async fn rpc_update_run(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let timeout_ms = p
        .get("timeoutMs")
        .and_then(|v| v.as_u64())
        .unwrap_or(300_000)
        .clamp(1_000, 900_000);
    let restart_delay_ms = p
        .get("restartDelayMs")
        .and_then(|v| v.as_u64())
        .unwrap_or(1_500)
        .clamp(0, 60_000);
    let session_key = p
        .get("sessionKey")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let note = p
        .get("note")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let restart_on_success = p.get("restart").and_then(|v| v.as_bool()).unwrap_or(true);
    let cwd = std::env::current_dir()
        .map_err(|e| rpc_error("UNAVAILABLE", &format!("cannot resolve cwd: {}", e)))?;
    let started = std::time::Instant::now();

    let mut steps = Vec::new();
    let git_step =
        run_update_step("git.pull", "git", &["pull", "--ff-only"], &cwd, timeout_ms).await;
    let mut ok = git_step
        .get("ok")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    steps.push(git_step);

    if ok {
        let build_step = run_update_step(
            "cargo.build",
            "cargo",
            &["build", "--workspace"],
            &cwd,
            timeout_ms,
        )
        .await;
        ok = build_step
            .get("ok")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        steps.push(build_step);
    }

    let status = if ok { "ok" } else { "error" };
    let failure_reason = steps
        .iter()
        .find(|s| !s.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
        .and_then(|s| s.get("error").and_then(|v| v.as_str()))
        .map(ToString::to_string);
    let sentinel_payload = serde_json::json!({
        "kind": "update",
        "status": status,
        "ts": now_epoch_ms(),
        "sessionKey": session_key,
        "message": note,
        "doctorHint": "Run `oclaw doctor` for diagnostics after restart if needed.",
        "stats": {
            "mode": "rust-local",
            "root": cwd,
            "steps": steps,
            "reason": failure_reason,
            "durationMs": started.elapsed().as_millis() as u64,
        }
    });
    let sentinel_path = write_restart_sentinel(&sentinel_payload)?;

    let restart = if ok && restart_on_success {
        let gateway_server = state.gateway_server.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(restart_delay_ms)).await;
            gateway_server.shutdown();
        });
        serde_json::json!({
            "scheduled": true,
            "delayMs": restart_delay_ms,
            "reason": "update.run",
        })
    } else {
        serde_json::Value::Null
    };

    Ok(serde_json::json!({
        "ok": ok,
        "result": {
            "status": status,
            "mode": "rust-local",
            "root": cwd,
            "steps": sentinel_payload["stats"]["steps"].clone(),
            "reason": sentinel_payload["stats"]["reason"].clone(),
            "durationMs": started.elapsed().as_millis() as u64,
        },
        "restart": restart,
        "sentinel": {
            "path": sentinel_path,
            "payload": sentinel_payload,
        }
    }))
}

fn browser_http_base(cdp_url: &str) -> String {
    let trimmed = cdp_url.trim().trim_end_matches('/');
    if let Ok(u) = url::Url::parse(trimmed) {
        let scheme = match u.scheme() {
            "ws" => "http",
            "wss" => "https",
            other => other,
        };
        if let Some(host) = u.host_str() {
            let mut origin = format!("{}://{}", scheme, host);
            if let Some(port) = u.port() {
                origin.push(':');
                origin.push_str(&port.to_string());
            }
            return origin;
        }
    }
    trimmed
        .replace("ws://", "http://")
        .replace("wss://", "https://")
}

fn cdp_port_from_url(cdp_url: &str) -> Option<u16> {
    let normalized = cdp_url
        .trim()
        .replace("ws://", "http://")
        .replace("wss://", "https://");
    url::Url::parse(&normalized)
        .ok()
        .and_then(|u| u.port_or_known_default())
}

async fn browser_fetch_json(
    base: &str,
    path: &str,
    timeout_ms: u64,
) -> Result<serde_json::Value, ErrorDetails> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(timeout_ms))
        .build()
        .map_err(|e| {
            rpc_error(
                "UNAVAILABLE",
                &format!("build browser client failed: {}", e),
            )
        })?;
    let url = format!("{}{}", base.trim_end_matches('/'), path);
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| rpc_error("UNAVAILABLE", &format!("browser request failed: {}", e)))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(rpc_error(
            "UNAVAILABLE",
            &format!("browser endpoint {} returned {}", path, status),
        ));
    }
    resp.json::<serde_json::Value>().await.map_err(|e| {
        rpc_error(
            "UNAVAILABLE",
            &format!("parse browser response failed: {}", e),
        )
    })
}

fn browser_cdp_url_from_state(state: &HttpState) -> Option<String> {
    state
        .full_config
        .as_ref()
        .and_then(|cfg| cfg.try_read().ok())
        .and_then(|cfg| cfg.browser.as_ref().and_then(|b| b.cdp_url.clone()))
        .filter(|s| !s.trim().is_empty())
}

fn browser_enabled_from_state(state: &HttpState) -> bool {
    state
        .full_config
        .as_ref()
        .and_then(|cfg| cfg.try_read().ok())
        .and_then(|cfg| cfg.browser.as_ref().and_then(|b| b.enabled))
        .unwrap_or(false)
}

fn browser_request_str<'a>(p: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    p.get("query")
        .and_then(|v| v.get(key))
        .and_then(|v| v.as_str())
        .or_else(|| {
            p.get("body")
                .and_then(|v| v.get(key))
                .and_then(|v| v.as_str())
        })
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn browser_request_bool(p: &serde_json::Value, key: &str) -> Option<bool> {
    p.get("query")
        .and_then(|v| v.get(key))
        .and_then(|v| v.as_bool())
        .or_else(|| {
            p.get("body")
                .and_then(|v| v.get(key))
                .and_then(|v| v.as_bool())
        })
}

fn browser_selected_profile(p: &serde_json::Value) -> Option<String> {
    browser_request_str(p, "profile").map(ToString::to_string)
}

fn browser_profile_cdp_url(state: &HttpState, profile: &str) -> Option<String> {
    state
        .full_config
        .as_ref()
        .and_then(|cfg| cfg.try_read().ok())
        .and_then(|cfg| cfg.browser.as_ref().and_then(|b| b.profiles.clone()))
        .and_then(|profiles| profiles.get(profile).cloned())
        .and_then(|p| {
            p.cdp_url
                .or_else(|| p.cdp_port.map(|port| format!("ws://127.0.0.1:{}", port)))
        })
        .filter(|v| !v.trim().is_empty())
}

fn browser_resolved_cdp_url(state: &HttpState, p: &serde_json::Value) -> (String, String) {
    let profile = browser_selected_profile(p).unwrap_or_else(|| "default".to_string());
    let cdp_url = browser_profile_cdp_url(state, &profile)
        .or_else(|| browser_cdp_url_from_state(state))
        .unwrap_or_else(|| "http://127.0.0.1:9222".to_string());
    (cdp_url, profile)
}

#[derive(Debug, Clone)]
struct BrowserNodeDescriptor {
    node_id: String,
    display_name: Option<String>,
    remote_ip: Option<String>,
    platform: Option<String>,
    caps: Vec<String>,
    commands: Vec<String>,
}

fn normalize_node_key(value: &str) -> String {
    value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect()
}

fn browser_gateway_node_policy(state: &HttpState) -> (String, Option<String>) {
    let Some(cfg) = state.full_config.as_ref() else {
        return ("auto".to_string(), None);
    };
    let Ok(cfg) = cfg.try_read() else {
        return ("auto".to_string(), None);
    };
    let Some(nodes) = cfg.gateway.as_ref().and_then(|g| g.nodes.as_ref()) else {
        return ("auto".to_string(), None);
    };
    let mode = nodes
        .browser
        .as_ref()
        .and_then(|b| b.mode.as_ref())
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "auto".to_string());
    let node = nodes
        .browser
        .as_ref()
        .and_then(|b| b.node.as_ref())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    (mode, node)
}

fn normalize_platform_id(platform: Option<&str>) -> &'static str {
    let raw = platform.unwrap_or("").trim().to_ascii_lowercase();
    if raw.starts_with("ios") {
        return "ios";
    }
    if raw.starts_with("android") {
        return "android";
    }
    if raw.starts_with("mac") || raw.starts_with("darwin") {
        return "macos";
    }
    if raw.starts_with("win") {
        return "windows";
    }
    if raw.starts_with("linux") {
        return "linux";
    }
    "unknown"
}

fn platform_default_allowlist(platform: &str) -> Vec<&'static str> {
    const CANVAS: &[&str] = &[
        "canvas.present",
        "canvas.hide",
        "canvas.navigate",
        "canvas.eval",
        "canvas.snapshot",
        "canvas.a2ui.push",
        "canvas.a2ui.pushJSONL",
        "canvas.a2ui.reset",
    ];
    const CAMERA: &[&str] = &["camera.list"];
    const LOCATION: &[&str] = &["location.get"];
    const DEVICE: &[&str] = &["device.info", "device.status"];
    const CONTACTS: &[&str] = &["contacts.search"];
    const CALENDAR: &[&str] = &["calendar.events"];
    const REMINDERS: &[&str] = &["reminders.list"];
    const PHOTOS: &[&str] = &["photos.latest"];
    const MOTION: &[&str] = &["motion.activity", "motion.pedometer"];
    const IOS_SYSTEM: &[&str] = &["system.notify"];
    const SYSTEM: &[&str] = &[
        "system.run",
        "system.which",
        "system.notify",
        "browser.proxy",
    ];

    match platform {
        "ios" => [
            CANVAS, CAMERA, LOCATION, DEVICE, CONTACTS, CALENDAR, REMINDERS, PHOTOS, MOTION,
            IOS_SYSTEM,
        ]
        .into_iter()
        .flat_map(|v| v.iter().copied())
        .collect(),
        "android" => [
            CANVAS, CAMERA, LOCATION, DEVICE, CONTACTS, CALENDAR, REMINDERS, PHOTOS, MOTION,
        ]
        .into_iter()
        .flat_map(|v| v.iter().copied())
        .collect(),
        "macos" => [
            CANVAS, CAMERA, LOCATION, DEVICE, CONTACTS, CALENDAR, REMINDERS, PHOTOS, MOTION, SYSTEM,
        ]
        .into_iter()
        .flat_map(|v| v.iter().copied())
        .collect(),
        "linux" | "windows" => SYSTEM.to_vec(),
        _ => [CANVAS, CAMERA, LOCATION, SYSTEM]
            .into_iter()
            .flat_map(|v| v.iter().copied())
            .collect(),
    }
}

fn browser_resolve_node_command_allowlist(
    state: &HttpState,
    platform: Option<&str>,
) -> HashSet<String> {
    let mut allow: HashSet<String> = platform_default_allowlist(normalize_platform_id(platform))
        .into_iter()
        .map(ToString::to_string)
        .collect();
    if let Some(cfg) = state.full_config.as_ref()
        && let Ok(cfg) = cfg.try_read()
        && let Some(nodes) = cfg.gateway.as_ref().and_then(|g| g.nodes.as_ref())
    {
        if let Some(extra) = nodes.allow_commands.as_ref() {
            for cmd in extra {
                let trimmed = cmd.trim();
                if !trimmed.is_empty() {
                    allow.insert(trimmed.to_string());
                }
            }
        }
        if let Some(deny) = nodes.deny_commands.as_ref() {
            for cmd in deny {
                let trimmed = cmd.trim();
                if !trimmed.is_empty() {
                    allow.remove(trimmed);
                }
            }
        }
    }
    allow
}

fn browser_validate_node_command_allowed(
    command: &str,
    declared_commands: &[String],
    allowlist: &HashSet<String>,
) -> Result<(), ErrorDetails> {
    if !allowlist.contains(command) {
        return Err(rpc_error(
            "INVALID_PARAMS",
            "node command not allowed: command not allowlisted",
        ));
    }
    if declared_commands.is_empty() {
        return Err(rpc_error(
            "INVALID_PARAMS",
            "node command not allowed: node did not declare commands",
        ));
    }
    if !declared_commands.iter().any(|cmd| cmd == command) {
        return Err(rpc_error(
            "INVALID_PARAMS",
            "node command not allowed: command not declared by node",
        ));
    }
    Ok(())
}

fn browser_node_is_capable(node: &BrowserNodeDescriptor) -> bool {
    node.caps.iter().any(|cap| cap == "browser")
        || node.commands.iter().any(|cmd| cmd == "browser.proxy")
}

fn node_caps_for_browser(rec: Option<&NodePairRecord>) -> Vec<String> {
    let caps = rec.and_then(|r| r.caps.clone()).unwrap_or_default();
    if !caps.is_empty() {
        return caps;
    }
    node_default_caps()
        .into_iter()
        .map(ToString::to_string)
        .collect()
}

fn node_commands_for_browser(rec: Option<&NodePairRecord>) -> Vec<String> {
    let commands = rec.and_then(|r| r.commands.clone()).unwrap_or_default();
    if !commands.is_empty() {
        return commands;
    }
    node_default_commands()
        .into_iter()
        .map(ToString::to_string)
        .collect()
}

async fn browser_connected_nodes(state: &HttpState) -> Vec<BrowserNodeDescriptor> {
    let records = state.node_pairs.lock().await.clone();
    let connected = state.node_connected.lock().await.clone();
    let mut by_node: HashMap<String, NodePairRecord> = HashMap::new();
    for rec in records.values() {
        by_node
            .entry(rec.node_id.clone())
            .and_modify(|existing| {
                if rec.approved && (!existing.approved || rec.ts > existing.ts) {
                    *existing = rec.clone();
                }
            })
            .or_insert_with(|| rec.clone());
    }
    let mut out = Vec::new();
    for node_id in connected {
        let rec = by_node.get(&node_id);
        out.push(BrowserNodeDescriptor {
            node_id: node_id.clone(),
            display_name: rec.and_then(|r| r.display_name.clone()),
            remote_ip: rec.and_then(|r| r.remote_ip.clone()),
            platform: rec.and_then(|r| r.platform.clone()),
            caps: node_caps_for_browser(rec),
            commands: node_commands_for_browser(rec),
        });
    }
    out.sort_by(|a, b| a.node_id.cmp(&b.node_id));
    out
}

fn browser_resolve_named_node(
    nodes: &[BrowserNodeDescriptor],
    query: &str,
) -> Result<Option<BrowserNodeDescriptor>, ErrorDetails> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(None);
    }
    let q_norm = normalize_node_key(q);
    let mut matches: Vec<BrowserNodeDescriptor> = Vec::new();
    for node in nodes {
        let by_id = node.node_id == q;
        let by_ip = node.remote_ip.as_deref() == Some(q);
        let by_name = node
            .display_name
            .as_deref()
            .map(normalize_node_key)
            .map(|name| name == q_norm)
            .unwrap_or(false);
        let by_prefix = q.len() >= 6 && node.node_id.starts_with(q);
        if by_id || by_ip || by_name || by_prefix {
            matches.push(node.clone());
        }
    }
    if matches.is_empty() {
        return Ok(None);
    }
    if matches.len() == 1 {
        return Ok(matches.into_iter().next());
    }
    let names = matches
        .iter()
        .map(|node| {
            node.display_name
                .clone()
                .or_else(|| node.remote_ip.clone())
                .unwrap_or_else(|| node.node_id.clone())
        })
        .collect::<Vec<_>>()
        .join(", ");
    Err(rpc_error(
        "UNAVAILABLE",
        &format!("ambiguous node: {} (matches: {})", q, names),
    ))
}

async fn browser_resolve_node_target(
    state: &HttpState,
    explicit_node: Option<&str>,
) -> Result<Option<BrowserNodeDescriptor>, ErrorDetails> {
    let nodes = browser_connected_nodes(state).await;
    let browser_nodes: Vec<BrowserNodeDescriptor> = nodes
        .iter()
        .filter(|node| browser_node_is_capable(node))
        .cloned()
        .collect();

    if let Some(explicit_node) = explicit_node {
        let resolved = browser_resolve_named_node(&nodes, explicit_node)?;
        if let Some(node) = resolved {
            return Ok(Some(node));
        }
        return Err(rpc_error(
            "UNAVAILABLE",
            &format!("Configured browser node not connected: {}", explicit_node),
        ));
    }

    let (mode, configured_node) = browser_gateway_node_policy(state);
    if mode == "off" {
        return Ok(None);
    }

    if browser_nodes.is_empty() {
        if configured_node.as_deref().is_some() {
            return Err(rpc_error(
                "UNAVAILABLE",
                "No connected browser-capable nodes.",
            ));
        }
        return Ok(None);
    }

    if let Some(query) = configured_node.as_deref() {
        let resolved = browser_resolve_named_node(&browser_nodes, query)?;
        if let Some(node) = resolved {
            return Ok(Some(node));
        }
        return Err(rpc_error(
            "UNAVAILABLE",
            &format!("Configured browser node not connected: {}", query),
        ));
    }

    if mode == "manual" {
        return Ok(None);
    }
    if browser_nodes.len() == 1 {
        return Ok(browser_nodes.first().cloned());
    }
    Ok(None)
}

async fn browser_ws_for_target(
    cdp_url: &str,
    target_id: &str,
    timeout_ms: u64,
) -> Result<String, ErrorDetails> {
    let base = browser_http_base(cdp_url);
    let list = browser_fetch_json(&base, "/json/list", timeout_ms).await?;
    let arr = list
        .as_array()
        .ok_or_else(|| rpc_error("UNAVAILABLE", "invalid /json/list response"))?;
    for item in arr {
        let id = item
            .get("id")
            .or_else(|| item.get("targetId"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if id == target_id {
            let ws = item
                .get("webSocketDebuggerUrl")
                .and_then(|v| v.as_str())
                .ok_or_else(|| rpc_error("UNAVAILABLE", "target missing websocket url"))?;
            return Ok(ws.to_string());
        }
    }
    Err(rpc_error(
        "INVALID_REQUEST",
        &format!("target not found: {}", target_id),
    ))
}

async fn browser_browser_ws(cdp_url: &str, timeout_ms: u64) -> Result<String, ErrorDetails> {
    let base = browser_http_base(cdp_url);
    let version = browser_fetch_json(&base, "/json/version", timeout_ms).await?;
    version
        .get("webSocketDebuggerUrl")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .ok_or_else(|| rpc_error("UNAVAILABLE", "missing browser websocket url"))
}

async fn browser_activate_target(
    cdp_url: &str,
    target_id: &str,
    timeout_ms: u64,
) -> Result<(), ErrorDetails> {
    let ws = browser_browser_ws(cdp_url, timeout_ms).await?;
    let conn = CdpConnection::connect(&ws)
        .await
        .map_err(|e| rpc_error("UNAVAILABLE", &format!("connect cdp failed: {}", e)))?;
    conn.send_command(
        &build_method(CdpDomain::Target, "activateTarget"),
        Some(serde_json::json!({ "targetId": target_id })),
    )
    .await
    .map_err(|e| rpc_error("UNAVAILABLE", &format!("activate target failed: {}", e)))?;
    Ok(())
}

async fn browser_close_target(
    cdp_url: &str,
    target_id: &str,
    timeout_ms: u64,
) -> Result<(), ErrorDetails> {
    let ws = browser_browser_ws(cdp_url, timeout_ms).await?;
    let conn = CdpConnection::connect(&ws)
        .await
        .map_err(|e| rpc_error("UNAVAILABLE", &format!("connect cdp failed: {}", e)))?;
    conn.send_command(
        &build_method(CdpDomain::Target, "closeTarget"),
        Some(serde_json::json!({ "targetId": target_id })),
    )
    .await
    .map_err(|e| rpc_error("UNAVAILABLE", &format!("close target failed: {}", e)))?;
    Ok(())
}

async fn browser_open_page_for_target(
    cdp_url: &str,
    target_id: &str,
    timeout_ms: u64,
) -> Result<Page, ErrorDetails> {
    let ws = browser_ws_for_target(cdp_url, target_id, timeout_ms).await?;
    Page::new(
        &ws,
        target_id.to_string(),
        Arc::new(RwLock::new(HashMap::new())),
    )
    .await
    .map_err(|e| rpc_error("UNAVAILABLE", &format!("open target page failed: {}", e)))
}

async fn browser_resolve_target_id(
    cdp_url: &str,
    p: &serde_json::Value,
    timeout_ms: u64,
) -> Result<String, ErrorDetails> {
    if let Some(explicit) = browser_request_str(p, "targetId") {
        return Ok(explicit.to_string());
    }
    let base = browser_http_base(cdp_url);
    let list = browser_fetch_json(&base, "/json/list", timeout_ms).await?;
    if let Some(arr) = list.as_array() {
        for item in arr {
            let kind = item.get("type").and_then(|v| v.as_str()).unwrap_or("page");
            let id = item
                .get("id")
                .or_else(|| item.get("targetId"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty());
            if kind == "page"
                && let Some(id) = id
            {
                return Ok(id.to_string());
            }
        }
    }

    let mut manager = BrowserManager::new(cdp_url)
        .await
        .map_err(|e| rpc_error("UNAVAILABLE", &format!("connect browser failed: {}", e)))?;
    manager
        .connect()
        .await
        .map_err(|e| rpc_error("UNAVAILABLE", &format!("init browser failed: {}", e)))?;
    let page = manager
        .create_page()
        .await
        .map_err(|e| rpc_error("UNAVAILABLE", &format!("open tab failed: {}", e)))?;
    Ok(page.target_id().to_string())
}

async fn browser_clear_cookies_for_target(
    cdp_url: &str,
    target_id: &str,
    timeout_ms: u64,
) -> Result<(), ErrorDetails> {
    let ws = browser_ws_for_target(cdp_url, target_id, timeout_ms).await?;
    let conn = CdpConnection::connect(&ws)
        .await
        .map_err(|e| rpc_error("UNAVAILABLE", &format!("connect cdp failed: {}", e)))?;
    conn.send_command(&build_method(CdpDomain::Network, "enable"), None)
        .await
        .map_err(|e| rpc_error("UNAVAILABLE", &format!("enable network failed: {}", e)))?;
    conn.send_command(
        &build_method(CdpDomain::Network, "clearBrowserCookies"),
        None,
    )
    .await
    .map_err(|e| rpc_error("UNAVAILABLE", &format!("clear cookies failed: {}", e)))?;
    Ok(())
}

async fn browser_set_offline_for_target(
    cdp_url: &str,
    target_id: &str,
    offline: bool,
    timeout_ms: u64,
) -> Result<(), ErrorDetails> {
    let ws = browser_ws_for_target(cdp_url, target_id, timeout_ms).await?;
    let conn = CdpConnection::connect(&ws)
        .await
        .map_err(|e| rpc_error("UNAVAILABLE", &format!("connect cdp failed: {}", e)))?;
    conn.send_command(&build_method(CdpDomain::Network, "enable"), None)
        .await
        .map_err(|e| rpc_error("UNAVAILABLE", &format!("enable network failed: {}", e)))?;
    conn.send_command(
        &build_method(CdpDomain::Network, "emulateNetworkConditions"),
        Some(serde_json::json!({
            "offline": offline,
            "latency": 0,
            "downloadThroughput": -1,
            "uploadThroughput": -1,
            "connectionType": if offline { "none" } else { "wifi" },
        })),
    )
    .await
    .map_err(|e| rpc_error("UNAVAILABLE", &format!("set offline failed: {}", e)))?;
    Ok(())
}

fn browser_path_param(path: &str, prefix: &str, suffix: &str) -> Option<String> {
    let p = path.trim();
    if !p.starts_with(prefix) || !p.ends_with(suffix) {
        return None;
    }
    let body = p
        .strip_prefix(prefix)?
        .strip_suffix(suffix)?
        .trim_matches('/')
        .trim();
    if body.is_empty() {
        None
    } else {
        Some(body.to_string())
    }
}

fn browser_proxy_ext_from_mime(mime: Option<&str>) -> &'static str {
    let mime = mime.unwrap_or("").to_ascii_lowercase();
    if mime.contains("png") {
        "png"
    } else if mime.contains("jpeg") || mime.contains("jpg") {
        "jpg"
    } else if mime.contains("webp") {
        "webp"
    } else if mime.contains("gif") {
        "gif"
    } else if mime.contains("pdf") {
        "pdf"
    } else if mime.contains("json") {
        "json"
    } else if mime.contains("html") {
        "html"
    } else if mime.contains("plain") {
        "txt"
    } else {
        "bin"
    }
}

async fn browser_persist_proxy_files(
    files: Option<&serde_json::Value>,
) -> Result<HashMap<String, String>, ErrorDetails> {
    let mut mapping = HashMap::new();
    let Some(files) = files.and_then(|v| v.as_array()) else {
        return Ok(mapping);
    };
    if files.is_empty() {
        return Ok(mapping);
    }
    let root = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".oclaw")
        .join("media")
        .join("browser");
    tokio::fs::create_dir_all(&root).await.map_err(|e| {
        rpc_error(
            "INTERNAL",
            &format!("create browser media dir failed: {}", e),
        )
    })?;
    for file in files {
        let Some(path_key) = file.get("path").and_then(|v| v.as_str()).map(str::trim) else {
            continue;
        };
        if path_key.is_empty() {
            continue;
        }
        let Some(base64_data) = file.get("base64").and_then(|v| v.as_str()) else {
            continue;
        };
        let bytes = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, base64_data)
            .map_err(|e| {
                rpc_error(
                    "INVALID_REQUEST",
                    &format!("invalid browser proxy base64: {}", e),
                )
            })?;
        let ext = browser_proxy_ext_from_mime(file.get("mimeType").and_then(|v| v.as_str()));
        let filename = format!(
            "browser-{}-{}.{}",
            now_epoch_ms(),
            uuid::Uuid::new_v4(),
            ext
        );
        let save_path = root.join(filename);
        tokio::fs::write(&save_path, bytes).await.map_err(|e| {
            rpc_error(
                "INTERNAL",
                &format!("write browser proxy file failed: {}", e),
            )
        })?;
        mapping.insert(
            path_key.to_string(),
            save_path.to_string_lossy().to_string(),
        );
    }
    Ok(mapping)
}

fn browser_apply_proxy_paths(result: &mut serde_json::Value, mapping: &HashMap<String, String>) {
    let Some(obj) = result.as_object_mut() else {
        return;
    };
    if let Some(path) = obj.get_mut("path")
        && let Some(raw) = path.as_str()
        && let Some(mapped) = mapping.get(raw)
    {
        *path = serde_json::Value::String(mapped.clone());
    }
    if let Some(path) = obj.get_mut("imagePath")
        && let Some(raw) = path.as_str()
        && let Some(mapped) = mapping.get(raw)
    {
        *path = serde_json::Value::String(mapped.clone());
    }
    if let Some(download) = obj.get_mut("download")
        && let Some(download_obj) = download.as_object_mut()
        && let Some(path) = download_obj.get_mut("path")
        && let Some(raw) = path.as_str()
        && let Some(mapped) = mapping.get(raw)
    {
        *path = serde_json::Value::String(mapped.clone());
    }
}

async fn browser_request_local(
    method: &str,
    path: &str,
    p: &serde_json::Value,
    state: &HttpState,
    timeout_ms: u64,
) -> RpcResult {
    let (cdp_url, profile_name) = browser_resolved_cdp_url(state, p);
    let base = browser_http_base(&cdp_url);

    if method == "GET" && path == "/" {
        let cdp_http = browser_fetch_json(&base, "/json/version", timeout_ms)
            .await
            .is_ok();
        let cdp_ready = BrowserManager::new(&cdp_url).await.is_ok();
        let resolved = state
            .full_config
            .as_ref()
            .and_then(|cfg| cfg.try_read().ok())
            .and_then(|cfg| cfg.browser.clone());
        return Ok(serde_json::json!({
            "enabled": browser_enabled_from_state(state),
            "profile": profile_name,
            "running": cdp_ready,
            "cdpReady": cdp_ready,
            "cdpHttp": cdp_http,
            "pid": serde_json::Value::Null,
            "cdpPort": cdp_port_from_url(&cdp_url),
            "cdpUrl": cdp_url,
            "color": resolved.as_ref().and_then(|b| b.color.clone()),
            "headless": resolved.as_ref().and_then(|b| b.headless).unwrap_or(true),
            "noSandbox": resolved.as_ref().and_then(|b| b.no_sandbox).unwrap_or(false),
            "executablePath": resolved.and_then(|b| b.executable_path),
            "attachOnly": state
                .full_config
                .as_ref()
                .and_then(|cfg| cfg.try_read().ok())
                .and_then(|cfg| cfg.browser.as_ref().and_then(|b| b.attach_only))
                .unwrap_or(true),
        }));
    }

    if method == "GET" && path == "/profiles" {
        let profiles = state
            .full_config
            .as_ref()
            .and_then(|cfg| cfg.try_read().ok())
            .and_then(|cfg| cfg.browser.as_ref().and_then(|b| b.profiles.clone()))
            .unwrap_or_default();
        let mut out = Vec::new();
        for (name, profile) in profiles {
            out.push(serde_json::json!({
                "name": name,
                "cdpPort": profile.cdp_port,
                "cdpUrl": profile.cdp_url,
                "driver": profile.driver,
                "color": profile.color,
            }));
        }
        if out.is_empty() {
            out.push(serde_json::json!({"name": "default", "cdpUrl": cdp_url}));
        }
        return Ok(serde_json::json!({ "profiles": out }));
    }

    if method == "POST" && path == "/profiles/create" {
        let name = browser_request_str(p, "name")
            .ok_or_else(|| rpc_error("INVALID_PARAMS", "name is required"))?;
        let valid = name
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.');
        if !valid {
            return Err(rpc_error(
                "INVALID_PARAMS",
                "invalid profile name: use [a-zA-Z0-9._-]",
            ));
        }
        let color = browser_request_str(p, "color").map(ToString::to_string);
        let cdp_url = browser_request_str(p, "cdpUrl").map(ToString::to_string);
        let cdp_port = p
            .get("body")
            .and_then(|v| v.get("cdpPort"))
            .and_then(|v| v.as_i64())
            .map(|v| v as i32);
        let driver = browser_request_str(p, "driver").map(ToString::to_string);

        let cfg = state
            .full_config
            .as_ref()
            .ok_or_else(|| rpc_error("NO_CONFIG", "No configuration loaded"))?;
        {
            let mut guard = cfg.write().await;
            let browser = guard
                .browser
                .get_or_insert_with(oclaw_config::settings::Browser::default);
            let profiles = browser.profiles.get_or_insert_with(HashMap::new);
            if profiles.contains_key(name) {
                return Err(rpc_error(
                    "INVALID_REQUEST",
                    &format!("profile already exists: {}", name),
                ));
            }
            profiles.insert(
                name.to_string(),
                oclaw_config::settings::BrowserProfile {
                    cdp_port,
                    cdp_url: cdp_url.clone(),
                    driver,
                    color,
                },
            );
        }
        persist_full_config(state).await?;
        return Ok(serde_json::json!({
            "ok": true,
            "name": name,
        }));
    }

    if method == "DELETE" && path.starts_with("/profiles/") {
        let name = path
            .trim_start_matches("/profiles/")
            .trim_matches('/')
            .trim();
        if name.is_empty() {
            return Err(rpc_error("INVALID_PARAMS", "profile name is required"));
        }
        if name == "default" {
            return Err(rpc_error("INVALID_PARAMS", "cannot delete default profile"));
        }
        let cfg = state
            .full_config
            .as_ref()
            .ok_or_else(|| rpc_error("NO_CONFIG", "No configuration loaded"))?;
        let removed = {
            let mut guard = cfg.write().await;
            let browser = guard
                .browser
                .get_or_insert_with(oclaw_config::settings::Browser::default);
            let profiles = browser.profiles.get_or_insert_with(HashMap::new);
            profiles.remove(name).is_some()
        };
        if !removed {
            return Err(rpc_error(
                "NOT_FOUND",
                &format!("profile not found: {}", name),
            ));
        }
        persist_full_config(state).await?;
        return Ok(serde_json::json!({
            "ok": true,
            "name": name,
            "deleted": true,
        }));
    }

    if method == "POST" && path == "/reset-profile" {
        let list = browser_fetch_json(&base, "/json/list", timeout_ms).await;
        let mut reset_tabs = 0u64;
        let mut errors: Vec<String> = Vec::new();
        if let Ok(list) = list
            && let Some(arr) = list.as_array()
        {
            for item in arr {
                let kind = item.get("type").and_then(|v| v.as_str()).unwrap_or("page");
                if kind != "page" {
                    continue;
                }
                let id = item
                    .get("id")
                    .or_else(|| item.get("targetId"))
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                let Some(id) = id else { continue };
                match browser_open_page_for_target(&cdp_url, id, timeout_ms).await {
                    Ok(page) => {
                        if let Err(e) = page
                            .evaluate(
                                "try{localStorage.clear();}catch(_e){};try{sessionStorage.clear();}catch(_e){};true",
                            )
                            .await
                        {
                            errors.push(format!("tab {} storage clear failed: {}", id, e));
                        }
                        reset_tabs = reset_tabs.saturating_add(1);
                    }
                    Err(e) => errors.push(format!("tab {} open failed: {}", id, e.message)),
                }
            }
        }
        if let Ok(target_id) = browser_resolve_target_id(&cdp_url, p, timeout_ms).await
            && let Err(e) = browser_clear_cookies_for_target(&cdp_url, &target_id, timeout_ms).await
        {
            errors.push(format!("clear cookies failed: {}", e.message));
        }
        return Ok(serde_json::json!({
            "ok": errors.is_empty(),
            "profile": profile_name,
            "resetTabs": reset_tabs,
            "errors": errors,
        }));
    }

    if method == "POST" && path == "/start" {
        let mut manager = BrowserManager::new(&cdp_url)
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("connect browser failed: {}", e)))?;
        manager
            .connect()
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("init browser failed: {}", e)))?;
        return Ok(serde_json::json!({
            "ok": true,
            "profile": profile_name,
        }));
    }

    if method == "POST" && path == "/stop" {
        return Ok(serde_json::json!({
            "ok": true,
            "stopped": false,
            "profile": profile_name,
            "note": "attached CDP endpoint is external; no local browser process to stop",
        }));
    }

    if method == "GET" && path == "/tabs" {
        let manager = BrowserManager::new(&cdp_url)
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("connect browser failed: {}", e)))?;
        let tabs = manager
            .list_targets()
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("list tabs failed: {}", e)))?;
        return Ok(serde_json::json!({
            "running": true,
            "tabs": tabs,
        }));
    }

    if method == "POST" && path == "/tabs/open" {
        let url = p
            .get("body")
            .and_then(|v| v.get("url"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| rpc_error("INVALID_PARAMS", "url is required"))?;
        let mut manager = BrowserManager::new(&cdp_url)
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("connect browser failed: {}", e)))?;
        manager
            .connect()
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("init browser failed: {}", e)))?;
        let mut page = manager
            .create_page()
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("open tab failed: {}", e)))?;
        page.navigate(url)
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("navigate failed: {}", e)))?;
        let target_id = page.target_id().to_string();
        let targets = manager.list_targets().await.unwrap_or_default();
        let tab = targets
            .iter()
            .find(|t| t.target_id == target_id)
            .cloned()
            .map(serde_json::to_value)
            .transpose()
            .ok()
            .flatten()
            .unwrap_or_else(|| serde_json::json!({ "targetId": target_id, "url": url }));
        return Ok(tab);
    }

    if method == "POST" && path == "/tabs/focus" {
        let target_id = p
            .get("body")
            .and_then(|v| v.get("targetId"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| rpc_error("INVALID_PARAMS", "targetId is required"))?;
        browser_activate_target(&cdp_url, target_id, timeout_ms).await?;
        return Ok(serde_json::json!({ "ok": true }));
    }

    if method == "DELETE" && path.starts_with("/tabs/") {
        let target_id = path.trim_start_matches("/tabs/").trim().to_string();
        if target_id.is_empty() {
            return Err(rpc_error("INVALID_PARAMS", "targetId is required"));
        }
        browser_close_target(&cdp_url, &target_id, timeout_ms).await?;
        return Ok(serde_json::json!({ "ok": true }));
    }

    if method == "POST" && path == "/tabs/action" {
        let action = p
            .get("body")
            .and_then(|v| v.get("action"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .unwrap_or("");
        let index = p
            .get("body")
            .and_then(|v| v.get("index"))
            .and_then(|v| v.as_u64())
            .map(|v| v as usize);
        match action {
            "list" => {
                let manager = BrowserManager::new(&cdp_url).await.map_err(|e| {
                    rpc_error("UNAVAILABLE", &format!("connect browser failed: {}", e))
                })?;
                let tabs = manager
                    .list_targets()
                    .await
                    .map_err(|e| rpc_error("UNAVAILABLE", &format!("list tabs failed: {}", e)))?;
                return Ok(serde_json::json!({ "ok": true, "tabs": tabs }));
            }
            "new" => {
                let mut manager = BrowserManager::new(&cdp_url).await.map_err(|e| {
                    rpc_error("UNAVAILABLE", &format!("connect browser failed: {}", e))
                })?;
                manager.connect().await.map_err(|e| {
                    rpc_error("UNAVAILABLE", &format!("init browser failed: {}", e))
                })?;
                let tab = manager
                    .create_page()
                    .await
                    .map_err(|e| rpc_error("UNAVAILABLE", &format!("open tab failed: {}", e)))?;
                return Ok(serde_json::json!({
                    "ok": true,
                    "tab": {"targetId": tab.target_id(), "wsUrl": tab.ws_url()},
                }));
            }
            "close" => {
                let manager = BrowserManager::new(&cdp_url).await.map_err(|e| {
                    rpc_error("UNAVAILABLE", &format!("connect browser failed: {}", e))
                })?;
                let tabs = manager
                    .list_targets()
                    .await
                    .map_err(|e| rpc_error("UNAVAILABLE", &format!("list tabs failed: {}", e)))?;
                let target = if let Some(idx) = index {
                    tabs.get(idx)
                } else {
                    tabs.first()
                }
                .ok_or_else(|| rpc_error("INVALID_REQUEST", "tab not found"))?;
                browser_close_target(&cdp_url, &target.target_id, timeout_ms).await?;
                return Ok(serde_json::json!({
                    "ok": true,
                    "targetId": target.target_id,
                }));
            }
            "select" => {
                let idx = index.ok_or_else(|| rpc_error("INVALID_PARAMS", "index is required"))?;
                let manager = BrowserManager::new(&cdp_url).await.map_err(|e| {
                    rpc_error("UNAVAILABLE", &format!("connect browser failed: {}", e))
                })?;
                let tabs = manager
                    .list_targets()
                    .await
                    .map_err(|e| rpc_error("UNAVAILABLE", &format!("list tabs failed: {}", e)))?;
                let target = tabs
                    .get(idx)
                    .ok_or_else(|| rpc_error("INVALID_REQUEST", "tab not found"))?;
                browser_activate_target(&cdp_url, &target.target_id, timeout_ms).await?;
                return Ok(serde_json::json!({
                    "ok": true,
                    "targetId": target.target_id,
                }));
            }
            _ => return Err(rpc_error("INVALID_PARAMS", "unknown tab action")),
        }
    }

    if let Some(target_id) = browser_path_param(path, "/tabs/", "/navigate")
        && method == "POST"
    {
        let url = p
            .get("body")
            .and_then(|v| v.get("url"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| rpc_error("INVALID_PARAMS", "url is required"))?;
        let mut page = browser_open_page_for_target(&cdp_url, &target_id, timeout_ms).await?;
        let frame_id = page
            .navigate(url)
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("navigate failed: {}", e)))?;
        return Ok(serde_json::json!({ "ok": true, "frameId": frame_id }));
    }

    if let Some(target_id) = browser_path_param(path, "/tabs/", "/evaluate")
        && method == "POST"
    {
        let expr = p
            .get("body")
            .and_then(|v| v.get("expression"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| rpc_error("INVALID_PARAMS", "expression is required"))?;
        let page = browser_open_page_for_target(&cdp_url, &target_id, timeout_ms).await?;
        let result = page
            .evaluate(expr)
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("evaluate failed: {}", e)))?;
        return serde_json::to_value(result).map_err(|e| rpc_error("INTERNAL", &e.to_string()));
    }

    if let Some(target_id) = browser_path_param(path, "/tabs/", "/screenshot")
        && method == "POST"
    {
        let page = browser_open_page_for_target(&cdp_url, &target_id, timeout_ms).await?;
        let bytes = page
            .take_screenshot()
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("screenshot failed: {}", e)))?;
        return Ok(serde_json::json!({
            "ok": true,
            "mimeType": "image/png",
            "base64": base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                bytes,
            )
        }));
    }

    if let Some(target_id) = browser_path_param(path, "/tabs/", "/html")
        && method == "GET"
    {
        let page = browser_open_page_for_target(&cdp_url, &target_id, timeout_ms).await?;
        let html = page
            .get_html()
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("get html failed: {}", e)))?;
        return Ok(serde_json::json!({
            "ok": true,
            "html": html,
        }));
    }

    if method == "POST" && path == "/navigate" {
        let url = browser_request_str(p, "url")
            .ok_or_else(|| rpc_error("INVALID_PARAMS", "url is required"))?;
        let target_id = browser_resolve_target_id(&cdp_url, p, timeout_ms).await?;
        let mut page = browser_open_page_for_target(&cdp_url, &target_id, timeout_ms).await?;
        let frame_id = page
            .navigate(url)
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("navigate failed: {}", e)))?;
        return Ok(serde_json::json!({
            "ok": true,
            "targetId": target_id,
            "url": url,
            "frameId": frame_id,
        }));
    }

    if method == "POST" && path == "/screenshot" {
        let target_id = browser_resolve_target_id(&cdp_url, p, timeout_ms).await?;
        let page = browser_open_page_for_target(&cdp_url, &target_id, timeout_ms).await?;
        let bytes = page
            .take_screenshot()
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("screenshot failed: {}", e)))?;
        return Ok(serde_json::json!({
            "ok": true,
            "targetId": target_id,
            "mimeType": "image/png",
            "base64": base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                bytes,
            )
        }));
    }

    if method == "POST" && path == "/pdf" {
        let target_id = browser_resolve_target_id(&cdp_url, p, timeout_ms).await?;
        let page = browser_open_page_for_target(&cdp_url, &target_id, timeout_ms).await?;
        let bytes = page
            .get_pdf()
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("pdf failed: {}", e)))?;
        return Ok(serde_json::json!({
            "ok": true,
            "targetId": target_id,
            "mimeType": "application/pdf",
            "base64": base64::Engine::encode(
                &base64::engine::general_purpose::STANDARD,
                bytes,
            )
        }));
    }

    if method == "GET" && path == "/snapshot" {
        let target_id = browser_resolve_target_id(&cdp_url, p, timeout_ms).await?;
        let page = browser_open_page_for_target(&cdp_url, &target_id, timeout_ms).await?;
        let html = page
            .get_html()
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("snapshot failed: {}", e)))?;
        let title_obj = page
            .evaluate("document.title")
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("title evaluate failed: {}", e)))?;
        let title = title_obj
            .value
            .as_ref()
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        return Ok(serde_json::json!({
            "ok": true,
            "targetId": target_id,
            "format": "html",
            "title": title,
            "html": html,
        }));
    }

    if method == "GET" && path == "/cookies" {
        let target_id = browser_resolve_target_id(&cdp_url, p, timeout_ms).await?;
        let page = browser_open_page_for_target(&cdp_url, &target_id, timeout_ms).await?;
        let cookies = page
            .get_cookies()
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("cookies get failed: {}", e)))?;
        return Ok(serde_json::json!({
            "ok": true,
            "targetId": target_id,
            "cookies": cookies,
        }));
    }

    if method == "POST" && path == "/cookies/set" {
        let target_id = browser_resolve_target_id(&cdp_url, p, timeout_ms).await?;
        let cookie = p
            .get("body")
            .and_then(|v| v.get("cookie"))
            .filter(|v| v.is_object())
            .ok_or_else(|| rpc_error("INVALID_PARAMS", "cookie is required"))?;
        let page = browser_open_page_for_target(&cdp_url, &target_id, timeout_ms).await?;
        page.set_cookies(vec![cookie.clone()])
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("cookies set failed: {}", e)))?;
        return Ok(serde_json::json!({
            "ok": true,
            "targetId": target_id,
        }));
    }

    if method == "POST" && path == "/cookies/clear" {
        let target_id = browser_resolve_target_id(&cdp_url, p, timeout_ms).await?;
        browser_clear_cookies_for_target(&cdp_url, &target_id, timeout_ms).await?;
        return Ok(serde_json::json!({
            "ok": true,
            "targetId": target_id,
        }));
    }

    if method == "GET" && path.starts_with("/storage/") {
        let kind = path
            .trim_start_matches("/storage/")
            .trim_matches('/')
            .to_string();
        if kind != "local" && kind != "session" {
            return Err(rpc_error("INVALID_PARAMS", "kind must be local|session"));
        }
        let key = browser_request_str(p, "key").map(ToString::to_string);
        let target_id = browser_resolve_target_id(&cdp_url, p, timeout_ms).await?;
        let page = browser_open_page_for_target(&cdp_url, &target_id, timeout_ms).await?;
        let storage = if kind == "local" {
            "localStorage"
        } else {
            "sessionStorage"
        };
        let expr = if let Some(key) = key {
            let key_json = serde_json::to_string(&key).unwrap_or_else(|_| "\"\"".to_string());
            format!(
                "(function(){{const s=window.{storage};return s.getItem({key_json});}})()",
                storage = storage,
                key_json = key_json
            )
        } else {
            format!(
                "(function(){{const s=window.{storage};const out={{}};for(let i=0;i<s.length;i++){{const k=s.key(i);out[k]=s.getItem(k);}}return out;}})()",
                storage = storage
            )
        };
        let value = page
            .evaluate(&expr)
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("storage get failed: {}", e)))?
            .value
            .unwrap_or(serde_json::Value::Null);
        return Ok(serde_json::json!({
            "ok": true,
            "targetId": target_id,
            "kind": kind,
            "value": value,
        }));
    }

    if method == "POST" && (path == "/storage/local/set" || path == "/storage/session/set") {
        let kind = if path.contains("/local/") {
            "localStorage"
        } else {
            "sessionStorage"
        };
        let key = browser_request_str(p, "key")
            .ok_or_else(|| rpc_error("INVALID_PARAMS", "key is required"))?;
        let value = p
            .get("body")
            .and_then(|v| v.get("value"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let target_id = browser_resolve_target_id(&cdp_url, p, timeout_ms).await?;
        let page = browser_open_page_for_target(&cdp_url, &target_id, timeout_ms).await?;
        let key_json = serde_json::to_string(key).unwrap_or_else(|_| "\"\"".to_string());
        let value_json = serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string());
        let expr = format!(
            "(function(){{window.{kind}.setItem({key_json},{value_json});return true;}})()",
            kind = kind,
            key_json = key_json,
            value_json = value_json
        );
        page.evaluate(&expr)
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("storage set failed: {}", e)))?;
        return Ok(serde_json::json!({
            "ok": true,
            "targetId": target_id,
        }));
    }

    if method == "POST" && (path == "/storage/local/clear" || path == "/storage/session/clear") {
        let kind = if path.contains("/local/") {
            "localStorage"
        } else {
            "sessionStorage"
        };
        let target_id = browser_resolve_target_id(&cdp_url, p, timeout_ms).await?;
        let page = browser_open_page_for_target(&cdp_url, &target_id, timeout_ms).await?;
        let expr = format!(
            "(function(){{window.{kind}.clear();return true;}})()",
            kind = kind
        );
        page.evaluate(&expr)
            .await
            .map_err(|e| rpc_error("UNAVAILABLE", &format!("storage clear failed: {}", e)))?;
        return Ok(serde_json::json!({
            "ok": true,
            "targetId": target_id,
        }));
    }

    if method == "POST" && path == "/set/offline" {
        let offline = browser_request_bool(p, "offline")
            .ok_or_else(|| rpc_error("INVALID_PARAMS", "offline is required"))?;
        let target_id = browser_resolve_target_id(&cdp_url, p, timeout_ms).await?;
        browser_set_offline_for_target(&cdp_url, &target_id, offline, timeout_ms).await?;
        return Ok(serde_json::json!({
            "ok": true,
            "targetId": target_id,
            "offline": offline,
        }));
    }

    Err(rpc_error(
        "INVALID_REQUEST",
        &format!("browser route not found: {} {}", method, path),
    ))
}

async fn rpc_browser_request(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let method = p
        .get("method")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("")
        .to_ascii_uppercase();
    let path = p
        .get("path")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("")
        .to_string();
    if method.is_empty() || path.is_empty() {
        return Err(rpc_error("INVALID_PARAMS", "method and path are required"));
    }
    if method != "GET" && method != "POST" && method != "DELETE" {
        return Err(rpc_error(
            "INVALID_PARAMS",
            "method must be GET, POST, or DELETE",
        ));
    }

    let timeout_ms = p
        .get("timeoutMs")
        .and_then(|v| v.as_u64())
        .unwrap_or(120_000)
        .min(600_000);
    let explicit_node = p
        .get("nodeId")
        .or_else(|| p.get("id"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);
    let node_target = browser_resolve_node_target(state, explicit_node.as_deref()).await?;
    if let Some(node) = node_target {
        let allowlist = browser_resolve_node_command_allowlist(state, node.platform.as_deref());
        browser_validate_node_command_allowed("browser.proxy", &node.commands, &allowlist)?;
        let mut query = p.get("query").cloned().unwrap_or(serde_json::json!({}));
        if !query.is_object() {
            query = serde_json::json!({});
        }
        let profile = p
            .get("query")
            .and_then(|v| v.get("profile"))
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty());
        if let Some(profile) = profile
            && let Some(obj) = query.as_object_mut()
        {
            obj.insert(
                "profile".to_string(),
                serde_json::Value::String(profile.to_string()),
            );
        }
        let invoke_params = serde_json::json!({
            "nodeId": node.node_id,
            "command": "browser.proxy",
            "params": {
                "method": method,
                "path": path,
                "query": query,
                "body": p.get("body").cloned().unwrap_or(serde_json::Value::Null),
                "timeoutMs": timeout_ms,
            },
            "timeoutMs": timeout_ms,
        });
        let response = rpc_node_invoke(&invoke_params, state).await?;
        let payload = response
            .get("payload")
            .cloned()
            .unwrap_or(serde_json::Value::Null);
        if let Some(obj) = payload.as_object()
            && obj.contains_key("result")
        {
            let mut result = obj
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let mapping = browser_persist_proxy_files(obj.get("files")).await?;
            browser_apply_proxy_paths(&mut result, &mapping);
            return Ok(result);
        }
        return Ok(payload);
    }

    if !browser_enabled_from_state(state) {
        return Err(rpc_error("UNAVAILABLE", "browser control is disabled"));
    }

    browser_request_local(&method, &path, p, state, timeout_ms).await
}

fn normalize_role(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

fn normalize_scopes(raw: Option<&serde_json::Value>) -> Vec<String> {
    raw.and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToString::to_string)
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn merge_optional_string_lists(
    a: Option<Vec<String>>,
    b: Option<Vec<String>>,
) -> Option<Vec<String>> {
    let mut merged = std::collections::BTreeSet::new();
    if let Some(values) = a {
        for v in values {
            let t = v.trim();
            if !t.is_empty() {
                merged.insert(t.to_string());
            }
        }
    }
    if let Some(values) = b {
        for v in values {
            let t = v.trim();
            if !t.is_empty() {
                merged.insert(t.to_string());
            }
        }
    }
    if merged.is_empty() {
        None
    } else {
        Some(merged.into_iter().collect())
    }
}

fn generate_device_auth_token() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.random()).collect();
    format!(
        "dev_{}",
        base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, bytes)
    )
}

fn summarize_device_tokens(
    tokens: &HashMap<String, DeviceAuthTokenRecord>,
) -> Vec<serde_json::Value> {
    let mut out: Vec<serde_json::Value> = tokens
        .values()
        .map(|v| {
            serde_json::json!({
                "role": v.role,
                "scopes": v.scopes,
                "createdAtMs": v.created_at_ms,
                "rotatedAtMs": v.rotated_at_ms,
                "revokedAtMs": v.revoked_at_ms,
                "lastUsedAtMs": v.last_used_at_ms,
            })
        })
        .collect();
    out.sort_by(|a, b| a["role"].as_str().cmp(&b["role"].as_str()));
    out
}

fn redact_paired_device(device: &DevicePairedRecord) -> serde_json::Value {
    serde_json::json!({
        "deviceId": device.device_id,
        "publicKey": device.public_key,
        "displayName": device.display_name,
        "platform": device.platform,
        "clientId": device.client_id,
        "clientMode": device.client_mode,
        "role": device.role,
        "roles": device.roles,
        "scopes": device.scopes,
        "approvedScopes": device.approved_scopes,
        "remoteIp": device.remote_ip,
        "tokens": summarize_device_tokens(&device.tokens),
        "createdAtMs": device.created_at_ms,
        "approvedAtMs": device.approved_at_ms,
    })
}

async fn persist_device_pairing_state(state: &HttpState) -> Result<(), ErrorDetails> {
    let pending = state.device_pair_pending.lock().await.clone();
    let paired = state.device_paired.lock().await.clone();
    let snapshot = DevicePairingSnapshot {
        pending_by_id: pending,
        paired_by_device_id: paired,
    };
    persist_device_pairing_snapshot(&snapshot)
        .map_err(|e| rpc_error("INTERNAL", &format!("persist device pairing failed: {}", e)))
}

async fn rpc_device_pair_list(_p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let mut pending: Vec<DevicePairPendingRecord> = state
        .device_pair_pending
        .lock()
        .await
        .values()
        .cloned()
        .collect();
    pending.sort_by(|a, b| b.ts.cmp(&a.ts));
    let mut paired: Vec<DevicePairedRecord> =
        state.device_paired.lock().await.values().cloned().collect();
    paired.sort_by(|a, b| b.approved_at_ms.cmp(&a.approved_at_ms));
    Ok(serde_json::json!({
        "pending": pending,
        "paired": paired.iter().map(redact_paired_device).collect::<Vec<_>>(),
    }))
}

async fn rpc_device_pair_approve(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let request_id = p
        .get("requestId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'requestId'"))?
        .to_string();

    let pending = {
        let mut pending_map = state.device_pair_pending.lock().await;
        let pending = pending_map.remove(&request_id);
        if let Some(rec) = pending.as_ref() {
            state
                .device_pair_pending_index
                .lock()
                .await
                .remove(&rec.device_id);
        }
        pending
    }
    .ok_or_else(|| rpc_error("INVALID_REQUEST", "unknown requestId"))?;

    let now = now_epoch_ms();
    let mut paired = state.device_paired.lock().await;
    let existing = paired.get(&pending.device_id).cloned();
    let mut tokens = existing
        .as_ref()
        .map(|d| d.tokens.clone())
        .unwrap_or_default();

    let role = normalize_role(pending.role.as_deref());
    let pending_roles = if let Some(ref roles) = pending.roles {
        Some(roles.clone())
    } else {
        role.clone().map(|r| vec![r])
    };
    let requested_scopes = pending.scopes.clone();
    if let Some(role_name) = role.as_ref() {
        let existing_token = tokens.get(role_name).cloned();
        let scopes = if let Some(scopes) = requested_scopes.clone() {
            scopes
        } else if let Some(token) = existing_token.as_ref() {
            token.scopes.clone()
        } else {
            existing
                .as_ref()
                .and_then(|d| d.approved_scopes.clone().or_else(|| d.scopes.clone()))
                .unwrap_or_default()
        };
        tokens.insert(
            role_name.clone(),
            DeviceAuthTokenRecord {
                role: role_name.clone(),
                token: generate_device_auth_token(),
                scopes,
                created_at_ms: existing_token
                    .as_ref()
                    .map(|t| t.created_at_ms)
                    .unwrap_or(now),
                rotated_at_ms: existing_token.as_ref().map(|_| now),
                revoked_at_ms: None,
                last_used_at_ms: existing_token.and_then(|t| t.last_used_at_ms),
            },
        );
    }

    let merged_roles = merge_optional_string_lists(
        existing.as_ref().and_then(|d| d.roles.clone()),
        pending_roles,
    );
    let merged_scopes = merge_optional_string_lists(
        existing.as_ref().and_then(|d| d.scopes.clone()),
        pending.scopes.clone(),
    );
    let merged_approved_scopes = merge_optional_string_lists(
        existing
            .as_ref()
            .and_then(|d| d.approved_scopes.clone().or_else(|| d.scopes.clone())),
        pending.scopes.clone(),
    );

    let rec = DevicePairedRecord {
        device_id: pending.device_id.clone(),
        public_key: pending.public_key.clone(),
        display_name: pending.display_name.clone(),
        platform: pending.platform.clone(),
        client_id: pending.client_id.clone(),
        client_mode: pending.client_mode.clone(),
        role,
        roles: merged_roles,
        scopes: merged_scopes.clone(),
        approved_scopes: merged_approved_scopes.or(merged_scopes),
        remote_ip: pending.remote_ip.clone(),
        tokens,
        created_at_ms: existing.as_ref().map(|d| d.created_at_ms).unwrap_or(now),
        approved_at_ms: now,
    };
    paired.insert(rec.device_id.clone(), rec.clone());
    drop(paired);
    persist_device_pairing_state(state).await?;
    state.emit_event(
        "device.pair.resolved",
        serde_json::json!({
            "requestId": request_id,
            "deviceId": rec.device_id,
            "decision": "approved",
            "ts": now,
        }),
    );

    Ok(serde_json::json!({
        "requestId": request_id,
        "device": redact_paired_device(&rec),
    }))
}

async fn rpc_device_pair_reject(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let request_id = p
        .get("requestId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'requestId'"))?
        .to_string();
    let removed = {
        let mut pending = state.device_pair_pending.lock().await;
        let removed = pending.remove(&request_id);
        if let Some(rec) = removed.as_ref() {
            state
                .device_pair_pending_index
                .lock()
                .await
                .remove(&rec.device_id);
        }
        removed
    };
    let Some(rec) = removed else {
        return Err(rpc_error("INVALID_REQUEST", "unknown requestId"));
    };
    persist_device_pairing_state(state).await?;
    state.emit_event(
        "device.pair.resolved",
        serde_json::json!({
            "requestId": request_id,
            "deviceId": rec.device_id,
            "decision": "rejected",
            "ts": now_epoch_ms(),
        }),
    );
    Ok(serde_json::json!({
        "requestId": request_id,
        "deviceId": rec.device_id,
        "rejected": true,
    }))
}

async fn rpc_device_pair_remove(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let device_id = p
        .get("deviceId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'deviceId'"))?
        .to_string();
    let removed = state.device_paired.lock().await.remove(&device_id);
    if removed.is_none() {
        return Err(rpc_error("INVALID_REQUEST", "unknown deviceId"));
    }
    persist_device_pairing_state(state).await?;
    Ok(serde_json::json!({
        "deviceId": device_id,
        "removed": true,
    }))
}

async fn rpc_device_token_rotate(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let device_id = p
        .get("deviceId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'deviceId'"))?
        .to_string();
    let role = normalize_role(p.get("role").and_then(|v| v.as_str()))
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'role'"))?;
    let requested_scopes = normalize_scopes(p.get("scopes"));

    let now = now_epoch_ms();
    let mut paired = state.device_paired.lock().await;
    let rec = paired
        .get_mut(&device_id)
        .ok_or_else(|| rpc_error("INVALID_REQUEST", "unknown deviceId"))?;
    let existing = rec.tokens.get(&role).cloned();

    let next_scopes = if !requested_scopes.is_empty() {
        requested_scopes
    } else if let Some(token) = existing.as_ref() {
        token.scopes.clone()
    } else {
        rec.approved_scopes
            .clone()
            .or_else(|| rec.scopes.clone())
            .unwrap_or_default()
    };
    let approved = rec
        .approved_scopes
        .clone()
        .or_else(|| rec.scopes.clone())
        .unwrap_or_default();
    if !approved.is_empty() {
        let approved_set: std::collections::HashSet<String> = approved.into_iter().collect();
        if next_scopes.iter().any(|s| !approved_set.contains(s)) {
            return Err(rpc_error(
                "INVALID_REQUEST",
                "requested scopes exceed approved scopes",
            ));
        }
    }

    let entry = DeviceAuthTokenRecord {
        role: role.clone(),
        token: generate_device_auth_token(),
        scopes: next_scopes.clone(),
        created_at_ms: existing.as_ref().map(|v| v.created_at_ms).unwrap_or(now),
        rotated_at_ms: Some(now),
        revoked_at_ms: None,
        last_used_at_ms: existing.and_then(|v| v.last_used_at_ms),
    };
    rec.tokens.insert(role.clone(), entry.clone());
    drop(paired);
    persist_device_pairing_state(state).await?;
    Ok(serde_json::json!({
        "deviceId": device_id,
        "role": role,
        "token": entry.token,
        "scopes": next_scopes,
        "rotatedAtMs": now,
    }))
}

async fn rpc_device_token_revoke(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let device_id = p
        .get("deviceId")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'deviceId'"))?
        .to_string();
    let role = normalize_role(p.get("role").and_then(|v| v.as_str()))
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'role'"))?;

    let now = now_epoch_ms();
    let mut paired = state.device_paired.lock().await;
    let rec = paired
        .get_mut(&device_id)
        .ok_or_else(|| rpc_error("INVALID_REQUEST", "unknown deviceId"))?;
    let Some(entry) = rec.tokens.get_mut(&role) else {
        return Err(rpc_error("INVALID_REQUEST", "unknown deviceId/role"));
    };
    entry.revoked_at_ms = Some(now);
    drop(paired);
    persist_device_pairing_state(state).await?;
    Ok(serde_json::json!({
        "deviceId": device_id,
        "role": role,
        "revokedAtMs": now,
    }))
}

// ── Wizard RPCs ─────────────────────────────────────────────────────────

async fn rpc_wizard_start(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let _wizard_type = p["type"].as_str().unwrap_or("setup");
    let session_id = uuid::Uuid::new_v4().to_string();
    let mut session = WizardSessionState::new();
    session.updated_at_ms = WizardSessionState::now_ms();
    let total_steps = session.steps.len();
    let current = session.steps.first().cloned();
    _state
        .wizard_sessions
        .lock()
        .await
        .insert(session_id.clone(), session);
    Ok(serde_json::json!({
        "sessionId": session_id,
        "status": "running",
        "step": 0,
        "totalSteps": total_steps,
        "prompt": current,
    }))
}

async fn rpc_wizard_next(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let session_id = p["sessionId"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'sessionId'"))?;
    let answer = p.get("answer").cloned();
    let mut sessions = state.wizard_sessions.lock().await;
    let session = sessions
        .get_mut(session_id)
        .ok_or_else(|| rpc_error("NOT_FOUND", "Wizard session not found"))?;
    if session.status != "running" {
        return Ok(serde_json::json!({
            "sessionId": session_id,
            "status": session.status,
            "done": true,
        }));
    }
    if let Some(ans) = answer
        && let Some(step) = session.steps.get_mut(session.current_step)
    {
        step.answer = Some(ans);
    }
    if session.current_step + 1 >= session.steps.len() {
        session.status = "completed".to_string();
    } else {
        session.current_step += 1;
    }
    session.updated_at_ms = WizardSessionState::now_ms();
    let done = session.status != "running";
    let prompt = if done {
        None
    } else {
        session.steps.get(session.current_step).cloned()
    };
    Ok(serde_json::json!({
        "sessionId": session_id,
        "status": session.status,
        "step": session.current_step,
        "totalSteps": session.steps.len(),
        "done": done,
        "prompt": prompt,
    }))
}

async fn rpc_wizard_cancel(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let session_id = p["sessionId"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'sessionId'"))?;
    let mut sessions = state.wizard_sessions.lock().await;
    let Some(mut session) = sessions.remove(session_id) else {
        return Err(rpc_error("NOT_FOUND", "Wizard session not found"));
    };
    session.status = "cancelled".to_string();
    session.updated_at_ms = WizardSessionState::now_ms();
    Ok(serde_json::json!({
        "sessionId": session_id,
        "status": session.status,
        "done": true,
    }))
}

async fn rpc_wizard_status(state: &HttpState) -> RpcResult {
    let session_id = state
        .wizard_sessions
        .lock()
        .await
        .iter()
        .next()
        .map(|(id, _)| id.clone());
    let Some(session_id) = session_id else {
        return Ok(serde_json::json!({"active": false}));
    };
    let sessions = state.wizard_sessions.lock().await;
    let Some(session) = sessions.get(&session_id) else {
        return Ok(serde_json::json!({"active": false}));
    };
    Ok(serde_json::json!({
        "active": session.status == "running",
        "sessionId": session_id,
        "status": session.status,
        "currentStep": session.current_step,
        "totalSteps": session.steps.len(),
        "createdAtMs": session.created_at_ms,
        "updatedAtMs": session.updated_at_ms,
        "error": session.error,
    }))
}

// ── Logs RPCs ───────────────────────────────────────────────────────────

fn resolve_default_log_file(state: &HttpState) -> PathBuf {
    if let Some(cfg) = state.full_config.as_ref()
        && let Ok(cfg_guard) = cfg.try_read()
        && let Some(logging) = cfg_guard.logging.as_ref()
        && let Some(file) = logging.file.as_ref()
    {
        return PathBuf::from(file);
    }
    if let Ok(file) = std::env::var("OCLAWS_LOG_FILE") {
        return PathBuf::from(file);
    }
    let log_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".oclaw")
        .join("logs");
    let mut latest: Option<(std::time::SystemTime, PathBuf)> = None;
    if let Ok(rd) = std::fs::read_dir(&log_dir) {
        for entry in rd.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("log") {
                continue;
            }
            let mtime = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .unwrap_or(std::time::UNIX_EPOCH);
            match &latest {
                Some((old, _)) if *old >= mtime => {}
                _ => latest = Some((mtime, path)),
            }
        }
    }
    latest
        .map(|(_, p)| p)
        .unwrap_or_else(|| log_dir.join("gateway.log"))
}

async fn rpc_logs_tail(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let limit = p["limit"].as_u64().unwrap_or(500).clamp(1, 5000) as usize;
    let max_bytes = p["maxBytes"]
        .as_u64()
        .unwrap_or(250_000)
        .clamp(1024, 1_000_000) as usize;
    let cursor = p["cursor"].as_u64();
    let file = p["file"]
        .as_str()
        .map(PathBuf::from)
        .unwrap_or_else(|| resolve_default_log_file(state));

    let meta = tokio::fs::metadata(&file)
        .await
        .map_err(|e| rpc_error("UNAVAILABLE", &format!("log file unavailable: {}", e)))?;
    let size = meta.len() as usize;

    let (start, truncated, reset) = if let Some(c) = cursor {
        let c = c as usize;
        if c > size {
            let s = size.saturating_sub(max_bytes);
            (s, s > 0, true)
        } else if size.saturating_sub(c) > max_bytes {
            let s = size.saturating_sub(max_bytes);
            (s, true, true)
        } else {
            (c, false, false)
        }
    } else {
        let s = size.saturating_sub(max_bytes);
        (s, s > 0, false)
    };

    let mut fh = tokio::fs::File::open(&file)
        .await
        .map_err(|e| rpc_error("UNAVAILABLE", &format!("open log failed: {}", e)))?;
    fh.seek(std::io::SeekFrom::Start(start as u64))
        .await
        .map_err(|e| rpc_error("UNAVAILABLE", &format!("seek log failed: {}", e)))?;
    let mut buf = Vec::with_capacity(size.saturating_sub(start));
    fh.read_to_end(&mut buf)
        .await
        .map_err(|e| rpc_error("UNAVAILABLE", &format!("read log failed: {}", e)))?;
    let text = String::from_utf8_lossy(&buf);
    let mut lines: Vec<String> = text.lines().map(ToString::to_string).collect();
    if lines.len() > limit {
        lines = lines.split_off(lines.len() - limit);
    }

    Ok(serde_json::json!({
        "file": file,
        "cursor": size as u64,
        "size": size as u64,
        "lines": lines,
        "truncated": truncated,
        "reset": reset,
    }))
}

// ── Exec Approval RPCs ──────────────────────────────────────────────────

fn approvals_snapshot_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".oclaw")
        .join("exec-approvals.json")
}

fn approvals_hash(snapshot: &ExecApprovalsFileSnapshot) -> Result<String, ErrorDetails> {
    use sha2::Digest;
    let raw = serde_json::to_vec(snapshot).map_err(|e| rpc_error("INTERNAL", &e.to_string()))?;
    let hash = sha2::Sha256::digest(raw);
    Ok(hex::encode(hash))
}

fn approval_decision_from_raw(raw: &str) -> Option<ApprovalDecision> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "allow" | "allow-once" | "once" | "approved" | "approve" => {
            Some(ApprovalDecision::Approved)
        }
        "allow-always" | "always" => Some(ApprovalDecision::Approved),
        "deny" | "reject" | "rejected" => Some(ApprovalDecision::Denied),
        "pending" => Some(ApprovalDecision::Pending),
        _ => None,
    }
}

async fn rpc_exec_approvals_list(state: &HttpState) -> RpcResult {
    let Some(ref gate) = state.approval_gate else {
        return Ok(serde_json::json!({
            "enabled": false,
            "pending": [],
            "note": "Approval gate not configured",
        }));
    };
    let pending = gate.pending_requests().await;
    let val = serde_json::to_value(&pending).map_err(|e| rpc_error("INTERNAL", &e.to_string()))?;
    Ok(serde_json::json!({"enabled": true, "pending": val}))
}

async fn rpc_exec_approvals_get(state: &HttpState) -> RpcResult {
    let snapshot = state.exec_approvals_snapshot.read().await.clone();
    let hash = approvals_hash(&snapshot)?;
    let path = approvals_snapshot_path();
    Ok(serde_json::json!({
        "path": path,
        "exists": path.exists(),
        "hash": hash,
        "file": snapshot,
    }))
}

async fn rpc_exec_approvals_set(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let file = p
        .get("file")
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'file'"))?;
    let incoming: ExecApprovalsFileSnapshot = serde_json::from_value(file.clone())
        .map_err(|e| rpc_error("INVALID_PARAMS", &format!("Invalid approvals file: {}", e)))?;

    let current = state.exec_approvals_snapshot.read().await.clone();
    let current_hash = approvals_hash(&current)?;
    let base_hash = p.get("baseHash").and_then(|v| v.as_str());
    let path = approvals_snapshot_path();
    if path.exists() {
        let Some(base_hash) = base_hash else {
            return Err(rpc_error(
                "INVALID_REQUEST",
                "exec approvals base hash required; re-run exec.approvals.get and retry",
            ));
        };
        if base_hash != current_hash {
            return Err(rpc_error(
                "INVALID_REQUEST",
                "exec approvals changed since last load; re-run exec.approvals.get and retry",
            ));
        }
    }

    {
        let mut guard = state.exec_approvals_snapshot.write().await;
        *guard = incoming.clone();
    }

    if let Some(cfg) = state.full_config.as_ref() {
        let mut cfg = cfg.write().await;
        cfg.approvals = Some(
            serde_json::to_value(&incoming).map_err(|e| rpc_error("INTERNAL", &e.to_string()))?,
        );
    }
    persist_full_config(state).await?;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| rpc_error("INTERNAL", &format!("Create approvals dir failed: {}", e)))?;
    }
    let raw = serde_json::to_string_pretty(&incoming)
        .map_err(|e| rpc_error("INTERNAL", &e.to_string()))?;
    std::fs::write(&path, format!("{}\n", raw))
        .map_err(|e| rpc_error("INTERNAL", &format!("Write approvals file failed: {}", e)))?;

    rpc_exec_approvals_get(state).await
}

async fn rpc_exec_approval_request(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let Some(ref gate) = state.approval_gate else {
        return Err(rpc_error("NOT_ENABLED", "Approval gate not configured"));
    };
    let tool = p
        .get("tool")
        .or_else(|| p.get("command"))
        .and_then(|v| v.as_str())
        .unwrap_or("exec");
    let summary_json = serde_json::to_string(p).unwrap_or_else(|_| "{}".to_string());
    let req = gate.request_approval(tool, &summary_json).await;
    Ok(serde_json::json!({
        "id": req.id,
        "request": p,
        "createdAtMs": req.created_at_ms,
        "expiresAtMs": req.created_at_ms.saturating_add(5 * 60 * 1000),
    }))
}

async fn rpc_exec_approval_wait_decision(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let Some(ref gate) = state.approval_gate else {
        return Err(rpc_error("NOT_ENABLED", "Approval gate not configured"));
    };
    let id = p["id"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    let timeout_ms = p["timeoutMs"].as_u64().unwrap_or(120_000);
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let req = gate.get_request(id).await;
        let Some(req) = req else {
            return Err(rpc_error(
                "NOT_FOUND",
                &format!("Approval request not found: {}", id),
            ));
        };
        if req.decision != ApprovalDecision::Pending {
            return Ok(serde_json::json!({
                "id": id,
                "decision": req.decision,
                "resolvedBy": req.resolved_by,
                "resolvedAtMs": req.resolved_at_ms,
                "timedOut": false,
            }));
        }
        if std::time::Instant::now() >= deadline {
            return Ok(serde_json::json!({
                "id": id,
                "decision": "pending",
                "timedOut": true,
            }));
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

async fn rpc_exec_approval_resolve(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let Some(ref gate) = state.approval_gate else {
        return Err(rpc_error("NOT_ENABLED", "Approval gate not configured"));
    };
    let id = p["id"]
        .as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    let raw = p
        .get("decision")
        .and_then(|v| v.as_str())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'decision'"))?;
    let decision = approval_decision_from_raw(raw)
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Invalid decision"))?;
    let resolved_by = p
        .get("resolvedBy")
        .and_then(|v| v.as_str())
        .map(ToString::to_string);
    let ok = gate.resolve(id, decision, resolved_by).await;
    if !ok {
        return Err(rpc_error(
            "NOT_FOUND",
            &format!("Approval request not found: {}", id),
        ));
    }
    let req = gate.get_request(id).await;
    Ok(serde_json::json!({
        "ok": true,
        "id": id,
        "decision": raw,
        "request": req,
    }))
}

async fn rpc_exec_approvals_approve(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let mut resolve_params = p.clone();
    if let Some(obj) = resolve_params.as_object_mut() {
        obj.insert(
            "decision".to_string(),
            serde_json::Value::String("allow-once".to_string()),
        );
    }
    rpc_exec_approval_resolve(&resolve_params, state).await
}

async fn rpc_exec_approvals_reject(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let mut resolve_params = p.clone();
    if let Some(obj) = resolve_params.as_object_mut() {
        obj.insert(
            "decision".to_string(),
            serde_json::Value::String("deny".to_string()),
        );
    }
    rpc_exec_approval_resolve(&resolve_params, state).await
}

fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn normalize_node_id(raw: &str) -> Result<String, ErrorDetails> {
    let node_id = raw.trim();
    if node_id.is_empty() {
        return Err(rpc_error("INVALID_PARAMS", "nodeId cannot be empty"));
    }
    if node_id.contains("..") || node_id.contains('/') || node_id.contains('\\') {
        return Err(rpc_error("INVALID_PARAMS", "Invalid nodeId"));
    }
    Ok(node_id.to_string())
}

fn require_node_id(p: &serde_json::Value) -> Result<String, ErrorDetails> {
    let raw = p
        .get("nodeId")
        .or_else(|| p.get("id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'nodeId'"))?;
    normalize_node_id(raw)
}

fn require_request_id(p: &serde_json::Value) -> Result<String, ErrorDetails> {
    let raw = p
        .get("requestId")
        .or_else(|| p.get("id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'requestId'"))?;
    let request_id = raw.trim();
    if request_id.is_empty() {
        return Err(rpc_error("INVALID_PARAMS", "requestId cannot be empty"));
    }
    Ok(request_id.to_string())
}

fn node_approvals_snapshot_path(node_id: &str) -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".oclaw")
        .join("nodes")
        .join(node_id)
        .join("exec-approvals.json")
}

async fn load_node_exec_approvals_snapshot(
    state: &HttpState,
    node_id: &str,
) -> Result<ExecApprovalsFileSnapshot, ErrorDetails> {
    if let Some(snapshot) = state.node_exec_approvals.read().await.get(node_id).cloned() {
        return Ok(snapshot);
    }
    let path = node_approvals_snapshot_path(node_id);
    if path.exists() {
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| rpc_error("INTERNAL", &format!("Read node approvals failed: {}", e)))?;
        let snapshot: ExecApprovalsFileSnapshot = serde_json::from_str(&raw)
            .map_err(|e| rpc_error("INVALID_REQUEST", &format!("Invalid node approvals: {}", e)))?;
        let mut guard = state.node_exec_approvals.write().await;
        guard.insert(node_id.to_string(), snapshot.clone());
        return Ok(snapshot);
    }
    let snapshot = ExecApprovalsFileSnapshot::default();
    let mut guard = state.node_exec_approvals.write().await;
    guard.insert(node_id.to_string(), snapshot.clone());
    Ok(snapshot)
}

fn persist_node_exec_approvals(
    node_id: &str,
    snapshot: &ExecApprovalsFileSnapshot,
) -> Result<(), ErrorDetails> {
    let path = node_approvals_snapshot_path(node_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            rpc_error(
                "INTERNAL",
                &format!("Create node approvals dir failed: {}", e),
            )
        })?;
    }
    let raw = serde_json::to_string_pretty(snapshot)
        .map_err(|e| rpc_error("INTERNAL", &e.to_string()))?;
    std::fs::write(path, format!("{}\n", raw))
        .map_err(|e| rpc_error("INTERNAL", &format!("Write node approvals failed: {}", e)))?;
    Ok(())
}

async fn rpc_exec_approvals_node_get(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let node_id = require_node_id(p)?;
    let snapshot = load_node_exec_approvals_snapshot(state, &node_id).await?;
    let hash = approvals_hash(&snapshot)?;
    let path = node_approvals_snapshot_path(&node_id);
    Ok(serde_json::json!({
        "nodeId": node_id,
        "path": path,
        "exists": path.exists(),
        "hash": hash,
        "file": snapshot,
    }))
}

async fn rpc_exec_approvals_node_set(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let node_id = require_node_id(p)?;
    let file = p
        .get("file")
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'file'"))?;
    let incoming: ExecApprovalsFileSnapshot = serde_json::from_value(file.clone())
        .map_err(|e| rpc_error("INVALID_PARAMS", &format!("Invalid approvals file: {}", e)))?;

    let current = load_node_exec_approvals_snapshot(state, &node_id).await?;
    let current_hash = approvals_hash(&current)?;
    let base_hash = p.get("baseHash").and_then(|v| v.as_str());
    let path = node_approvals_snapshot_path(&node_id);
    if path.exists() {
        let Some(base_hash) = base_hash else {
            return Err(rpc_error(
                "INVALID_REQUEST",
                "node exec approvals base hash required; re-run exec.approvals.node.get and retry",
            ));
        };
        if base_hash != current_hash {
            return Err(rpc_error(
                "INVALID_REQUEST",
                "node exec approvals changed since last load; re-run exec.approvals.node.get and retry",
            ));
        }
    }

    {
        let mut guard = state.node_exec_approvals.write().await;
        guard.insert(node_id.clone(), incoming.clone());
    }
    persist_node_exec_approvals(&node_id, &incoming)?;
    rpc_exec_approvals_node_get(&serde_json::json!({ "nodeId": node_id }), state).await
}

// ── TTS RPCs ────────────────────────────────────────────────────────────

fn tts_provider_id(provider: TtsProvider) -> &'static str {
    match provider {
        TtsProvider::OpenAI => "openai",
        TtsProvider::ElevenLabs => "elevenlabs",
        TtsProvider::Edge => "edge",
    }
}

fn parse_tts_provider(raw: &str) -> Option<TtsProvider> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "openai" => Some(TtsProvider::OpenAI),
        "elevenlabs" => Some(TtsProvider::ElevenLabs),
        "edge" => Some(TtsProvider::Edge),
        _ => None,
    }
}

async fn resolve_tts_api_key(
    state: &HttpState,
    provider: TtsProvider,
    p: &serde_json::Value,
) -> Option<String> {
    if let Some(k) = p.get("apiKey").and_then(|v| v.as_str()) {
        let trimmed = k.trim();
        if !trimmed.is_empty() {
            return Some(trimmed.to_string());
        }
    }
    let provider_id = tts_provider_id(provider);
    if let Some(cfg) = state.full_config.as_ref() {
        let cfg = cfg.read().await;
        if let Some(talk) = cfg.talk.as_ref()
            && let Some(api_key) = talk.api_key.as_ref()
        {
            let trimmed = api_key.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        if let Some(models) = cfg.models.as_ref()
            && let Some(providers) = models.providers.as_ref()
        {
            if let Some(mp) = providers.get(provider_id)
                && let Some(api_key) = mp.api_key.as_ref()
            {
                let trimmed = api_key.trim();
                if !trimmed.is_empty() {
                    return Some(trimmed.to_string());
                }
            }
            for mp in providers.values() {
                if mp.provider.eq_ignore_ascii_case(provider_id)
                    && let Some(api_key) = mp.api_key.as_ref()
                {
                    let trimmed = api_key.trim();
                    if !trimmed.is_empty() {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }
    }
    match provider {
        TtsProvider::OpenAI => std::env::var("OPENAI_API_KEY")
            .ok()
            .or_else(|| std::env::var("OCLAWS_PROVIDER_OPENAI_API_KEY").ok()),
        TtsProvider::ElevenLabs => std::env::var("ELEVENLABS_API_KEY")
            .ok()
            .or_else(|| std::env::var("OCLAWS_PROVIDER_ELEVENLABS_API_KEY").ok()),
        TtsProvider::Edge => None,
    }
}

async fn resolve_tts_model(state: &HttpState, p: &serde_json::Value) -> String {
    if let Some(m) = p.get("model").and_then(|v| v.as_str()) {
        let trimmed = m.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(cfg) = state.full_config.as_ref() {
        let cfg = cfg.read().await;
        if let Some(talk) = cfg.talk.as_ref()
            && let Some(model_id) = talk.model_id.as_ref()
        {
            let trimmed = model_id.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
        if let Some(models) = cfg.models.as_ref()
            && let Some(providers) = models.providers.as_ref()
        {
            for candidate in ["openai", "tts", "voice"] {
                if let Some(mp) = providers.get(candidate)
                    && let Some(model) = mp.model.as_ref()
                {
                    let trimmed = model.trim();
                    if !trimmed.is_empty() {
                        return trimmed.to_string();
                    }
                }
            }
        }
    }
    "tts-1".to_string()
}

async fn resolve_tts_output_format(state: &HttpState, p: &serde_json::Value) -> String {
    if let Some(v) = p.get("outputFormat").and_then(|v| v.as_str()) {
        let trimmed = v.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(cfg) = state.full_config.as_ref() {
        let cfg = cfg.read().await;
        if let Some(talk) = cfg.talk.as_ref()
            && let Some(fmt) = talk.output_format.as_ref()
        {
            let trimmed = fmt.trim();
            if !trimmed.is_empty() {
                return trimmed.to_string();
            }
        }
    }
    "mp3".to_string()
}

fn tts_nested_value<'a>(p: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    p.get(key)
        .or_else(|| p.get("edge").and_then(|v| v.get(key)))
}

fn tts_string_param(p: &serde_json::Value, key: &str) -> Option<String> {
    tts_nested_value(p, key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
}

fn tts_bool_param(p: &serde_json::Value, key: &str) -> Option<bool> {
    tts_nested_value(p, key).and_then(|v| v.as_bool())
}

fn resolve_edge_tts_options(p: &serde_json::Value, output_format: &str) -> EdgeTtsOptions {
    let timeout_ms = tts_nested_value(p, "timeoutMs")
        .or_else(|| tts_nested_value(p, "timeout"))
        .and_then(|v| v.as_u64());
    EdgeTtsOptions {
        lang: tts_string_param(p, "lang").or_else(|| tts_string_param(p, "languageCode")),
        output_format: Some(output_format.to_string()),
        save_subtitles: tts_bool_param(p, "saveSubtitles").unwrap_or(false),
        proxy: tts_string_param(p, "proxy"),
        rate: tts_string_param(p, "rate"),
        pitch: tts_string_param(p, "pitch"),
        volume: tts_string_param(p, "volume"),
        timeout_ms,
    }
}

fn resolve_tts_output_path(provider: TtsProvider, output_format: &str) -> PathBuf {
    let ext = output_format
        .split(['/', ';', '.'])
        .next_back()
        .map(|v| v.trim().to_ascii_lowercase())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "mp3".to_string());
    let file = format!(
        "{}-{}-{}.{}",
        tts_provider_id(provider),
        now_epoch_ms(),
        uuid::Uuid::new_v4(),
        ext
    );
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".oclaw")
        .join("tts")
        .join(file)
}

fn tts_providers_payload(
    active: TtsProvider,
    has_openai: bool,
    has_elevenlabs: bool,
) -> serde_json::Value {
    serde_json::json!({
        "providers": [
            {
                "id": "openai",
                "name": "OpenAI",
                "configured": has_openai,
                "models": ["tts-1","tts-1-hd"],
                "voices": ["alloy","echo","fable","onyx","nova","shimmer"],
            },
            {
                "id": "elevenlabs",
                "name": "ElevenLabs",
                "configured": has_elevenlabs,
                "models": ["eleven_multilingual_v2","eleven_turbo_v2_5","eleven_monolingual_v1"],
                "voices": ["21m00Tcm4TlvDq8ikWAM","AZnzlk1XvdvUeBnXmlld","EXAVITQu4vr4xnSDxMaL","MF3mGyEYCl7XYWbV9V6O"],
            },
            {
                "id": "edge",
                "name": "Edge TTS",
                "configured": true,
                "models": [],
                "voices": [
                    "en-US-AriaNeural","en-US-GuyNeural","en-US-JennyNeural",
                    "zh-CN-XiaoxiaoNeural","zh-CN-YunxiNeural","ja-JP-NanamiNeural",
                ],
            }
        ],
        "active": tts_provider_id(active),
    })
}

async fn rpc_tts_status(state: &HttpState) -> RpcResult {
    let runtime = state.tts_runtime.read().await.clone();
    let has_openai = resolve_tts_api_key(state, TtsProvider::OpenAI, &serde_json::Value::Null)
        .await
        .is_some();
    let has_elevenlabs =
        resolve_tts_api_key(state, TtsProvider::ElevenLabs, &serde_json::Value::Null)
            .await
            .is_some();
    Ok(serde_json::json!({
        "enabled": runtime.enabled,
        "auto": if runtime.enabled { "always" } else { "off" },
        "provider": tts_provider_id(runtime.provider),
        "fallbackProvider": serde_json::Value::Null,
        "fallbackProviders": [],
        "voice": runtime.voice,
        "hasOpenAIKey": has_openai,
        "hasElevenLabsKey": has_elevenlabs,
        "edgeEnabled": true,
    }))
}

async fn rpc_tts_enable(state: &HttpState) -> RpcResult {
    let mut runtime = state.tts_runtime.write().await;
    runtime.enabled = true;
    Ok(serde_json::json!({
        "enabled": true,
        "provider": tts_provider_id(runtime.provider),
        "voice": runtime.voice,
    }))
}

async fn rpc_tts_disable(state: &HttpState) -> RpcResult {
    let mut runtime = state.tts_runtime.write().await;
    runtime.enabled = false;
    Ok(serde_json::json!({
        "enabled": false,
        "provider": tts_provider_id(runtime.provider),
        "voice": runtime.voice,
    }))
}

async fn rpc_tts_set_provider(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let provider_raw = p
        .get("provider")
        .and_then(|v| v.as_str())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'provider'"))?;
    let provider = parse_tts_provider(provider_raw).ok_or_else(|| {
        rpc_error(
            "INVALID_PARAMS",
            "Invalid provider. Use openai, elevenlabs, or edge.",
        )
    })?;
    let mut runtime = state.tts_runtime.write().await;
    runtime.provider = provider;
    if let Some(v) = p.get("voice").and_then(|v| v.as_str()) {
        let voice = v.trim();
        runtime.voice = if voice.is_empty() {
            None
        } else {
            Some(voice.to_string())
        };
    }
    Ok(serde_json::json!({
        "provider": tts_provider_id(runtime.provider),
        "voice": runtime.voice,
    }))
}

async fn rpc_tts_convert(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let text = p
        .get("text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "tts.convert requires text"))?;
    let prepared = prepare_for_tts(text);
    if prepared.is_empty() {
        return Err(rpc_error(
            "INVALID_PARAMS",
            "text is empty after preprocessing",
        ));
    }

    let runtime = state.tts_runtime.read().await.clone();
    let provider = if let Some(raw) = p.get("provider").and_then(|v| v.as_str()) {
        parse_tts_provider(raw).ok_or_else(|| {
            rpc_error(
                "INVALID_PARAMS",
                "Invalid provider. Use openai, elevenlabs, or edge.",
            )
        })?
    } else {
        runtime.provider
    };
    let voice = p
        .get("voice")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
        .or_else(|| runtime.voice.clone());
    let output_format = resolve_tts_output_format(state, p).await;
    let output_path = resolve_tts_output_path(provider, &output_format);
    let started = std::time::Instant::now();

    let synth_res = match provider {
        TtsProvider::OpenAI => {
            let Some(key) = resolve_tts_api_key(state, provider, p).await else {
                return Err(rpc_error(
                    "UNAVAILABLE",
                    "OpenAI TTS API key is not configured",
                ));
            };
            let model = resolve_tts_model(state, p).await;
            let backend = OpenAiTts::new(key).with_model(model);
            backend
                .synthesize(&prepared, voice.as_deref(), &output_path)
                .await
        }
        TtsProvider::ElevenLabs => {
            let Some(key) = resolve_tts_api_key(state, provider, p).await else {
                return Err(rpc_error(
                    "UNAVAILABLE",
                    "ElevenLabs TTS API key is not configured",
                ));
            };
            let backend = ElevenLabsTts::new(key);
            backend
                .synthesize(&prepared, voice.as_deref(), &output_path)
                .await
        }
        TtsProvider::Edge => {
            let edge_options = resolve_edge_tts_options(p, &output_format);
            let backend = EdgeTts::new().with_options(edge_options);
            backend
                .synthesize(&prepared, voice.as_deref(), &output_path)
                .await
        }
    };

    let result = synth_res
        .map_err(|e| rpc_error("UNAVAILABLE", &format!("TTS conversion failed: {}", e)))?;
    let latency_ms = started.elapsed().as_millis() as u64;
    let include_base64 = p
        .get("includeBase64")
        .or_else(|| p.get("returnBase64"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let audio_base64 = if include_base64 {
        let bytes = std::fs::read(&result.output_path)
            .map_err(|e| rpc_error("INTERNAL", &format!("Read audio failed: {}", e)))?;
        Some(base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            bytes,
        ))
    } else {
        None
    };
    let voice_compatible = output_format.to_ascii_lowercase().contains("opus")
        || output_format.to_ascii_lowercase().contains("ogg")
        || provider != TtsProvider::Edge;

    Ok(serde_json::json!({
        "audioPath": result.output_path,
        "provider": tts_provider_id(provider),
        "outputFormat": output_format,
        "voiceCompatible": voice_compatible,
        "voice": voice,
        "bytesWritten": result.bytes_written,
        "durationMs": result.duration_ms,
        "latencyMs": latency_ms,
        "audioBase64": audio_base64,
    }))
}

async fn rpc_tts_providers(state: &HttpState) -> RpcResult {
    let runtime = state.tts_runtime.read().await.clone();
    let has_openai = resolve_tts_api_key(state, TtsProvider::OpenAI, &serde_json::Value::Null)
        .await
        .is_some();
    let has_elevenlabs =
        resolve_tts_api_key(state, TtsProvider::ElevenLabs, &serde_json::Value::Null)
            .await
            .is_some();
    Ok(tts_providers_payload(
        runtime.provider,
        has_openai,
        has_elevenlabs,
    ))
}

// ── Node RPCs ───────────────────────────────────────────────────────────

fn node_pair_pending_payload(rec: &NodePairRecord) -> serde_json::Value {
    serde_json::json!({
        "requestId": rec.request_id,
        "nodeId": rec.node_id,
        "displayName": rec.display_name,
        "platform": rec.platform,
        "version": rec.version,
        "coreVersion": rec.core_version,
        "uiVersion": rec.ui_version,
        "remoteIp": rec.remote_ip,
        "caps": rec.caps,
        "commands": rec.commands,
        "isRepair": rec.is_repair,
        "ts": rec.ts,
    })
}

fn node_pair_paired_payload(rec: &NodePairRecord, include_secrets: bool) -> serde_json::Value {
    let token = if include_secrets {
        rec.token.clone()
    } else {
        None
    };
    serde_json::json!({
        "nodeId": rec.node_id,
        "token": token,
        "displayName": rec.display_name,
        "platform": rec.platform,
        "version": rec.version,
        "coreVersion": rec.core_version,
        "uiVersion": rec.ui_version,
        "remoteIp": rec.remote_ip,
        "caps": rec.caps,
        "commands": rec.commands,
        "createdAtMs": rec.ts,
        "approvedAtMs": rec.approved_at_ms,
        "lastConnectedAtMs": rec.last_connected_at_ms,
    })
}

fn node_default_caps() -> Vec<&'static str> {
    vec!["system", "canvas", "camera", "location", "notify"]
}

fn node_default_commands() -> Vec<&'static str> {
    vec![
        "system.run",
        "system.notify",
        "canvas.present",
        "canvas.hide",
        "canvas.navigate",
        "camera.capture",
        "location.get",
    ]
}

fn parse_node_string_list(value: Option<&serde_json::Value>) -> Option<Vec<String>> {
    let arr = value.and_then(|v| v.as_array())?;
    let normalized: Vec<String> = arr
        .iter()
        .filter_map(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

async fn rpc_node_list(state: &HttpState) -> RpcResult {
    let records = state.node_pairs.lock().await.clone();
    let connected = state.node_connected.lock().await.clone();
    let mut by_node: HashMap<String, NodePairRecord> = HashMap::new();
    for rec in records.values() {
        by_node
            .entry(rec.node_id.clone())
            .and_modify(|existing| {
                if rec.approved && (!existing.approved || rec.ts > existing.ts) {
                    *existing = rec.clone();
                }
            })
            .or_insert_with(|| rec.clone());
    }

    let mut node_ids: Vec<String> = by_node.keys().cloned().collect();
    for node_id in &connected {
        if !node_ids.contains(node_id) {
            node_ids.push(node_id.clone());
        }
    }
    node_ids.sort();

    let nodes: Vec<serde_json::Value> = node_ids
        .iter()
        .map(|node_id| {
            let rec = by_node.get(node_id);
            let caps = rec
                .and_then(|r| r.caps.clone())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| {
                    node_default_caps()
                        .into_iter()
                        .map(ToString::to_string)
                        .collect()
                });
            let commands = rec
                .and_then(|r| r.commands.clone())
                .filter(|v| !v.is_empty())
                .unwrap_or_else(|| {
                    node_default_commands()
                        .into_iter()
                        .map(ToString::to_string)
                        .collect()
                });
            serde_json::json!({
                "nodeId": node_id,
                "displayName": rec.and_then(|r| r.display_name.clone()),
                "platform": rec.and_then(|r| r.platform.clone()),
                "version": rec.and_then(|r| r.version.clone()),
                "coreVersion": rec.and_then(|r| r.core_version.clone()),
                "uiVersion": rec.and_then(|r| r.ui_version.clone()),
                "remoteIp": rec.and_then(|r| r.remote_ip.clone()),
                "deviceFamily": serde_json::Value::Null,
                "modelIdentifier": serde_json::Value::Null,
                "pathEnv": serde_json::Value::Null,
                "caps": caps,
                "commands": commands,
                "permissions": serde_json::json!({}),
                "paired": rec.map(|r| r.approved).unwrap_or(false),
                "connected": connected.contains(node_id),
                "connectedAtMs": rec.and_then(|r| r.last_connected_at_ms),
            })
        })
        .collect();

    Ok(serde_json::json!({
        "ts": now_epoch_ms(),
        "nodes": nodes,
    }))
}

async fn rpc_node_describe(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let node_id = require_node_id(p)?;
    let listing = rpc_node_list(state).await?;
    let node = listing
        .get("nodes")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.iter().find(|n| n["nodeId"] == node_id))
        .cloned()
        .ok_or_else(|| rpc_error("INVALID_REQUEST", "unknown nodeId"))?;
    Ok(serde_json::json!({
        "ts": now_epoch_ms(),
        "nodeId": node["nodeId"],
        "displayName": node["displayName"],
        "platform": node["platform"],
        "version": node["version"],
        "coreVersion": node["coreVersion"],
        "uiVersion": node["uiVersion"],
        "deviceFamily": node["deviceFamily"],
        "modelIdentifier": node["modelIdentifier"],
        "remoteIp": node["remoteIp"],
        "caps": node["caps"],
        "commands": node["commands"],
        "pathEnv": node["pathEnv"],
        "permissions": node["permissions"],
        "connectedAtMs": node["connectedAtMs"],
        "paired": node["paired"],
        "connected": node["connected"],
    }))
}

async fn rpc_node_pair_request(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let node_id = require_node_id(p)?;

    let existing_request = {
        let idx = state.node_pair_index.lock().await;
        idx.get(&node_id).cloned()
    };
    if let Some(existing_request) = existing_request {
        let pairs = state.node_pairs.lock().await;
        if let Some(rec) = pairs.get(&existing_request)
            && !rec.approved
        {
            return Ok(serde_json::json!({
                "status": "pending",
                "created": false,
                "request": node_pair_pending_payload(rec),
            }));
        }
    }

    let req = {
        let mut store = state.node_pairing_store.lock().await;
        store
            .create_request()
            .map_err(|e| rpc_error("UNAVAILABLE", &e.to_string()))?
    };

    let rec = NodePairRecord {
        request_id: req.id.clone(),
        node_id: node_id.clone(),
        display_name: p
            .get("displayName")
            .or_else(|| p.get("name"))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        platform: p
            .get("platform")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        version: p
            .get("version")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        core_version: p
            .get("coreVersion")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        ui_version: p
            .get("uiVersion")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        remote_ip: p
            .get("remoteIp")
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        caps: parse_node_string_list(p.get("caps")),
        commands: parse_node_string_list(p.get("commands")),
        is_repair: p.get("isRepair").and_then(|v| v.as_bool()).unwrap_or(false),
        ts: now_epoch_ms(),
        approved: false,
        token: None,
        approved_at_ms: None,
        last_connected_at_ms: None,
    };

    {
        let mut pairs = state.node_pairs.lock().await;
        pairs.insert(req.id.clone(), rec.clone());
    }
    {
        let mut idx = state.node_pair_index.lock().await;
        idx.insert(node_id, req.id.clone());
    }

    Ok(serde_json::json!({
        "status": "pending",
        "created": true,
        "request": node_pair_pending_payload(&rec),
        "code": req.code,
    }))
}

async fn rpc_node_pair_list(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let include_secrets = p
        .get("includeSecrets")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    let mut records: Vec<NodePairRecord> =
        state.node_pairs.lock().await.values().cloned().collect();
    records.sort_by(|a, b| b.ts.cmp(&a.ts));
    let pending: Vec<serde_json::Value> = records
        .iter()
        .filter(|rec| !rec.approved)
        .map(node_pair_pending_payload)
        .collect();
    let paired: Vec<serde_json::Value> = records
        .iter()
        .filter(|rec| rec.approved)
        .map(|rec| node_pair_paired_payload(rec, include_secrets))
        .collect();
    Ok(serde_json::json!({
        "pending": pending,
        "paired": paired,
    }))
}

async fn rpc_node_pair_approve(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let request_id = require_request_id(p)?;
    {
        let mut store = state.node_pairing_store.lock().await;
        store
            .approve(&request_id)
            .map_err(|e| rpc_error("INVALID_REQUEST", &e.to_string()))?;
    }

    let mut pairs = state.node_pairs.lock().await;
    let rec = pairs
        .get_mut(&request_id)
        .ok_or_else(|| rpc_error("INVALID_REQUEST", "unknown requestId"))?;
    let token = base64::Engine::encode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        uuid::Uuid::new_v4().as_bytes(),
    );
    rec.approved = true;
    rec.token = Some(token.clone());
    rec.approved_at_ms = Some(now_epoch_ms());
    Ok(serde_json::json!({
        "requestId": rec.request_id,
        "node": {
            "nodeId": rec.node_id,
            "displayName": rec.display_name,
            "platform": rec.platform,
        },
        "token": token,
    }))
}

async fn rpc_node_pair_reject(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let request_id = require_request_id(p)?;
    {
        let mut store = state.node_pairing_store.lock().await;
        store
            .reject(&request_id)
            .map_err(|e| rpc_error("INVALID_REQUEST", &e.to_string()))?;
    }

    let removed = {
        let mut pairs = state.node_pairs.lock().await;
        pairs.remove(&request_id)
    };
    if let Some(removed) = removed {
        state.node_pair_index.lock().await.remove(&removed.node_id);
        state.node_connected.lock().await.remove(&removed.node_id);
        return Ok(serde_json::json!({
            "requestId": request_id,
            "nodeId": removed.node_id,
            "rejected": true,
        }));
    }

    Ok(serde_json::json!({
        "requestId": request_id,
        "rejected": true,
    }))
}

async fn rpc_node_pair_verify(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let node_id = require_node_id(p)?;
    let token = p
        .get("token")
        .and_then(|v| v.as_str())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'token'"))?;
    let token = token.trim();
    if token.is_empty() {
        return Err(rpc_error("INVALID_PARAMS", "token cannot be empty"));
    }

    let request_id = state
        .node_pair_index
        .lock()
        .await
        .get(&node_id)
        .cloned()
        .ok_or_else(|| rpc_error("INVALID_REQUEST", "unknown nodeId"))?;
    let mut pairs = state.node_pairs.lock().await;
    let rec = pairs
        .get_mut(&request_id)
        .ok_or_else(|| rpc_error("INVALID_REQUEST", "unknown nodeId"))?;
    if !rec.approved {
        return Err(rpc_error("INVALID_REQUEST", "node not approved"));
    }
    if rec.token.as_deref() != Some(token) {
        return Err(rpc_error("INVALID_REQUEST", "invalid token"));
    }
    let ts = now_epoch_ms();
    rec.last_connected_at_ms = Some(ts);
    state.node_connected.lock().await.insert(node_id.clone());

    Ok(serde_json::json!({
        "ok": true,
        "nodeId": node_id,
        "verified": true,
        "ts": ts,
    }))
}

async fn rpc_node_rename(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let node_id = require_node_id(p)?;
    let display_name = p
        .get("displayName")
        .or_else(|| p.get("name"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'displayName'"))?;
    let display_name = display_name.trim();
    if display_name.is_empty() {
        return Err(rpc_error("INVALID_PARAMS", "displayName cannot be empty"));
    }

    let request_id = state
        .node_pair_index
        .lock()
        .await
        .get(&node_id)
        .cloned()
        .ok_or_else(|| rpc_error("INVALID_REQUEST", "unknown nodeId"))?;
    let mut pairs = state.node_pairs.lock().await;
    let rec = pairs
        .get_mut(&request_id)
        .ok_or_else(|| rpc_error("INVALID_REQUEST", "unknown nodeId"))?;
    rec.display_name = Some(display_name.to_string());
    Ok(serde_json::json!({
        "nodeId": node_id,
        "displayName": display_name,
    }))
}

async fn rpc_node_invoke(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let node_id = require_node_id(p)?;
    let command = p
        .get("command")
        .or_else(|| p.get("method"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'command'"))?;
    let command = command.trim();
    if command.is_empty() {
        return Err(rpc_error("INVALID_PARAMS", "command cannot be empty"));
    }
    if command == "system.execApprovals.get" || command == "system.execApprovals.set" {
        return Err(rpc_error(
            "INVALID_REQUEST",
            "node.invoke does not allow system.execApprovals.*; use exec.approvals.node.*",
        ));
    }

    let connected = state.node_connected.lock().await.contains(&node_id);
    if !connected {
        return Err(rpc_error("UNAVAILABLE", "node not connected"));
    }

    let invocation_id = uuid::Uuid::new_v4().to_string();
    let params = p.get("params").cloned().unwrap_or(serde_json::Value::Null);
    let timeout_ms = p
        .get("timeoutMs")
        .and_then(|v| v.as_u64())
        .unwrap_or(120_000)
        .min(600_000);
    let record = NodeInvokeRecord {
        id: invocation_id.clone(),
        node_id: node_id.clone(),
        command: command.to_string(),
        params: params.clone(),
        created_at_ms: now_epoch_ms(),
        status: "pending".to_string(),
        result: None,
        error: None,
    };
    state
        .node_invocations
        .lock()
        .await
        .insert(invocation_id.clone(), record);

    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    loop {
        let current = state
            .node_invocations
            .lock()
            .await
            .get(&invocation_id)
            .cloned()
            .ok_or_else(|| rpc_error("UNAVAILABLE", "invoke state lost"))?;
        if current.status == "ok" {
            return Ok(serde_json::json!({
                "ok": true,
                "id": current.id,
                "nodeId": current.node_id,
                "command": current.command,
                "payload": current.result,
            }));
        }
        if current.status == "error" {
            let message = current
                .error
                .as_ref()
                .and_then(|v| v.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("node invocation failed");
            return Err(rpc_error("UNAVAILABLE", message));
        }
        if std::time::Instant::now() >= deadline {
            return Err(rpc_error(
                "UNAVAILABLE",
                &format!(
                    "node.invoke timed out (id={}, timeoutMs={})",
                    invocation_id, timeout_ms
                ),
            ));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn rpc_node_invoke_result(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let invoke_id = p
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?
        .trim()
        .to_string();
    if invoke_id.is_empty() {
        return Err(rpc_error("INVALID_PARAMS", "id cannot be empty"));
    }
    let node_id = require_node_id(p)?;
    let ok = p.get("ok").and_then(|v| v.as_bool()).unwrap_or(true);
    let payload = p
        .get("payload")
        .cloned()
        .or_else(|| p.get("result").cloned());
    let error = p.get("error").cloned();

    let mut invocations = state.node_invocations.lock().await;
    let Some(record) = invocations.get_mut(&invoke_id) else {
        return Ok(serde_json::json!({"ok": true, "ignored": true}));
    };
    if record.node_id != node_id {
        return Err(rpc_error("INVALID_REQUEST", "nodeId mismatch"));
    }
    if ok {
        record.status = "ok".to_string();
        record.result = payload;
        record.error = None;
    } else {
        record.status = "error".to_string();
        record.error = error.or_else(|| Some(serde_json::json!({"message": "node invoke failed"})));
    }
    Ok(serde_json::json!({"ok": true}))
}

async fn rpc_node_event(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let event = p
        .get("event")
        .and_then(|v| v.as_str())
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'event'"))?;
    let node_id = p
        .get("nodeId")
        .or_else(|| p.get("id"))
        .and_then(|v| v.as_str())
        .map(normalize_node_id)
        .transpose()?
        .unwrap_or_else(|| "node".to_string());

    if !state.node_connected.lock().await.contains(&node_id) {
        return Err(rpc_error(
            "UNAVAILABLE",
            &format!(
                "Node '{}' not connected, cannot send event '{}'",
                node_id, event
            ),
        ));
    }

    let payload = p.get("payload").cloned().unwrap_or(serde_json::Value::Null);

    if event == "device.pair.requested" {
        let request_id = payload
            .get("requestId")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let device_id = payload
            .get("deviceId")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                rpc_error(
                    "INVALID_PARAMS",
                    "device.pair.requested requires payload.deviceId",
                )
            })?
            .to_string();
        let rec = DevicePairPendingRecord {
            request_id: request_id.clone(),
            device_id: device_id.clone(),
            public_key: payload
                .get("publicKey")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            display_name: payload
                .get("displayName")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            platform: payload
                .get("platform")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            client_id: payload
                .get("clientId")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            client_mode: payload
                .get("clientMode")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            role: payload
                .get("role")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            roles: parse_node_string_list(payload.get("roles")),
            scopes: parse_node_string_list(payload.get("scopes")),
            remote_ip: payload
                .get("remoteIp")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            silent: payload
                .get("silent")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            is_repair: payload
                .get("isRepair")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            ts: payload
                .get("ts")
                .and_then(|v| v.as_u64())
                .unwrap_or_else(now_epoch_ms),
        };
        {
            let mut pending = state.device_pair_pending.lock().await;
            pending.insert(request_id.clone(), rec.clone());
        }
        {
            let mut idx = state.device_pair_pending_index.lock().await;
            idx.insert(device_id.clone(), request_id.clone());
        }
        persist_device_pairing_state(state).await?;
        state.emit_event(
            "device.pair.requested",
            serde_json::to_value(&rec).map_err(|e| rpc_error("INTERNAL", &e.to_string()))?,
        );
        return Ok(serde_json::json!({
            "ok": true,
            "nodeId": node_id,
            "event": event,
            "requestId": request_id,
            "acceptedAtMs": now_epoch_ms(),
        }));
    }

    state.emit_event(event, payload.clone());

    Ok(serde_json::json!({
        "ok": true,
        "nodeId": node_id,
        "event": event,
        "acceptedAtMs": now_epoch_ms(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    fn reset_system_runtime_state_for_tests() {
        if let Ok(mut q) = SYSTEM_EVENT_QUEUES.lock() {
            q.clear();
        }
        if let Ok(mut w) = PENDING_HEARTBEAT_WAKES.lock() {
            w.clear();
        }
        GLOBAL_HEARTBEATS_ENABLED.store(true, Ordering::Relaxed);
    }

    #[tokio::test]
    async fn media_debug_returns_no_attachment_decisions_when_all_attachments_are_non_media() {
        let payload = base64::engine::general_purpose::STANDARD.encode(b"%PDF-1.7 dummy");
        let attachments = vec![NormalizedRpcAttachment {
            label: "doc.pdf".to_string(),
            kind: Some("file".to_string()),
            mime_type: Some("application/pdf".to_string()),
            content_base64: payload,
        }];

        let media = collect_media_understanding_debug_payload(attachments, 5_000_000, 5_000)
            .await
            .expect("media debug payload");
        let decisions = media
            .get("decisions")
            .and_then(|v| v.as_array())
            .cloned()
            .expect("decisions array");
        assert_eq!(decisions.len(), 3);

        let mut image_ok = false;
        let mut audio_ok = false;
        let mut video_ok = false;
        for decision in decisions {
            let capability = decision
                .get("capability")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let outcome = decision
                .get("outcome")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            if capability == "image" {
                image_ok = outcome == "no_attachment";
            } else if capability == "audio" {
                audio_ok = outcome == "no_attachment";
            } else if capability == "video" {
                video_ok = outcome == "no_attachment";
            }
        }

        assert!(image_ok);
        assert!(audio_ok);
        assert!(video_ok);
        assert_eq!(
            media
                .get("outputs")
                .and_then(|v| v.as_array())
                .map(|v| v.len())
                .unwrap_or_default(),
            0
        );
        assert_eq!(
            media
                .get("errors")
                .and_then(|v| v.as_array())
                .map(|v| v.len())
                .unwrap_or_default(),
            0
        );
    }

    #[test]
    fn augment_message_with_media_outputs_appends_notes() {
        let media = serde_json::json!({
            "outputs": [
                {
                    "kind": "audio_transcription",
                    "attachmentIndex": 2,
                    "text": "hello from audio"
                }
            ]
        });
        let out = augment_message_with_media_outputs("原始消息", Some(&media));
        assert!(out.contains("原始消息"));
        assert!(out.contains("[Media audio_transcription #2]"));
        assert!(out.contains("hello from audio"));
    }

    #[test]
    fn augment_message_with_media_outputs_handles_empty_base_message() {
        let media = serde_json::json!({
            "outputs": [
                {
                    "kind": "video_description",
                    "attachmentIndex": 0,
                    "text": "video summary"
                }
            ]
        });
        let out = augment_message_with_media_outputs("", Some(&media));
        assert!(!out.starts_with('\n'));
        assert!(out.contains("[Media video_description #0]"));
        assert!(out.contains("video summary"));
    }

    #[test]
    fn normalize_channel_alias_maps_node_style_names() {
        assert_eq!(normalize_channel_alias("googlechat"), "google_chat");
        assert_eq!(normalize_channel_alias("google-chat"), "google_chat");
        assert_eq!(normalize_channel_alias("nextcloud-talk"), "nextcloud");
        assert_eq!(normalize_channel_alias("synology-chat"), "synology");
        assert_eq!(normalize_channel_alias("teams"), "msteams");
        assert_eq!(normalize_channel_alias("lark"), "feishu");
        assert_eq!(normalize_channel_alias("imessage"), "bluebubbles");
        assert_eq!(normalize_channel_alias("telegram"), "telegram");
    }

    #[test]
    fn parse_wake_mode_accepts_supported_values() {
        assert_eq!(parse_wake_mode("now"), Some("now"));
        assert_eq!(parse_wake_mode("NOW"), Some("now"));
        assert_eq!(parse_wake_mode("next-heartbeat"), Some("next-heartbeat"));
        assert_eq!(parse_wake_mode("NEXT-HEARTBEAT"), Some("next-heartbeat"));
    }

    #[test]
    fn parse_wake_mode_rejects_invalid_values() {
        assert_eq!(parse_wake_mode(""), None);
        assert_eq!(parse_wake_mode("later"), None);
        assert_eq!(parse_wake_mode("next"), None);
    }

    #[test]
    fn system_event_queue_dedupes_and_drains() {
        reset_system_runtime_state_for_tests();
        let session = format!("test-session-{}", uuid::Uuid::new_v4());
        enqueue_system_event_entry(&session, "alpha", None).expect("enqueue alpha");
        enqueue_system_event_entry(&session, "alpha", None).expect("enqueue duplicate");
        enqueue_system_event_entry(&session, "beta", None).expect("enqueue beta");
        let drained = drain_system_event_entries(&session).expect("drain");
        let texts: Vec<String> = drained.iter().map(|e| e.text.clone()).collect();
        assert_eq!(texts, vec!["alpha".to_string(), "beta".to_string()]);
        let drained_again = drain_system_event_entries(&session).expect("drain again");
        assert!(drained_again.is_empty());
    }

    #[test]
    fn system_event_context_change_tracks_last_context_key() {
        reset_system_runtime_state_for_tests();
        let session = format!("test-session-{}", uuid::Uuid::new_v4());
        assert!(is_system_event_context_changed(&session, Some("node-1")).expect("first check"));
        enqueue_system_event_entry(&session, "Node: one", Some("node-1")).expect("enqueue");
        assert!(!is_system_event_context_changed(&session, Some("node-1")).expect("same check"));
        assert!(is_system_event_context_changed(&session, Some("node-2")).expect("next check"));
    }

    #[test]
    fn pending_wake_queue_coalesces_by_priority_per_target() {
        reset_system_runtime_state_for_tests();
        queue_pending_heartbeat_wake(Some("interval"), Some("default"), Some("main"))
            .expect("queue interval");
        queue_pending_heartbeat_wake(Some("manual"), Some("default"), Some("main"))
            .expect("queue manual");
        let wakes = take_pending_heartbeat_wakes().expect("take wakes");
        assert_eq!(wakes.len(), 1);
        assert_eq!(wakes[0].reason, "manual");
        assert_eq!(wakes[0].agent_id.as_deref(), Some("default"));
        assert_eq!(wakes[0].session_key.as_deref(), Some("main"));
    }

    #[test]
    fn system_presence_list_contains_self_entry() {
        let entries = list_system_presence_entries().expect("presence list");
        assert!(!entries.is_empty());
        assert!(entries.iter().any(|entry| {
            entry
                .reason
                .as_deref()
                .map(|v| v.eq_ignore_ascii_case("self"))
                .unwrap_or(false)
        }));
    }

    #[test]
    fn system_presence_update_merges_lists_for_same_device() {
        let device_id = format!("test-device-{}", uuid::Uuid::new_v4());
        update_system_presence_entry(&serde_json::json!({
            "text": "Node: alpha",
            "deviceId": device_id,
            "roles": ["mobile", "assistant"],
            "scopes": ["chat"]
        }))
        .expect("first update");
        update_system_presence_entry(&serde_json::json!({
            "text": "Node: alpha updated",
            "deviceId": device_id,
            "roles": ["assistant", "automation"],
            "scopes": ["chat", "system"]
        }))
        .expect("second update");

        let entries = list_system_presence_entries().expect("presence list");
        let entry = entries
            .iter()
            .find(|item| item.device_id.as_deref() == Some(device_id.as_str()))
            .expect("merged entry");
        let roles = entry.roles.clone().unwrap_or_default();
        let scopes = entry.scopes.clone().unwrap_or_default();
        assert!(roles.iter().any(|v| v == "mobile"));
        assert!(roles.iter().any(|v| v == "assistant"));
        assert!(roles.iter().any(|v| v == "automation"));
        assert!(scopes.iter().any(|v| v == "chat"));
        assert!(scopes.iter().any(|v| v == "system"));
    }
}
