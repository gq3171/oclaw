use crate::traits::*;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct NostrChannel {
    relay_urls: Vec<String>,
    private_key: Option<String>,
    public_key: Option<String>,
    client: Option<Client>,
    status: ChannelStatus,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct NostrEvent {
    kind: u16,
    content: String,
    tags: Vec<Vec<String>>,
    created_at: u64,
    pubkey: String,
    sig: Option<String>,
}

#[derive(Debug, Serialize)]
struct NostrSendRequest {
    #[serde(rename = "event")]
    event: NostrEvent,
    #[serde(rename = "id")]
    id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct NostrResponse {
    #[serde(default)]
    ok: Option<bool>,
    #[serde(default)]
    event_id: Option<String>,
    #[serde(default)]
    message: Option<String>,
}

impl NostrChannel {
    pub fn new() -> Self {
        Self {
            relay_urls: Vec::new(),
            private_key: None,
            public_key: None,
            client: None,
            status: ChannelStatus::Disconnected,
        }
    }

    pub fn with_relays(mut self, relays: Vec<&str>) -> Self {
        self.relay_urls = relays.into_iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn with_keys(mut self, private_key: &str, public_key: &str) -> Self {
        self.private_key = Some(private_key.to_string());
        self.public_key = Some(public_key.to_string());
        self
    }

    async fn send_to_relay(&self, relay_url: &str, event: &NostrEvent) -> ChannelResult<String> {
        let client = self.client.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".to_string()))?;

        let request = NostrSendRequest {
            event: event.clone(),
            id: None,
        };

        let response = client
            .post(relay_url)
            .json(&request)
            .send()
            .await
            .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;

        let nostr_resp: NostrResponse = response.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        if nostr_resp.ok.unwrap_or(false) {
            Ok(nostr_resp.event_id.unwrap_or_else(|| "unknown".to_string()))
        } else {
            Err(ChannelError::MessageError(
                nostr_resp.message.unwrap_or_else(|| "Unknown error".to_string())
            ))
        }
    }
}

impl Default for NostrChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Channel for NostrChannel {
    fn channel_type(&self) -> &str {
        "nostr"
    }

    async fn connect(&mut self) -> ChannelResult<()> {
        if self.private_key.is_none() || self.public_key.is_none() {
            return Err(ChannelError::AuthenticationError(
                "Private and public keys required".to_string(),
            ));
        }

        self.status = ChannelStatus::Connecting;
        self.client = Some(Client::new());

        tracing::info!("Nostr channel connecting to {} relays", self.relay_urls.len());

        self.status = ChannelStatus::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> ChannelResult<()> {
        self.client = None;
        self.status = ChannelStatus::Disconnected;
        tracing::info!("Nostr channel disconnected");
        Ok(())
    }

    fn status(&self) -> ChannelStatus {
        self.status
    }

    async fn send_message(&self, message: &ChannelMessage) -> ChannelResult<String> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let pubkey = self.public_key.as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("Public key not set".to_string()))?;

        let tags: Vec<Vec<String>> = message.metadata
            .get("tags")
            .map(|t| {
                t.split(',')
                    .map(|s| vec!["p".to_string(), s.to_string()])
                    .collect()
            })
            .unwrap_or_default();

        let event = NostrEvent {
            kind: 1,
            content: message.content.clone(),
            tags,
            created_at: chrono::Utc::now().timestamp() as u64,
            pubkey: pubkey.clone(),
            sig: None,
        };

        let mut last_error = None;
        for relay in &self.relay_urls {
            match self.send_to_relay(relay, &event).await {
                Ok(event_id) => return Ok(event_id),
                Err(e) => last_error = Some(e),
            }
        }

        Err(last_error.unwrap_or_else(|| ChannelError::MessageError("Failed to send to all relays".to_string())))
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let account = ChannelAccount {
            id: self.public_key.clone().unwrap_or_default(),
            name: "Nostr User".to_string(),
            channel: "nostr".to_string(),
            avatar: None,
            status: Some("active".to_string()),
        };

        Ok(vec![account])
    }

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()> {
        tracing::debug!("Received Nostr event: {:?}", event);
        Ok(())
    }

    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>> {
        Ok(Box::new(NostrSender {
            channel: Arc::new(RwLock::new(self.clone())),
        }))
    }
}

impl Clone for NostrChannel {
    fn clone(&self) -> Self {
        Self {
            relay_urls: self.relay_urls.clone(),
            private_key: self.private_key.clone(),
            public_key: self.public_key.clone(),
            client: self.client.clone(),
            status: self.status,
        }
    }
}

struct NostrSender {
    channel: Arc<RwLock<NostrChannel>>,
}

impl MessageSender for NostrSender {
    fn send<'a>(&'a self, content: &'a str, metadata: HashMap<String, String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = ChannelResult<String>> + Send + 'a>> {
        let channel = self.channel.clone();
        let content = content.to_string();
        let metadata = metadata.clone();
        
        Box::pin(async move {
            let message = ChannelMessage {
                id: uuid::Uuid::new_v4().to_string(),
                channel: "nostr".to_string(),
                sender: String::new(),
                content,
                timestamp: chrono::Utc::now().timestamp_millis(),
                metadata,
            };
            
            channel.read().await.send_message(&message).await
        })
    }
}
