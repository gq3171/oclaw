use crate::traits::*;
use crate::types::*;
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use reqwest::Client;
use serde::Serialize;
use sha2::Sha256;
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
        let token = self
            .bot_token
            .as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("Bot token not set".to_string()))?;

        let url = format!("https://slack.com/api/{}", method);

        let client = self
            .client
            .as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".to_string()))?;

        let mut request = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json; charset=utf-8");

        if let Some(body) = body {
            request = request.json(body);
        }

        let response = request
            .send()
            .await
            .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;

        let slack_resp: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        let ok = slack_resp
            .get("ok")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if ok {
            Ok(slack_resp)
        } else {
            Err(ChannelError::MessageError(
                slack_resp
                    .get("error")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown error")
                    .to_string(),
            ))
        }
    }

    fn payload_string(payload: &serde_json::Value, keys: &[&str]) -> Option<String> {
        keys.iter()
            .filter_map(|key| payload.get(*key).and_then(|v| v.as_str()))
            .map(str::trim)
            .find(|value| !value.is_empty())
            .map(ToString::to_string)
    }

    fn payload_usize(payload: &serde_json::Value, keys: &[&str]) -> Option<usize> {
        keys.iter().find_map(|key| {
            payload
                .get(*key)
                .and_then(|v| v.as_u64())
                .and_then(|v| usize::try_from(v).ok())
        })
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

        let channel_id = message
            .metadata
            .get("channel_id")
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

        let response: serde_json::Value = self
            .send_api_request(
                "chat.postMessage",
                Some(
                    &serde_json::to_value(&slack_msg)
                        .map_err(|e| ChannelError::MessageError(e.to_string()))?,
                ),
            )
            .await?;

        let ts = response
            .get("ts")
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

        let members = response
            .get("members")
            .and_then(|m| m.as_array())
            .ok_or_else(|| ChannelError::MessageError("No members found".to_string()))?;

        let accounts: Vec<ChannelAccount> = members
            .iter()
            .filter_map(|m| {
                Some(ChannelAccount {
                    id: m.get("id")?.as_str()?.to_string(),
                    name: m.get("name")?.as_str()?.to_string(),
                    channel: "slack".to_string(),
                    avatar: m
                        .get("profile")
                        .and_then(|p| p.get("image_72"))
                        .and_then(|i| i.as_str())
                        .map(|s| s.to_string()),
                    status: m
                        .get("presence")
                        .and_then(|p| p.as_str())
                        .map(|s| s.to_string()),
                })
            })
            .collect();

        Ok(accounts)
    }

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()> {
        tracing::debug!("Received Slack event: {:?}", event);

        if let Some(secret) = &self.signing_secret
            && let (Some(ts), Some(sig), Some(body)) = (
                event
                    .payload
                    .get("_slack_timestamp")
                    .and_then(|v| v.as_str()),
                event
                    .payload
                    .get("_slack_signature")
                    .and_then(|v| v.as_str()),
                event
                    .payload
                    .get("_slack_raw_body")
                    .and_then(|v| v.as_str()),
            )
        {
            let base = format!("v0:{}:{}", ts, body);
            let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
                .map_err(|e| ChannelError::AuthenticationError(e.to_string()))?;
            mac.update(base.as_bytes());
            let expected = format!("v0={}", hex::encode(mac.finalize().into_bytes()));
            if expected != sig {
                return Err(ChannelError::AuthenticationError(
                    "Invalid Slack signature".to_string(),
                ));
            }
        }

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

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            reactions: true,
            threads: true,
            media: true,
            streaming: true,
            editing: true,
            deletion: true,
            directory: true,
            ..Default::default()
        }
    }

    async fn send_reaction(
        &self,
        message_id: &str,
        emoji: &str,
        _metadata: &HashMap<String, String>,
    ) -> ChannelResult<()> {
        // message_id format: "channelId_timestamp"
        let parts: Vec<&str> = message_id.splitn(2, '_').collect();
        if parts.len() != 2 {
            return Err(ChannelError::MessageError(
                "Invalid message_id format, expected channelId_ts".into(),
            ));
        }
        let body = serde_json::json!({
            "channel": parts[0],
            "timestamp": parts[1],
            "name": emoji.trim_matches(':'),
        });
        self.send_api_request("reactions.add", Some(&body)).await?;
        Ok(())
    }

    async fn remove_reaction(&self, message_id: &str, emoji: &str) -> ChannelResult<()> {
        let parts: Vec<&str> = message_id.splitn(2, '_').collect();
        if parts.len() != 2 {
            return Err(ChannelError::MessageError(
                "Invalid message_id format".into(),
            ));
        }
        let body = serde_json::json!({
            "channel": parts[0],
            "timestamp": parts[1],
            "name": emoji.trim_matches(':'),
        });
        self.send_api_request("reactions.remove", Some(&body))
            .await?;
        Ok(())
    }

    async fn send_thread_reply(
        &self,
        thread_id: &str,
        message: &ChannelMessage,
    ) -> ChannelResult<String> {
        let channel_id = message
            .metadata
            .get("channel_id")
            .or_else(|| message.metadata.get("channel"))
            .cloned()
            .ok_or_else(|| {
                ChannelError::MessageError("Channel ID required for thread reply".into())
            })?;
        let body = serde_json::json!({
            "channel": channel_id,
            "text": message.content,
            "thread_ts": thread_id,
        });
        let resp = self
            .send_api_request("chat.postMessage", Some(&body))
            .await?;
        let ts = resp.get("ts").and_then(|t| t.as_str()).unwrap_or_default();
        Ok(format!("{}_{}", channel_id, ts))
    }

    async fn edit_message(&self, message_id: &str, content: &str) -> ChannelResult<()> {
        let parts: Vec<&str> = message_id.splitn(2, '_').collect();
        if parts.len() != 2 {
            return Err(ChannelError::MessageError(
                "Invalid message_id format".into(),
            ));
        }
        let body = serde_json::json!({
            "channel": parts[0],
            "ts": parts[1],
            "text": content,
        });
        self.send_api_request("chat.update", Some(&body)).await?;
        Ok(())
    }

    async fn delete_message(&self, message_id: &str) -> ChannelResult<()> {
        let parts: Vec<&str> = message_id.splitn(2, '_').collect();
        if parts.len() != 2 {
            return Err(ChannelError::MessageError(
                "Invalid message_id format".into(),
            ));
        }
        let body = serde_json::json!({
            "channel": parts[0],
            "ts": parts[1],
        });
        self.send_api_request("chat.delete", Some(&body)).await?;
        Ok(())
    }

    async fn list_members(&self, group_id: &str) -> ChannelResult<Vec<ChannelAccount>> {
        let body = serde_json::json!({"channel": group_id});
        let resp = self
            .send_api_request("conversations.members", Some(&body))
            .await?;
        let member_ids = resp
            .get("members")
            .and_then(|m| m.as_array())
            .ok_or_else(|| ChannelError::MessageError("No members in response".into()))?;
        let mut accounts = Vec::new();
        for mid in member_ids.iter().take(50) {
            if let Some(uid) = mid.as_str() {
                accounts.push(ChannelAccount {
                    id: uid.to_string(),
                    name: uid.to_string(),
                    channel: "slack".to_string(),
                    avatar: None,
                    status: None,
                });
            }
        }
        Ok(accounts)
    }

    async fn list_groups(&self) -> ChannelResult<Vec<GroupInfo>> {
        let body = serde_json::json!({"types": "public_channel,private_channel", "limit": 100});
        let resp = self
            .send_api_request("conversations.list", Some(&body))
            .await?;
        let channels = resp
            .get("channels")
            .and_then(|c| c.as_array())
            .ok_or_else(|| ChannelError::MessageError("No channels in response".into()))?;
        Ok(channels
            .iter()
            .filter_map(|c| {
                Some(GroupInfo {
                    id: c.get("id")?.as_str()?.to_string(),
                    name: c.get("name")?.as_str()?.to_string(),
                    member_count: c
                        .get("num_members")
                        .and_then(|n| n.as_u64())
                        .map(|n| n as u32),
                    group_type: if c
                        .get("is_channel")
                        .and_then(|b| b.as_bool())
                        .unwrap_or(false)
                    {
                        ChatType::Channel
                    } else {
                        ChatType::Group
                    },
                })
            })
            .collect())
    }

    async fn list_reactions(
        &self,
        target: Option<&str>,
        message_id: &str,
        limit: Option<usize>,
    ) -> ChannelResult<serde_json::Value> {
        let (channel_id, ts) = if let Some((ch, rest)) = message_id.split_once('_') {
            (ch.to_string(), rest.to_string())
        } else {
            let channel = target
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or_else(|| {
                    ChannelError::MessageError("target channel is required for reactions".into())
                })?;
            (channel.to_string(), message_id.to_string())
        };
        let body = serde_json::json!({
            "channel": channel_id,
            "timestamp": ts,
            "full": true
        });
        let resp = self.send_api_request("reactions.get", Some(&body)).await?;
        let mut items = resp
            .get("message")
            .and_then(|m| m.get("reactions"))
            .and_then(|r| r.as_array())
            .cloned()
            .unwrap_or_default();
        if let Some(max) = limit {
            items.truncate(max);
        }
        Ok(serde_json::json!({
            "channel_id": channel_id,
            "timestamp": ts,
            "items": items
        }))
    }

    async fn read_messages(
        &self,
        target: &str,
        limit: Option<usize>,
        before: Option<&str>,
        after: Option<&str>,
        around: Option<&str>,
    ) -> ChannelResult<serde_json::Value> {
        let mut body = serde_json::json!({
            "channel": target,
            "limit": limit.unwrap_or(20).clamp(1, 200)
        });
        if let Some(v) = before.map(str::trim).filter(|v| !v.is_empty())
            && let Some(obj) = body.as_object_mut()
        {
            obj.insert(
                "latest".to_string(),
                serde_json::Value::String(v.to_string()),
            );
        }
        if let Some(v) = after.map(str::trim).filter(|v| !v.is_empty())
            && let Some(obj) = body.as_object_mut()
        {
            obj.insert(
                "oldest".to_string(),
                serde_json::Value::String(v.to_string()),
            );
        }
        if let Some(v) = around.map(str::trim).filter(|v| !v.is_empty())
            && let Some(obj) = body.as_object_mut()
        {
            obj.insert(
                "latest".to_string(),
                serde_json::Value::String(v.to_string()),
            );
            obj.insert("inclusive".to_string(), serde_json::Value::Bool(true));
        }
        let resp = self
            .send_api_request("conversations.history", Some(&body))
            .await?;
        let items = resp
            .get("messages")
            .and_then(|m| m.as_array())
            .cloned()
            .unwrap_or_default();
        Ok(serde_json::json!({
            "channel_id": target,
            "items": items
        }))
    }

    async fn search_messages(
        &self,
        target: Option<&str>,
        query: &str,
        limit: Option<usize>,
    ) -> ChannelResult<serde_json::Value> {
        let trimmed = query.trim();
        if trimmed.is_empty() {
            return Err(ChannelError::MessageError("query required".into()));
        }
        let query_text = if let Some(ch) = target.map(str::trim).filter(|v| !v.is_empty()) {
            format!("in:{} {}", ch, trimmed)
        } else {
            trimmed.to_string()
        };
        let body = serde_json::json!({
            "query": query_text,
            "count": limit.unwrap_or(20).clamp(1, 100),
            "sort": "timestamp",
            "sort_dir": "desc"
        });
        self.send_api_request("search.messages", Some(&body)).await
    }

    async fn pin_message(&self, target: &str, message_id: &str) -> ChannelResult<()> {
        let ts = if let Some((_, rest)) = message_id.split_once('_') {
            rest.to_string()
        } else {
            message_id.to_string()
        };
        let body = serde_json::json!({
            "channel": target,
            "timestamp": ts
        });
        self.send_api_request("pins.add", Some(&body)).await?;
        Ok(())
    }

    async fn unpin_message(&self, target: &str, message_id: &str) -> ChannelResult<()> {
        let ts = if let Some((_, rest)) = message_id.split_once('_') {
            rest.to_string()
        } else {
            message_id.to_string()
        };
        let body = serde_json::json!({
            "channel": target,
            "timestamp": ts
        });
        self.send_api_request("pins.remove", Some(&body)).await?;
        Ok(())
    }

    async fn list_pins(
        &self,
        target: &str,
        limit: Option<usize>,
    ) -> ChannelResult<serde_json::Value> {
        let body = serde_json::json!({"channel": target});
        let resp = self.send_api_request("pins.list", Some(&body)).await?;
        let mut items = resp
            .get("items")
            .and_then(|i| i.as_array())
            .cloned()
            .unwrap_or_default();
        if let Some(max) = limit {
            items.truncate(max);
        }
        Ok(serde_json::json!({
            "channel_id": target,
            "items": items
        }))
    }

    async fn get_permissions(&self, target: &str) -> ChannelResult<serde_json::Value> {
        let body = serde_json::json!({"channel": target, "include_num_members": true});
        let resp = self
            .send_api_request("conversations.info", Some(&body))
            .await?;
        Ok(serde_json::json!({
            "channel_id": target,
            "channel": resp.get("channel").cloned().unwrap_or(serde_json::Value::Null),
            "raw": resp
        }))
    }

    async fn custom_action(
        &self,
        action: &str,
        target: Option<&str>,
        payload: &serde_json::Value,
    ) -> ChannelResult<serde_json::Value> {
        let action = action.trim().to_ascii_lowercase();
        match action.as_str() {
            "member_info" => {
                let user_id = Self::payload_string(payload, &["user_id", "userId"])
                    .or_else(|| {
                        target
                            .map(str::trim)
                            .filter(|v| !v.is_empty())
                            .map(ToString::to_string)
                    })
                    .ok_or_else(|| ChannelError::MessageError("user_id required".into()))?;
                let body = serde_json::json!({ "user": user_id });
                let resp = self.send_api_request("users.info", Some(&body)).await?;
                Ok(serde_json::json!({
                    "user": resp.get("user").cloned().unwrap_or(serde_json::Value::Null),
                    "raw": resp
                }))
            }
            "emoji_list" => {
                let resp = self.send_api_request("emoji.list", None).await?;
                let emoji_obj = resp
                    .get("emoji")
                    .and_then(|v| v.as_object())
                    .cloned()
                    .unwrap_or_default();
                let mut entries: Vec<(String, serde_json::Value)> = emoji_obj.into_iter().collect();
                entries.sort_by(|a, b| a.0.cmp(&b.0));
                if let Some(limit) = Self::payload_usize(payload, &["limit"])
                    && entries.len() > limit
                {
                    entries.truncate(limit);
                }
                Ok(serde_json::json!({
                    "emoji": serde_json::Value::Object(entries.into_iter().collect()),
                    "raw": resp
                }))
            }
            _ => Err(ChannelError::UnsupportedOperation(format!(
                "action:{}",
                action
            ))),
        }
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
