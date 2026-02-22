use crate::traits::*;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct SlackChannel {
    bot_token: Option<String>,
    client: Option<Client>,
    status: ChannelStatus,
    signing_secret: Option<String>,
    webhook_url: Option<String>,
    channel_ids: Vec<String>,
}

#[derive(Debug, Serialize)]
struct SlackMessage {
    channel: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    blocks: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thread_ts: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SlackResponse {
    ok: bool,
    #[serde(default)]
    ts: Option<String>,
    #[serde(default)]
    channel: Option<String>,
    error: Option<String>,
}

impl SlackChannel {
    pub fn new() -> Self {
        Self {
            bot_token: None,
            client: None,
            status: ChannelStatus::Disconnected,
            signing_secret: None,
            webhook_url: None,
            channel_ids: Vec::new(),
        }
    }

    pub fn with_config(mut self, bot_token: &str, signing_secret: Option<&str>) -> Self {
        self.bot_token = Some(bot_token.to_string());
        if let Some(secret) = signing_secret {
            self.signing_secret = Some(secret.to_string());
        }
        self
    }

    pub fn with_webhook(mut self, url: &str) -> Self {
        self.webhook_url = Some(url.to_string());
        self
    }

    pub fn add_channel(mut self, channel_id: &str) -> Self {
        self.channel_ids.push(channel_id.to_string());
        self
    }

    async fn send_api_request(
        &self,
        method: &str,
        body: Option<&serde_json::Value>,
    ) -> ChannelResult<serde_json::Value> {
        let token = self.bot_token.as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("Bot token not set".to_string()))?;

        let url = format!("https://slack.com/api/{}", method);

        let client = self.client.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".to_string()))?;

        let mut request = client.post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json; charset=utf-8");

        if let Some(body) = body {
            request = request.json(body);
        }

        let response = request.send().await
            .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;

        let slack_resp: SlackResponse = response.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        if slack_resp.ok {
            Ok(serde_json::json!({
                "ts": slack_resp.ts,
                "channel": slack_resp.channel
            }))
        } else {
            Err(ChannelError::MessageError(
                slack_resp.error.unwrap_or_else(|| "Unknown error".to_string())
            ))
        }
    }
}

impl Default for SlackChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Channel for SlackChannel {
    fn channel_type(&self) -> &str {
        "slack"
    }

    async fn connect(&mut self) -> ChannelResult<()> {
        if self.bot_token.is_none() {
            return Err(ChannelError::AuthenticationError(
                "Bot token required".to_string(),
            ));
        }

        self.status = ChannelStatus::Connecting;
        self.client = Some(Client::new());

        let response: serde_json::Value = self.send_api_request("auth.test", None).await?;

        tracing::info!("Slack bot connected: {:?}", response);

        self.status = ChannelStatus::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> ChannelResult<()> {
        self.client = None;
        self.status = ChannelStatus::Disconnected;
        tracing::info!("Slack channel disconnected");
        Ok(())
    }

    fn status(&self) -> ChannelStatus {
        self.status
    }

    async fn send_message(&self, message: &ChannelMessage) -> ChannelResult<String> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let channel_id = message.metadata.get("channel_id")
            .or_else(|| message.metadata.get("channel"))
            .or_else(|| message.metadata.get("recipient"))
            .cloned()
            .ok_or_else(|| ChannelError::MessageError("Channel ID not specified".to_string()))?;

        let slack_msg = SlackMessage {
            channel: channel_id.clone(),
            text: message.content.clone(),
            blocks: None,
            thread_ts: message.metadata.get("thread_ts").cloned(),
        };

        let response: serde_json::Value = self.send_api_request(
            "chat.postMessage",
            Some(&serde_json::to_value(&slack_msg).map_err(|e| ChannelError::MessageError(e.to_string()))?),
        ).await?;

        let ts = response.get("ts")
            .and_then(|t| t.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| ChannelError::MessageError("No timestamp returned".to_string()))?;

        Ok(format!("{}_{}", channel_id, ts))
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let response: serde_json::Value = self.send_api_request("users.list", None).await?;

        let members = response.get("members")
            .and_then(|m| m.as_array())
            .ok_or_else(|| ChannelError::MessageError("No members found".to_string()))?;

        let accounts: Vec<ChannelAccount> = members.iter()
            .filter_map(|m| {
                Some(ChannelAccount {
                    id: m.get("id")?.as_str()?.to_string(),
                    name: m.get("name")?.as_str()?.to_string(),
                    channel: "slack".to_string(),
                    avatar: m.get("profile")
                        .and_then(|p| p.get("image_72"))
                        .and_then(|i| i.as_str())
                        .map(|s| s.to_string()),
                    status: m.get("presence")
                        .and_then(|p| p.as_str())
                        .map(|s| s.to_string()),
                })
            })
            .collect();

        Ok(accounts)
    }

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()> {
        tracing::debug!("Received Slack event: {:?}", event);
        
        match event.event_type.as_str() {
            "message" => {
                tracing::info!("Received Slack message: {:?}", event.payload);
            }
            "reaction_added" => {
                tracing::info!("Slack reaction: {:?}", event.payload);
            }
            _ => {}
        }
        
        Ok(())
    }

    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>> {
        Ok(Box::new(SlackSender {
            channel: Arc::new(RwLock::new(self.clone())),
        }))
    }
}

impl Clone for SlackChannel {
    fn clone(&self) -> Self {
        Self {
            bot_token: self.bot_token.clone(),
            client: None,
            status: self.status,
            signing_secret: self.signing_secret.clone(),
            webhook_url: self.webhook_url.clone(),
            channel_ids: self.channel_ids.clone(),
        }
    }
}

struct SlackSender {
    channel: Arc<RwLock<SlackChannel>>,
}

impl MessageSender for SlackSender {
    fn send<'a>(&'a self, content: &'a str, metadata: HashMap<String, String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = ChannelResult<String>> + Send + 'a>> {
        let channel = self.channel.clone();
        let content = content.to_string();
        let metadata = metadata.clone();
        
        Box::pin(async move {
            let message = ChannelMessage {
                id: uuid::Uuid::new_v4().to_string(),
                channel: "slack".to_string(),
                sender: String::new(),
                content,
                timestamp: chrono::Utc::now().timestamp_millis(),
                metadata,
            };
            
            channel.read().await.send_message(&message).await
        })
    }
}
