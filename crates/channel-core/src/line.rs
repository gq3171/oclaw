use crate::traits::*;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct LineChannel {
    channel_access_token: Option<String>,
    channel_secret: Option<String>,
    client: Option<Client>,
    status: ChannelStatus,
    user_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct LineSendMessage {
    to: String,
    messages: Vec<LineMessageContent>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum LineMessageContent {
    Text { text: String },
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct LineResponse {
    #[serde(default)]
    sent: Option<bool>,
    #[serde(default)]
    error: Option<LineError>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct LineError {
    message: Option<String>,
    details: Option<Vec<serde_json::Value>>,
}

impl LineChannel {
    pub fn new() -> Self {
        Self {
            channel_access_token: None,
            channel_secret: None,
            client: None,
            status: ChannelStatus::Disconnected,
            user_id: None,
        }
    }

    pub fn with_config(mut self, access_token: &str, channel_secret: Option<&str>) -> Self {
        self.channel_access_token = Some(access_token.to_string());
        if let Some(secret) = channel_secret {
            self.channel_secret = Some(secret.to_string());
        }
        self
    }

    pub fn with_user_id(mut self, user_id: &str) -> Self {
        self.user_id = Some(user_id.to_string());
        self
    }

    async fn send_api_request(
        &self,
        body: &serde_json::Value,
    ) -> ChannelResult<serde_json::Value> {
        let token = self.channel_access_token.as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("Channel access token not set".to_string()))?;

        let client = self.client.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".to_string()))?;

        let response = client
            .post("https://api.line.me/v2/bot/message/push")
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json")
            .json(body)
            .send()
            .await
            .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;

        if response.status().is_success() {
            Ok(serde_json::json!({ "sent": true }))
        } else {
            let line_resp: LineResponse = response.json().await
                .map_err(|e| ChannelError::MessageError(e.to_string()))?;
            Err(ChannelError::MessageError(
                line_resp.error.and_then(|e| e.message).unwrap_or_else(|| "Unknown error".to_string())
            ))
        }
    }
}

impl Default for LineChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Channel for LineChannel {
    fn channel_type(&self) -> &str {
        "line"
    }

    async fn connect(&mut self) -> ChannelResult<()> {
        if self.channel_access_token.is_none() {
            return Err(ChannelError::AuthenticationError(
                "Channel access token required".to_string(),
            ));
        }

        self.status = ChannelStatus::Connecting;
        self.client = Some(Client::new());

        tracing::info!("LINE channel connecting");

        self.status = ChannelStatus::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> ChannelResult<()> {
        self.client = None;
        self.status = ChannelStatus::Disconnected;
        tracing::info!("LINE channel disconnected");
        Ok(())
    }

    fn status(&self) -> ChannelStatus {
        self.status
    }

    async fn send_message(&self, message: &ChannelMessage) -> ChannelResult<String> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let user_id = message.metadata.get("user_id")
            .or(self.user_id.as_ref())
            .cloned()
            .ok_or_else(|| ChannelError::MessageError("User ID not specified".to_string()))?;

        let line_msg = LineSendMessage {
            to: user_id.clone(),
            messages: vec![LineMessageContent::Text {
                text: message.content.clone(),
            }],
        };

        self.send_api_request(&serde_json::to_value(&line_msg).map_err(|e| ChannelError::MessageError(e.to_string()))?).await?;

        Ok(format!("{}_{}", user_id, uuid::Uuid::new_v4()))
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let account = ChannelAccount {
            id: self.user_id.clone().unwrap_or_default(),
            name: "LINE User".to_string(),
            channel: "line".to_string(),
            avatar: None,
            status: Some("active".to_string()),
        };

        Ok(vec![account])
    }

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()> {
        tracing::debug!("Received LINE event: {:?}", event);
        
        match event.event_type.as_str() {
            "message" => {
                tracing::info!("Received LINE message: {:?}", event.payload);
            }
            "follow" => {
                tracing::info!("LINE user followed: {:?}", event.payload);
            }
            "unfollow" => {
                tracing::info!("LINE user unfollowed: {:?}", event.payload);
            }
            _ => {}
        }
        
        Ok(())
    }

    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>> {
        Ok(Box::new(LineSender {
            channel: Arc::new(RwLock::new(self.clone())),
        }))
    }
}

impl Clone for LineChannel {
    fn clone(&self) -> Self {
        Self {
            channel_access_token: self.channel_access_token.clone(),
            channel_secret: self.channel_secret.clone(),
            client: self.client.clone(),
            status: self.status,
            user_id: self.user_id.clone(),
        }
    }
}

struct LineSender {
    channel: Arc<RwLock<LineChannel>>,
}

impl MessageSender for LineSender {
    fn send<'a>(&'a self, content: &'a str, metadata: HashMap<String, String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = ChannelResult<String>> + Send + 'a>> {
        let channel = self.channel.clone();
        let content = content.to_string();
        let metadata = metadata.clone();
        
        Box::pin(async move {
            let message = ChannelMessage {
                id: uuid::Uuid::new_v4().to_string(),
                channel: "line".to_string(),
                sender: String::new(),
                content,
                timestamp: chrono::Utc::now().timestamp_millis(),
                metadata,
            };
            
            channel.read().await.send_message(&message).await
        })
    }
}
