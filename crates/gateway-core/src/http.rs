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

use crate::auth::AuthState;
use crate::error::{GatewayError, GatewayResult};
use crate::message::MessageHandler;
use crate::server::GatewayServer;
use oclaws_protocol::frames::{ErrorDetails, GatewayFrame, HelloOk, ServerFeatures, ServerInfo, Policy};
use oclaws_protocol::snapshot::{AuthMode, Snapshot, StateVersion};

pub mod agent_bridge;
pub mod auth;
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
    full_config: Option<Arc<RwLock<oclaws_config::settings::Config>>>,
    config_path: Option<PathBuf>,
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
            full_config: None,
            config_path: None,
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

    pub fn with_full_config(mut self, config: oclaws_config::settings::Config, path: PathBuf) -> Self {
        self.full_config = Some(Arc::new(RwLock::new(config)));
        self.config_path = Some(path);
        self
    }

    pub fn into_router(self) -> Router {
        let cors = self.build_cors_layer();
        let mut hc = HealthChecker::new();
        hc.register(Box::new(SystemHealthCheck::new()));
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
            metrics: Arc::new(metrics::AppMetrics::new()),
            health_checker: Arc::new(hc),
            full_config: self.full_config.clone(),
            config_path: self.config_path.clone(),
            echo_tracker: Arc::new(tokio::sync::Mutex::new(EchoTracker::default())),
            group_activation: GroupActivation::default(),
        });

        // Webhook routes skip auth middleware (they use their own verification)
        let webhook_routes = Router::new()
            .route("/webhooks/telegram", post(webhooks::telegram_webhook))
            .route("/webhooks/slack", post(webhooks::slack_webhook))
            .route("/webhooks/discord", post(webhooks::discord_webhook))
            .route("/webhooks/feishu", post(webhooks::feishu_webhook))
            .route("/webhooks/{channel}", post(webhooks::generic_webhook))
            .with_state(state.clone());

        // Config UI routes skip auth (local admin use)
        let config_ui_routes = Router::new()
            .route("/api/config/full", get(routes::config_full_get_handler))
            .route("/api/config/full", put(routes::config_full_put_handler))
            .route("/ui/config", get(routes::config_ui_handler))
            .route("/ui/chat", get(routes::webchat_ui_handler))
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
        if let Some(ref regs) = self.plugin_registrations {
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
    pub metrics: Arc<metrics::AppMetrics>,
    pub health_checker: Arc<HealthChecker>,
    pub full_config: Option<Arc<RwLock<oclaws_config::settings::Config>>>,
    pub config_path: Option<PathBuf>,
    pub echo_tracker: Arc<tokio::sync::Mutex<EchoTracker>>,
    pub group_activation: GroupActivation,
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

    let gateway_server = state.gateway_server.clone();

    ws.on_upgrade(move |socket| async move {
        if let Err(e) = handle_ws(socket, addr, gateway_server).await {
            error!("WebSocket error: {}", e);
        }
    })
}

async fn handle_ws(
    socket: axum::extract::ws::WebSocket,
    addr: SocketAddr,
    gateway_server: Arc<GatewayServer>,
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
                "session.send".to_string(),
                "session.receive".to_string(),
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
        canvas_host_url: None,
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
                    if let Some(resp) = handle_frame(frame, &gateway_server).await? {
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
    gateway_server: &Arc<GatewayServer>,
) -> Result<Option<oclaws_protocol::frames::ResponseFrame>, Box<dyn std::error::Error + Send + Sync>> {
    match frame {
        GatewayFrame::Request(req) => {
            let response = match req.method.as_str() {
                "session.create" => {
                    let payload: Option<serde_json::Value> = req.params.map(serde_json::from_value).transpose()?;
                    let manager = gateway_server.session_manager.read().await;
                    let key = payload.as_ref().and_then(|v| v["key"].as_str()).unwrap_or("default");
                    let agent_id = payload.as_ref().and_then(|v| v["agentId"].as_str()).unwrap_or("default");
                    let session = manager.create_session(key, agent_id)?;
                    MessageHandler::new_response(&req.id, true, Some(serde_json::to_value(&session)?), None)
                }
                "session.list" => {
                    let manager = gateway_server.session_manager.read().await;
                    let sessions = manager.list_sessions()?;
                    MessageHandler::new_response(&req.id, true, Some(serde_json::to_value(&sessions)?), None)
                }
                _ => MessageHandler::new_response(
                    &req.id, false, None,
                    Some(ErrorDetails {
                        code: "METHOD_NOT_FOUND".to_string(),
                        message: format!("Unknown method: {}", req.method),
                        details: None, retryable: Some(false), retry_after_ms: None,
                    }),
                ),
            };
            Ok(Some(response))
        }
        _ => Ok(None),
    }
}
