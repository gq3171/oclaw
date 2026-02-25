use crate::cdp::{CdpCommand, CdpEvent, CdpMessage, CdpResponse};
use crate::error::{BrowserError, BrowserResult};
use futures_util::{SinkExt, StreamExt};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tokio_tungstenite::connect_async;
use tracing::{debug, error, info};

pub struct CdpConnection {
    ws_url: String,
    sender: mpsc::Sender<CdpCommand>,
    event_receiver: broadcast::Receiver<CdpEvent>,
    next_id: Arc<RwLock<i32>>,
    pending_commands: Arc<RwLock<HashMap<i32, mpsc::Sender<BrowserResult<CdpResponse>>>>>,
    _task_handle: tokio::task::JoinHandle<()>,
}

impl Drop for CdpConnection {
    fn drop(&mut self) {
        self._task_handle.abort();
    }
}

impl CdpConnection {
    pub async fn connect(ws_url: &str) -> BrowserResult<Self> {
        info!("Connecting to CDP WebSocket: {}", ws_url);

        let (ws_stream, _) = connect_async(ws_url).await.map_err(|e| {
            BrowserError::ConnectionError(format!("Failed to connect to {}: {}", ws_url, e))
        })?;

        let (mut write, mut read) = ws_stream.split();

        let (cmd_tx, mut cmd_rx) = mpsc::channel::<CdpCommand>(100);
        let (event_tx, event_rx) = broadcast::channel::<CdpEvent>(100);

        let next_id = Arc::new(RwLock::new(1i32));
        let pending_commands: Arc<RwLock<HashMap<i32, mpsc::Sender<BrowserResult<CdpResponse>>>>> = 
            Arc::new(RwLock::new(HashMap::new()));
        
        let _next_id_clone = Arc::clone(&next_id);
        let pending_clone = Arc::clone(&pending_commands);
        let event_tx_clone = event_tx.clone();

        let task_handle = tokio::spawn(async move {
            loop {
                tokio::select! {
                    cmd = cmd_rx.recv() => {
                        let Some(cmd) = cmd else {
                            // CdpConnection dropped — sender closed
                            info!("CDP command channel closed, shutting down");
                            break;
                        };
                        let json = match serde_json::to_string(&cmd) {
                            Ok(j) => j,
                            Err(e) => {
                                error!("Failed to serialize CDP command: {}", e);
                                continue;
                            }
                        };
                        if let Err(e) = write.send(tokio_tungstenite::tungstenite::Message::Text(json.into())).await {
                            error!("Failed to send CDP command: {}", e);
                            break;
                        }
                    }
                    msg = read.next() => {
                        match msg {
                            Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                                match serde_json::from_str::<CdpMessage>(&text) {
                                    Ok(CdpMessage::Response(resp)) => {
                                        let mut pending = pending_clone.write().await;
                                        if let Some(tx) = pending.remove(&resp.id) {
                                            drop(tx.send(Ok(resp)));
                                        }
                                    }
                                    Ok(CdpMessage::Event(event)) => {
                                        let _ = event_tx_clone.send(event);
                                    }
                                    Err(e) => {
                                        debug!("Failed to parse CDP message: {}", e);
                                    }
                                }
                            }
                            Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => {
                                info!("CDP connection closed");
                                break;
                            }
                            Some(Err(e)) => {
                                error!("CDP WebSocket error: {}", e);
                                break;
                            }
                            None => break,
                            _ => {}
                        }
                    }
                }
            }
        });

        Ok(Self {
            ws_url: ws_url.to_string(),
            sender: cmd_tx,
            event_receiver: event_rx,
            next_id,
            pending_commands,
            _task_handle: task_handle,
        })
    }

    pub async fn send_command(&self, method: &str, params: Option<serde_json::Value>) -> BrowserResult<CdpResponse> {
        let id = {
            let mut id = self.next_id.write().await;
            let current = *id;
            *id += 1;
            current
        };

        let (tx, mut rx) = mpsc::channel(1);
        
        {
            let mut pending = self.pending_commands.write().await;
            pending.insert(id, tx);
        }

        let cmd = CdpCommand {
            id,
            method: method.to_string(),
            params,
        };

        self.sender.send(cmd).await.map_err(|e| {
            BrowserError::ConnectionError(format!("Failed to send command: {}", e))
        })?;

        rx.recv().await.ok_or_else(|| {
            BrowserError::ConnectionError("Connection closed".to_string())
        })?
    }

    pub fn subscribe(&self) -> broadcast::Receiver<CdpEvent> {
        self.event_receiver.resubscribe()
    }

    pub async fn enable_domains(&self, domains: &[&str]) -> BrowserResult<()> {
        for domain in domains {
            let method = format!("{}.enable", domain);
            self.send_command(&method, None).await?;
            info!("Enabled CDP domain: {}", domain);
        }
        Ok(())
    }

    pub fn ws_url(&self) -> &str {
        &self.ws_url
    }
}
