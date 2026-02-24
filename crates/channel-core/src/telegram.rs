use crate::traits::*;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct TelegramChannel {
    bot_token: Option<String>,
    api_url: Option<String>,
    client: Option<Client>,
    status: ChannelStatus,
    webhook_url: Option<String>,
}

#[derive(Debug, Serialize)]
struct TelegramSendMessage {
    chat_id: String,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_to_message_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reply_markup: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct TelegramSetWebhook {
    url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    certificate: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_connections: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    allowed_updates: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct TelegramResponse {
    ok: bool,
    result: Option<serde_json::Value>,
    #[serde(default)]
    _error_code: Option<i32>,
    description: Option<String>,
}

impl TelegramChannel {
    pub fn new() -> Self {
        Self {
            bot_token: None,
            api_url: None,
            client: None,
            status: ChannelStatus::Disconnected,
            webhook_url: None,
        }
    }

    pub fn with_bot_token(mut self, token: &str) -> Self {
        self.bot_token = Some(token.to_string());
        self.api_url = Some(format!("https://api.telegram.org/bot{}", token));
        self
    }

    pub fn with_webhook(mut self, url: &str) -> Self {
        self.webhook_url = Some(url.to_string());
        self
    }

    pub async fn send_photo(&self, chat_id: &str, photo: &str, caption: Option<&str>, reply_to: Option<i64>) -> ChannelResult<serde_json::Value> {
        let mut body = serde_json::json!({"chat_id": chat_id, "photo": photo});
        if let Some(c) = caption { body["caption"] = c.into(); body["parse_mode"] = "Markdown".into(); }
        if let Some(r) = reply_to { body["reply_to_message_id"] = r.into(); }
        self.send_api_request("sendPhoto", Some(&body)).await
    }

    pub async fn send_audio(&self, chat_id: &str, audio: &str, caption: Option<&str>, reply_to: Option<i64>) -> ChannelResult<serde_json::Value> {
        let mut body = serde_json::json!({"chat_id": chat_id, "audio": audio});
        if let Some(c) = caption { body["caption"] = c.into(); body["parse_mode"] = "Markdown".into(); }
        if let Some(r) = reply_to { body["reply_to_message_id"] = r.into(); }
        self.send_api_request("sendAudio", Some(&body)).await
    }

    pub async fn send_voice(&self, chat_id: &str, voice: &str, caption: Option<&str>, reply_to: Option<i64>) -> ChannelResult<serde_json::Value> {
        let mut body = serde_json::json!({"chat_id": chat_id, "voice": voice});
        if let Some(c) = caption { body["caption"] = c.into(); }
        if let Some(r) = reply_to { body["reply_to_message_id"] = r.into(); }
        self.send_api_request("sendVoice", Some(&body)).await
    }

    pub async fn send_video(&self, chat_id: &str, video: &str, caption: Option<&str>, reply_to: Option<i64>) -> ChannelResult<serde_json::Value> {
        let mut body = serde_json::json!({"chat_id": chat_id, "video": video});
        if let Some(c) = caption { body["caption"] = c.into(); body["parse_mode"] = "Markdown".into(); }
        if let Some(r) = reply_to { body["reply_to_message_id"] = r.into(); }
        self.send_api_request("sendVideo", Some(&body)).await
    }

    pub async fn send_document(&self, chat_id: &str, document: &str, caption: Option<&str>, reply_to: Option<i64>) -> ChannelResult<serde_json::Value> {
        let mut body = serde_json::json!({"chat_id": chat_id, "document": document});
        if let Some(c) = caption { body["caption"] = c.into(); body["parse_mode"] = "Markdown".into(); }
        if let Some(r) = reply_to { body["reply_to_message_id"] = r.into(); }
        self.send_api_request("sendDocument", Some(&body)).await
    }

    pub async fn send_sticker(&self, chat_id: &str, sticker: &str, reply_to: Option<i64>) -> ChannelResult<serde_json::Value> {
        let mut body = serde_json::json!({"chat_id": chat_id, "sticker": sticker});
        if let Some(r) = reply_to { body["reply_to_message_id"] = r.into(); }
        self.send_api_request("sendSticker", Some(&body)).await
    }

    pub async fn send_chat_action(&self, chat_id: &str, action: &str) -> ChannelResult<serde_json::Value> {
        self.send_api_request("sendChatAction", Some(&serde_json::json!({"chat_id": chat_id, "action": action}))).await
    }

    pub async fn get_file(&self, file_id: &str) -> ChannelResult<String> {
        let resp = self.send_api_request("getFile", Some(&serde_json::json!({"file_id": file_id}))).await?;
        let path = resp.pointer("/file_path").and_then(|p| p.as_str()).unwrap_or_default();
        let token = self.bot_token.as_deref().unwrap_or_default();
        Ok(format!("https://api.telegram.org/file/bot{}/{}", token, path))
    }

    async fn send_api_request(
        &self,
        method: &str,
        body: Option<&serde_json::Value>,
    ) -> ChannelResult<serde_json::Value> {
        let token = self.bot_token.as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("Bot token not set".to_string()))?;

        let url = format!("https://api.telegram.org/bot{}/{}", token, method);

        let client = self.client.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".to_string()))?;

        let mut request = client.post(&url);

        if let Some(body) = body {
            request = request.json(body);
        }

        let response = request.send().await
            .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;

        let telegram_resp: TelegramResponse = response.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        if telegram_resp.ok {
            telegram_resp.result
                .ok_or_else(|| ChannelError::MessageError("No result in response".to_string()))
        } else {
            let error_msg = telegram_resp.description
                .unwrap_or_else(|| "Unknown error".to_string());
            Err(ChannelError::MessageError(error_msg))
        }
    }
}

impl Default for TelegramChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    fn channel_type(&self) -> &str {
        "telegram"
    }

    async fn connect(&mut self) -> ChannelResult<()> {
        if self.bot_token.is_none() {
            return Err(ChannelError::AuthenticationError(
                "Bot token required".to_string(),
            ));
        }

        self.status = ChannelStatus::Connecting;

        self.client = Some(Client::new());

        let me_response: serde_json::Value = self.send_api_request("getMe", None).await?;
        
        tracing::info!("Telegram bot connected: {:?}", me_response);

        if let Some(webhook_url) = &self.webhook_url {
            let webhook = TelegramSetWebhook {
                url: webhook_url.clone(),
                certificate: None,
                max_connections: Some(100),
                allowed_updates: Some(vec![
                    "message".to_string(),
                    "edited_message".to_string(),
                    "callback_query".to_string(),
                ]),
            };
            
            self.send_api_request(
                "setWebhook",
                Some(&serde_json::to_value(&webhook).map_err(|e| ChannelError::MessageError(e.to_string()))?),
            ).await?;
            
            tracing::info!("Telegram webhook set to: {}", webhook_url);
        }

        self.status = ChannelStatus::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> ChannelResult<()> {
        if let Some(webhook_url) = &self.webhook_url {
            let _: serde_json::Value = self.send_api_request(
                "deleteWebhook",
                Some(&serde_json::json!({ "url": webhook_url })),
            ).await?;
        }
        
        self.client = None;
        self.status = ChannelStatus::Disconnected;
        tracing::info!("Telegram channel disconnected");
        Ok(())
    }

    fn status(&self) -> ChannelStatus {
        self.status
    }

    async fn send_message(&self, message: &ChannelMessage) -> ChannelResult<String> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let chat_id = message.metadata.get("chat_id")
            .or_else(|| message.metadata.get("recipient"))
            .cloned()
            .ok_or_else(|| ChannelError::MessageError("Chat ID not specified".to_string()))?;

        let reply_to = message.metadata.get("reply_to_message_id")
            .and_then(|v| v.parse::<i64>().ok());

        // Split long messages (Telegram limit: 4096 chars)
        let chunks = split_message(&message.content, 4096);
        let mut last_msg_id = String::new();

        for chunk in &chunks {
            let telegram_msg = TelegramSendMessage {
                chat_id: chat_id.clone(),
                text: chunk.clone(),
                parse_mode: Some("Markdown".to_string()),
                reply_to_message_id: if last_msg_id.is_empty() { reply_to } else { None },
                reply_markup: None,
            };

            let response = self.send_api_request(
                "sendMessage",
                Some(&serde_json::to_value(&telegram_msg)
                    .map_err(|e| ChannelError::MessageError(e.to_string()))?),
            ).await?;

            last_msg_id = response.get("message_id")
                .and_then(|m| m.as_i64())
                .map(|id| id.to_string())
                .unwrap_or_default();
        }

        Ok(format!("{}_{}", chat_id, last_msg_id))
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let response: serde_json::Value = self.send_api_request("getMe", None).await?;

        let account = ChannelAccount {
            id: response.get("id")
                .and_then(|id| id.as_i64())
                .map(|id| id.to_string())
                .unwrap_or_default(),
            name: response.get("username")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string(),
            channel: "telegram".to_string(),
            avatar: None,
            status: Some("active".to_string()),
        };

        Ok(vec![account])
    }

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()> {
        tracing::debug!("Received Telegram event: {:?}", event);
        
        match event.event_type.as_str() {
            "message" => {
                tracing::info!("Received Telegram message: {:?}", event.payload);
            }
            "callback_query" => {
                tracing::info!("Telegram callback query: {:?}", event.payload);
            }
            _ => {}
        }
        
        Ok(())
    }

    async fn send_typing_status(&self, user_id: &str, status: TypingStatus) -> ChannelResult<()> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let chat_id = user_id;
        
        let method = match status {
            TypingStatus::Started => "sendChatAction",
            TypingStatus::Stopped => return Ok(()),
        };

        let body = serde_json::json!({
            "chat_id": chat_id,
            "action": "typing"
        });

        let _: serde_json::Value = self.send_api_request(
            method,
            Some(&body),
        ).await?;

        Ok(())
    }

    async fn send_message_with_attachments(&self, message: &ChannelMessageWithAttachments) -> ChannelResult<String> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }
        let chat_id = message.message.metadata.get("chat_id")
            .or_else(|| message.message.metadata.get("recipient"))
            .cloned()
            .ok_or_else(|| ChannelError::MessageError("Chat ID not specified".to_string()))?;
        let reply_to = message.message.metadata.get("reply_to_message_id")
            .and_then(|v| v.parse::<i64>().ok());
        let caption = if message.message.content.is_empty() { None } else { Some(message.message.content.as_str()) };

        let mut last_id = String::new();
        for att in &message.attachments {
            let mime = att.mime_type.as_deref().unwrap_or("");
            let result = if mime.starts_with("image/") {
                self.send_photo(&chat_id, &att.url, caption, reply_to).await
            } else if mime.starts_with("video/") {
                self.send_video(&chat_id, &att.url, caption, reply_to).await
            } else if mime.starts_with("audio/") || mime == "application/ogg" {
                self.send_audio(&chat_id, &att.url, caption, reply_to).await
            } else {
                self.send_document(&chat_id, &att.url, caption, reply_to).await
            };
            if let Ok(resp) = result {
                last_id = resp.get("message_id").and_then(|m| m.as_i64())
                    .map(|id| id.to_string()).unwrap_or_default();
            }
        }
        // Send text if no attachments or text remains
        if message.attachments.is_empty() && !message.message.content.is_empty() {
            return self.send_message(&message.message).await;
        }
        Ok(format!("{}_{}", chat_id, last_id))
    }

    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>> {
        Ok(Box::new(TelegramSender {
            channel: Arc::new(RwLock::new(self.clone())),
        }))
    }
}

impl Clone for TelegramChannel {
    fn clone(&self) -> Self {
        Self {
            bot_token: self.bot_token.clone(),
            api_url: self.api_url.clone(),
            client: None,
            status: self.status,
            webhook_url: self.webhook_url.clone(),
        }
    }
}

struct TelegramSender {
    channel: Arc<RwLock<TelegramChannel>>,
}

impl MessageSender for TelegramSender {
    fn send<'a>(&'a self, content: &'a str, metadata: HashMap<String, String>) -> Pin<Box<dyn std::future::Future<Output = ChannelResult<String>> + Send + 'a>> {
        let channel = self.channel.clone();
        let content = content.to_string();
        let metadata = metadata.clone();
        
        Box::pin(async move {
            let message = ChannelMessage {
                id: uuid::Uuid::new_v4().to_string(),
                channel: "telegram".to_string(),
                sender: String::new(),
                content,
                timestamp: chrono::Utc::now().timestamp_millis(),
                metadata,
            };
            
            channel.read().await.send_message(&message).await
        })
    }
}

fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            chunks.push(remaining.to_string());
            break;
        }
        // Try to split at last newline within limit
        let split_at = remaining[..max_len]
            .rfind('\n')
            .unwrap_or(max_len);
        chunks.push(remaining[..split_at].to_string());
        remaining = remaining[split_at..].trim_start_matches('\n');
    }
    chunks
}
