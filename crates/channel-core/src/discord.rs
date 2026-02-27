use crate::traits::*;
use crate::types::*;
use async_trait::async_trait;
use base64::Engine as _;
use futures_util::{SinkExt, StreamExt};
use reqwest::Client;
use serde::Serialize;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, OnceLock};
use tokio::sync::{RwLock, mpsc};
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message;

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

#[derive(Debug, Clone)]
struct DiscordPresenceUpdate {
    status: String,
    activities: Vec<serde_json::Value>,
}

impl DiscordPresenceUpdate {
    fn online() -> Self {
        Self {
            status: "online".to_string(),
            activities: Vec::new(),
        }
    }
}

#[derive(Clone)]
struct DiscordPresenceHandle {
    tx: mpsc::UnboundedSender<DiscordPresenceUpdate>,
}

static DISCORD_PRESENCE_GATEWAYS: OnceLock<
    std::sync::Mutex<HashMap<String, DiscordPresenceHandle>>,
> = OnceLock::new();

fn presence_gateways() -> &'static std::sync::Mutex<HashMap<String, DiscordPresenceHandle>> {
    DISCORD_PRESENCE_GATEWAYS.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
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
        let token = self
            .bot_token
            .as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("Bot token not set".to_string()))?;

        let url = format!("{}{}", self.api_url, endpoint);

        let client = self
            .client
            .as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".to_string()))?;

        let mut request = match method.to_uppercase().as_str() {
            "GET" => client.get(&url),
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
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

        let response = request
            .send()
            .await
            .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;

        let status = response.status();
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        if status.is_success() {
            Ok(json)
        } else {
            let error_msg = json
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");

            Err(ChannelError::MessageError(format!(
                "Discord error: {}",
                error_msg
            )))
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

    fn payload_bool(payload: &serde_json::Value, keys: &[&str]) -> Option<bool> {
        keys.iter()
            .find_map(|key| payload.get(*key).and_then(|v| v.as_bool()))
    }

    fn channel_type_id(raw: Option<&str>, fallback: i64) -> i64 {
        let Some(raw) = raw else {
            return fallback;
        };
        match raw.trim().to_ascii_lowercase().as_str() {
            "text" | "guild_text" => 0,
            "voice" | "guild_voice" => 2,
            "category" | "guild_category" => 4,
            "announcement" | "news" => 5,
            "forum" => 15,
            "media" => 16,
            "thread_public" => 11,
            "thread_private" => 12,
            _ => fallback,
        }
    }

    fn activity_type_id(raw: &str) -> Option<i64> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "playing" => Some(0),
            "streaming" => Some(1),
            "listening" => Some(2),
            "watching" => Some(3),
            "custom" => Some(4),
            "competing" => Some(5),
            _ => None,
        }
    }

    fn parse_gateway_message(msg: Message) -> Option<serde_json::Value> {
        match msg {
            Message::Text(text) => serde_json::from_str(&text).ok(),
            Message::Binary(bin) => serde_json::from_slice(&bin).ok(),
            _ => None,
        }
    }

    fn presence_key(&self, account_id: Option<&str>) -> ChannelResult<String> {
        let token = self
            .bot_token
            .as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("Bot token not set".to_string()))?;
        let mut hasher = sha2::Sha256::new();
        use sha2::Digest as _;
        hasher.update(token.as_bytes());
        let digest = hasher.finalize();
        let fingerprint = hex::encode(&digest[..8]);
        let account = account_id
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("default");
        Ok(format!("{}:{}", account, fingerprint))
    }

    async fn ensure_presence_handle(
        &self,
        account_id: Option<&str>,
    ) -> ChannelResult<DiscordPresenceHandle> {
        let key = self.presence_key(account_id)?;
        {
            let mut guard = presence_gateways()
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if let Some(existing) = guard.get(&key).cloned() {
                if !existing.tx.is_closed() {
                    return Ok(existing);
                }
                guard.remove(&key);
            }
        }

        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError(
                "Discord gateway is not connected".to_string(),
            ));
        }
        let token = self
            .bot_token
            .as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("Bot token not set".to_string()))?
            .clone();
        let gateway = self.send_api_request("GET", "/gateway/bot", None).await?;
        let raw_url = gateway
            .get("url")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                ChannelError::ConnectionError("Discord gateway URL unavailable".to_string())
            })?;
        let ws_url = if raw_url.contains('?') {
            format!("{}&v=10&encoding=json", raw_url)
        } else {
            format!("{}?v=10&encoding=json", raw_url)
        };
        let (tx, rx) = mpsc::unbounded_channel();
        let handle = DiscordPresenceHandle { tx: tx.clone() };
        {
            let mut guard = presence_gateways()
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            guard.insert(key.clone(), handle.clone());
        }
        tokio::spawn(async move {
            DiscordChannel::run_presence_gateway_loop(key, token, ws_url, rx).await;
        });
        Ok(handle)
    }

    async fn run_presence_gateway_loop(
        gateway_key: String,
        token: String,
        ws_url: String,
        mut rx: mpsc::UnboundedReceiver<DiscordPresenceUpdate>,
    ) {
        let mut current_presence = DiscordPresenceUpdate::online();
        loop {
            while let Ok(update) = rx.try_recv() {
                current_presence = update;
            }
            if rx.is_closed() {
                break;
            }

            let connect = connect_async(&ws_url).await;
            let (stream, _) = match connect {
                Ok(pair) => pair,
                Err(err) => {
                    tracing::warn!("Discord presence gateway connect failed: {}", err);
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    continue;
                }
            };
            let (mut sink, mut source) = stream.split();

            let mut seq: Option<i64> = None;
            let mut heartbeat_interval_ms: u64 = 45_000;
            let mut got_hello = false;
            while let Some(frame) = source.next().await {
                match frame {
                    Ok(Message::Ping(payload)) => {
                        let _ = sink.send(Message::Pong(payload)).await;
                    }
                    Ok(msg) => {
                        let Some(json) = Self::parse_gateway_message(msg) else {
                            continue;
                        };
                        if let Some(s) = json.get("s").and_then(|v| v.as_i64()) {
                            seq = Some(s);
                        }
                        let op = json.get("op").and_then(|v| v.as_i64()).unwrap_or_default();
                        if op == 10 {
                            heartbeat_interval_ms = json
                                .get("d")
                                .and_then(|d| d.get("heartbeat_interval"))
                                .and_then(|v| v.as_u64())
                                .unwrap_or(45_000)
                                .max(1_000);
                            got_hello = true;
                            break;
                        }
                    }
                    Err(err) => {
                        tracing::warn!("Discord presence gateway pre-hello error: {}", err);
                        break;
                    }
                }
            }
            if !got_hello {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            }

            let identify = serde_json::json!({
                "op": 2,
                "d": {
                    "token": token.clone(),
                    "intents": 0,
                    "properties": {
                        "os": std::env::consts::OS,
                        "browser": "oclaw",
                        "device": "oclaw"
                    },
                    "presence": {
                        "since": serde_json::Value::Null,
                        "activities": current_presence.activities.clone(),
                        "status": current_presence.status.clone(),
                        "afk": false
                    }
                }
            });
            if sink
                .send(Message::Text(identify.to_string().into()))
                .await
                .is_err()
            {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                continue;
            }

            let mut heartbeat =
                tokio::time::interval(std::time::Duration::from_millis(heartbeat_interval_ms));
            heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let _ = heartbeat.tick().await;

            let mut disconnected = false;
            while !disconnected {
                tokio::select! {
                    _ = heartbeat.tick() => {
                        let payload = serde_json::json!({
                            "op": 1,
                            "d": seq
                        });
                        if sink.send(Message::Text(payload.to_string().into())).await.is_err() {
                            disconnected = true;
                        }
                    }
                    maybe_update = rx.recv() => {
                        match maybe_update {
                            Some(update) => {
                                current_presence = update.clone();
                                let op3 = serde_json::json!({
                                    "op": 3,
                                    "d": {
                                        "since": serde_json::Value::Null,
                                        "activities": update.activities,
                                        "status": update.status,
                                        "afk": false
                                    }
                                });
                                if sink.send(Message::Text(op3.to_string().into())).await.is_err() {
                                    disconnected = true;
                                }
                            }
                            None => {
                                disconnected = true;
                            }
                        }
                    }
                    incoming = source.next() => {
                        match incoming {
                            Some(Ok(Message::Ping(payload))) => {
                                let _ = sink.send(Message::Pong(payload)).await;
                            }
                            Some(Ok(msg)) => {
                                if let Some(json) = Self::parse_gateway_message(msg) {
                                    if let Some(s) = json.get("s").and_then(|v| v.as_i64()) {
                                        seq = Some(s);
                                    }
                                    match json.get("op").and_then(|v| v.as_i64()).unwrap_or_default() {
                                        1 => {
                                            let payload = serde_json::json!({
                                                "op": 1,
                                                "d": seq
                                            });
                                            if sink.send(Message::Text(payload.to_string().into())).await.is_err() {
                                                disconnected = true;
                                            }
                                        }
                                        7 | 9 => {
                                            disconnected = true;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                            Some(Err(err)) => {
                                tracing::warn!("Discord presence gateway stream error: {}", err);
                                disconnected = true;
                            }
                            None => {
                                disconnected = true;
                            }
                        }
                    }
                }
            }
            if rx.is_closed() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
        let mut guard = presence_gateways()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = guard.get(&gateway_key)
            && existing.tx.is_closed()
        {
            guard.remove(&gateway_key);
        }
    }

    fn guess_mime_from_name(name: &str) -> String {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".png") {
            "image/png".to_string()
        } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
            "image/jpeg".to_string()
        } else if lower.ends_with(".gif") {
            "image/gif".to_string()
        } else if lower.ends_with(".webp") {
            "image/webp".to_string()
        } else if lower.ends_with(".json") {
            "application/json".to_string()
        } else {
            "application/octet-stream".to_string()
        }
    }

    fn decode_data_url(raw: &str) -> ChannelResult<(Option<String>, Vec<u8>)> {
        let Some((header, body)) = raw.split_once(',') else {
            return Err(ChannelError::MessageError("Malformed data URL".into()));
        };
        if !header.starts_with("data:") {
            return Err(ChannelError::MessageError("Invalid data URL".into()));
        }
        if !header.contains(";base64") {
            return Err(ChannelError::MessageError(
                "Only base64 data URL is supported".into(),
            ));
        }
        let mime = header
            .trim_start_matches("data:")
            .split(';')
            .next()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToString::to_string);
        let compact: String = body.chars().filter(|c| !c.is_whitespace()).collect();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(compact.as_bytes())
            .map_err(|e| ChannelError::MessageError(format!("Invalid data URL payload: {}", e)))?;
        Ok((mime, bytes))
    }

    async fn resolve_binary_source(
        &self,
        raw: &str,
        filename_hint: Option<&str>,
    ) -> ChannelResult<(Vec<u8>, String, String)> {
        let value = raw.trim();
        if value.is_empty() {
            return Err(ChannelError::MessageError("media source is empty".into()));
        }
        if value.starts_with("data:") {
            let (mime, bytes) = Self::decode_data_url(value)?;
            let filename = filename_hint.unwrap_or("upload.bin").to_string();
            let mime_type = mime.unwrap_or_else(|| Self::guess_mime_from_name(&filename));
            return Ok((bytes, filename, mime_type));
        }
        if value.starts_with("http://") || value.starts_with("https://") {
            let client = self
                .client
                .as_ref()
                .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".into()))?;
            let resp = client
                .get(value)
                .send()
                .await
                .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;
            let mime = resp
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .map(ToString::to_string);
            let bytes = resp
                .bytes()
                .await
                .map_err(|e| ChannelError::MessageError(e.to_string()))?
                .to_vec();
            let filename = filename_hint
                .map(ToString::to_string)
                .unwrap_or_else(|| "upload.bin".to_string());
            return Ok((
                bytes,
                filename.clone(),
                mime.unwrap_or_else(|| Self::guess_mime_from_name(&filename)),
            ));
        }
        let path = std::path::Path::new(value);
        let bytes = std::fs::read(path)
            .map_err(|e| ChannelError::MessageError(format!("Failed to read {}: {}", value, e)))?;
        let filename = filename_hint
            .map(ToString::to_string)
            .or_else(|| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .map(ToString::to_string)
            })
            .unwrap_or_else(|| "upload.bin".to_string());
        let mime = Self::guess_mime_from_name(&filename);
        Ok((bytes, filename, mime))
    }

    async fn send_binary_message(
        &self,
        channel_id: &str,
        content: Option<&str>,
        bytes: Vec<u8>,
        filename: &str,
        mime_type: Option<&str>,
        reply_to: Option<&str>,
    ) -> ChannelResult<serde_json::Value> {
        let token = self
            .bot_token
            .as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("Bot token not set".to_string()))?;
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".to_string()))?;
        let mime = mime_type
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| Self::guess_mime_from_name(filename));
        let file_part = reqwest::multipart::Part::bytes(bytes)
            .file_name(filename.to_string())
            .mime_str(&mime)
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        let mut payload = serde_json::json!({
            "content": content.unwrap_or(""),
            "attachments": [{
                "id": 0,
                "filename": filename
            }]
        });
        if let Some(reply) = reply_to.map(str::trim).filter(|v| !v.is_empty()) {
            payload["message_reference"] = serde_json::json!({
                "message_id": reply,
                "fail_if_not_exists": false
            });
        }
        let form = reqwest::multipart::Form::new()
            .part("files[0]", file_part)
            .text("payload_json", payload.to_string());
        let url = format!("{}/channels/{}/messages", self.api_url, channel_id);
        let response = client
            .post(url)
            .header("Authorization", format!("Bot {}", token))
            .header("User-Agent", "OCLAWS/1.0")
            .multipart(form)
            .send()
            .await
            .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;
        let status = response.status();
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        if status.is_success() {
            Ok(json)
        } else {
            Err(ChannelError::MessageError(
                json.get("message")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Discord media upload failed")
                    .to_string(),
            ))
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

        let user_response: serde_json::Value =
            self.send_api_request("GET", "/users/@me", None).await?;

        tracing::info!("Discord bot connected: {:?}", user_response);

        if let Some(guild_id) = &self.guild_id {
            let guild_response: serde_json::Value = self
                .send_api_request("GET", &format!("/guilds/{}", guild_id), None)
                .await?;

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

        let channel_id = message
            .metadata
            .get("channel_id")
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

        let response: serde_json::Value = self
            .send_api_request(
                "POST",
                &format!("/channels/{}/messages", channel_id),
                Some(
                    &serde_json::to_value(&discord_msg)
                        .map_err(|e| ChannelError::MessageError(e.to_string()))?,
                ),
            )
            .await?;

        let message_id = response
            .get("id")
            .and_then(|id| id.as_str())
            .map(|id| id.to_string())
            .ok_or_else(|| ChannelError::MessageError("No message ID returned".to_string()))?;

        Ok(message_id)
    }

    async fn send_message_with_attachments(
        &self,
        message_with_attachments: &ChannelMessageWithAttachments,
    ) -> ChannelResult<String> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let channel_id = message_with_attachments
            .message
            .metadata
            .get("channel_id")
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
            tts: message_with_attachments
                .message
                .metadata
                .get("tts")
                .and_then(|v| v.parse().ok()),
            embeds: None,
            components: None,
        };

        let mut request_body = serde_json::to_value(&discord_msg)
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        if let Some(obj) = request_body.as_object_mut() {
            obj.insert(
                "attachments".to_string(),
                serde_json::json!(attachments_json),
            );
        }

        let response: serde_json::Value = self
            .send_api_request(
                "POST",
                &format!("/channels/{}/messages", channel_id),
                Some(&request_body),
            )
            .await?;

        let message_id = response
            .get("id")
            .and_then(|id| id.as_str())
            .map(|id| id.to_string())
            .ok_or_else(|| ChannelError::MessageError("No message ID returned".to_string()))?;

        Ok(message_id)
    }

    async fn send_typing_status(&self, _user_id: &str, _status: TypingStatus) -> ChannelResult<()> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let channel_id = self
            .channel_ids
            .first()
            .ok_or_else(|| ChannelError::MessageError("No channel configured".to_string()))?;

        match _status {
            TypingStatus::Started => {
                let _: serde_json::Value = self
                    .send_api_request("POST", &format!("/channels/{}/typing", channel_id), None)
                    .await?;
                Ok(())
            }
            TypingStatus::Stopped => Ok(()),
        }
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let user_response: serde_json::Value =
            self.send_api_request("GET", "/users/@me", None).await?;

        let account = ChannelAccount {
            id: user_response
                .get("id")
                .and_then(|id| id.as_str())
                .unwrap_or("unknown")
                .to_string(),
            name: user_response
                .get("username")
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string(),
            channel: "discord".to_string(),
            avatar: user_response
                .get("avatar")
                .and_then(|a| a.as_str())
                .and_then(|a| {
                    let id = user_response
                        .get("id")
                        .and_then(|i| i.as_str())
                        .unwrap_or("");
                    if id.chars().all(|c| c.is_ascii_digit())
                        && a.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    {
                        Some(format!(
                            "https://cdn.discordapp.com/avatars/{}/{}.png",
                            id, a
                        ))
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

    async fn send_reaction(
        &self,
        message_id: &str,
        emoji: &str,
        _metadata: &HashMap<String, String>,
    ) -> ChannelResult<()> {
        // message_id is raw Discord message ID; channel_id from metadata or first configured
        let channel_id = _metadata
            .get("channel_id")
            .or_else(|| self.channel_ids.first())
            .ok_or_else(|| ChannelError::MessageError("Channel ID required for reaction".into()))?;
        let encoded = urlencoding::encode(emoji);
        self.send_api_request(
            "PUT",
            &format!(
                "/channels/{}/messages/{}/reactions/{}/@me",
                channel_id, message_id, encoded
            ),
            None,
        )
        .await?;
        Ok(())
    }

    async fn remove_reaction(&self, message_id: &str, emoji: &str) -> ChannelResult<()> {
        let channel_id = self
            .channel_ids
            .first()
            .ok_or_else(|| ChannelError::MessageError("Channel ID required".into()))?;
        let encoded = urlencoding::encode(emoji);
        self.send_api_request(
            "DELETE",
            &format!(
                "/channels/{}/messages/{}/reactions/{}/@me",
                channel_id, message_id, encoded
            ),
            None,
        )
        .await?;
        Ok(())
    }

    async fn create_thread(&self, message_id: &str, name: Option<&str>) -> ChannelResult<String> {
        let channel_id = self
            .channel_ids
            .first()
            .ok_or_else(|| ChannelError::MessageError("Channel ID required".into()))?;
        let body = serde_json::json!({
            "name": name.unwrap_or("Thread"),
            "auto_archive_duration": 1440,
        });
        let resp = self
            .send_api_request(
                "POST",
                &format!("/channels/{}/messages/{}/threads", channel_id, message_id),
                Some(&body),
            )
            .await?;
        Ok(resp
            .get("id")
            .and_then(|id| id.as_str())
            .unwrap_or_default()
            .to_string())
    }

    async fn send_thread_reply(
        &self,
        thread_id: &str,
        message: &ChannelMessage,
    ) -> ChannelResult<String> {
        let mut body = serde_json::json!({"content": message.content});
        let reply_to = message
            .metadata
            .get("reply_to")
            .or_else(|| message.metadata.get("replyTo"))
            .or_else(|| message.metadata.get("message_id"))
            .or_else(|| message.metadata.get("messageId"))
            .map(String::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToString::to_string);
        if let Some(reply) = reply_to {
            body["message_reference"] = serde_json::json!({
                "message_id": reply,
                "fail_if_not_exists": false
            });
        }
        let resp = self
            .send_api_request(
                "POST",
                &format!("/channels/{}/messages", thread_id),
                Some(&body),
            )
            .await?;
        Ok(resp
            .get("id")
            .and_then(|id| id.as_str())
            .unwrap_or_default()
            .to_string())
    }

    async fn edit_message(&self, message_id: &str, content: &str) -> ChannelResult<()> {
        let channel_id = self
            .channel_ids
            .first()
            .ok_or_else(|| ChannelError::MessageError("Channel ID required".into()))?;
        let body = serde_json::json!({"content": content});
        self.send_api_request(
            "PATCH",
            &format!("/channels/{}/messages/{}", channel_id, message_id),
            Some(&body),
        )
        .await?;
        Ok(())
    }

    async fn delete_message(&self, message_id: &str) -> ChannelResult<()> {
        let channel_id = self
            .channel_ids
            .first()
            .ok_or_else(|| ChannelError::MessageError("Channel ID required".into()))?;
        self.send_api_request(
            "DELETE",
            &format!("/channels/{}/messages/{}", channel_id, message_id),
            None,
        )
        .await?;
        Ok(())
    }

    async fn send_media(&self, target: &str, media: &ChannelMedia) -> ChannelResult<String> {
        let caption = media.caption.as_deref();
        let reply_to = None;
        let uploaded = match &media.data {
            MediaData::Bytes(bytes) => {
                let filename = media
                    .filename
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .unwrap_or("upload.bin")
                    .to_string();
                let mime = media
                    .mime_type
                    .as_deref()
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(ToString::to_string)
                    .unwrap_or_else(|| Self::guess_mime_from_name(&filename));
                Some((bytes.clone(), filename, mime))
            }
            MediaData::Url(raw) | MediaData::FileId(raw) => {
                match self
                    .resolve_binary_source(raw, media.filename.as_deref())
                    .await
                {
                    Ok((bytes, filename, mime)) => Some((bytes, filename, mime)),
                    Err(_) => None,
                }
            }
        };
        if let Some((bytes, filename, mime)) = uploaded {
            let resp = self
                .send_binary_message(target, caption, bytes, &filename, Some(&mime), reply_to)
                .await?;
            return Ok(resp
                .get("id")
                .and_then(|id| id.as_str())
                .unwrap_or_default()
                .to_string());
        }
        let url_str = match &media.data {
            MediaData::Url(u) => u.clone(),
            MediaData::FileId(id) => id.clone(),
            MediaData::Bytes(_) => {
                return Err(ChannelError::MessageError(
                    "Discord media payload could not be materialized".into(),
                ));
            }
        };
        let body = serde_json::json!({
            "content": caption.unwrap_or(""),
            "embeds": [{"image": {"url": url_str}}],
        });
        let resp = self
            .send_api_request(
                "POST",
                &format!("/channels/{}/messages", target),
                Some(&body),
            )
            .await?;
        Ok(resp
            .get("id")
            .and_then(|id| id.as_str())
            .unwrap_or_default()
            .to_string())
    }

    async fn list_members(&self, _group_id: &str) -> ChannelResult<Vec<ChannelAccount>> {
        let guild_id = self
            .guild_id
            .as_deref()
            .ok_or_else(|| ChannelError::ConfigError("Guild ID required".into()))?;
        let resp = self
            .send_api_request(
                "GET",
                &format!("/guilds/{}/members?limit=100", guild_id),
                None,
            )
            .await?;
        let members = resp
            .as_array()
            .ok_or_else(|| ChannelError::MessageError("Unexpected response".into()))?;
        Ok(members
            .iter()
            .filter_map(|m| {
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
            })
            .collect())
    }

    async fn list_groups(&self) -> ChannelResult<Vec<GroupInfo>> {
        let guild_id = self
            .guild_id
            .as_deref()
            .ok_or_else(|| ChannelError::ConfigError("Guild ID required".into()))?;
        let resp = self
            .send_api_request("GET", &format!("/guilds/{}/channels", guild_id), None)
            .await?;
        let channels = resp
            .as_array()
            .ok_or_else(|| ChannelError::MessageError("Unexpected response".into()))?;
        Ok(channels
            .iter()
            .filter_map(|c| {
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
            })
            .collect())
    }

    async fn send_poll(&self, target: &str, poll: &PollRequest) -> ChannelResult<String> {
        let answers: Vec<serde_json::Value> = poll
            .options
            .iter()
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
        let resp = self
            .send_api_request(
                "POST",
                &format!("/channels/{}/messages", target),
                Some(&body),
            )
            .await?;
        Ok(resp
            .get("id")
            .and_then(|id| id.as_str())
            .unwrap_or_default()
            .to_string())
    }

    async fn list_reactions(
        &self,
        target: Option<&str>,
        message_id: &str,
        limit: Option<usize>,
    ) -> ChannelResult<serde_json::Value> {
        let channel_id = target
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .or_else(|| self.channel_ids.first().map(String::as_str))
            .ok_or_else(|| ChannelError::MessageError("Channel ID required".into()))?;
        let resp = self
            .send_api_request(
                "GET",
                &format!("/channels/{}/messages/{}", channel_id, message_id),
                None,
            )
            .await?;
        let mut items = resp
            .get("reactions")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if let Some(max) = limit {
            items.truncate(max);
        }
        Ok(serde_json::json!({
            "message_id": message_id,
            "channel_id": channel_id,
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
        let mut params: Vec<String> = Vec::new();
        let max_limit = limit.unwrap_or(20).clamp(1, 100);
        params.push(format!("limit={}", max_limit));
        if let Some(v) = before.map(str::trim).filter(|v| !v.is_empty()) {
            params.push(format!("before={}", urlencoding::encode(v)));
        }
        if let Some(v) = after.map(str::trim).filter(|v| !v.is_empty()) {
            params.push(format!("after={}", urlencoding::encode(v)));
        }
        if let Some(v) = around.map(str::trim).filter(|v| !v.is_empty()) {
            params.push(format!("around={}", urlencoding::encode(v)));
        }
        let query = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };
        let resp = self
            .send_api_request(
                "GET",
                &format!("/channels/{}/messages{}", target, query),
                None,
            )
            .await?;
        let items = resp.as_array().cloned().unwrap_or_default();
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
        let guild_id = self
            .guild_id
            .as_deref()
            .ok_or_else(|| ChannelError::UnsupportedOperation("search requires guild_id".into()))?;
        let trimmed_query = query.trim();
        if trimmed_query.is_empty() {
            return Err(ChannelError::MessageError("query required".into()));
        }
        let mut params: Vec<String> =
            vec![format!("content={}", urlencoding::encode(trimmed_query))];
        if let Some(channel_id) = target.map(str::trim).filter(|v| !v.is_empty()) {
            params.push(format!("channel_id={}", urlencoding::encode(channel_id)));
        }
        let max_limit = limit.unwrap_or(20).clamp(1, 25);
        params.push(format!("limit={}", max_limit));
        let resp = self
            .send_api_request(
                "GET",
                &format!("/guilds/{}/messages/search?{}", guild_id, params.join("&")),
                None,
            )
            .await?;
        Ok(resp)
    }

    async fn pin_message(&self, target: &str, message_id: &str) -> ChannelResult<()> {
        self.send_api_request(
            "PUT",
            &format!("/channels/{}/pins/{}", target, message_id),
            None,
        )
        .await?;
        Ok(())
    }

    async fn unpin_message(&self, target: &str, message_id: &str) -> ChannelResult<()> {
        self.send_api_request(
            "DELETE",
            &format!("/channels/{}/pins/{}", target, message_id),
            None,
        )
        .await?;
        Ok(())
    }

    async fn list_pins(
        &self,
        target: &str,
        limit: Option<usize>,
    ) -> ChannelResult<serde_json::Value> {
        let resp = self
            .send_api_request("GET", &format!("/channels/{}/pins", target), None)
            .await?;
        let mut items = resp.as_array().cloned().unwrap_or_default();
        if let Some(max) = limit {
            items.truncate(max);
        }
        Ok(serde_json::json!({
            "channel_id": target,
            "items": items
        }))
    }

    async fn get_permissions(&self, target: &str) -> ChannelResult<serde_json::Value> {
        let resp = self
            .send_api_request("GET", &format!("/channels/{}", target), None)
            .await?;
        Ok(serde_json::json!({
            "channel_id": target,
            "permission_overwrites": resp.get("permission_overwrites").cloned().unwrap_or(serde_json::Value::Array(vec![])),
            "nsfw": resp.get("nsfw").cloned(),
            "rate_limit_per_user": resp.get("rate_limit_per_user").cloned(),
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
        let resolved_target = target
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToString::to_string)
            .or_else(|| {
                Self::payload_string(payload, &["target", "to", "channel_id", "channelId"])
            });
        let payload_guild_id = Self::payload_string(payload, &["guild_id", "guildId"]);
        let guild_id = payload_guild_id.as_deref().or(self.guild_id.as_deref());
        let payload_account_id = Self::payload_string(payload, &["account_id", "accountId"]);

        match action.as_str() {
            "channel_info" => {
                let channel_id = resolved_target
                    .as_deref()
                    .ok_or_else(|| ChannelError::MessageError("target channel required".into()))?;
                self.send_api_request("GET", &format!("/channels/{}", channel_id), None)
                    .await
            }
            "channel_list" => {
                let gid = guild_id.ok_or_else(|| {
                    ChannelError::UnsupportedOperation("channel_list requires guild_id".into())
                })?;
                self.send_api_request("GET", &format!("/guilds/{}/channels", gid), None)
                    .await
            }
            "channel_create" => {
                let gid = guild_id.ok_or_else(|| {
                    ChannelError::UnsupportedOperation("channel_create requires guild_id".into())
                })?;
                let name = Self::payload_string(payload, &["name", "channel_name", "channelName"])
                    .ok_or_else(|| ChannelError::MessageError("name required".into()))?;
                let ctype = Self::channel_type_id(
                    Self::payload_string(payload, &["channel_type", "channelType"]).as_deref(),
                    0,
                );
                let mut body = serde_json::json!({
                    "name": name,
                    "type": ctype,
                });
                if let Some(topic) = Self::payload_string(payload, &["topic"]) {
                    body["topic"] = serde_json::Value::String(topic);
                }
                if let Some(parent_id) = Self::payload_string(payload, &["parent_id", "parentId"]) {
                    body["parent_id"] = serde_json::Value::String(parent_id);
                }
                if let Some(pos) =
                    Self::payload_usize(payload, &["position"]).and_then(|v| i64::try_from(v).ok())
                {
                    body["position"] = serde_json::Value::Number(pos.into());
                }
                self.send_api_request("POST", &format!("/guilds/{}/channels", gid), Some(&body))
                    .await
            }
            "channel_edit" => {
                let channel_id = resolved_target
                    .as_deref()
                    .ok_or_else(|| ChannelError::MessageError("target channel required".into()))?;
                let mut body = serde_json::json!({});
                if let Some(name) =
                    Self::payload_string(payload, &["name", "channel_name", "channelName"])
                {
                    body["name"] = serde_json::Value::String(name);
                }
                if let Some(topic) = Self::payload_string(payload, &["topic"]) {
                    body["topic"] = serde_json::Value::String(topic);
                }
                if let Some(parent_id) = Self::payload_string(payload, &["parent_id", "parentId"]) {
                    body["parent_id"] = serde_json::Value::String(parent_id);
                }
                if let Some(pos) =
                    Self::payload_usize(payload, &["position"]).and_then(|v| i64::try_from(v).ok())
                {
                    body["position"] = serde_json::Value::Number(pos.into());
                }
                self.send_api_request("PATCH", &format!("/channels/{}", channel_id), Some(&body))
                    .await
            }
            "channel_delete" => {
                let channel_id = resolved_target
                    .as_deref()
                    .ok_or_else(|| ChannelError::MessageError("target channel required".into()))?;
                self.send_api_request("DELETE", &format!("/channels/{}", channel_id), None)
                    .await
            }
            "channel_move" => {
                let gid = guild_id.ok_or_else(|| {
                    ChannelError::UnsupportedOperation("channel_move requires guild_id".into())
                })?;
                let channel_id = resolved_target
                    .as_deref()
                    .ok_or_else(|| ChannelError::MessageError("target channel required".into()))?;
                let position = Self::payload_usize(payload, &["position"])
                    .ok_or_else(|| ChannelError::MessageError("position required".into()))?;
                let mut entry = serde_json::json!({
                    "id": channel_id,
                    "position": position
                });
                if let Some(parent_id) = Self::payload_string(payload, &["parent_id", "parentId"]) {
                    entry["parent_id"] = serde_json::Value::String(parent_id);
                }
                let body = serde_json::json!([entry]);
                self.send_api_request("PATCH", &format!("/guilds/{}/channels", gid), Some(&body))
                    .await
            }
            "channel_permission_set" => {
                let channel_id = resolved_target
                    .as_deref()
                    .ok_or_else(|| ChannelError::MessageError("target channel required".into()))?;
                let target_id = Self::payload_string(
                    payload,
                    &["target_id", "targetId", "overwrite_id", "overwriteId"],
                )
                .or_else(|| Self::payload_string(payload, &["role_id", "roleId"]))
                .or_else(|| Self::payload_string(payload, &["user_id", "userId"]))
                .ok_or_else(|| ChannelError::MessageError("target_id required".into()))?;
                let target_type = Self::payload_string(payload, &["target_type", "targetType"])
                    .unwrap_or_else(|| "role".to_string());
                let overwrite_type = if target_type.eq_ignore_ascii_case("member")
                    || target_type.eq_ignore_ascii_case("user")
                {
                    1
                } else {
                    0
                };
                let allow = Self::payload_string(payload, &["allow"]).unwrap_or_default();
                let deny = Self::payload_string(payload, &["deny"]).unwrap_or_default();
                let body = serde_json::json!({
                    "type": overwrite_type,
                    "allow": allow,
                    "deny": deny
                });
                self.send_api_request(
                    "PUT",
                    &format!("/channels/{}/permissions/{}", channel_id, target_id),
                    Some(&body),
                )
                .await
            }
            "channel_permission_remove" => {
                let channel_id = resolved_target
                    .as_deref()
                    .ok_or_else(|| ChannelError::MessageError("target channel required".into()))?;
                let target_id = Self::payload_string(
                    payload,
                    &["target_id", "targetId", "overwrite_id", "overwriteId"],
                )
                .or_else(|| Self::payload_string(payload, &["role_id", "roleId"]))
                .or_else(|| Self::payload_string(payload, &["user_id", "userId"]))
                .ok_or_else(|| ChannelError::MessageError("target_id required".into()))?;
                self.send_api_request(
                    "DELETE",
                    &format!("/channels/{}/permissions/{}", channel_id, target_id),
                    None,
                )
                .await
            }
            "category_create" => {
                let gid = guild_id.ok_or_else(|| {
                    ChannelError::UnsupportedOperation("category_create requires guild_id".into())
                })?;
                let name =
                    Self::payload_string(payload, &["name", "category_name", "categoryName"])
                        .ok_or_else(|| ChannelError::MessageError("name required".into()))?;
                let body = serde_json::json!({
                    "name": name,
                    "type": 4
                });
                self.send_api_request("POST", &format!("/guilds/{}/channels", gid), Some(&body))
                    .await
            }
            "category_edit" => {
                let category_id = resolved_target
                    .as_deref()
                    .ok_or_else(|| ChannelError::MessageError("target category required".into()))?;
                let name =
                    Self::payload_string(payload, &["name", "category_name", "categoryName"])
                        .ok_or_else(|| ChannelError::MessageError("name required".into()))?;
                let body = serde_json::json!({ "name": name });
                self.send_api_request("PATCH", &format!("/channels/{}", category_id), Some(&body))
                    .await
            }
            "category_delete" => {
                let category_id = resolved_target
                    .as_deref()
                    .ok_or_else(|| ChannelError::MessageError("target category required".into()))?;
                self.send_api_request("DELETE", &format!("/channels/{}", category_id), None)
                    .await
            }
            "topic_create" => {
                let channel_id = resolved_target
                    .as_deref()
                    .ok_or_else(|| ChannelError::MessageError("target channel required".into()))?;
                let topic = Self::payload_string(payload, &["topic", "text", "message", "content"])
                    .ok_or_else(|| ChannelError::MessageError("topic required".into()))?;
                let body = serde_json::json!({ "topic": topic });
                self.send_api_request("PATCH", &format!("/channels/{}", channel_id), Some(&body))
                    .await
            }
            "thread_create" => {
                let channel_id = resolved_target
                    .as_deref()
                    .ok_or_else(|| ChannelError::MessageError("target channel required".into()))?;
                let thread_name =
                    Self::payload_string(payload, &["thread_name", "threadName", "name"])
                        .ok_or_else(|| ChannelError::MessageError("thread_name required".into()))?;
                let message_id = Self::payload_string(payload, &["message_id", "messageId"]);
                let content = Self::payload_string(payload, &["text", "message", "content"])
                    .filter(|v| {
                        let trimmed = v.trim();
                        !trimmed.is_empty()
                    });
                let auto_archive_minutes = Self::payload_usize(
                    payload,
                    &[
                        "auto_archive_minutes",
                        "autoArchiveMinutes",
                        "autoArchiveMin",
                        "auto_archive_duration",
                        "autoArchiveDuration",
                    ],
                )
                .and_then(|v| i64::try_from(v).ok());
                let thread_type =
                    Self::payload_usize(payload, &["type", "thread_type", "threadType"])
                        .and_then(|v| i64::try_from(v).ok())
                        .or_else(|| {
                            Self::payload_string(payload, &["type", "thread_type", "threadType"])
                                .map(|raw| Self::channel_type_id(Some(raw.as_str()), 11))
                        });

                let mut body = serde_json::json!({ "name": thread_name.clone() });
                if let Some(minutes) = auto_archive_minutes {
                    body["auto_archive_duration"] = serde_json::Value::Number(minutes.into());
                }

                if let Some(mid) = message_id {
                    let resp = self
                        .send_api_request(
                            "POST",
                            &format!("/channels/{}/messages/{}/threads", channel_id, mid),
                            Some(&body),
                        )
                        .await?;
                    return Ok(serde_json::json!({ "thread": resp }));
                }

                if let Some(kind) = thread_type {
                    body["type"] = serde_json::Value::Number(kind.into());
                }

                let channel_type = self
                    .send_api_request("GET", &format!("/channels/{}", channel_id), None)
                    .await
                    .ok()
                    .and_then(|raw| raw.get("type").and_then(|v| v.as_i64()));
                let is_forum_like = matches!(channel_type, Some(15 | 16));
                if is_forum_like {
                    let starter = content.clone().unwrap_or_else(|| thread_name.clone());
                    body["message"] = serde_json::json!({ "content": starter });
                } else if body.get("type").is_none() {
                    body["type"] = serde_json::Value::Number(11.into());
                }

                let thread = self
                    .send_api_request(
                        "POST",
                        &format!("/channels/{}/threads", channel_id),
                        Some(&body),
                    )
                    .await?;
                if !is_forum_like
                    && let Some(starter) = content
                    && let Some(thread_id) = thread.get("id").and_then(|v| v.as_str())
                {
                    let starter_body = serde_json::json!({ "content": starter });
                    let _ = self
                        .send_api_request(
                            "POST",
                            &format!("/channels/{}/messages", thread_id),
                            Some(&starter_body),
                        )
                        .await?;
                }
                Ok(serde_json::json!({ "thread": thread }))
            }
            "thread_list" => {
                let gid = guild_id.ok_or_else(|| {
                    ChannelError::UnsupportedOperation("thread_list requires guild_id".into())
                })?;
                let include_archived =
                    Self::payload_bool(payload, &["include_archived", "includeArchived"])
                        .unwrap_or(false);
                if include_archived {
                    let channel_id = resolved_target.as_deref().ok_or_else(|| {
                        ChannelError::MessageError(
                            "thread_list include_archived=true requires target channel".into(),
                        )
                    })?;
                    let limit = Self::payload_usize(payload, &["limit"])
                        .unwrap_or(50)
                        .clamp(1, 100);
                    let before = Self::payload_string(payload, &["before"]);
                    let mut endpoint = format!(
                        "/channels/{}/threads/archived/public?limit={}",
                        channel_id, limit
                    );
                    if let Some(cursor) = before {
                        endpoint.push_str("&before=");
                        endpoint.push_str(&urlencoding::encode(&cursor));
                    }
                    self.send_api_request("GET", &endpoint, None).await
                } else {
                    let _ = resolved_target;
                    self.send_api_request("GET", &format!("/guilds/{}/threads/active", gid), None)
                        .await
                }
            }
            "add_participant" => {
                let thread_id = resolved_target
                    .as_deref()
                    .ok_or_else(|| ChannelError::MessageError("target thread required".into()))?;
                let user_id = Self::payload_string(
                    payload,
                    &[
                        "user_id",
                        "userId",
                        "participant_id",
                        "participantId",
                        "member_id",
                        "memberId",
                    ],
                )
                .ok_or_else(|| ChannelError::MessageError("user_id required".into()))?;
                self.send_api_request(
                    "PUT",
                    &format!("/channels/{}/thread-members/{}", thread_id, user_id),
                    None,
                )
                .await
            }
            "remove_participant" => {
                let thread_id = resolved_target
                    .as_deref()
                    .ok_or_else(|| ChannelError::MessageError("target thread required".into()))?;
                let user_id = Self::payload_string(
                    payload,
                    &[
                        "user_id",
                        "userId",
                        "participant_id",
                        "participantId",
                        "member_id",
                        "memberId",
                    ],
                )
                .ok_or_else(|| ChannelError::MessageError("user_id required".into()))?;
                self.send_api_request(
                    "DELETE",
                    &format!("/channels/{}/thread-members/{}", thread_id, user_id),
                    None,
                )
                .await
            }
            "leave_group" => {
                let thread_id = resolved_target
                    .as_deref()
                    .ok_or_else(|| ChannelError::MessageError("target thread required".into()))?;
                self.send_api_request(
                    "DELETE",
                    &format!("/channels/{}/thread-members/@me", thread_id),
                    None,
                )
                .await
            }
            "member_info" => {
                let gid = guild_id.ok_or_else(|| {
                    ChannelError::UnsupportedOperation("member_info requires guild_id".into())
                })?;
                let user_id =
                    Self::payload_string(payload, &["user_id", "userId", "member_id", "memberId"])
                        .or_else(|| resolved_target.clone())
                        .ok_or_else(|| ChannelError::MessageError("user_id required".into()))?;
                let resp = self
                    .send_api_request("GET", &format!("/guilds/{}/members/{}", gid, user_id), None)
                    .await?;
                Ok(serde_json::json!({
                    "member": resp
                }))
            }
            "role_info" => {
                let gid = guild_id.ok_or_else(|| {
                    ChannelError::UnsupportedOperation("role_info requires guild_id".into())
                })?;
                let role_id = Self::payload_string(payload, &["role_id", "roleId"]);
                let resp = self
                    .send_api_request("GET", &format!("/guilds/{}/roles", gid), None)
                    .await?;
                if let Some(rid) = role_id {
                    let filtered = resp
                        .as_array()
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .find(|item| item.get("id").and_then(|v| v.as_str()) == Some(rid.as_str()))
                        .unwrap_or(serde_json::Value::Null);
                    Ok(serde_json::json!({ "role": filtered }))
                } else {
                    Ok(resp)
                }
            }
            "role_add" => {
                let gid = guild_id.ok_or_else(|| {
                    ChannelError::UnsupportedOperation("role_add requires guild_id".into())
                })?;
                let user_id =
                    Self::payload_string(payload, &["user_id", "userId", "member_id", "memberId"])
                        .ok_or_else(|| ChannelError::MessageError("user_id required".into()))?;
                let role_id = Self::payload_string(payload, &["role_id", "roleId"])
                    .ok_or_else(|| ChannelError::MessageError("role_id required".into()))?;
                self.send_api_request(
                    "PUT",
                    &format!("/guilds/{}/members/{}/roles/{}", gid, user_id, role_id),
                    None,
                )
                .await
            }
            "role_remove" => {
                let gid = guild_id.ok_or_else(|| {
                    ChannelError::UnsupportedOperation("role_remove requires guild_id".into())
                })?;
                let user_id =
                    Self::payload_string(payload, &["user_id", "userId", "member_id", "memberId"])
                        .ok_or_else(|| ChannelError::MessageError("user_id required".into()))?;
                let role_id = Self::payload_string(payload, &["role_id", "roleId"])
                    .ok_or_else(|| ChannelError::MessageError("role_id required".into()))?;
                self.send_api_request(
                    "DELETE",
                    &format!("/guilds/{}/members/{}/roles/{}", gid, user_id, role_id),
                    None,
                )
                .await
            }
            "kick_member" => {
                let gid = guild_id.ok_or_else(|| {
                    ChannelError::UnsupportedOperation("kick_member requires guild_id".into())
                })?;
                let user_id =
                    Self::payload_string(payload, &["user_id", "userId", "member_id", "memberId"])
                        .ok_or_else(|| ChannelError::MessageError("user_id required".into()))?;
                self.send_api_request(
                    "DELETE",
                    &format!("/guilds/{}/members/{}", gid, user_id),
                    None,
                )
                .await
            }
            "ban_member" => {
                let gid = guild_id.ok_or_else(|| {
                    ChannelError::UnsupportedOperation("ban_member requires guild_id".into())
                })?;
                let user_id =
                    Self::payload_string(payload, &["user_id", "userId", "member_id", "memberId"])
                        .ok_or_else(|| ChannelError::MessageError("user_id required".into()))?;
                let delete_seconds = Self::payload_usize(
                    payload,
                    &["delete_message_seconds", "deleteMessageSeconds"],
                )
                .unwrap_or(0)
                .clamp(0, 604800);
                let body = serde_json::json!({ "delete_message_seconds": delete_seconds });
                self.send_api_request(
                    "PUT",
                    &format!("/guilds/{}/bans/{}", gid, user_id),
                    Some(&body),
                )
                .await
            }
            "timeout_member" => {
                let gid = guild_id.ok_or_else(|| {
                    ChannelError::UnsupportedOperation("timeout_member requires guild_id".into())
                })?;
                let user_id =
                    Self::payload_string(payload, &["user_id", "userId", "member_id", "memberId"])
                        .ok_or_else(|| ChannelError::MessageError("user_id required".into()))?;
                let minutes =
                    Self::payload_usize(payload, &["duration_minutes", "durationMinutes"])
                        .unwrap_or(10)
                        .clamp(1, 40320);
                let until =
                    (chrono::Utc::now() + chrono::Duration::minutes(minutes as i64)).to_rfc3339();
                let body = serde_json::json!({ "communication_disabled_until": until });
                self.send_api_request(
                    "PATCH",
                    &format!("/guilds/{}/members/{}", gid, user_id),
                    Some(&body),
                )
                .await
            }
            "event_list" => {
                let gid = guild_id.ok_or_else(|| {
                    ChannelError::UnsupportedOperation("event_list requires guild_id".into())
                })?;
                self.send_api_request(
                    "GET",
                    &format!("/guilds/{}/scheduled-events?with_user_count=true", gid),
                    None,
                )
                .await
            }
            "event_create" => {
                let gid = guild_id.ok_or_else(|| {
                    ChannelError::UnsupportedOperation("event_create requires guild_id".into())
                })?;
                let name = Self::payload_string(payload, &["name", "title"])
                    .ok_or_else(|| ChannelError::MessageError("name required".into()))?;
                let start = Self::payload_string(payload, &["start_time", "startTime"])
                    .unwrap_or_else(|| {
                        (chrono::Utc::now() + chrono::Duration::minutes(10)).to_rfc3339()
                    });
                let mut body = serde_json::json!({
                    "name": name,
                    "privacy_level": 2,
                    "scheduled_start_time": start
                });
                if let Some(end) = Self::payload_string(payload, &["end_time", "endTime"]) {
                    body["scheduled_end_time"] = serde_json::Value::String(end);
                }
                if let Some(channel_id) = resolved_target.clone() {
                    body["entity_type"] = serde_json::Value::Number(2.into());
                    body["channel_id"] = serde_json::Value::String(channel_id);
                } else {
                    body["entity_type"] = serde_json::Value::Number(3.into());
                    let location = Self::payload_string(payload, &["location"])
                        .unwrap_or_else(|| "External".to_string());
                    body["entity_metadata"] = serde_json::json!({ "location": location });
                    if body.get("scheduled_end_time").is_none() {
                        body["scheduled_end_time"] = serde_json::Value::String(
                            (chrono::Utc::now() + chrono::Duration::hours(1)).to_rfc3339(),
                        );
                    }
                }
                self.send_api_request(
                    "POST",
                    &format!("/guilds/{}/scheduled-events", gid),
                    Some(&body),
                )
                .await
            }
            "emoji_list" => {
                let gid = guild_id.ok_or_else(|| {
                    ChannelError::UnsupportedOperation("emoji_list requires guild_id".into())
                })?;
                self.send_api_request("GET", &format!("/guilds/{}/emojis", gid), None)
                    .await
            }
            "emoji_upload" => {
                let gid = guild_id.ok_or_else(|| {
                    ChannelError::UnsupportedOperation("emoji_upload requires guild_id".into())
                })?;
                let name = Self::payload_string(payload, &["name"])
                    .ok_or_else(|| ChannelError::MessageError("name required".into()))?;
                let image_raw = Self::payload_string(
                    payload,
                    &["image", "media", "path", "filePath", "buffer"],
                )
                .ok_or_else(|| ChannelError::MessageError("image/media required".into()))?;
                let image_data = if image_raw.starts_with("data:") {
                    image_raw
                } else {
                    let filename_hint = Self::payload_string(payload, &["filename"]);
                    let (bytes, filename, mime) = self
                        .resolve_binary_source(&image_raw, filename_hint.as_deref())
                        .await?;
                    let mime_type = Self::payload_string(payload, &["mime_type", "mimeType"])
                        .unwrap_or_else(|| {
                            if mime == "application/octet-stream" {
                                Self::guess_mime_from_name(&filename)
                            } else {
                                mime
                            }
                        });
                    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                    format!("data:{};base64,{}", mime_type, encoded)
                };
                let body = serde_json::json!({
                    "name": name,
                    "image": image_data
                });
                self.send_api_request("POST", &format!("/guilds/{}/emojis", gid), Some(&body))
                    .await
            }
            "sticker_search" => {
                let gid = guild_id.ok_or_else(|| {
                    ChannelError::UnsupportedOperation("sticker_search requires guild_id".into())
                })?;
                let query = Self::payload_string(payload, &["query", "name"]).unwrap_or_default();
                let limit = Self::payload_usize(payload, &["limit"]).unwrap_or(20);
                let resp = self
                    .send_api_request("GET", &format!("/guilds/{}/stickers", gid), None)
                    .await?;
                let mut items = resp
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|row| {
                        if query.trim().is_empty() {
                            return true;
                        }
                        row.get("name")
                            .and_then(|v| v.as_str())
                            .map(|name| {
                                name.to_ascii_lowercase()
                                    .contains(&query.to_ascii_lowercase())
                            })
                            .unwrap_or(false)
                    })
                    .collect::<Vec<serde_json::Value>>();
                items.truncate(limit);
                Ok(serde_json::json!({ "items": items }))
            }
            "sticker_upload" => {
                let gid = guild_id.ok_or_else(|| {
                    ChannelError::UnsupportedOperation("sticker_upload requires guild_id".into())
                })?;
                let name = Self::payload_string(payload, &["name"])
                    .ok_or_else(|| ChannelError::MessageError("name required".into()))?;
                let tags = Self::payload_string(payload, &["tags", "tag"])
                    .unwrap_or_else(|| "sticker".to_string());
                let media = Self::payload_string(payload, &["media", "path", "filePath", "buffer"])
                    .ok_or_else(|| {
                        ChannelError::MessageError("media/path/filePath required".into())
                    })?;
                let filename_hint = Self::payload_string(payload, &["filename"]);
                let (bytes, filename, detected_mime) = self
                    .resolve_binary_source(&media, filename_hint.as_deref())
                    .await?;
                let mime = Self::payload_string(payload, &["mime_type", "mimeType"])
                    .unwrap_or(detected_mime);
                let token = self.bot_token.as_ref().ok_or_else(|| {
                    ChannelError::AuthenticationError("Bot token not set".to_string())
                })?;
                let client = self.client.as_ref().ok_or_else(|| {
                    ChannelError::ConnectionError("Client not initialized".to_string())
                })?;
                let file_part = reqwest::multipart::Part::bytes(bytes)
                    .file_name(filename)
                    .mime_str(&mime)
                    .map_err(|e| ChannelError::MessageError(e.to_string()))?;
                let mut form = reqwest::multipart::Form::new()
                    .part("file", file_part)
                    .text("name", name)
                    .text("tags", tags);
                if let Some(description) = Self::payload_string(payload, &["description"]) {
                    form = form.text("description", description);
                }
                let url = format!("{}/guilds/{}/stickers", self.api_url, gid);
                let response = client
                    .post(url)
                    .header("Authorization", format!("Bot {}", token))
                    .header("User-Agent", "OCLAWS/1.0")
                    .multipart(form)
                    .send()
                    .await
                    .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;
                let status = response.status();
                let json: serde_json::Value = response
                    .json()
                    .await
                    .map_err(|e| ChannelError::MessageError(e.to_string()))?;
                if status.is_success() {
                    Ok(json)
                } else {
                    Err(ChannelError::MessageError(
                        json.get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Discord sticker upload failed")
                            .to_string(),
                    ))
                }
            }
            "sticker" | "send_sticker" => {
                let channel_id = resolved_target
                    .as_deref()
                    .ok_or_else(|| ChannelError::MessageError("target channel required".into()))?;
                let sticker_id = Self::payload_string(payload, &["sticker_id", "stickerId", "id"])
                    .ok_or_else(|| ChannelError::MessageError("sticker_id required".into()))?;
                let content = Self::payload_string(payload, &["text", "message", "content"]);
                let mut body = serde_json::json!({
                    "sticker_ids": [sticker_id]
                });
                if let Some(text) = content {
                    body["content"] = serde_json::Value::String(text);
                }
                self.send_api_request(
                    "POST",
                    &format!("/channels/{}/messages", channel_id),
                    Some(&body),
                )
                .await
            }
            "voice_status" => {
                let gid = guild_id.ok_or_else(|| {
                    ChannelError::UnsupportedOperation("voice_status requires guild_id".into())
                })?;
                let user_id = Self::payload_string(payload, &["user_id", "userId"])
                    .or_else(|| resolved_target.clone())
                    .ok_or_else(|| ChannelError::MessageError("user_id required".into()))?;
                self.send_api_request(
                    "GET",
                    &format!("/guilds/{}/voice-states/{}", gid, user_id),
                    None,
                )
                .await
            }
            "rename_group" => {
                let channel_id = resolved_target
                    .as_deref()
                    .ok_or_else(|| ChannelError::MessageError("target channel required".into()))?;
                let name = Self::payload_string(payload, &["name", "group_name", "groupName"])
                    .ok_or_else(|| ChannelError::MessageError("name required".into()))?;
                let body = serde_json::json!({ "name": name });
                self.send_api_request("PATCH", &format!("/channels/{}", channel_id), Some(&body))
                    .await
            }
            "set_group_icon" => {
                let channel_id = resolved_target
                    .as_deref()
                    .ok_or_else(|| ChannelError::MessageError("target channel required".into()))?;
                let icon_raw =
                    Self::payload_string(payload, &["icon", "icon_data", "iconData", "media"])
                        .ok_or_else(|| ChannelError::MessageError("icon data required".into()))?;
                let icon = if icon_raw.starts_with("data:") {
                    icon_raw
                } else {
                    let (bytes, filename, mime) =
                        self.resolve_binary_source(&icon_raw, None).await?;
                    let mime_type = if mime == "application/octet-stream" {
                        Self::guess_mime_from_name(&filename)
                    } else {
                        mime
                    };
                    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                    format!("data:{};base64,{}", mime_type, encoded)
                };
                let body = serde_json::json!({ "icon": icon });
                self.send_api_request("PATCH", &format!("/channels/{}", channel_id), Some(&body))
                    .await
            }
            "set_presence" => {
                let status = Self::payload_string(payload, &["status"])
                    .unwrap_or_else(|| "online".to_string());
                let normalized_status = status.trim().to_ascii_lowercase();
                if !matches!(
                    normalized_status.as_str(),
                    "online" | "dnd" | "idle" | "invisible"
                ) {
                    return Err(ChannelError::MessageError(format!(
                        "Invalid status '{}'. Must be online/dnd/idle/invisible",
                        status
                    )));
                }
                let activity_type_raw =
                    Self::payload_string(payload, &["activity_type", "activityType"]);
                let activity_name =
                    Self::payload_string(payload, &["activity_name", "activityName"]);
                if activity_name.is_some() && activity_type_raw.is_none() {
                    return Err(ChannelError::MessageError(
                        "activityType is required when activityName is provided".into(),
                    ));
                }
                let mut activities = Vec::new();
                if let Some(raw_type) = activity_type_raw {
                    let atype = Self::activity_type_id(&raw_type).ok_or_else(|| {
                        ChannelError::MessageError(
                            "Invalid activityType. Use playing/streaming/listening/watching/custom/competing"
                                .into(),
                        )
                    })?;
                    let mut activity = serde_json::json!({
                        "type": atype,
                        "name": activity_name.clone().unwrap_or_default()
                    });
                    if atype == 1
                        && let Some(url) =
                            Self::payload_string(payload, &["activity_url", "activityUrl"])
                    {
                        activity["url"] = serde_json::Value::String(url);
                    }
                    if let Some(state) =
                        Self::payload_string(payload, &["activity_state", "activityState"])
                    {
                        activity["state"] = serde_json::Value::String(state);
                    }
                    activities.push(activity);
                }
                let presence = DiscordPresenceUpdate {
                    status: normalized_status.clone(),
                    activities: activities.clone(),
                };
                let handle = self
                    .ensure_presence_handle(payload_account_id.as_deref())
                    .await?;
                handle.tx.send(presence).map_err(|_| {
                    ChannelError::ConnectionError("Discord gateway not connected".into())
                })?;
                Ok(serde_json::json!({
                    "ok": true,
                    "status": normalized_status,
                    "activities": activities
                }))
            }
            "send_with_effect" => Err(ChannelError::UnsupportedOperation(
                "send_with_effect is not supported by Discord REST API".into(),
            )),
            _ => Err(ChannelError::UnsupportedOperation(format!(
                "action:{}",
                action
            ))),
        }
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
    fn send<'a>(
        &'a self,
        content: &'a str,
        metadata: HashMap<String, String>,
    ) -> Pin<Box<dyn std::future::Future<Output = ChannelResult<String>> + Send + 'a>> {
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
