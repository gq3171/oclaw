//! iMessage / BlueBubbles channel
//!
//! Connects to a BlueBubbles server (self-hosted iMessage bridge) via REST API.
//! BlueBubbles exposes iMessage on non-Apple platforms through a macOS relay.

use crate::traits::*;
use async_trait::async_trait;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct BlueBubblesChannel {
    server_url: Option<String>,
    password: Option<String>,
    status: ChannelStatus,
    client: Option<reqwest::Client>,
}

impl BlueBubblesChannel {
    pub fn new() -> Self {
        Self {
            server_url: None,
            password: None,
            status: ChannelStatus::Disconnected,
            client: None,
        }
    }

    pub fn with_config(mut self, server_url: &str, password: &str) -> Self {
        self.server_url = Some(server_url.into());
        self.password = Some(password.into());
        self
    }
}

impl Default for BlueBubblesChannel {
    fn default() -> Self { Self::new() }
}

impl Clone for BlueBubblesChannel {
    fn clone(&self) -> Self {
        Self {
            server_url: self.server_url.clone(),
            password: self.password.clone(),
            status: self.status,
            client: None,
        }
    }
}

#[async_trait]
impl Channel for BlueBubblesChannel {
    fn channel_type(&self) -> &str { "bluebubbles" }

    async fn connect(&mut self) -> ChannelResult<()> {
        self.status = ChannelStatus::Connecting;
        let url = self.server_url.as_ref()
            .ok_or_else(|| ChannelError::ConfigError("Server URL not set".into()))?;
        let password = self.password.as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("Password not set".into()))?;

        let client = reqwest::Client::new();

        // Ping BlueBubbles server to verify connectivity
        let ping_url = format!("{}/api/v1/ping?password={}", url.trim_end_matches('/'), password);
        let resp = client.get(&ping_url).send().await
            .map_err(|e| ChannelError::ConnectionError(format!("BlueBubbles ping failed: {}", e)))?;

        if !resp.status().is_success() {
            return Err(ChannelError::ConnectionError(
                format!("BlueBubbles server returned {}", resp.status()),
            ));
        }

        self.client = Some(client);
        self.status = ChannelStatus::Connected;
        tracing::info!("BlueBubbles channel connected to {}", url);
        Ok(())
    }

    async fn disconnect(&mut self) -> ChannelResult<()> {
        self.client = None;
        self.status = ChannelStatus::Disconnected;
        tracing::info!("BlueBubbles channel disconnected");
        Ok(())
    }

    fn status(&self) -> ChannelStatus { self.status }

    async fn send_message(&self, message: &ChannelMessage) -> ChannelResult<String> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".into()));
        }
        let base_url = self.server_url.as_ref()
            .ok_or_else(|| ChannelError::ConfigError("Server URL not set".into()))?;
        let password = self.password.as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("Password not set".into()))?;
        let client = self.client.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".into()))?;

        // Determine chat GUID from metadata or use channel field
        let chat_guid = message.metadata.get("chat_guid")
            .or_else(|| message.metadata.get("chatGuid"))
            .cloned()
            .unwrap_or_else(|| message.channel.clone());

        // BlueBubbles send text API
        let url = format!(
            "{}/api/v1/message/text?password={}",
            base_url.trim_end_matches('/'), password
        );

        // Chunk long messages (iMessage limit ~4000 chars)
        let chunks = chunk_text(&message.content, 4000);
        let mut last_id = String::new();

        for chunk in chunks {
            let body = serde_json::json!({
                "chatGuid": chat_guid,
                "message": chunk,
                "method": "private-api"
            });

            let resp = client.post(&url)
                .json(&body)
                .send().await
                .map_err(|e| ChannelError::MessageError(e.to_string()))?;

            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                return Err(ChannelError::MessageError(
                    format!("BlueBubbles API error ({}): {}", status, text),
                ));
            }

            let json: serde_json::Value = resp.json().await.unwrap_or_default();
            last_id = json["data"]["guid"].as_str()
                .unwrap_or("unknown")
                .to_string();
        }

        Ok(last_id)
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>> {
        Ok(vec![])
    }

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()> {
        tracing::debug!("BlueBubbles event: {:?}", event);
        Ok(())
    }

    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>> {
        Ok(Box::new(BlueBubblesSender {
            channel: Arc::new(RwLock::new(self.clone())),
        }))
    }
}

struct BlueBubblesSender {
    channel: Arc<RwLock<BlueBubblesChannel>>,
}

impl MessageSender for BlueBubblesSender {
    fn send<'a>(
        &'a self,
        content: &'a str,
        metadata: HashMap<String, String>,
    ) -> Pin<Box<dyn std::future::Future<Output = ChannelResult<String>> + Send + 'a>> {
        let channel = self.channel.clone();
        let content = content.to_string();
        Box::pin(async move {
            let msg = ChannelMessage {
                id: uuid::Uuid::new_v4().to_string(),
                channel: "bluebubbles".into(),
                sender: String::new(),
                content,
                timestamp: chrono::Utc::now().timestamp_millis(),
                metadata,
            };
            channel.read().await.send_message(&msg).await
        })
    }
}

fn chunk_text(text: &str, max_len: usize) -> Vec<&str> {
    if text.len() <= max_len {
        return vec![text];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let end = (start + max_len).min(text.len());
        chunks.push(&text[start..end]);
        start = end;
    }
    chunks
}
