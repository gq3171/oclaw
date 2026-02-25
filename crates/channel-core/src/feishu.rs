use crate::traits::*;
use crate::types::*;
use async_trait::async_trait;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

const BASE_URL: &str = "https://open.feishu.cn/open-apis";

pub struct FeishuChannel {
    app_id: Option<String>,
    app_secret: Option<String>,
    token: Arc<tokio::sync::RwLock<Option<String>>>,
    client: Option<Client>,
    status: ChannelStatus,
}

impl FeishuChannel {
    pub fn new() -> Self {
        Self {
            app_id: None, app_secret: None,
            token: Arc::new(tokio::sync::RwLock::new(None)),
            client: None, status: ChannelStatus::Disconnected,
        }
    }

    pub fn with_config(mut self, app_id: &str, app_secret: &str) -> Self {
        self.app_id = Some(app_id.into());
        self.app_secret = Some(app_secret.into());
        self
    }

    /// Obtain or refresh tenant_access_token.
    pub async fn refresh_token(&self) -> ChannelResult<String> {
        let client = self.client.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".into()))?;
        let body = serde_json::json!({
            "app_id": self.app_id.as_deref().unwrap_or(""),
            "app_secret": self.app_secret.as_deref().unwrap_or(""),
        });
        let resp = client.post(format!("{}/auth/v3/tenant_access_token/internal", BASE_URL))
            .json(&body).send().await
            .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;
        let json: serde_json::Value = resp.json().await
            .map_err(|e| ChannelError::AuthenticationError(e.to_string()))?;
        let new_token = json["tenant_access_token"].as_str()
            .ok_or_else(|| ChannelError::AuthenticationError(format!("Token error: {}", json)))?
            .to_string();
        *self.token.write().await = Some(new_token.clone());
        tracing::info!("Feishu token refreshed");
        Ok(new_token)
    }

    async fn get_token(&self) -> ChannelResult<String> {
        self.token.read().await.clone()
            .ok_or_else(|| ChannelError::AuthenticationError("No token".into()))
    }

    /// Check if a response indicates token expiry (99991663) and auto-refresh + retry.
    fn is_token_expired(json: &serde_json::Value) -> bool {
        json.get("code").and_then(|c| c.as_i64()) == Some(99991663)
    }

    fn client(&self) -> ChannelResult<&Client> {
        self.client.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".into()))
    }

    /// Send a message (text, post, image, interactive card, file, etc.).
    /// `msg_type`: "text", "post", "image", "interactive", "file", "audio", "media", "sticker"
    /// `content`: JSON string matching the msg_type schema.
    /// `receive_id_type`: "open_id", "user_id", "union_id", "email", "chat_id"
    pub async fn send_raw(
        &self, receive_id: &str, receive_id_type: &str, msg_type: &str, content: &str,
    ) -> ChannelResult<serde_json::Value> {
        let url = format!("{}/im/v1/messages?receive_id_type={}", BASE_URL, receive_id_type);
        let body = serde_json::json!({
            "receive_id": receive_id,
            "msg_type": msg_type,
            "content": content,
        });
        let token = self.get_token().await?;
        let resp = self.client()?.post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body).send().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        let json: serde_json::Value = resp.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        if Self::is_token_expired(&json) {
            tracing::warn!("Feishu token expired, refreshing and retrying send_raw");
            let new_token = self.refresh_token().await?;
            let resp = self.client()?.post(&url)
                .header("Authorization", format!("Bearer {}", new_token))
                .json(&body).send().await
                .map_err(|e| ChannelError::MessageError(e.to_string()))?;
            return resp.json().await.map_err(|e| ChannelError::MessageError(e.to_string()));
        }
        Ok(json)
    }

    /// Reply to a specific message.
    pub async fn reply_raw(
        &self, message_id: &str, msg_type: &str, content: &str,
    ) -> ChannelResult<serde_json::Value> {
        let url = format!("{}/im/v1/messages/{}/reply", BASE_URL, message_id);
        let body = serde_json::json!({ "msg_type": msg_type, "content": content });
        let token = self.get_token().await?;
        let resp = self.client()?.post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body).send().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        let json: serde_json::Value = resp.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        if Self::is_token_expired(&json) {
            tracing::warn!("Feishu token expired, refreshing and retrying reply_raw");
            let new_token = self.refresh_token().await?;
            let resp = self.client()?.post(&url)
                .header("Authorization", format!("Bearer {}", new_token))
                .json(&body).send().await
                .map_err(|e| ChannelError::MessageError(e.to_string()))?;
            return resp.json().await.map_err(|e| ChannelError::MessageError(e.to_string()));
        }
        Ok(json)
    }

    /// Upload an image and return the image_key.
    pub async fn upload_image(&self, data: Vec<u8>, filename: &str) -> ChannelResult<String> {
        let url = format!("{}/im/v1/images", BASE_URL);
        let token = self.get_token().await?;
        let part = reqwest::multipart::Part::bytes(data.clone())
            .file_name(filename.to_string())
            .mime_str("application/octet-stream").unwrap();
        let form = reqwest::multipart::Form::new()
            .text("image_type", "message")
            .part("image", part);
        let resp = self.client()?.post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .multipart(form).send().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        let json: serde_json::Value = resp.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        if Self::is_token_expired(&json) {
            tracing::warn!("Feishu token expired, refreshing and retrying upload_image");
            let new_token = self.refresh_token().await?;
            let part = reqwest::multipart::Part::bytes(data)
                .file_name(filename.to_string())
                .mime_str("application/octet-stream").unwrap();
            let form = reqwest::multipart::Form::new()
                .text("image_type", "message")
                .part("image", part);
            let resp = self.client()?.post(&url)
                .header("Authorization", format!("Bearer {}", new_token))
                .multipart(form).send().await
                .map_err(|e| ChannelError::MessageError(e.to_string()))?;
            let json: serde_json::Value = resp.json().await
                .map_err(|e| ChannelError::MessageError(e.to_string()))?;
            return json["data"]["image_key"].as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| ChannelError::MessageError(format!("Upload failed: {}", json)));
        }
        json["data"]["image_key"].as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| ChannelError::MessageError(format!("Upload failed: {}", json)))
    }

    /// Upload a file and return the file_key.
    pub async fn upload_file(
        &self, data: Vec<u8>, filename: &str, file_type: &str,
    ) -> ChannelResult<String> {
        let url = format!("{}/im/v1/files", BASE_URL);
        let token = self.get_token().await?;
        let part = reqwest::multipart::Part::bytes(data.clone())
            .file_name(filename.to_string())
            .mime_str("application/octet-stream").unwrap();
        let form = reqwest::multipart::Form::new()
            .text("file_type", file_type.to_string())
            .text("file_name", filename.to_string())
            .part("file", part);
        let resp = self.client()?.post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .multipart(form).send().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        let json: serde_json::Value = resp.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        if Self::is_token_expired(&json) {
            tracing::warn!("Feishu token expired, refreshing and retrying upload_file");
            let new_token = self.refresh_token().await?;
            let part = reqwest::multipart::Part::bytes(data)
                .file_name(filename.to_string())
                .mime_str("application/octet-stream").unwrap();
            let form = reqwest::multipart::Form::new()
                .text("file_type", file_type.to_string())
                .text("file_name", filename.to_string())
                .part("file", part);
            let resp = self.client()?.post(&url)
                .header("Authorization", format!("Bearer {}", new_token))
                .multipart(form).send().await
                .map_err(|e| ChannelError::MessageError(e.to_string()))?;
            let json: serde_json::Value = resp.json().await
                .map_err(|e| ChannelError::MessageError(e.to_string()))?;
            return json["data"]["file_key"].as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| ChannelError::MessageError(format!("Upload failed: {}", json)));
        }
        json["data"]["file_key"].as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| ChannelError::MessageError(format!("Upload failed: {}", json)))
    }

    /// Add reaction (emoji) to a message.
    pub async fn add_reaction(
        &self, message_id: &str, emoji_type: &str,
    ) -> ChannelResult<serde_json::Value> {
        let url = format!("{}/im/v1/messages/{}/reactions", BASE_URL, message_id);
        let body = serde_json::json!({ "reaction_type": { "emoji_type": emoji_type } });
        let token = self.get_token().await?;
        let resp = self.client()?.post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body).send().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        let json: serde_json::Value = resp.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        if Self::is_token_expired(&json) {
            let new_token = self.refresh_token().await?;
            let resp = self.client()?.post(&url)
                .header("Authorization", format!("Bearer {}", new_token))
                .json(&body).send().await
                .map_err(|e| ChannelError::MessageError(e.to_string()))?;
            return resp.json().await.map_err(|e| ChannelError::MessageError(e.to_string()));
        }
        Ok(json)
    }

    /// Get message read status.
    pub async fn get_read_users(&self, message_id: &str) -> ChannelResult<serde_json::Value> {
        let url = format!("{}/im/v1/messages/{}/read_users", BASE_URL, message_id);
        let token = self.get_token().await?;
        let resp = self.client()?.get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        let json: serde_json::Value = resp.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        if Self::is_token_expired(&json) {
            let new_token = self.refresh_token().await?;
            let resp = self.client()?.get(&url)
                .header("Authorization", format!("Bearer {}", new_token))
                .send().await
                .map_err(|e| ChannelError::MessageError(e.to_string()))?;
            return resp.json().await.map_err(|e| ChannelError::MessageError(e.to_string()));
        }
        Ok(json)
    }

    /// Get chat (group) info.
    pub async fn get_chat(&self, chat_id: &str) -> ChannelResult<serde_json::Value> {
        let url = format!("{}/im/v1/chats/{}", BASE_URL, chat_id);
        let token = self.get_token().await?;
        let resp = self.client()?.get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        let json: serde_json::Value = resp.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        if Self::is_token_expired(&json) {
            let new_token = self.refresh_token().await?;
            let resp = self.client()?.get(&url)
                .header("Authorization", format!("Bearer {}", new_token))
                .send().await
                .map_err(|e| ChannelError::MessageError(e.to_string()))?;
            return resp.json().await.map_err(|e| ChannelError::MessageError(e.to_string()));
        }
        Ok(json)
    }

    /// List chats the bot has joined.
    pub async fn list_chats(&self) -> ChannelResult<serde_json::Value> {
        let url = format!("{}/im/v1/chats", BASE_URL);
        let token = self.get_token().await?;
        let resp = self.client()?.get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        let json: serde_json::Value = resp.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        if Self::is_token_expired(&json) {
            let new_token = self.refresh_token().await?;
            let resp = self.client()?.get(&url)
                .header("Authorization", format!("Bearer {}", new_token))
                .send().await
                .map_err(|e| ChannelError::MessageError(e.to_string()))?;
            return resp.json().await.map_err(|e| ChannelError::MessageError(e.to_string()));
        }
        Ok(json)
    }

    /// Recall (delete) a message.
    pub async fn delete_message(&self, message_id: &str) -> ChannelResult<serde_json::Value> {
        let url = format!("{}/im/v1/messages/{}", BASE_URL, message_id);
        let token = self.get_token().await?;
        let resp = self.client()?.delete(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        let json: serde_json::Value = resp.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        if Self::is_token_expired(&json) {
            let new_token = self.refresh_token().await?;
            let resp = self.client()?.delete(&url)
                .header("Authorization", format!("Bearer {}", new_token))
                .send().await
                .map_err(|e| ChannelError::MessageError(e.to_string()))?;
            return resp.json().await.map_err(|e| ChannelError::MessageError(e.to_string()));
        }
        Ok(json)
    }

    /// Update (edit) a sent message.
    pub async fn update_message(
        &self, message_id: &str, content: &str,
    ) -> ChannelResult<serde_json::Value> {
        let url = format!("{}/im/v1/messages/{}", BASE_URL, message_id);
        let body = serde_json::json!({ "content": content });
        let token = self.get_token().await?;
        let resp = self.client()?.patch(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body).send().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        let json: serde_json::Value = resp.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        if Self::is_token_expired(&json) {
            let new_token = self.refresh_token().await?;
            let resp = self.client()?.patch(&url)
                .header("Authorization", format!("Bearer {}", new_token))
                .json(&body).send().await
                .map_err(|e| ChannelError::MessageError(e.to_string()))?;
            return resp.json().await.map_err(|e| ChannelError::MessageError(e.to_string()));
        }
        Ok(json)
    }

    /// Forward a message to another chat.
    pub async fn forward_message(
        &self, message_id: &str, receive_id: &str, receive_id_type: &str,
    ) -> ChannelResult<serde_json::Value> {
        let url = format!(
            "{}/im/v1/messages/{}/forward?receive_id_type={}", BASE_URL, message_id, receive_id_type
        );
        let body = serde_json::json!({ "receive_id": receive_id });
        let token = self.get_token().await?;
        let resp = self.client()?.post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body).send().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        let json: serde_json::Value = resp.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        if Self::is_token_expired(&json) {
            let new_token = self.refresh_token().await?;
            let resp = self.client()?.post(&url)
                .header("Authorization", format!("Bearer {}", new_token))
                .json(&body).send().await
                .map_err(|e| ChannelError::MessageError(e.to_string()))?;
            return resp.json().await.map_err(|e| ChannelError::MessageError(e.to_string()));
        }
        Ok(json)
    }

    /// Get user info by user_id, open_id, or union_id.
    pub async fn get_user(&self, user_id: &str, id_type: &str) -> ChannelResult<serde_json::Value> {
        let url = format!("{}/contact/v3/users/{}?user_id_type={}", BASE_URL, user_id, id_type);
        let token = self.get_token().await?;
        let resp = self.client()?.get(&url)
            .header("Authorization", format!("Bearer {}", token))
            .send().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        let json: serde_json::Value = resp.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        if Self::is_token_expired(&json) {
            let new_token = self.refresh_token().await?;
            let resp = self.client()?.get(&url)
                .header("Authorization", format!("Bearer {}", new_token))
                .send().await
                .map_err(|e| ChannelError::MessageError(e.to_string()))?;
            return resp.json().await.map_err(|e| ChannelError::MessageError(e.to_string()));
        }
        Ok(json)
    }
}

impl Default for FeishuChannel {
    fn default() -> Self { Self::new() }
}

impl Clone for FeishuChannel {
    fn clone(&self) -> Self {
        Self {
            app_id: self.app_id.clone(), app_secret: self.app_secret.clone(),
            token: self.token.clone(),
            client: None, status: self.status,
        }
    }
}

#[async_trait]
impl Channel for FeishuChannel {
    fn channel_type(&self) -> &str { "feishu" }

    async fn connect(&mut self) -> ChannelResult<()> {
        self.status = ChannelStatus::Connecting;
        self.client = Some(Client::new());
        self.refresh_token().await?;
        self.status = ChannelStatus::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> ChannelResult<()> {
        self.client = None;
        *self.token.write().await = None;
        self.status = ChannelStatus::Disconnected;
        Ok(())
    }

    fn status(&self) -> ChannelStatus { self.status }

    async fn send_message(&self, message: &ChannelMessage) -> ChannelResult<String> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".into()));
        }
        let receive_id = message.metadata.get("receive_id")
            .or_else(|| message.metadata.get("chat_id"))
            .ok_or_else(|| ChannelError::MessageError("receive_id or chat_id required".into()))?;
        let receive_id_type = message.metadata.get("receive_id_type")
            .map(|s| s.as_str()).unwrap_or("chat_id");

        // Support msg_type override from metadata (default: text)
        let msg_type = message.metadata.get("msg_type").map(|s| s.as_str()).unwrap_or("text");
        let content = if msg_type == "text" {
            serde_json::json!({"text": message.content}).to_string()
        } else {
            message.content.clone()
        };

        // Update mode: if update_message_id is set, try editing that message.
        // NOTE: Feishu PATCH API only supports "interactive" (card) messages.
        // For text/post/image etc., skip the update and send a new message instead.
        if let Some(mid) = message.metadata.get("update_message_id") {
            if msg_type == "interactive"
                && let Ok(json) = self.update_message(mid, &content).await {
                    if json.get("code").and_then(|c| c.as_i64()).unwrap_or(-1) == 0 {
                        return Ok(mid.clone());
                    }
                    tracing::warn!("Feishu update_message failed: {}", json);
            }
            // Fallback: send new message (don't delete placeholder to avoid "撤回" notification)
            let json = self.send_raw(receive_id, receive_id_type, msg_type, &content).await?;
            return Ok(json["data"]["message_id"].as_str().unwrap_or("sent").to_string());
        }

        // Reply mode: if message_id is set, reply to that message
        if let Some(message_id) = message.metadata.get("message_id") {
            let json = self.reply_raw(message_id, msg_type, &content).await?;
            return Ok(json["data"]["message_id"].as_str().unwrap_or("sent").to_string());
        }

        let json = self.send_raw(receive_id, receive_id_type, msg_type, &content).await?;
        Ok(json["data"]["message_id"].as_str().unwrap_or("sent").to_string())
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>> {
        Ok(vec![])
    }

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()> {
        tracing::debug!("Feishu event: {:?}", event);
        Ok(())
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            reactions: true,
            threads: true,
            media: true,
            editing: true,
            deletion: true,
            directory: true,
            ..Default::default()
        }
    }

    async fn send_reaction(&self, message_id: &str, emoji: &str, _metadata: &HashMap<String, String>) -> ChannelResult<()> {
        self.add_reaction(message_id, emoji).await?;
        Ok(())
    }

    async fn send_thread_reply(&self, thread_id: &str, message: &ChannelMessage) -> ChannelResult<String> {
        let msg_type = message.metadata.get("msg_type").map(|s| s.as_str()).unwrap_or("text");
        let content = if msg_type == "text" {
            serde_json::json!({"text": message.content}).to_string()
        } else {
            message.content.clone()
        };
        let json = self.reply_raw(thread_id, msg_type, &content).await?;
        Ok(json["data"]["message_id"].as_str().unwrap_or("sent").to_string())
    }

    async fn edit_message(&self, message_id: &str, content: &str) -> ChannelResult<()> {
        self.update_message(message_id, content).await?;
        Ok(())
    }

    async fn delete_message(&self, message_id: &str) -> ChannelResult<()> {
        self.delete_message(message_id).await?;
        Ok(())
    }

    async fn list_groups(&self) -> ChannelResult<Vec<GroupInfo>> {
        let json = self.list_chats().await?;
        let items = json["data"]["items"].as_array()
            .ok_or_else(|| ChannelError::MessageError("No items in response".into()))?;
        Ok(items.iter().filter_map(|c| {
            Some(GroupInfo {
                id: c.get("chat_id")?.as_str()?.to_string(),
                name: c.get("name")?.as_str()?.to_string(),
                member_count: c.get("member_count").and_then(|n| n.as_u64()).map(|n| n as u32),
                group_type: ChatType::Group,
            })
        }).collect())
    }

    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>> {
        Ok(Box::new(FeishuSender { channel: Arc::new(RwLock::new(self.clone())) }))
    }
}

struct FeishuSender {
    channel: Arc<RwLock<FeishuChannel>>,
}

impl MessageSender for FeishuSender {
    fn send<'a>(&'a self, content: &'a str, metadata: HashMap<String, String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = ChannelResult<String>> + Send + 'a>> {
        let channel = self.channel.clone();
        let content = content.to_string();
        Box::pin(async move {
            let msg = ChannelMessage {
                id: uuid::Uuid::new_v4().to_string(),
                channel: "feishu".into(), sender: String::new(),
                content, timestamp: chrono::Utc::now().timestamp_millis(),
                metadata,
            };
            channel.read().await.send_message(&msg).await
        })
    }
}
