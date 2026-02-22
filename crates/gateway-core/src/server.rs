use crate::error::{GatewayError, GatewayResult};
use crate::message::{MessageHandler, SessionManager};
use futures_util::{SinkExt, StreamExt};
use oclaws_protocol::frames::{GatewayFrame, HelloOk, ServerFeatures, ServerInfo, Policy};
use oclaws_protocol::snapshot::{AuthMode, Snapshot, StateVersion};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, RwLock};
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tracing::{info, error, debug};

pub struct GatewayServer {
    port: u16,
    pub session_manager: Arc<RwLock<SessionManager>>,
    shutdown_tx: broadcast::Sender<()>,
}

impl GatewayServer {
    pub fn new(port: u16) -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            port,
            session_manager: Arc::new(RwLock::new(SessionManager::new())),
            shutdown_tx,
        }
    }

    pub async fn start(&self) -> GatewayResult<()> {
        let addr = format!("0.0.0.0:{}", self.port);
        let listener = TcpListener::bind(&addr).await.map_err(|e| {
            GatewayError::ServerError(format!("Failed to bind to {}: {}", addr, e))
        })?;

        info!("Gateway server listening on {}", addr);

        let session_manager = Arc::clone(&self.session_manager);
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        loop {
            tokio::select! {
                result = listener.accept() => {
                    match result {
                        Ok((stream, addr)) => {
                            let session_manager = Arc::clone(&session_manager);
                            let shutdown_tx = self.shutdown_tx.clone();
                            
                            tokio::spawn(async move {
                                if let Err(e) = handle_connection(stream, addr, session_manager, shutdown_tx).await {
                                    error!("Connection error: {}", e);
                                }
                            });
                        }
                        Err(e) => {
                            error!("Failed to accept connection: {}", e);
                        }
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!("Shutting down server");
                    break;
                }
            }
        }

        Ok(())
    }

    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
    }
}

async fn handle_connection(
    stream: TcpStream,
    addr: SocketAddr,
    session_manager: Arc<RwLock<SessionManager>>,
    shutdown_tx: broadcast::Sender<()>,
) -> GatewayResult<()> {
    let ws_stream = accept_async(stream).await.map_err(|e| {
        GatewayError::WebSocketError(format!("WebSocket handshake failed: {}", e))
    })?;

    let (mut write, mut read) = ws_stream.split();
    
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

    let hello_json = serde_json::to_vec(&hello).map_err(GatewayError::JsonError)?;
    write.send(Message::Binary(hello_json.into())).await.map_err(|e| {
        GatewayError::WebSocketError(format!("Failed to send hello: {}", e))
    })?;

    debug!("Sent hello to {}", addr);

    let mut shutdown_rx = shutdown_tx.subscribe();

    loop {
        tokio::select! {
            msg = read.next() => {
                match msg {
                    Some(Ok(Message::Binary(data))) => {
                        let frame: GatewayFrame = serde_json::from_slice(&data).map_err(|e| {
                            GatewayError::InvalidFrame(e.to_string())
                        })?;
                        
                        handle_frame(frame, &mut write, &session_manager).await?;
                    }
                    Some(Ok(Message::Text(text))) => {
                        let frame: GatewayFrame = serde_json::from_str(&text).map_err(|e| {
                            GatewayError::InvalidFrame(e.to_string())
                        })?;
                        
                        handle_frame(frame, &mut write, &session_manager).await?;
                    }
                    Some(Ok(Message::Close(_))) => {
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
            _ = shutdown_rx.recv() => {
                info!("Shutting down connection {}", addr);
                break;
            }
        }
    }

    Ok(())
}

async fn handle_frame(
    frame: GatewayFrame,
    write: &mut futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<TcpStream>, Message>,
    session_manager: &Arc<RwLock<SessionManager>>,
) -> GatewayResult<()> {
    match frame {
        GatewayFrame::Request(req) => {
            debug!("Received request: {} {}", req.id, req.method);
            
            let response = match req.method.as_str() {
                "session.create" => {
                    let payload: Option<serde_json::Value> = req.params.map(serde_json::from_value).transpose()?;
                    
                    let mut manager = session_manager.write().await;
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
                        Some(serde_json::to_value(&session).map_err(GatewayError::JsonError)?),
                        None,
                    )
                }
                "session.list" => {
                    let manager = session_manager.read().await;
                    let sessions: Vec<_> = manager.list_sessions().into_iter().cloned().collect();
                    
                    MessageHandler::new_response(
                        &req.id,
                        true,
                        Some(serde_json::to_value(&sessions).map_err(GatewayError::JsonError)?),
                        None,
                    )
                }
                _ => {
                    MessageHandler::new_response(
                        &req.id,
                        false,
                        None,
                        Some(oclaws_protocol::frames::ErrorDetails {
                            code: "METHOD_NOT_FOUND".to_string(),
                            message: format!("Unknown method: {}", req.method),
                            details: None,
                            retryable: Some(false),
                            retry_after_ms: None,
                        }),
                    )
                }
            };

            let response_json = serde_json::to_vec(&response).map_err(GatewayError::JsonError)?;
            write.send(Message::Binary(response_json.into())).await.map_err(|e| {
                GatewayError::WebSocketError(format!("Failed to send response: {}", e))
            })?;
        }
        GatewayFrame::Hello(_) => {
            debug!("Received hello frame");
        }
        GatewayFrame::HelloOk(_) => {
            debug!("Received hello-ok frame (ignored in server mode)");
        }
        GatewayFrame::SessionCreate(_) => {
            debug!("Received session.create frame");
        }
        GatewayFrame::SessionCreateOk(_) => {
            debug!("Received session.create-ok frame");
        }
        GatewayFrame::SessionStart(_) => {
            debug!("Received session.start frame");
        }
        GatewayFrame::SessionStartOk(_) => {
            debug!("Received session.start-ok frame");
        }
        GatewayFrame::Response(_) => {
            debug!("Received response (ignored in server mode)");
        }
        GatewayFrame::Event(event) => {
            debug!("Received event: {}", event.event);
        }
        GatewayFrame::Error(_) => {
            debug!("Received error frame");
        }
    }

    Ok(())
}
