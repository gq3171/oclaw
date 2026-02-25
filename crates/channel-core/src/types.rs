use serde::{Deserialize, Serialize};

/// Chat context type — direct message, group, channel, or thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatType {
    Direct,
    Group,
    Channel,
    Thread,
}

impl std::fmt::Display for ChatType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Direct => write!(f, "direct"),
            Self::Group => write!(f, "group"),
            Self::Channel => write!(f, "channel"),
            Self::Thread => write!(f, "thread"),
        }
    }
}

/// Declares what a channel implementation supports.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ChannelCapabilities {
    pub reactions: bool,
    pub threads: bool,
    pub polls: bool,
    pub media: bool,
    pub streaming: bool,
    pub editing: bool,
    pub deletion: bool,
    pub typing_indicator: bool,
    pub read_receipts: bool,
    pub native_commands: bool,
    pub webhooks: bool,
    pub directory: bool,
}

/// Media attachment for sending through a channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMedia {
    pub media_type: MediaType,
    pub data: MediaData,
    pub filename: Option<String>,
    pub caption: Option<String>,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
    Photo,
    Audio,
    Voice,
    Video,
    Document,
    Sticker,
    File,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaData {
    Url(String),
    Bytes(Vec<u8>),
    FileId(String),
}

/// Request to create a poll in a channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PollRequest {
    pub question: String,
    /// Poll options (max 10).
    pub options: Vec<String>,
    pub is_anonymous: bool,
    pub allows_multiple: bool,
}

/// Information about a group/channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupInfo {
    pub id: String,
    pub name: String,
    pub member_count: Option<u32>,
    pub group_type: ChatType,
}
