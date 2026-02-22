use crate::error::GatewayError;
use futures_util::{StreamExt};
use oclaws_protocol::frames::HelloOk;
use tokio::sync::broadcast;
use tokio_tungstenite::connect_async;
use tracing::{info, debug};

pub struct GatewayClient {
    url: String,
    session_id: Option<String>,
    shutdown_tx: broadcast::Sender<()>,
}

impl GatewayClient {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            session_id: None,
            shutdown_tx: broadcast::channel(1).0,
        }
    }

    pub async fn connect(&mut self) -> Result<HelloOk, GatewayError> {
        let (ws_stream, _) = connect_async(&self.url).await.map_err(|e| {
            GatewayError::ConnectionError(format!("Failed to connect to {}: {}", self.url, e))
        })?;

        info!("Connected to {}", self.url);

        let (_write, mut read) = ws_stream.split();

        let hello_msg = read.next().await
            .ok_or_else(|| GatewayError::ConnectionError("Failed to receive hello".to_string()))?;

        let msg_result = hello_msg.map_err(|e| GatewayError::WebSocketError(e.to_string()))?;

        let hello_ok: HelloOk = match msg_result {
            tokio_tungstenite::tungstenite::Message::Binary(data) => {
                serde_json::from_slice(&data).map_err(|e| GatewayError::InvalidFrame(e.to_string()))?
            }
            tokio_tungstenite::tungstenite::Message::Text(text) => {
                serde_json::from_str(&text).map_err(|e| GatewayError::InvalidFrame(e.to_string()))?
            }
            _ => {
                return Err(GatewayError::ConnectionError("Unexpected message type".to_string()));
            }
        };

        debug!("Received hello: protocol {}", hello_ok.protocol);

        self.session_id = Some(hello_ok.server.conn_id.clone());

        Ok(hello_ok)
    }

    pub async fn disconnect(&self) {
        let _ = self.shutdown_tx.send(());
    }
}
