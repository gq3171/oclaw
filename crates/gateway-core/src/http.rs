use axum::{
    extract::{
        ConnectInfo, DefaultBodyLimit, State, WebSocketUpgrade,
    },
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, delete, get, post, put},
    middleware as axum_mw,
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use oclaws_config::settings::Gateway;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tower_http::timeout::TimeoutLayer;
use tower_http::trace::TraceLayer;
use tower_http::services::ServeDir;
use tracing::{error, info};

use oclaws_llm_core::providers::LlmProvider;
use oclaws_plugin_core::HookPipeline;
use oclaws_channel_core::ChannelManager;
use oclaws_channel_core::group_gate::GroupActivation;
use oclaws_agent_core::EchoTracker;
use oclaws_doctor_core::{HealthChecker, SystemHealthCheck};
use oclaws_tools_core::tool::ToolRegistry;
use oclaws_plugin_core::PluginRegistrations;
use oclaws_memory_core::MemoryManager;

use crate::auth::AuthState;
use crate::error::{GatewayError, GatewayResult};
use crate::message::MessageHandler;
use crate::server::GatewayServer;
use oclaws_protocol::frames::{ErrorDetails, GatewayFrame, HelloOk, ServerFeatures, ServerInfo, Policy};
use oclaws_protocol::snapshot::{AuthMode, Snapshot, StateVersion};

pub mod agent_bridge;
pub mod auth;
pub mod cron_executor;
pub mod metrics;
pub mod middleware;
pub mod rate_limit;
pub mod routes;
pub mod webhooks;

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
    plugin_registrations: Option<Arc<PluginRegistrations>>,
    cron_service: Option<Arc<oclaws_cron_core::CronService>>,
    memory_manager: Option<Arc<MemoryManager>>,
    workspace: Option<Arc<oclaws_workspace_core::files::Workspace>>,
    full_config: Option<Arc<RwLock<oclaws_config::settings::Config>>>,
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

    pub fn with_plugin_registrations(mut self, regs: Arc<PluginRegistrations>) -> Self {
        self.plugin_registrations = Some(regs);
        self
    }

    pub fn with_cron_service(mut self, svc: Arc<oclaws_cron_core::CronService>) -> Self {
        self.cron_service = Some(svc);
        self
    }

    pub fn with_memory_manager(mut self, manager: Arc<MemoryManager>) -> Self {
        self.memory_manager = Some(manager);
        self
    }

    pub fn with_workspace(mut self, workspace: Arc<oclaws_workspace_core::files::Workspace>) -> Self {
        self.workspace = Some(workspace);
        self
    }

    pub fn with_full_config(mut self, config: oclaws_config::settings::Config, path: PathBuf) -> Self {
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

        let state = Arc::new(HttpState {
            auth_state: self.auth_state.clone(),
            gateway_server: self.gateway_server.clone(),
            _gateway: self.gateway.clone(),
            llm_provider: self.llm_provider.clone(),
            hook_pipeline: self.hook_pipeline.clone(),
            channel_manager: self.channel_manager.clone(),
            tool_registry: self.tool_registry.clone(),
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
            .route("/v1/chat/completions", post(routes::chat_completions_handler))
            .route("/v1/responses", post(routes::responses_handler))
            .route("/ws", get(ws_handler))
            .route("/agent/status", get(routes::agent_status_handler))
            .route("/sessions", get(routes::sessions_list_handler))
            .route("/sessions/{key}", delete(routes::sessions_delete_handler))
            .route("/config", get(routes::config_get_handler))
            .route("/config/reload", post(routes::config_reload_handler))
            .route("/models", get(routes::models_list_handler))
            .route("/cron/jobs", get(routes::cron_list_handler))
            .route("/cron/jobs", post(routes::cron_create_handler))
            .route("/cron/jobs/{id}", delete(routes::cron_delete_handler))
            .route("/cron/jobs/{id}/trigger", post(routes::cron_trigger_handler))
            .route("/cron/jobs/{id}/logs", get(routes::cron_logs_handler))
            .route("/cron/status", get(routes::cron_status_handler))
            .route("/metrics", get(metrics::metrics_handler))
            .route("/", any(root_handler))
            .layer(axum_mw::from_fn(middleware::security_headers_middleware))
            .layer(axum_mw::from_fn_with_state(state.clone(), middleware::hook_middleware))
            .layer(axum_mw::from_fn_with_state(state.clone(), middleware::auth_middleware))
            .layer(axum_mw::from_fn_with_state(state.clone(), middleware::request_id_middleware))
            .layer(cors)
            .layer(TraceLayer::new_for_http())
            .layer(rate_limit::RateLimitLayer::new(100, 60))
            .layer(TimeoutLayer::with_status_code(StatusCode::GATEWAY_TIMEOUT, std::time::Duration::from_secs(30)))
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
                    unsafe extern "system" { fn SetHandleInformation(h: usize, mask: u32, flags: u32) -> i32; }
                    SetHandleInformation(sock.as_raw_socket() as usize, 1, 0);
                }
            }
            sock.bind(addr).map_err(|e| {
                GatewayError::ServerError(format!("Failed to bind to {}: {}", addr, e))
            })?;
            sock.listen(1024).map_err(|e| {
                GatewayError::ServerError(format!("Failed to listen: {}", e))
            })?
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
            let delivery = Arc::new(
                crate::heartbeat_runner::GatewayHeartbeatDelivery::new(
                    provider, hb_channel_mgr,
                ),
            );
            let hb_config = oclaws_workspace_core::heartbeat::HeartbeatConfig::default();
            let mut runner = crate::heartbeat_runner::HeartbeatRunner::new(
                hb_config, workspace, delivery,
            );
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(
                    std::time::Duration::from_secs(60),
                );
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            if let Err(e) = runner.tick().await {
                                tracing::warn!("Heartbeat tick error: {}", e);
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
                && !origins.is_empty() {
                    let origins: Vec<axum::http::HeaderValue> = origins
                        .iter()
                        .filter_map(|o| o.parse().ok())
                        .collect();
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
        Option<Arc<oclaws_cron_core::scheduler::CronScheduler>>,
        Option<oclaws_cron_core::events::CronEventSender>,
        Option<Arc<oclaws_cron_core::run_log::RunLog>>,
    ) {
        let (Some(cron_svc), Some(provider)) =
            (&self.cron_service, &self.llm_provider)
        else {
            return (None, None, None);
        };

        let mut exec = cron_executor::GatewayCronExecutor::new(provider.clone());
        if let Some(ref reg) = self.tool_registry {
            exec = exec.with_tool_registry(reg.clone());
        }
        if let Some(ref regs) = self.plugin_registrations {
            exec = exec.with_plugin_registrations(regs.clone());
        }
        if let Some(ref cm) = self.channel_manager {
            exec = exec.with_channel_manager(cm.clone());
        }

        let log_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("oclaws")
            .join("cron")
            .join("logs");
        let run_log = Arc::new(oclaws_cron_core::run_log::RunLog::new(log_dir));
        let (events_tx, _) = oclaws_cron_core::events::event_channel();

        let scheduler = Arc::new(oclaws_cron_core::scheduler::CronScheduler::new(
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
    pub plugin_registrations: Option<Arc<PluginRegistrations>>,
    pub cron_service: Option<Arc<oclaws_cron_core::CronService>>,
    pub cron_scheduler: Option<Arc<oclaws_cron_core::scheduler::CronScheduler>>,
    pub cron_events: Option<oclaws_cron_core::events::CronEventSender>,
    pub cron_run_log: Option<Arc<oclaws_cron_core::run_log::RunLog>>,
    pub memory_manager: Option<Arc<MemoryManager>>,
    pub workspace: Option<Arc<oclaws_workspace_core::files::Workspace>>,
    pub metrics: Arc<metrics::AppMetrics>,
    pub health_checker: Arc<HealthChecker>,
    pub full_config: Option<Arc<RwLock<oclaws_config::settings::Config>>>,
    pub config_path: Option<PathBuf>,
    pub echo_tracker: Arc<tokio::sync::Mutex<EchoTracker>>,
    pub group_activation: GroupActivation,
    pub dm_scope: crate::session_key::DmScope,
    pub identity_links: Option<Arc<crate::session_key::IdentityLinks>>,
    pub needs_hatching: Arc<std::sync::atomic::AtomicBool>,
}

async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

pub async fn readiness_handler(
    State(state): State<Arc<HttpState>>,
) -> Response {
    let report = state.health_checker.check_all();
    let status = if report.is_healthy() { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };
    (status, Json(serde_json::to_value(&report).unwrap_or_default())).into_response()
}

async fn plugin_route_handler(
    State(state): State<Arc<HttpState>>,
    axum::extract::Path((plugin_id, rest)): axum::extract::Path<(String, String)>,
    method: axum::http::Method,
    body: axum::body::Bytes,
) -> Response {
    let Some(ref regs) = state.plugin_registrations else {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "no plugins loaded"}))).into_response();
    };
    let routes = regs.http_routes.read().await;
    let path = format!("/{}", rest);
    let route = routes.iter().find(|r| r.plugin_id == plugin_id && path.starts_with(&r.path));
    match route {
        Some(r) => {
            match r.handler.handle(method.as_str(), &body).await {
                Ok((status, body)) => {
                    let sc = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
                    (sc, body).into_response()
                }
                Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))).into_response(),
            }
        }
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "plugin route not found"}))).into_response(),
    }
}

async fn root_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "service": "oclaws-gateway",
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
    
    let hello = HelloOk {
        frame_type: oclaws_protocol::frames::HelloOkType::HelloOk,
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
            ],
            events: vec![
                "tick".to_string(),
                "shutdown".to_string(),
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
    write.send(axum::extract::ws::Message::Binary(hello_json.into())).await?;

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
        }
    }

    Ok(())
}

async fn handle_frame(
    frame: GatewayFrame,
    state: &Arc<HttpState>,
) -> Result<Option<oclaws_protocol::frames::ResponseFrame>, Box<dyn std::error::Error + Send + Sync>> {
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
) -> oclaws_protocol::frames::ResponseFrame {
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

        // ── Chat RPCs ──
        "chat.send" => rpc_chat_send(&p, state).await,
        "chat.history" => rpc_chat_history(&p, state).await,
        "chat.abort" => rpc_chat_abort(&p, state).await,

        // ── Agents RPCs ──
        "agents.list" => rpc_agents_list(state).await,
        "agents.create" => rpc_agents_create(&p, state).await,
        "agents.update" => rpc_agents_update(&p, state).await,
        "agents.delete" => rpc_agents_delete(&p, state).await,
        "agents.files.list" => rpc_agents_files_list(&p, state).await,
        "agents.files.get" => rpc_agents_files_get(&p, state).await,

        // ── System RPCs ──
        "system.health" => rpc_system_health(state).await,
        "system.status" => rpc_system_status(state).await,
        "system.heartbeat" => rpc_system_heartbeat(state).await,
        "system.presence" => rpc_system_presence(state).await,
        "usage.tokens" => rpc_usage_tokens(&p, state).await,

        // ── Config RPCs ──
        "config.get" => rpc_config_get(state).await,
        "config.set" => rpc_config_set(&p, state).await,

        // ── Models RPCs ──
        "models.list" => rpc_models_list(state).await,

        // ── Channel RPCs ──
        "channels.status" => rpc_channels_status(state).await,

        // ── Cron RPCs ──
        "cron.list" => rpc_cron_list(state).await,
        "cron.create" => rpc_cron_create(&p, state).await,
        "cron.delete" => rpc_cron_delete(&p, state).await,
        "cron.trigger" => rpc_cron_trigger(&p, state).await,
        "cron.logs" => rpc_cron_logs(&p, state).await,
        "cron.status" => rpc_cron_status(state).await,

        // ── Skills RPCs ──
        "skills.status" => rpc_skills_status(state).await,
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

        // ── TTS RPCs ──
        "tts.status" => rpc_tts_status(state).await,
        "tts.convert" => rpc_tts_convert(&p, state).await,
        "tts.providers" => rpc_tts_providers(state).await,

        // ── Node RPCs ──
        "node.list" => rpc_node_list(state).await,
        "node.describe" => rpc_node_describe(&p, state).await,
        "node.pair.request" => rpc_node_pair_request(&p, state).await,
        "node.pair.list" => rpc_node_pair_list(state).await,
        "node.pair.approve" => rpc_node_pair_approve(&p, state).await,
        "node.pair.reject" => rpc_node_pair_reject(&p, state).await,
        "node.rename" => rpc_node_rename(&p, state).await,
        "node.invoke" => rpc_node_invoke(&p, state).await,
        "node.event" => rpc_node_event(&p, state).await,

        _ => Err(rpc_error("METHOD_NOT_FOUND", &format!("Unknown method: {}", method))),
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
    let session = manager.create_session(key, agent_id)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    serde_json::to_value(&session).map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_session_list(state: &HttpState) -> RpcResult {
    let manager = state.gateway_server.session_manager.read().await;
    let sessions = manager.list_sessions()
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    serde_json::to_value(&sessions).map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_session_get(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let key = p["key"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'key' parameter"))?;
    let manager = state.gateway_server.session_manager.read().await;
    let session = manager.get_session(key)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    serde_json::to_value(&session).map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_session_delete(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let key = p["key"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'key' parameter"))?;
    let manager = state.gateway_server.session_manager.read().await;
    manager.remove_session(key)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    Ok(serde_json::json!({"deleted": true}))
}

async fn rpc_session_preview(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let manager = state.gateway_server.session_manager.read().await;
    let limit = p["limit"].as_u64().unwrap_or(20) as usize;
    let sessions = manager.list_sessions()
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    let previews: Vec<serde_json::Value> = sessions.iter().take(limit).map(|s| {
        serde_json::json!({
            "key": s.key,
            "agent_id": s.agent_id,
            "created_at": s.created_at,
            "updated_at": s.updated_at,
            "message_count": s.message_count,
        })
    }).collect();
    Ok(serde_json::json!({"sessions": previews}))
}

async fn rpc_session_resolve(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let key = p["key"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'key'"))?;
    let manager = state.gateway_server.session_manager.read().await;
    let session = manager.get_session(key)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    match session {
        Some(s) => {
            let val = serde_json::to_value(&s)
                .map_err(|e| rpc_error("INTERNAL", &e.to_string()))?;
            Ok(serde_json::json!({"found": true, "session": val}))
        }
        None => Ok(serde_json::json!({"found": false, "session": null})),
    }
}

async fn rpc_session_patch(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let key = p["key"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'key'"))?;
    let manager = state.gateway_server.session_manager.read().await;
    // Verify session exists
    manager.get_session(key)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?
        .ok_or_else(|| rpc_error("NOT_FOUND", "Session not found"))?;
    if let Some(agent_id) = p["agentId"].as_str() {
        manager.update_agent_id(key, agent_id)
            .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    } else {
        manager.touch_session(key)
            .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    }
    Ok(serde_json::json!({"patched": true}))
}

async fn rpc_session_reset(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let key = p["key"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'key'"))?;
    let manager = state.gateway_server.session_manager.read().await;
    manager.get_session(key)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?
        .ok_or_else(|| rpc_error("NOT_FOUND", "Session not found"))?;
    manager.clear_messages(key)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    Ok(serde_json::json!({"reset": true}))
}

async fn rpc_session_compact(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let key = p["key"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'key'"))?;
    let max_messages = p["maxMessages"].as_u64().unwrap_or(50) as usize;
    let manager = state.gateway_server.session_manager.read().await;
    manager.get_session(key)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?
        .ok_or_else(|| rpc_error("NOT_FOUND", "Session not found"))?;
    let (original_count, new_count) = manager.compact_messages(key, max_messages)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    Ok(serde_json::json!({
        "compacted": true,
        "original_count": original_count,
        "new_count": new_count,
    }))
}

async fn rpc_session_history(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let key = p["key"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'key'"))?;
    let limit = p["limit"].as_u64().unwrap_or(50) as usize;
    let manager = state.gateway_server.session_manager.read().await;
    let session = manager.get_session(key)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?
        .ok_or_else(|| rpc_error("NOT_FOUND", "Session not found"))?;
    let messages = manager.get_messages(key, limit)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    let val = serde_json::to_value(&messages)
        .map_err(|e| rpc_error("INTERNAL", &e.to_string()))?;
    Ok(serde_json::json!({"messages": val, "total": session.message_count}))
}

async fn rpc_session_send(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let key = p["key"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'key'"))?;
    let text = p["text"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'text'"))?;
    let role = p["role"].as_str().unwrap_or("user");
    let manager = state.gateway_server.session_manager.read().await;
    manager.get_session(key)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?
        .ok_or_else(|| rpc_error("NOT_FOUND", "Session not found"))?;
    manager.add_message(key, role, text)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    let session = manager.get_session(key)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?
        .ok_or_else(|| rpc_error("NOT_FOUND", "Session not found"))?;
    Ok(serde_json::json!({"sent": true, "message_count": session.message_count}))
}

// ── Chat RPCs ───────────────────────────────────────────────────────────

async fn rpc_chat_send(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let message = p["message"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'message' parameter"))?;
    let session_id = p["sessionId"].as_str();

    let provider = state.llm_provider.as_ref()
        .ok_or_else(|| rpc_error("NO_PROVIDER", "No LLM provider configured"))?;

    let reply = if let Some(ref registry) = state.tool_registry {
        let executor = agent_bridge::ToolRegistryExecutor::new(registry.clone());
        agent_bridge::agent_reply_with_session(provider, &executor, message, session_id).await
            .unwrap_or_else(|e| format!("Agent error: {}", e))
    } else {
        let request = oclaws_llm_core::chat::ChatRequest {
            model: provider.default_model().to_string(),
            messages: vec![oclaws_llm_core::chat::ChatMessage {
                role: oclaws_llm_core::chat::MessageRole::User,
                content: message.to_string(),
                name: None, tool_calls: None, tool_call_id: None,
            }],
            temperature: None, top_p: None, max_tokens: None,
            stop: None, tools: None, tool_choice: None,
            stream: None, response_format: None,
        };
        provider.chat(request).await
            .map(|c| c.choices.first().map(|ch| ch.message.content.clone()).unwrap_or_default())
            .unwrap_or_else(|e| format!("LLM error: {}", e))
    };

    Ok(serde_json::json!({"reply": reply}))
}

async fn rpc_chat_history(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let session_id = p["sessionId"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'sessionId' parameter"))?;
    let manager = state.gateway_server.session_manager.read().await;
    let session = manager.get_session(session_id)
        .map_err(|e| rpc_error("SESSION_ERROR", &e.to_string()))?;
    serde_json::to_value(&session).map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_chat_abort(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let session_id = p["sessionId"].as_str();
    Ok(serde_json::json!({
        "aborted": true,
        "sessionId": session_id,
    }))
}

// ── Agents RPCs ──────────────────────────────────────────────────────────

async fn rpc_agents_list(state: &HttpState) -> RpcResult {
    let manager = state.gateway_server.session_manager.read().await;
    let agents = manager.list_agents()
        .unwrap_or_default();
    serde_json::to_value(&agents).map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_agents_create(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let id = p["id"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    let name = p["name"].as_str().unwrap_or(id);
    let model = p["model"].as_str().unwrap_or("default");
    let system_prompt = p["systemPrompt"].as_str().unwrap_or("");

    let manager = state.gateway_server.session_manager.read().await;
    manager.create_agent(id, name, model, system_prompt)
        .map_err(|e| rpc_error("AGENT_ERROR", &e.to_string()))?;
    Ok(serde_json::json!({"created": true, "id": id}))
}

async fn rpc_agents_update(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let id = p["id"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    let manager = state.gateway_server.session_manager.read().await;
    manager.update_agent(id, p)
        .map_err(|e| rpc_error("AGENT_ERROR", &e.to_string()))?;
    Ok(serde_json::json!({"updated": true, "id": id}))
}

async fn rpc_agents_delete(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let id = p["id"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    let manager = state.gateway_server.session_manager.read().await;
    manager.delete_agent(id)
        .map_err(|e| rpc_error("AGENT_ERROR", &e.to_string()))?;
    Ok(serde_json::json!({"deleted": true, "id": id}))
}

async fn rpc_agents_files_list(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let id = p["id"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    // Agent file storage not yet implemented — return empty list
    Ok(serde_json::json!({"agent_id": id, "files": []}))
}

async fn rpc_agents_files_get(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let id = p["id"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    let name = p["name"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'name'"))?;
    Err(rpc_error("NOT_IMPLEMENTED", &format!("Agent file storage not yet available (agent={}, file={})", id, name)))
}

// ── System RPCs ──────────────────────────────────────────────────────────

async fn rpc_system_health(state: &HttpState) -> RpcResult {
    let report = state.health_checker.check_all();
    serde_json::to_value(&report).map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_system_status(state: &HttpState) -> RpcResult {
    let session_count = state.gateway_server.session_manager.read().await
        .list_sessions().map(|s| s.len()).unwrap_or(0);
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

async fn rpc_system_heartbeat(_state: &HttpState) -> RpcResult {
    Ok(serde_json::json!({
        "timestamp": chrono::Utc::now().timestamp_millis(),
        "alive": true,
    }))
}

async fn rpc_system_presence(state: &HttpState) -> RpcResult {
    let has_llm = state.llm_provider.is_some();
    let channel_count = match &state.channel_manager {
        Some(cm) => cm.read().await.list().await.len(),
        None => 0,
    };
    Ok(serde_json::json!({
        "online": true,
        "llm_available": has_llm,
        "channel_count": channel_count,
        "timestamp": chrono::Utc::now().timestamp_millis(),
    }))
}

async fn rpc_usage_tokens(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let session_key = p["sessionKey"].as_str();
    let total_requests = state.metrics.request_count.load(std::sync::atomic::Ordering::Relaxed);
    Ok(serde_json::json!({
        "session_key": session_key,
        "total_requests": total_requests,
        "note": "Detailed token tracking requires storage backend",
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
    let new_config: oclaws_config::settings::Config = serde_json::from_value(p.clone())
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

// ── Models RPCs ─────────────────────────────────────────────────────────

async fn rpc_models_list(state: &HttpState) -> RpcResult {
    let Some(ref provider) = state.llm_provider else {
        return Ok(serde_json::json!({"models": []}));
    };
    let models = provider.list_models().await
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
    let mut channels = Vec::new();
    for name in &names {
        if let Some(ch) = mgr.get(name).await {
            let ch = ch.read().await;
            let status = format!("{:?}", ch.status());
            channels.push(serde_json::json!({
                "name": name,
                "type": ch.channel_type(),
                "status": status,
            }));
        }
    }
    Ok(serde_json::json!({"channels": channels}))
}

// ── Cron RPCs ───────────────────────────────────────────────────────────

async fn rpc_cron_list(state: &HttpState) -> RpcResult {
    let Some(ref svc) = state.cron_service else {
        return Ok(serde_json::json!({"jobs": []}));
    };
    let jobs = svc.list().await;
    serde_json::to_value(&jobs)
        .map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_cron_create(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let svc = state.cron_service.as_ref()
        .ok_or_else(|| rpc_error("NO_CRON", "Cron service not configured"))?;

    // Accept a full CronJob JSON or build one from simple params
    let job: oclaws_cron_core::CronJob = serde_json::from_value(p.clone())
        .map_err(|e| rpc_error("INVALID_PARAMS", &format!("Invalid job: {}", e)))?;

    let created = svc.add(job).await
        .map_err(|e| rpc_error("CRON_ERROR", &e.to_string()))?;
    serde_json::to_value(&created)
        .map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_cron_delete(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let svc = state.cron_service.as_ref()
        .ok_or_else(|| rpc_error("NO_CRON", "Cron service not configured"))?;
    let id = p["id"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    svc.remove(id).await
        .map_err(|e| rpc_error("CRON_ERROR", &e.to_string()))?;
    Ok(serde_json::json!({"deleted": true}))
}

async fn rpc_cron_trigger(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let svc = state.cron_service.as_ref()
        .ok_or_else(|| rpc_error("NO_CRON", "Cron service not configured"))?;
    let id = p["id"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    svc.trigger(id).await
        .map_err(|e| rpc_error("CRON_ERROR", &e.to_string()))?;
    Ok(serde_json::json!({"triggered": true}))
}

async fn rpc_cron_logs(p: &serde_json::Value, state: &HttpState) -> RpcResult {
    let run_log = state.cron_run_log.as_ref()
        .ok_or_else(|| rpc_error("NO_CRON", "Cron run log not available"))?;
    let id = p["id"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    let limit = p["limit"].as_u64().unwrap_or(50) as usize;
    let entries = run_log.read(id, limit).await
        .map_err(|e| rpc_error("CRON_ERROR", &e.to_string()))?;
    serde_json::to_value(&entries)
        .map_err(|e| rpc_error("INTERNAL", &e.to_string()))
}

async fn rpc_cron_status(state: &HttpState) -> RpcResult {
    let scheduler_running = state.cron_scheduler.as_ref()
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

async fn rpc_skills_status(_state: &HttpState) -> RpcResult {
    Ok(serde_json::json!({
        "installed": [],
        "available": 0,
        "note": "Skills management via RPC not yet implemented",
    }))
}

async fn rpc_skills_install(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let name = p["name"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'name'"))?;
    Err(rpc_error("NOT_IMPLEMENTED", &format!("Skill install not yet available: {}", name)))
}

async fn rpc_skills_update(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let id = p["id"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    Err(rpc_error("NOT_IMPLEMENTED", &format!("Skill update not yet available: {}", id)))
}

async fn rpc_skills_bins(_state: &HttpState) -> RpcResult {
    Ok(serde_json::json!({"bins": []}))
}

// ── Wizard RPCs ─────────────────────────────────────────────────────────

async fn rpc_wizard_start(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let wizard_type = p["type"].as_str().unwrap_or("setup");
    Ok(serde_json::json!({
        "started": true,
        "wizard_type": wizard_type,
        "step": 0,
        "total_steps": 5,
    }))
}

async fn rpc_wizard_next(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let step = p["step"].as_u64().unwrap_or(0);
    let answer = p["answer"].clone();
    Ok(serde_json::json!({
        "step": step + 1,
        "accepted": true,
        "previous_answer": answer,
    }))
}

async fn rpc_wizard_cancel(_p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    Ok(serde_json::json!({"cancelled": true}))
}

async fn rpc_wizard_status(_state: &HttpState) -> RpcResult {
    Ok(serde_json::json!({
        "active": false,
        "current_step": null,
        "wizard_type": null,
    }))
}

// ── Logs RPCs ───────────────────────────────────────────────────────────

async fn rpc_logs_tail(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let lines = p["lines"].as_u64().unwrap_or(50);
    let level = p["level"].as_str().unwrap_or("info");
    Ok(serde_json::json!({
        "lines": [],
        "requested": lines,
        "level": level,
        "note": "Log tailing requires tracing subscriber integration",
    }))
}

// ── Exec Approval RPCs ──────────────────────────────────────────────────

async fn rpc_exec_approvals_list(_state: &HttpState) -> RpcResult {
    Ok(serde_json::json!({"pending": []}))
}

async fn rpc_exec_approvals_approve(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let id = p["id"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    Ok(serde_json::json!({"approved": true, "id": id}))
}

async fn rpc_exec_approvals_reject(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let id = p["id"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    let reason = p["reason"].as_str().unwrap_or("rejected by user");
    Ok(serde_json::json!({"rejected": true, "id": id, "reason": reason}))
}

// ── TTS RPCs ────────────────────────────────────────────────────────────

async fn rpc_tts_status(_state: &HttpState) -> RpcResult {
    Ok(serde_json::json!({
        "enabled": false,
        "provider": null,
        "voice": null,
    }))
}

async fn rpc_tts_convert(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let text = p["text"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'text'"))?;
    let provider = p["provider"].as_str().unwrap_or("edge");
    let voice = p["voice"].as_str();
    Err(rpc_error("NOT_IMPLEMENTED", &format!(
        "TTS conversion not yet available (provider={}, text_len={}, voice={:?})",
        provider, text.len(), voice
    )))
}

async fn rpc_tts_providers(_state: &HttpState) -> RpcResult {
    Ok(serde_json::json!({
        "providers": ["openai", "elevenlabs", "edge"],
        "default": "edge",
    }))
}

// ── Node RPCs ───────────────────────────────────────────────────────────

async fn rpc_node_list(_state: &HttpState) -> RpcResult {
    Ok(serde_json::json!({"nodes": []}))
}

async fn rpc_node_describe(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let id = p["id"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    Err(rpc_error("NOT_FOUND", &format!("Node not found: {}", id)))
}

async fn rpc_node_pair_request(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let name = p["name"].as_str().unwrap_or("unnamed");
    let code = uuid::Uuid::new_v4().to_string()[..8].to_uppercase();
    Ok(serde_json::json!({
        "request_id": uuid::Uuid::new_v4().to_string(),
        "code": code,
        "name": name,
        "status": "pending",
    }))
}

async fn rpc_node_pair_list(_state: &HttpState) -> RpcResult {
    Ok(serde_json::json!({"requests": []}))
}

async fn rpc_node_pair_approve(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let id = p["id"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    Ok(serde_json::json!({"approved": true, "id": id}))
}

async fn rpc_node_pair_reject(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let id = p["id"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    Ok(serde_json::json!({"rejected": true, "id": id}))
}

async fn rpc_node_rename(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let id = p["id"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    let name = p["name"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'name'"))?;
    Ok(serde_json::json!({"renamed": true, "id": id, "name": name}))
}

async fn rpc_node_invoke(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let id = p["id"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    let method = p["method"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'method'"))?;
    Err(rpc_error("NOT_FOUND", &format!(
        "Node '{}' not connected, cannot invoke '{}'", id, method
    )))
}

async fn rpc_node_event(p: &serde_json::Value, _state: &HttpState) -> RpcResult {
    let id = p["id"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'id'"))?;
    let event = p["event"].as_str()
        .ok_or_else(|| rpc_error("INVALID_PARAMS", "Missing 'event'"))?;
    Err(rpc_error("NOT_FOUND", &format!(
        "Node '{}' not connected, cannot send event '{}'", id, event
    )))
}
