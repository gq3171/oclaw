use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;

use crate::types::{ChannelCapabilities, ChannelMedia, ChatType, GroupInfo, PollRequest};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMessage {
    pub id: String,
    pub channel: String,
    pub sender: String,
    pub content: String,
    pub timestamp: i64,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMessageWithAttachments {
    pub message: ChannelMessage,
    pub attachments: Vec<ChannelAttachment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelAttachment {
    pub id: Option<String>,
    pub url: String,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub size: Option<i64>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub duration: Option<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelAccount {
    pub id: String,
    pub name: String,
    pub channel: String,
    pub avatar: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelEvent {
    pub event_type: String,
    pub channel: String,
    pub payload: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelStatus {
    Disconnected,
    Connecting,
    Connected,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypingStatus {
    Started,
    Stopped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMetadata {
    pub name: String,
    pub version: String,
    pub author: Option<String>,
    pub description: Option<String>,
    pub capabilities: Vec<String>,
}

pub trait ChannelPluginConfig: Send + Sync + Clone {
    fn validate(&self) -> Result<(), String>;
    fn plugin_type(&self) -> &str;
}

#[async_trait]
pub trait ChannelPlugin: Send + Sync {
    type Config: ChannelPluginConfig;

    fn metadata(&self) -> PluginMetadata;

    async fn validate_config(config: &Self::Config) -> Result<(), String>;

    async fn on_connect(&self) -> Result<(), String> {
        Ok(())
    }

    async fn on_disconnect(&self) -> Result<(), String> {
        Ok(())
    }

    async fn on_message(&self, _message: &ChannelMessage) -> Result<Option<ChannelMessage>, String> {
        Ok(None)
    }

    async fn on_error(&self, error: &str) -> Result<(), String> {
        tracing::warn!("Channel plugin error: {}", error);
        Ok(())
    }
}

#[async_trait]
pub trait Channel: Send + Sync {
    fn channel_type(&self) -> &str;
    
    async fn connect(&mut self) -> ChannelResult<()>;
    
    async fn disconnect(&mut self) -> ChannelResult<()>;
    
    fn status(&self) -> ChannelStatus;
    
    async fn send_message(&self, message: &ChannelMessage) -> ChannelResult<String>;
    
    async fn send_message_with_attachments(&self, message: &ChannelMessageWithAttachments) -> ChannelResult<String> {
        self.send_message(&message.message).await
    }
    
    async fn send_typing_status(&self, _user_id: &str, _status: TypingStatus) -> ChannelResult<()> {
        Ok(())
    }

    /// Send a reaction (emoji) to a message. Default: no-op.
    async fn send_reaction(&self, _message_id: &str, _emoji: &str, _metadata: &HashMap<String, String>) -> ChannelResult<()> {
        Ok(())
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>>;

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()>;

    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>>;

    // ── Adapter methods (Phase 1) ──────────────────────────────────

    /// Declare what this channel supports.
    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities::default()
    }

    /// The chat type this channel instance represents, if known.
    fn chat_type(&self) -> Option<ChatType> {
        None
    }

    /// Remove a previously-sent reaction.
    async fn remove_reaction(&self, _message_id: &str, _emoji: &str) -> ChannelResult<()> {
        Err(ChannelError::UnsupportedOperation("reactions".into()))
    }

    /// Reply inside a thread.
    async fn send_thread_reply(&self, _thread_id: &str, _message: &ChannelMessage) -> ChannelResult<String> {
        Err(ChannelError::UnsupportedOperation("threads".into()))
    }

    /// Create a new thread from an existing message.
    async fn create_thread(&self, _message_id: &str, _name: Option<&str>) -> ChannelResult<String> {
        Err(ChannelError::UnsupportedOperation("threads".into()))
    }

    /// Send a media attachment (photo, audio, video, document, etc.).
    async fn send_media(&self, _target: &str, _media: &ChannelMedia) -> ChannelResult<String> {
        Err(ChannelError::UnsupportedOperation("media".into()))
    }

    /// Download media by its platform-specific ID.
    async fn download_media(&self, _media_id: &str) -> ChannelResult<Vec<u8>> {
        Err(ChannelError::UnsupportedOperation("media".into()))
    }

    /// Edit an already-sent message.
    async fn edit_message(&self, _message_id: &str, _content: &str) -> ChannelResult<()> {
        Err(ChannelError::UnsupportedOperation("editing".into()))
    }

    /// Delete a message.
    async fn delete_message(&self, _message_id: &str) -> ChannelResult<()> {
        Err(ChannelError::UnsupportedOperation("deletion".into()))
    }

    /// List members of a group/channel.
    async fn list_members(&self, _group_id: &str) -> ChannelResult<Vec<ChannelAccount>> {
        Err(ChannelError::UnsupportedOperation("directory".into()))
    }

    /// List groups/channels visible to the bot.
    async fn list_groups(&self) -> ChannelResult<Vec<GroupInfo>> {
        Err(ChannelError::UnsupportedOperation("directory".into()))
    }

    /// Send a poll.
    async fn send_poll(&self, _target: &str, _poll: &PollRequest) -> ChannelResult<String> {
        Err(ChannelError::UnsupportedOperation("polls".into()))
    }

    /// Parse an incoming webhook payload into a `WebhookMessage`.
    /// Each channel should override this to extract text and chat_id
    /// from its own platform-specific JSON format.
    /// Default implementation tries common field names.
    fn parse_webhook(&self, payload: &serde_json::Value) -> Option<WebhookMessage> {
        // Try common text fields
        let text = payload.get("text").and_then(|v| v.as_str())
            .or_else(|| payload.get("content").and_then(|v| v.as_str()))
            .or_else(|| payload.get("message").and_then(|v| v.as_str()))
            .or_else(|| payload.get("body").and_then(|v| v.as_str()))
            .or_else(|| payload.pointer("/message/text").and_then(|v| v.as_str()))
            .or_else(|| payload.pointer("/data/text").and_then(|v| v.as_str()))?;

        // Try common chat_id fields
        let chat_id = payload.get("chat_id").and_then(|v| v.as_str())
            .or_else(|| payload.get("channel_id").and_then(|v| v.as_str()))
            .or_else(|| payload.get("room_id").and_then(|v| v.as_str()))
            .or_else(|| payload.get("conversation_id").and_then(|v| v.as_str()))
            .or_else(|| payload.get("user_id").and_then(|v| v.as_str()))
            .or_else(|| payload.get("from").and_then(|v| v.as_str()))
            .unwrap_or("default");

        Some(WebhookMessage {
            text: text.to_string(),
            chat_id: chat_id.to_string(),
            is_group: false,
            has_mention: false,
            metadata: HashMap::new(),
        })
    }
}

/// Parsed webhook message for pipeline processing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookMessage {
    /// The user's text content.
    pub text: String,
    /// Chat/conversation identifier for session tracking.
    pub chat_id: String,
    /// Whether this is a group conversation.
    pub is_group: bool,
    /// Whether the bot was explicitly mentioned.
    pub has_mention: bool,
    /// Extra metadata to pass through to the pipeline.
    pub metadata: HashMap<String, String>,
}

pub trait MessageSender: Send + Sync {
    fn send<'a>(&'a self, content: &'a str, metadata: HashMap<String, String>) -> Pin<Box<dyn std::future::Future<Output = ChannelResult<String>> + Send + 'a>>;
}

#[derive(Debug)]
pub enum ChannelError {
    ConnectionError(String),
    AuthenticationError(String),
    MessageError(String),
    NotFound(String),
    RateLimitError(String),
    ConfigError(String),
    UnsupportedOperation(String),
}

impl std::fmt::Display for ChannelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionError(s) => write!(f, "Connection error: {}", s),
            Self::AuthenticationError(s) => write!(f, "Authentication error: {}", s),
            Self::MessageError(s) => write!(f, "Message error: {}", s),
            Self::NotFound(s) => write!(f, "Not found: {}", s),
            Self::RateLimitError(s) => write!(f, "Rate limit: {}", s),
            Self::ConfigError(s) => write!(f, "Config error: {}", s),
            Self::UnsupportedOperation(s) => write!(f, "Unsupported operation: {}", s),
        }
    }
}

impl std::error::Error for ChannelError {}

pub type ChannelResult<T> = Result<T, ChannelError>;
