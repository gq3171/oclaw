use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;

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
    
    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>>;
    
    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()>;
    
    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>>;
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
    ConfigurationError(String),
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
            Self::ConfigurationError(s) => write!(f, "Configuration error: {}", s),
        }
    }
}

impl std::error::Error for ChannelError {}

pub type ChannelResult<T> = Result<T, ChannelError>;
