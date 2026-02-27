use crate::traits::*;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct GoogleChatChannel {
    space_name: Option<String>,
    service_account_json: Option<String>,
    client: Option<Client>,
    status: ChannelStatus,
}

#[derive(Debug, Serialize)]
struct GoogleChatMessage {
    #[serde(rename = "text")]
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    thread: Option<Thread>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cards: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Serialize)]
struct Thread {
    #[serde(rename = "threadKey")]
    thread_key: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GoogleChatResponse {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    error: Option<GoogleChatError>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GoogleChatError {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    status: Option<String>,
}

impl GoogleChatChannel {
    pub fn new() -> Self {
        Self {
            space_name: None,
            service_account_json: None,
            client: None,
            status: ChannelStatus::Disconnected,
        }
    }

    pub fn with_space(mut self, space_name: &str) -> Self {
        self.space_name = Some(space_name.to_string());
        self
    }

    pub fn with_service_account(mut self, json_path: &str) -> Self {
        self.service_account_json = Some(json_path.to_string());
        self
    }

    async fn send_api_request(&self, body: &serde_json::Value) -> ChannelResult<String> {
        let space = self
            .space_name
            .as_ref()
            .ok_or_else(|| ChannelError::ConfigError("Space name not configured".to_string()))?;

        let url = format!("https://chat.googleapis.com/v1/{}/messages", space);

        let client = self
            .client
            .as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".to_string()))?;

        let response = client
            .post(&url)
            .json(body)
            .send()
            .await
            .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;

        let chat_resp: GoogleChatResponse = response
            .json()
            .await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        if let Some(error) = chat_resp.error {
            return Err(ChannelError::MessageError(
                error
                    .message
                    .unwrap_or_else(|| error.status.unwrap_or_default()),
            ));
        }

        Ok(chat_resp.name.unwrap_or_else(|| "unknown".to_string()))
    }
}

impl Default for GoogleChatChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Channel for GoogleChatChannel {
    fn channel_type(&self) -> &str {
        "google_chat"
    }

    async fn connect(&mut self) -> ChannelResult<()> {
        if self.space_name.is_none() {
            return Err(ChannelError::AuthenticationError(
                "Space name required".to_string(),
            ));
        }

        self.status = ChannelStatus::Connecting;
        self.client = Some(Client::new());

        tracing::info!(
            "Google Chat channel connecting to space: {:?}",
            self.space_name
        );

        self.status = ChannelStatus::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> ChannelResult<()> {
        self.client = None;
        self.status = ChannelStatus::Disconnected;
        tracing::info!("Google Chat channel disconnected");
        Ok(())
    }

    fn status(&self) -> ChannelStatus {
        self.status
    }

    async fn send_message(&self, message: &ChannelMessage) -> ChannelResult<String> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let mut chat_msg = GoogleChatMessage {
            text: message.content.clone(),
            thread: None,
            cards: None,
        };

        if let Some(thread_key) = message.metadata.get("thread_key") {
            chat_msg.thread = Some(Thread {
                thread_key: thread_key.clone(),
            });
        }

        self.send_api_request(
            &serde_json::to_value(&chat_msg)
                .map_err(|e| ChannelError::MessageError(e.to_string()))?,
        )
        .await
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let account = ChannelAccount {
            id: self.space_name.clone().unwrap_or_default(),
            name: "Google Chat Bot".to_string(),
            channel: "google_chat".to_string(),
            avatar: None,
            status: Some("active".to_string()),
        };

        Ok(vec![account])
    }

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()> {
        tracing::debug!("Received Google Chat event: {:?}", event);

        if event.event_type.as_str() == "MESSAGE" {
            tracing::info!("Received Google Chat message: {:?}", event.payload);
        }

        Ok(())
    }

    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>> {
        Ok(Box::new(GoogleChatSender {
            channel: Arc::new(RwLock::new(self.clone())),
        }))
    }

    fn parse_webhook(&self, payload: &serde_json::Value) -> Option<WebhookMessage> {
        // Google Chat: /message/text, /space/name
        let text = payload.pointer("/message/text").and_then(|v| v.as_str())?;
        let space = payload
            .pointer("/space/name")
            .or_else(|| payload.pointer("/message/space/name"))
            .and_then(|v| v.as_str())
            .unwrap_or("default");
        let space_type = payload
            .pointer("/space/type")
            .and_then(|v| v.as_str())
            .unwrap_or("DM");
        Some(WebhookMessage {
            text: text.to_string(),
            chat_id: space.to_string(),
            is_group: space_type == "ROOM" || space_type == "SPACE",
            has_mention: false,
            metadata: HashMap::new(),
        })
    }
}

impl Clone for GoogleChatChannel {
    fn clone(&self) -> Self {
        Self {
            space_name: self.space_name.clone(),
            service_account_json: self.service_account_json.clone(),
            client: self.client.clone(),
            status: self.status,
        }
    }
}

struct GoogleChatSender {
    channel: Arc<RwLock<GoogleChatChannel>>,
}

impl MessageSender for GoogleChatSender {
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
                channel: "google_chat".to_string(),
                sender: String::new(),
                content,
                timestamp: chrono::Utc::now().timestamp_millis(),
                metadata,
            };

            channel.read().await.send_message(&message).await
        })
    }
}
