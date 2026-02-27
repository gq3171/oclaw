use crate::traits::*;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct MattermostChannel {
    server_url: Option<String>,
    access_token: Option<String>,
    team_id: Option<String>,
    client: Option<Client>,
    status: ChannelStatus,
    channel_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct MattermostPost {
    channel_id: String,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    root_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_ids: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct MattermostResponse {
    id: Option<String>,
    #[serde(default)]
    create_at: Option<i64>,
    #[serde(default)]
    status_code: Option<u16>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    detailed_error: Option<String>,
}

impl MattermostChannel {
    pub fn new() -> Self {
        Self {
            server_url: None,
            access_token: None,
            team_id: None,
            client: None,
            status: ChannelStatus::Disconnected,
            channel_id: None,
        }
    }

    pub fn with_config(mut self, server_url: &str, access_token: &str) -> Self {
        self.server_url = Some(server_url.to_string());
        self.access_token = Some(access_token.to_string());
        self
    }

    pub fn with_team(mut self, team_id: &str) -> Self {
        self.team_id = Some(team_id.to_string());
        self
    }

    pub fn with_channel(mut self, channel_id: &str) -> Self {
        self.channel_id = Some(channel_id.to_string());
        self
    }

    async fn send_api_request(
        &self,
        method: &str,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> ChannelResult<serde_json::Value> {
        let server = self
            .server_url
            .as_ref()
            .ok_or_else(|| ChannelError::ConfigError("Server URL not set".to_string()))?;

        let url = format!("{}{}", server, path);

        let client = self
            .client
            .as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".to_string()))?;

        let http_method = reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| {
            ChannelError::ConfigError(format!("Invalid HTTP method '{}': {}", method, e))
        })?;
        let mut request = client.request(http_method, &url);

        if let Some(token) = &self.access_token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        if let Some(body) = body {
            request = request.json(body);
        }

        let response = request
            .send()
            .await
            .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;

        if response.status().as_u16() >= 400 {
            let mm_resp: MattermostResponse = response
                .json()
                .await
                .map_err(|e| ChannelError::MessageError(e.to_string()))?;
            return Err(ChannelError::MessageError(
                mm_resp
                    .message
                    .unwrap_or_else(|| "Unknown error".to_string()),
            ));
        }

        let mm_resp: MattermostResponse = response
            .json()
            .await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        Ok(serde_json::json!({
            "id": mm_resp.id,
            "create_at": mm_resp.create_at
        }))
    }
}

impl Default for MattermostChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Channel for MattermostChannel {
    fn channel_type(&self) -> &str {
        "mattermost"
    }

    async fn connect(&mut self) -> ChannelResult<()> {
        if self.server_url.is_none() || self.access_token.is_none() {
            return Err(ChannelError::AuthenticationError(
                "Server URL and access token required".to_string(),
            ));
        }

        self.status = ChannelStatus::Connecting;
        self.client = Some(Client::new());

        let _: serde_json::Value = self
            .send_api_request("GET", "/api/v4/users/me", None)
            .await?;

        tracing::info!("Mattermost channel connected");

        self.status = ChannelStatus::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> ChannelResult<()> {
        self.client = None;
        self.status = ChannelStatus::Disconnected;
        tracing::info!("Mattermost channel disconnected");
        Ok(())
    }

    fn status(&self) -> ChannelStatus {
        self.status
    }

    async fn send_message(&self, message: &ChannelMessage) -> ChannelResult<String> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let channel_id = message
            .metadata
            .get("channel_id")
            .or(self.channel_id.as_ref())
            .cloned()
            .ok_or_else(|| ChannelError::MessageError("Channel ID not specified".to_string()))?;

        let post = MattermostPost {
            channel_id: channel_id.clone(),
            message: message.content.clone(),
            root_id: message.metadata.get("root_id").cloned(),
            file_ids: None,
        };

        let response: serde_json::Value = self
            .send_api_request(
                "POST",
                "/api/v4/posts",
                Some(
                    &serde_json::to_value(&post)
                        .map_err(|e| ChannelError::MessageError(e.to_string()))?,
                ),
            )
            .await?;

        let post_id = response
            .get("id")
            .and_then(|i| i.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| ChannelError::MessageError("No post ID returned".to_string()))?;

        Ok(post_id)
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let response: serde_json::Value = self
            .send_api_request("GET", "/api/v4/users/me", None)
            .await?;

        let account = ChannelAccount {
            id: response
                .get("id")
                .and_then(|i| i.as_str())
                .unwrap_or("unknown")
                .to_string(),
            name: response
                .get("username")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string(),
            channel: "mattermost".to_string(),
            avatar: response
                .get("last_picture_update")
                .and_then(|p| p.as_i64())
                .map(|_| "avatar".to_string()),
            status: Some("active".to_string()),
        };

        Ok(vec![account])
    }

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()> {
        tracing::debug!("Received Mattermost event: {:?}", event);

        if event.event_type.as_str() == "posted" {
            tracing::info!("Received Mattermost message: {:?}", event.payload);
        }

        Ok(())
    }

    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>> {
        Ok(Box::new(MattermostSender {
            channel: Arc::new(RwLock::new(self.clone())),
        }))
    }

    fn parse_webhook(&self, payload: &serde_json::Value) -> Option<WebhookMessage> {
        // Mattermost outgoing webhook: /text, /channel_id
        let text = payload.get("text").and_then(|v| v.as_str())?;
        let chat_id = payload
            .get("channel_id")
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

impl Clone for MattermostChannel {
    fn clone(&self) -> Self {
        Self {
            server_url: self.server_url.clone(),
            access_token: self.access_token.clone(),
            team_id: self.team_id.clone(),
            client: self.client.clone(),
            status: self.status,
            channel_id: self.channel_id.clone(),
        }
    }
}

struct MattermostSender {
    channel: Arc<RwLock<MattermostChannel>>,
}

impl MessageSender for MattermostSender {
    fn send<'a>(
        &'a self,
        content: &'a str,
        metadata: HashMap<String, String>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ChannelResult<String>> + Send + 'a>>
    {
        let channel = self.channel.clone();
        let content = content.to_string();
        let metadata = metadata.clone();

        Box::pin(async move {
            let message = ChannelMessage {
                id: uuid::Uuid::new_v4().to_string(),
                channel: "mattermost".to_string(),
                sender: String::new(),
                content,
                timestamp: chrono::Utc::now().timestamp_millis(),
                metadata,
            };

            channel.read().await.send_message(&message).await
        })
    }
}
