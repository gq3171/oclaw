//! Zalo channel — Webhook + REST API
//!
//! Zalo OA (Official Account) uses webhook for incoming messages and REST API for sending.
//! Text limit: 2000 chars, media: 5MB.

use crate::traits::*;
use async_trait::async_trait;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct ZaloChannel {
    app_id: Option<String>,
    access_token: Option<String>,
    webhook_secret: Option<String>,
    status: ChannelStatus,
    client: Option<reqwest::Client>,
}

impl ZaloChannel {
    pub fn new() -> Self {
        Self {
            app_id: None,
            access_token: None,
            webhook_secret: None,
            status: ChannelStatus::Disconnected,
            client: None,
        }
    }

    pub fn with_config(mut self, app_id: &str, access_token: &str) -> Self {
        self.app_id = Some(app_id.into());
        self.access_token = Some(access_token.into());
        self
    }

    pub fn with_webhook_secret(mut self, secret: &str) -> Self {
        self.webhook_secret = Some(secret.into());
        self
    }
}

impl Default for ZaloChannel {
    fn default() -> Self { Self::new() }
}

impl Clone for ZaloChannel {
    fn clone(&self) -> Self {
        Self {
            app_id: self.app_id.clone(),
            access_token: self.access_token.clone(),
            webhook_secret: self.webhook_secret.clone(),
            status: self.status,
            client: None,
        }
    }
}

#[async_trait]
impl Channel for ZaloChannel {
    fn channel_type(&self) -> &str { "zalo" }

    async fn connect(&mut self) -> ChannelResult<()> {
        self.status = ChannelStatus::Connecting;
        let _token = self.access_token.as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("Access token not set".into()))?;
        self.client = Some(reqwest::Client::new());
        tracing::info!("Zalo channel connected");
        self.status = ChannelStatus::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> ChannelResult<()> {
        self.client = None;
        self.status = ChannelStatus::Disconnected;
        tracing::info!("Zalo channel disconnected");
        Ok(())
    }

    fn status(&self) -> ChannelStatus { self.status }

    async fn send_message(&self, message: &ChannelMessage) -> ChannelResult<String> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".into()));
        }
        let token = self.access_token.as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("No token".into()))?;
        let client = self.client.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".into()))?;

        let recipient_id = message.metadata.get("recipient_id")
            .ok_or_else(|| ChannelError::MessageError("recipient_id required in metadata".into()))?;

        // Zalo OA Send Message API
        let body = serde_json::json!({
            "recipient": { "user_id": recipient_id },
            "message": { "text": message.content }
        });

        let resp = client.post("https://openapi.zalo.me/v3.0/oa/message/cs")
            .header("access_token", token)
            .json(&body)
            .send().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        let json: serde_json::Value = resp.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        Ok(json["data"]["message_id"].as_str().unwrap_or("sent").to_string())
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>> {
        Ok(vec![])
    }

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()> {
        tracing::debug!("Zalo event: {:?}", event);
        Ok(())
    }

    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>> {
        Ok(Box::new(ZaloSender { channel: Arc::new(RwLock::new(self.clone())) }))
    }

    fn parse_webhook(&self, payload: &serde_json::Value) -> Option<WebhookMessage> {
        // Zalo OA: /message/text, /sender/id
        let text = payload.pointer("/message/text")
            .and_then(|v| v.as_str())?;
        let chat_id = payload.pointer("/sender/id")
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

struct ZaloSender {
    channel: Arc<RwLock<ZaloChannel>>,
}

impl MessageSender for ZaloSender {
    fn send<'a>(&'a self, content: &'a str, metadata: HashMap<String, String>) -> Pin<Box<dyn std::future::Future<Output = ChannelResult<String>> + Send + 'a>> {
        let channel = self.channel.clone();
        let content = content.to_string();
        Box::pin(async move {
            let msg = ChannelMessage {
                id: uuid::Uuid::new_v4().to_string(),
                channel: "zalo".into(),
                sender: String::new(),
                content,
                timestamp: chrono::Utc::now().timestamp_millis(),
                metadata,
            };
            channel.read().await.send_message(&msg).await
        })
    }
}
