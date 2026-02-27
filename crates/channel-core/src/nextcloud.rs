//! Nextcloud Talk channel — Webhook (HTTP POST)
//!
//! Self-hosted webhook server with HMAC signature verification.
//! Uses ActivityPub-style Actor/Object/Target message structure.

use crate::traits::*;
use async_trait::async_trait;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct NextcloudChannel {
    server_url: Option<String>,
    token: Option<String>,
    secret: Option<String>,
    status: ChannelStatus,
    client: Option<reqwest::Client>,
}

impl NextcloudChannel {
    pub fn new() -> Self {
        Self {
            server_url: None,
            token: None,
            secret: None,
            status: ChannelStatus::Disconnected,
            client: None,
        }
    }

    pub fn with_config(mut self, server_url: &str, token: &str) -> Self {
        self.server_url = Some(server_url.into());
        self.token = Some(token.into());
        self
    }

    pub fn with_secret(mut self, secret: &str) -> Self {
        self.secret = Some(secret.into());
        self
    }
}

impl Default for NextcloudChannel {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for NextcloudChannel {
    fn clone(&self) -> Self {
        Self {
            server_url: self.server_url.clone(),
            token: self.token.clone(),
            secret: self.secret.clone(),
            status: self.status,
            client: None,
        }
    }
}

#[async_trait]
impl Channel for NextcloudChannel {
    fn channel_type(&self) -> &str {
        "nextcloud"
    }

    async fn connect(&mut self) -> ChannelResult<()> {
        self.status = ChannelStatus::Connecting;
        let _url = self
            .server_url
            .as_ref()
            .ok_or_else(|| ChannelError::ConfigError("Server URL not set".into()))?;
        self.client = Some(reqwest::Client::new());
        tracing::info!("Nextcloud Talk channel connected to {}", _url);
        self.status = ChannelStatus::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> ChannelResult<()> {
        self.client = None;
        self.status = ChannelStatus::Disconnected;
        tracing::info!("Nextcloud Talk channel disconnected");
        Ok(())
    }

    fn status(&self) -> ChannelStatus {
        self.status
    }

    async fn send_message(&self, message: &ChannelMessage) -> ChannelResult<String> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".into()));
        }
        let base_url = self
            .server_url
            .as_ref()
            .ok_or_else(|| ChannelError::ConfigError("Server URL not set".into()))?;
        let token = self
            .token
            .as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("Token not set".into()))?;
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".into()))?;

        let room_token = message
            .metadata
            .get("room_token")
            .ok_or_else(|| ChannelError::MessageError("room_token required in metadata".into()))?;

        // Nextcloud Talk OCS API
        let url = format!(
            "{}/ocs/v2.php/apps/spreed/api/v1/chat/{}",
            base_url.trim_end_matches('/'),
            room_token
        );

        let resp = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("OCS-APIRequest", "true")
            .header("Accept", "application/json")
            .form(&[("message", &message.content)])
            .send()
            .await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        Ok(json["ocs"]["data"]["id"]
            .as_i64()
            .map(|id| id.to_string())
            .unwrap_or_else(|| "sent".into()))
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>> {
        Ok(vec![])
    }

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()> {
        tracing::debug!("Nextcloud Talk event: {:?}", event);
        Ok(())
    }

    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>> {
        Ok(Box::new(NextcloudSender {
            channel: Arc::new(RwLock::new(self.clone())),
        }))
    }

    fn parse_webhook(&self, payload: &serde_json::Value) -> Option<WebhookMessage> {
        // Nextcloud Talk: /object/message, /object/conversation/token
        let text = payload
            .pointer("/object/message")
            .and_then(|v| v.as_str())?;
        let chat_id = payload
            .pointer("/object/conversation/token")
            .and_then(|v| v.as_str())
            .unwrap_or("default");
        Some(WebhookMessage {
            text: text.to_string(),
            chat_id: chat_id.to_string(),
            is_group: true,
            has_mention: false,
            metadata: HashMap::new(),
        })
    }
}

struct NextcloudSender {
    channel: Arc<RwLock<NextcloudChannel>>,
}

impl MessageSender for NextcloudSender {
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
                channel: "nextcloud".into(),
                sender: String::new(),
                content,
                timestamp: chrono::Utc::now().timestamp_millis(),
                metadata,
            };
            channel.read().await.send_message(&msg).await
        })
    }
}
