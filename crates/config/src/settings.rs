use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<Env>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wizard: Option<Wizard>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<Diagnostics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logging: Option<Logging>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update: Option<Update>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser: Option<Browser>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui: Option<Ui>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<Auth>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub models: Option<ModelsConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_host: Option<NodeHost>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agents: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bindings: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub broadcast: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media: Option<Media>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub messages: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commands: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub approvals: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron: Option<Cron>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hooks: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub web: Option<Web>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channels: Option<Channels>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discovery: Option<Discovery>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canvas_host: Option<CanvasHost>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub talk: Option<Talk>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gateway: Option<Gateway>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Meta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_touched_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_touched_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Env {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_env: Option<ShellEnv>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vars: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellEnv {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Wizard {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run_mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flags: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub otel: Option<Otel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_trace: Option<CacheTrace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Otel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub protocol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub traces: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logs: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sample_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flush_interval_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheTrace {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_messages: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_prompt: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_system: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Logging {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub console_level: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub console_style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redact_sensitive: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redact_patterns: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check_on_start: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Browser {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evaluate_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cdp_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_cdp_timeout_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_cdp_handshake_timeout_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub executable_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headless: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_sandbox: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attach_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_profile: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_defaults: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssrf_policy: Option<SsrfPolicy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profiles: Option<HashMap<String, BrowserProfile>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SsrfPolicy {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_private_network: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_hostnames: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname_allowlist: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserProfile {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cdp_port: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cdp_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driver: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ui {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seam_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assistant: Option<Assistant>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Assistant {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Auth {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profiles: Option<HashMap<String, AuthProfile>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<HashMap<String, Vec<String>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cooldowns: Option<AuthCooldowns>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthProfile {
    pub provider: String,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthCooldowns {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_backoff_hours: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_backoff_hours_by_provider: Option<HashMap<String, f64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub billing_max_hours: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failure_window_hours: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelsConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub providers: Option<HashMap<String, ModelProvider>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProvider {
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_concurrency: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeHost {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser_proxy: Option<BrowserProxy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserProxy {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_profiles: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Media {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preserve_filenames: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Cron {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_concurrent_runs: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_retention: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Web {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heartbeat_seconds: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reconnect: Option<Reconnect>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reconnect {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub initial_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub factor: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jitter: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_attempts: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Channels {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webchat: Option<WebchatChannel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub whatsapp: Option<WhatsappChannel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub telegram: Option<TelegramChannel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub discord: Option<DiscordChannel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slack: Option<SlackChannel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal: Option<SignalChannel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<LineChannel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matrix: Option<MatrixChannel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nostr: Option<NostrChannel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub irc: Option<IrcChannel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub google_chat: Option<GoogleChatChannel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mattermost: Option<MattermostChannel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebchatChannel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<WebchatAuth>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebchatAuth {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsappChannel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone_number_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub business_account_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_verify_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramChannel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bot_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordChannel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bot_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guild_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlackChannel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bot_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signing_secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub webhook_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalChannel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phone_number: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signal_cli_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineChannel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_secret: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MatrixChannel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homeserver: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NostrChannel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub relay_urls: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IrcChannel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub nick: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channels: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GoogleChatChannel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service_account_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MattermostChannel {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wide_area: Option<WideAreaDiscovery>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mdns: Option<MdnsDiscovery>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WideAreaDiscovery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MdnsDiscovery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanvasHost {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub live_reload: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Talk {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub voice_aliases: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interrupt_on_speech: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Gateway {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_bind_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub control_ui: Option<ControlUi>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<GatewayAuth>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trusted_proxies: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_real_ip_fallback: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<GatewayTools>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel_health_check_minutes: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tailscale: Option<Tailscale>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote: Option<GatewayRemote>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reload: Option<GatewayReload>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls: Option<GatewayTls>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http: Option<GatewayHttp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ControlUi {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_origins: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_insecure_auth: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dangerously_disable_device_auth: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayAuth {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_tailscale: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimit>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trusted_proxy: Option<TrustedProxy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimit {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_attempts: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lockout_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exempt_loopback: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedProxy {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_header: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_headers: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_users: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayTools {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deny: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Tailscale {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reset_on_exit: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayRemote {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ssh_identity: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayReload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub debounce_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayTls {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auto_generate: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cert_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ca_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GatewayHttp {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoints: Option<HttpEndpoints>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HttpEndpoints {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chat_completions: Option<HttpEndpoint>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub responses: Option<HttpEndpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HttpEndpoint {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert!(config.meta.is_none());
        assert!(config.env.is_none());
    }

    #[test]
    fn test_config_serialization() {
        let config = Config {
            meta: Some(Meta {
                last_touched_version: Some("1.0.0".to_string()),
                last_touched_at: Some("2024-01-01".to_string()),
            }),
            ..Default::default()
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("1.0.0"));
        assert!(json.contains("2024-01-01"));
    }

    #[test]
    fn test_config_deserialization() {
        let json = r#"{
            "meta": {
                "lastTouchedVersion": "2.0.0",
                "lastTouchedAt": "2024-06-15"
            },
            "gateway": {
                "port": 8080
            }
        }"#;

        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(
            config.meta.as_ref().unwrap().last_touched_version,
            Some("2.0.0".to_string())
        );
        assert_eq!(config.gateway.as_ref().unwrap().port, Some(8080));
    }

    #[test]
    fn test_meta_serialization() {
        let meta = Meta {
            last_touched_version: Some("1.0.0".to_string()),
            last_touched_at: Some("2024-01-01".to_string()),
        };

        let json = serde_json::to_string(&meta).unwrap();
        assert!(json.contains("lastTouchedVersion"));
        assert!(json.contains("1.0.0"));
    }

    #[test]
    fn test_model_provider_serialization() {
        let provider = ModelProvider {
            provider: "openai".to_string(),
            api_key: Some("sk-test".to_string()),
            base_url: Some("https://api.openai.com".to_string()),
            model: Some("gpt-4".to_string()),
            max_tokens: Some(4096),
            temperature: Some(0.7),
            max_concurrency: Some(10),
            headers: None,
        };

        let json = serde_json::to_string(&provider).unwrap();
        assert!(json.contains("openai"));
        assert!(json.contains("gpt-4"));
    }

    #[test]
    fn test_telegram_channel_config() {
        let channel = TelegramChannel {
            enabled: Some(true),
            bot_token: Some("test_token".to_string()),
            api_url: None,
        };

        let json = serde_json::to_string(&channel).unwrap();
        assert!(json.contains("test_token"));

        let deserialized: TelegramChannel = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.enabled, Some(true));
        assert_eq!(deserialized.bot_token, Some("test_token".to_string()));
    }

    #[test]
    fn test_browser_config() {
        let browser = Browser {
            enabled: Some(true),
            headless: Some(true),
            no_sandbox: Some(true),
            cdp_url: Some("ws://localhost:9222".to_string()),
            ..Default::default()
        };

        let json = serde_json::to_string(&browser).unwrap();
        assert!(json.contains("ws://localhost:9222"));
    }

    #[test]
    fn test_gateway_config() {
        let gateway = Gateway {
            port: Some(8080),
            mode: Some("normal".to_string()),
            bind: Some("0.0.0.0".to_string()),
            control_ui: Some(ControlUi {
                enabled: Some(true),
                base_path: Some("/ui".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let json = serde_json::to_string(&gateway).unwrap();
        assert!(json.contains("8080"));
        assert!(json.contains("controlUi"));
    }

    #[test]
    fn test_auth_profile() {
        let profile = AuthProfile {
            provider: "google".to_string(),
            mode: "oauth".to_string(),
            email: Some("test@example.com".to_string()),
        };

        let json = serde_json::to_string(&profile).unwrap();
        assert!(json.contains("google"));
        assert!(json.contains("oauth"));

        let deserialized: AuthProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.provider, "google");
        assert_eq!(deserialized.email, Some("test@example.com".to_string()));
    }

    #[test]
    fn test_reconnect_config() {
        let reconnect = Reconnect {
            initial_ms: Some(1000),
            max_ms: Some(60000),
            factor: Some(2.0),
            jitter: Some(0.1),
            max_attempts: Some(5),
        };

        let json = serde_json::to_string(&reconnect).unwrap();
        assert!(json.contains("1000"));
        assert!(json.contains("60000"));
    }

    #[test]
    fn test_channels_config() {
        let channels = Channels {
            telegram: Some(TelegramChannel {
                enabled: Some(true),
                bot_token: Some("token".to_string()),
                api_url: None,
            }),
            discord: Some(DiscordChannel {
                enabled: Some(false),
                bot_token: None,
                guild_id: None,
                channel_ids: None,
            }),
            ..Default::default()
        };

        let json = serde_json::to_string(&channels).unwrap();
        assert!(json.contains("telegram"));
        assert!(json.contains("discord"));
    }
}
