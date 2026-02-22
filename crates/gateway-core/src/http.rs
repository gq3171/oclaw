use axum::{
    extract::{
        ConnectInfo, DefaultBodyLimit, State, WebSocketUpgrade,
    },
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use oclaws_config::settings::Gateway;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower::ServiceBuilder;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{error, info};

use crate::auth::AuthState;
use crate::error::{GatewayError, GatewayResult};
use crate::message::MessageHandler;
use crate::server::GatewayServer;
use oclaws_protocol::frames::{ErrorDetails, GatewayFrame, HelloOk, ServerFeatures, ServerInfo, Policy};
use oclaws_protocol::snapshot::{AuthMode, Snapshot, StateVersion};

pub mod auth;
pub mod routes;

pub struct HttpServer {
    addr: SocketAddr,
    gateway: Arc<Gateway>,
    auth_state: Arc<RwLock<AuthState>>,
    gateway_server: Arc<GatewayServer>,
    tls_config: Option<Arc<rustls::ServerConfig>>,
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
        }
    }

    pub fn with_tls(mut self, tls_config: Arc<rustls::ServerConfig>) -> Self {
        self.tls_config = Some(tls_config);
        self
    }

    pub async fn start(&self) -> GatewayResult<()> {
        let cors = self.build_cors_layer();

        let app = Router::new()
            .route("/health", get(health_handler))
            .route("/v1/chat/completions", post(routes::chat_completions_handler))
            .route("/v1/responses", post(routes::responses_handler))
            .route("/ws", get(ws_handler))
            .route("/", any(root_handler))
            .layer(cors)
            .layer(TraceLayer::new_for_http())
            .layer(ServiceBuilder::new().layer(DefaultBodyLimit::max(10 * 1024 * 1024)))
            .with_state(Arc::new(HttpState {
                auth_state: self.auth_state.clone(),
                gateway_server: self.gateway_server.clone(),
                _gateway: self.gateway.clone(),
            }));

        let listener = tokio::net::TcpListener::bind(self.addr).await.map_err(|e| {
            GatewayError::ServerError(format!("Failed to bind to {}: {}", self.addr, e))
        })?;

        info!("HTTP server listening on {}", self.addr);

        let app = app;
        
        axum::serve(listener, app).await.map_err(|e: std::io::Error| {
            GatewayError::ServerError(format!("HTTP server error: {}", e))
        })?;

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
    auth_state: Arc<RwLock<AuthState>>,
    gateway_server: Arc<GatewayServer>,
    _gateway: Arc<Gateway>,
}

async fn health_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

async fn root_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "service": "oclaws-gateway",
        "version": env!("CARGO_PKG_VERSION"),
        "endpoints": [
            "/health",
            "/ws",
            "/v1/chat/completions",
            "/v1/responses"
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
                match msg {
                    Some(Ok(axum::extract::ws::Message::Binary(data))) => {
                        let frame: GatewayFrame = serde_json::from_slice(&data)?;
                        
                        match frame {
                            GatewayFrame::Request(req) => {
                                let response = match req.method.as_str() {
                                    "session.create" => {
                                        let payload: Option<serde_json::Value> = req.params.map(serde_json::from_value).transpose()?;
                                        
                                        let mut manager = gateway_server.session_manager.write().await;
                                        let key = payload.as_ref()
                                            .and_then(|v| v.get("key"))
                                            .and_then(|k| k.as_str())
                                            .unwrap_or("default");
                                        let agent_id = payload.as_ref()
                                            .and_then(|v| v.get("agentId"))
                                            .and_then(|a| a.as_str())
                                            .unwrap_or("default");
                                        
                                        let session = manager.create_session(key, agent_id);
                                        
                                        MessageHandler::new_response(
                                            &req.id,
                                            true,
                                            Some(serde_json::to_value(&session)?),
                                            None,
                                        )
                                    }
                                    "session.list" => {
                                        let manager = gateway_server.session_manager.read().await;
                                        let sessions: Vec<_> = manager.list_sessions().into_iter().cloned().collect();
                                        
                                        MessageHandler::new_response(
                                            &req.id,
                                            true,
                                            Some(serde_json::to_value(&sessions)?),
                                            None,
                                        )
                                    }
                                    _ => {
                                        MessageHandler::new_response(
                                            &req.id,
                                            false,
                                            None,
                                            Some(ErrorDetails {
                                                code: "METHOD_NOT_FOUND".to_string(),
                                                message: format!("Unknown method: {}", req.method),
                                                details: None,
                                                retryable: Some(false),
                                                retry_after_ms: None,
                                            }),
                                        )
                                    }
                                };

                                let response_json = serde_json::to_vec(&response)?;
                                write.send(axum::extract::ws::Message::Binary(response_json.into())).await?;
                            }
                            GatewayFrame::Response(_) => {}
                            GatewayFrame::Event(_) => {}
                        }
                    }
                    Some(Ok(axum::extract::ws::Message::Text(text))) => {
                        let frame: GatewayFrame = serde_json::from_str(&text)?;
                        
                        match frame {
                            GatewayFrame::Request(req) => {
                                let response = match req.method.as_str() {
                                    "session.create" => {
                                        let payload: Option<serde_json::Value> = req.params.map(serde_json::from_value).transpose()?;
                                        
                                        let mut manager = gateway_server.session_manager.write().await;
                                        let key = payload.as_ref()
                                            .and_then(|v| v.get("key"))
                                            .and_then(|k| k.as_str())
                                            .unwrap_or("default");
                                        let agent_id = payload.as_ref()
                                            .and_then(|v| v.get("agentId"))
                                            .and_then(|a| a.as_str())
                                            .unwrap_or("default");
                                        
                                        let session = manager.create_session(key, agent_id);
                                        
                                        MessageHandler::new_response(
                                            &req.id,
                                            true,
                                            Some(serde_json::to_value(&session)?),
                                            None,
                                        )
                                    }
                                    "session.list" => {
                                        let manager = gateway_server.session_manager.read().await;
                                        let sessions: Vec<_> = manager.list_sessions().into_iter().cloned().collect();
                                        
                                        MessageHandler::new_response(
                                            &req.id,
                                            true,
                                            Some(serde_json::to_value(&sessions)?),
                                            None,
                                        )
                                    }
                                    _ => {
                                        MessageHandler::new_response(
                                            &req.id,
                                            false,
                                            None,
                                            Some(ErrorDetails {
                                                code: "METHOD_NOT_FOUND".to_string(),
                                                message: format!("Unknown method: {}", req.method),
                                                details: None,
                                                retryable: Some(false),
                                                retry_after_ms: None,
                                            }),
                                        )
                                    }
                                };

                                let response_json = serde_json::to_vec(&response)?;
                                write.send(axum::extract::ws::Message::Binary(response_json.into())).await?;
                            }
                            GatewayFrame::Response(_) => {}
                            GatewayFrame::Event(_) => {}
                        }
                    }
                    Some(Ok(axum::extract::ws::Message::Close(_))) => {
                        info!("Client {} disconnected", addr);
                        break;
                    }
                    Some(Err(e)) => {
                        error!("WebSocket error: {}", e);
                        break;
                    }
                    None => {
                        break;
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}
