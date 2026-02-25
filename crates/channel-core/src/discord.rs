use crate::traits::*;
use crate::types::*;
use async_trait::async_trait;
use reqwest::Client;
use serde::Serialize;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct DiscordChannel {
    bot_token: Option<String>,
    guild_id: Option<String>,
    channel_ids: Vec<String>,
    api_url: String,
    client: Option<Client>,
    status: ChannelStatus,
}

#[derive(Debug, Serialize)]
struct DiscordMessage {
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tts: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    embeds: Option<Vec<DiscordEmbed>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    components: Option<Vec<serde_json::Value>>,
}

#[derive(Debug, Serialize)]
struct DiscordEmbed {
    title: Option<String>,
    description: Option<String>,
    color: Option<i32>,
    footer: Option<DiscordEmbedFooter>,
    fields: Option<Vec<DiscordEmbedField>>,
}

#[derive(Debug, Serialize)]
struct DiscordEmbedFooter {
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    icon_url: Option<String>,
}

#[derive(Debug, Serialize)]
struct DiscordEmbedField {
    name: String,
    value: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    inline: Option<bool>,
}

impl DiscordChannel {
    pub fn new() -> Self {
        Self {
            bot_token: None,
            guild_id: None,
            channel_ids: vec![],
            api_url: "https://discord.com/api/v10".to_string(),
            client: None,
            status: ChannelStatus::Disconnected,
        }
    }

    pub fn with_config(mut self, bot_token: &str, guild_id: &str) -> Self {
        self.bot_token = Some(bot_token.to_string());
        self.guild_id = Some(guild_id.to_string());
        self
    }

    pub fn add_channel(mut self, channel_id: &str) -> Self {
        self.channel_ids.push(channel_id.to_string());
        self
    }

    async fn send_api_request(
        &self,
        method: &str,
        endpoint: &str,
        body: Option<&serde_json::Value>,
    ) -> ChannelResult<serde_json::Value> {
        let token = self.bot_token.as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("Bot token not set".to_string()))?;

        let url = format!("{}{}", self.api_url, endpoint);

        let client = self.client.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".to_string()))?;

        let mut request = match method.to_uppercase().as_str() {
            "GET" => client.get(&url),
            "POST" => client.post(&url),
            "DELETE" => client.delete(&url),
            "PATCH" => client.patch(&url),
            _ => return Err(ChannelError::ConnectionError("Invalid method".to_string())),
        };

        request = request
            .header("Authorization", format!("Bot {}", token))
            .header("Content-Type", "application/json")
            .header("User-Agent", "OCLAWS/1.0");

        if let Some(body) = body {
            request = request.json(body);
        }

        let response = request.send().await
            .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;

        let status = response.status();
        let json: serde_json::Value = response.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        if status.is_success() {
            Ok(json)
        } else {
            let error_msg = json.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            
            Err(ChannelError::MessageError(format!("Discord error: {}", error_msg)))
        }
    }
}

impl Default for DiscordChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Channel for DiscordChannel {
    fn channel_type(&self) -> &str {
        "discord"
    }

    async fn connect(&mut self) -> ChannelResult<()> {
        if self.bot_token.is_none() {
            return Err(ChannelError::AuthenticationError(
                "Bot token required".to_string(),
            ));
        }

        self.status = ChannelStatus::Connecting;

        self.client = Some(Client::new());

        let user_response: serde_json::Value = self.send_api_request("GET", "/users/@me", None).await?;
        
        tracing::info!("Discord bot connected: {:?}", user_response);

        if let Some(guild_id) = &self.guild_id {
            let guild_response: serde_json::Value = self.send_api_request(
                "GET",
                &format!("/guilds/{}", guild_id),
                None,
            ).await?;
            
            tracing::info!("Connected to Discord guild: {:?}", guild_response);
        }

        self.status = ChannelStatus::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> ChannelResult<()> {
        self.client = None;
        self.status = ChannelStatus::Disconnected;
        tracing::info!("Discord channel disconnected");
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
            .or_else(|| self.channel_ids.first())
            .cloned()
            .ok_or_else(|| ChannelError::MessageError("Channel ID not specified".to_string()))?;

        let discord_msg = DiscordMessage {
            content: message.content.clone(),
            tts: message.metadata.get("tts").and_then(|v| v.parse().ok()),
            embeds: None,
            components: None,
        };

        let response: serde_json::Value = self.send_api_request(
            "POST",
            &format!("/channels/{}/messages", channel_id),
            Some(&serde_json::to_value(&discord_msg).map_err(|e| ChannelError::MessageError(e.to_string()))?),
        ).await?;

        let message_id = response.get("id")
            .and_then(|id| id.as_str())
            .map(|id| id.to_string())
            .ok_or_else(|| ChannelError::MessageError("No message ID returned".to_string()))?;

        Ok(message_id)
    }

    async fn send_message_with_attachments(&self, message_with_attachments: &ChannelMessageWithAttachments) -> ChannelResult<String> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let channel_id = message_with_attachments.message.metadata.get("channel_id")
            .or_else(|| message_with_attachments.message.metadata.get("channel"))
            .or_else(|| self.channel_ids.first())
            .cloned()
            .ok_or_else(|| ChannelError::MessageError("Channel ID not specified".to_string()))?;

        let mut attachments_json: Vec<serde_json::Value> = Vec::new();
        
        for attachment in &message_with_attachments.attachments {
            attachments_json.push(serde_json::json!({
                "filename": attachment.filename.clone().unwrap_or_else(|| "file".to_string()),
                "file_url": attachment.url
            }));
        }

        let discord_msg = DiscordMessage {
            content: message_with_attachments.message.content.clone(),
            tts: message_with_attachments.message.metadata.get("tts").and_then(|v| v.parse().ok()),
            embeds: None,
            components: None,
        };

        let mut request_body = serde_json::to_value(&discord_msg).map_err(|e| ChannelError::MessageError(e.to_string()))?;
        
        if let Some(obj) = request_body.as_object_mut() {
            obj.insert("attachments".to_string(), serde_json::json!(attachments_json));
        }

        let response: serde_json::Value = self.send_api_request(
            "POST",
            &format!("/channels/{}/messages", channel_id),
            Some(&request_body),
        ).await?;

        let message_id = response.get("id")
            .and_then(|id| id.as_str())
            .map(|id| id.to_string())
            .ok_or_else(|| ChannelError::MessageError("No message ID returned".to_string()))?;

        Ok(message_id)
    }

    async fn send_typing_status(&self, _user_id: &str, _status: TypingStatus) -> ChannelResult<()> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let channel_id = self.channel_ids.first()
            .ok_or_else(|| ChannelError::MessageError("No channel configured".to_string()))?;

        match _status {
            TypingStatus::Started => {
                let _: serde_json::Value = self.send_api_request(
                    "POST",
                    &format!("/channels/{}/typing", channel_id),
                    None,
                ).await?;
                Ok(())
            }
            TypingStatus::Stopped => Ok(()),
        }
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let user_response: serde_json::Value = self.send_api_request("GET", "/users/@me", None).await?;

        let account = ChannelAccount {
            id: user_response.get("id")
                .and_then(|id| id.as_str())
                .unwrap_or("unknown")
                .to_string(),
            name: user_response.get("username")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string(),
            channel: "discord".to_string(),
            avatar: user_response.get("avatar")
                .and_then(|a| a.as_str())
                .and_then(|a| {
                    let id = user_response.get("id").and_then(|i| i.as_str()).unwrap_or("");
                    if id.chars().all(|c| c.is_ascii_digit()) && a.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                        Some(format!("https://cdn.discordapp.com/avatars/{}/{}.png", id, a))
                    } else {
                        None
                    }
                }),
            status: Some("active".to_string()),
        };

        Ok(vec![account])
    }

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()> {
        tracing::debug!("Received Discord event: {:?}", event);
        
        match event.event_type.as_str() {
            "message_create" => {
                tracing::info!("Received Discord message: {:?}", event.payload);
            }
            "interaction_create" => {
                tracing::info!("Discord interaction: {:?}", event.payload);
            }
            _ => {}
        }
        
        Ok(())
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            reactions: true,
            threads: true,
            polls: true,
            media: true,
            streaming: true,
            editing: true,
            deletion: true,
            typing_indicator: true,
            directory: true,
            ..Default::default()
        }
    }

    async fn send_reaction(&self, message_id: &str, emoji: &str, _metadata: &HashMap<String, String>) -> ChannelResult<()> {
        // message_id is raw Discord message ID; channel_id from metadata or first configured
        let channel_id = _metadata.get("channel_id")
            .or_else(|| self.channel_ids.first())
            .ok_or_else(|| ChannelError::MessageError("Channel ID required for reaction".into()))?;
        let encoded = urlencoding::encode(emoji);
        self.send_api_request(
            "PUT",
            &format!("/channels/{}/messages/{}/reactions/{}/@me", channel_id, message_id, encoded),
            None,
        ).await?;
        Ok(())
    }

    async fn remove_reaction(&self, message_id: &str, emoji: &str) -> ChannelResult<()> {
        let channel_id = self.channel_ids.first()
            .ok_or_else(|| ChannelError::MessageError("Channel ID required".into()))?;
        let encoded = urlencoding::encode(emoji);
        self.send_api_request(
            "DELETE",
            &format!("/channels/{}/messages/{}/reactions/{}/@me", channel_id, message_id, encoded),
            None,
        ).await?;
        Ok(())
    }

    async fn create_thread(&self, message_id: &str, name: Option<&str>) -> ChannelResult<String> {
        let channel_id = self.channel_ids.first()
            .ok_or_else(|| ChannelError::MessageError("Channel ID required".into()))?;
        let body = serde_json::json!({
            "name": name.unwrap_or("Thread"),
            "auto_archive_duration": 1440,
        });
        let resp = self.send_api_request(
            "POST",
            &format!("/channels/{}/messages/{}/threads", channel_id, message_id),
            Some(&body),
        ).await?;
        Ok(resp.get("id").and_then(|id| id.as_str()).unwrap_or_default().to_string())
    }

    async fn send_thread_reply(&self, thread_id: &str, message: &ChannelMessage) -> ChannelResult<String> {
        let body = serde_json::json!({"content": message.content});
        let resp = self.send_api_request(
            "POST",
            &format!("/channels/{}/messages", thread_id),
            Some(&body),
        ).await?;
        Ok(resp.get("id").and_then(|id| id.as_str()).unwrap_or_default().to_string())
    }

    async fn edit_message(&self, message_id: &str, content: &str) -> ChannelResult<()> {
        let channel_id = self.channel_ids.first()
            .ok_or_else(|| ChannelError::MessageError("Channel ID required".into()))?;
        let body = serde_json::json!({"content": content});
        self.send_api_request(
            "PATCH",
            &format!("/channels/{}/messages/{}", channel_id, message_id),
            Some(&body),
        ).await?;
        Ok(())
    }

    async fn delete_message(&self, message_id: &str) -> ChannelResult<()> {
        let channel_id = self.channel_ids.first()
            .ok_or_else(|| ChannelError::MessageError("Channel ID required".into()))?;
        self.send_api_request(
            "DELETE",
            &format!("/channels/{}/messages/{}", channel_id, message_id),
            None,
        ).await?;
        Ok(())
    }

    async fn send_media(&self, target: &str, media: &ChannelMedia) -> ChannelResult<String> {
        let url_str = match &media.data {
            MediaData::Url(u) => u.clone(),
            MediaData::FileId(id) => id.clone(),
            MediaData::Bytes(_) => return Err(ChannelError::UnsupportedOperation(
                "Discord send_media with raw bytes not yet supported".into(),
            )),
        };
        let body = serde_json::json!({
            "content": media.caption.as_deref().unwrap_or(""),
            "embeds": [{"image": {"url": url_str}}],
        });
        let resp = self.send_api_request(
            "POST",
            &format!("/channels/{}/messages", target),
            Some(&body),
        ).await?;
        Ok(resp.get("id").and_then(|id| id.as_str()).unwrap_or_default().to_string())
    }

    async fn list_members(&self, _group_id: &str) -> ChannelResult<Vec<ChannelAccount>> {
        let guild_id = self.guild_id.as_deref()
            .ok_or_else(|| ChannelError::ConfigError("Guild ID required".into()))?;
        let resp = self.send_api_request(
            "GET",
            &format!("/guilds/{}/members?limit=100", guild_id),
            None,
        ).await?;
        let members = resp.as_array()
            .ok_or_else(|| ChannelError::MessageError("Unexpected response".into()))?;
        Ok(members.iter().filter_map(|m| {
            let user = m.get("user")?;
            Some(ChannelAccount {
                id: user.get("id")?.as_str()?.to_string(),
                name: user.get("username")?.as_str()?.to_string(),
                channel: "discord".to_string(),
                avatar: user.get("avatar").and_then(|a| a.as_str()).map(|a| {
                    let uid = user.get("id").and_then(|i| i.as_str()).unwrap_or("");
                    format!("https://cdn.discordapp.com/avatars/{}/{}.png", uid, a)
                }),
                status: None,
            })
        }).collect())
    }

    async fn list_groups(&self) -> ChannelResult<Vec<GroupInfo>> {
        let guild_id = self.guild_id.as_deref()
            .ok_or_else(|| ChannelError::ConfigError("Guild ID required".into()))?;
        let resp = self.send_api_request(
            "GET",
            &format!("/guilds/{}/channels", guild_id),
            None,
        ).await?;
        let channels = resp.as_array()
            .ok_or_else(|| ChannelError::MessageError("Unexpected response".into()))?;
        Ok(channels.iter().filter_map(|c| {
            Some(GroupInfo {
                id: c.get("id")?.as_str()?.to_string(),
                name: c.get("name")?.as_str()?.to_string(),
                member_count: None,
                group_type: match c.get("type").and_then(|t| t.as_i64()).unwrap_or(0) {
                    0 => ChatType::Channel,
                    2 => ChatType::Channel,
                    11 | 12 => ChatType::Thread,
                    _ => ChatType::Group,
                },
            })
        }).collect())
    }

    async fn send_poll(&self, target: &str, poll: &PollRequest) -> ChannelResult<String> {
        let answers: Vec<serde_json::Value> = poll.options.iter()
            .take(10)
            .enumerate()
            .map(|(i, o)| serde_json::json!({"answer_id": i + 1, "poll_media": {"text": o}}))
            .collect();
        let body = serde_json::json!({
            "poll": {
                "question": {"text": poll.question},
                "answers": answers,
                "allow_multiselect": poll.allows_multiple,
                "duration": 24,
            }
        });
        let resp = self.send_api_request(
            "POST",
            &format!("/channels/{}/messages", target),
            Some(&body),
        ).await?;
        Ok(resp.get("id").and_then(|id| id.as_str()).unwrap_or_default().to_string())
    }

    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>> {
        Ok(Box::new(DiscordSender {
            channel: Arc::new(RwLock::new(self.clone())),
        }))
    }
}

impl Clone for DiscordChannel {
    fn clone(&self) -> Self {
        Self {
            bot_token: self.bot_token.clone(),
            guild_id: self.guild_id.clone(),
            channel_ids: self.channel_ids.clone(),
            api_url: self.api_url.clone(),
            client: None,
            status: self.status,
        }
    }
}

struct DiscordSender {
    channel: Arc<RwLock<DiscordChannel>>,
}

impl MessageSender for DiscordSender {
    fn send<'a>(&'a self, content: &'a str, metadata: HashMap<String, String>) -> Pin<Box<dyn std::future::Future<Output = ChannelResult<String>> + Send + 'a>> {
        let channel = self.channel.clone();
        let content = content.to_string();
        let metadata = metadata.clone();
        
        Box::pin(async move {
            let message = ChannelMessage {
                id: uuid::Uuid::new_v4().to_string(),
                channel: "discord".to_string(),
                sender: String::new(),
                content,
                timestamp: chrono::Utc::now().timestamp_millis(),
                metadata,
            };
            
            channel.read().await.send_message(&message).await
        })
    }
}
