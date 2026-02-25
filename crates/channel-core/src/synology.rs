//! Synology Chat channel — Webhook (Incoming/Outgoing)
//!
//! Simple JSON payload webhook integration with Synology Chat.

use crate::traits::*;
use async_trait::async_trait;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct SynologyChannel {
    server_url: Option<String>,
    token: Option<String>,
    status: ChannelStatus,
    client: Option<reqwest::Client>,
}

impl SynologyChannel {
    pub fn new() -> Self {
        Self {
            server_url: None,
            token: None,
            status: ChannelStatus::Disconnected,
            client: None,
        }
    }

    pub fn with_config(mut self, server_url: &str, token: &str) -> Self {
        self.server_url = Some(server_url.into());
        self.token = Some(token.into());
        self
    }
}

impl Default for SynologyChannel {
    fn default() -> Self { Self::new() }
}

impl Clone for SynologyChannel {
    fn clone(&self) -> Self {
        Self {
            server_url: self.server_url.clone(),
            token: self.token.clone(),
            status: self.status,
            client: None,
        }
    }
}

#[async_trait]
impl Channel for SynologyChannel {
    fn channel_type(&self) -> &str { "synology" }

    async fn connect(&mut self) -> ChannelResult<()> {
        self.status = ChannelStatus::Connecting;
        let _url = self.server_url.as_ref()
            .ok_or_else(|| ChannelError::ConfigError("Server URL not set".into()))?;
        self.client = Some(reqwest::Client::new());
        tracing::info!("Synology Chat channel connected to {}", _url);
        self.status = ChannelStatus::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> ChannelResult<()> {
        self.client = None;
        self.status = ChannelStatus::Disconnected;
        tracing::info!("Synology Chat channel disconnected");
        Ok(())
    }

    fn status(&self) -> ChannelStatus { self.status }

    async fn send_message(&self, message: &ChannelMessage) -> ChannelResult<String> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".into()));
        }
        let base_url = self.server_url.as_ref()
            .ok_or_else(|| ChannelError::ConfigError("Server URL not set".into()))?;
        let token = self.token.as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("Token not set".into()))?;
        let client = self.client.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".into()))?;

        // Synology Chat incoming webhook
        let url = format!(
            "{}/webapi/entry.cgi?api=SYNO.Chat.External&method=incoming&version=2&token={}",
            base_url.trim_end_matches('/'), token
        );

        let payload = serde_json::json!({
            "text": message.content
        });

        let resp = client.post(&url)
            .form(&[("payload", serde_json::to_string(&payload).unwrap_or_default())])
            .send().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(ChannelError::MessageError(format!("Synology API error ({}): {}", status, body)));
        }

        Ok(format!("synology_{}", uuid::Uuid::new_v4()))
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>> {
        Ok(vec![])
    }

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()> {
        tracing::debug!("Synology Chat event: {:?}", event);
        Ok(())
    }

    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>> {
        Ok(Box::new(SynologySender { channel: Arc::new(RwLock::new(self.clone())) }))
    }

    fn parse_webhook(&self, payload: &serde_json::Value) -> Option<WebhookMessage> {
        // Synology Chat: /text, /user_id or /channel_id
        let text = payload.get("text")
            .and_then(|v| v.as_str())?;
        let chat_id = payload.get("user_id")
            .or_else(|| payload.get("channel_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("default");
        Some(WebhookMessage {
            text: text.to_string(),
            chat_id: chat_id.to_string(),
            is_group: false,
            has_mention: false,
            metadata: HashMap::new(),
        })
    }
}

struct SynologySender {
    channel: Arc<RwLock<SynologyChannel>>,
}

impl MessageSender for SynologySender {
    fn send<'a>(&'a self, content: &'a str, metadata: HashMap<String, String>) -> Pin<Box<dyn std::future::Future<Output = ChannelResult<String>> + Send + 'a>> {
        let channel = self.channel.clone();
        let content = content.to_string();
        Box::pin(async move {
            let msg = ChannelMessage {
                id: uuid::Uuid::new_v4().to_string(),
                channel: "synology".into(),
                sender: String::new(),
                content,
                timestamp: chrono::Utc::now().timestamp_millis(),
                metadata,
            };
            channel.read().await.send_message(&msg).await
        })
    }
}
