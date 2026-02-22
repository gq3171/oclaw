use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageType {
    Text,
    Image,
    Video,
    Audio,
    File,
    Location,
    Sticker,
    Template,
    Interactive,
}

impl Default for MessageType {
    fn default() -> Self {
        MessageType::Text
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: Option<String>,
    pub message_type: MessageType,
    pub url: Option<String>,
    pub mime_type: Option<String>,
    pub size: Option<i64>,
    pub filename: Option<String>,
    pub width: Option<i32>,
    pub height: Option<i32>,
    pub duration: Option<i32>,
    pub thumbnail_url: Option<String>,
    pub caption: Option<String>,
    pub metadata: HashMap<String, String>,
}

impl Attachment {
    pub fn image(url: &str) -> Self {
        Self {
            id: None,
            message_type: MessageType::Image,
            url: Some(url.to_string()),
            mime_type: Some("image/*".to_string()),
            size: None,
            filename: None,
            width: None,
            height: None,
            duration: None,
            thumbnail_url: None,
            caption: None,
            metadata: HashMap::new(),
        }
    }

    pub fn video(url: &str) -> Self {
        Self {
            id: None,
            message_type: MessageType::Video,
            url: Some(url.to_string()),
            mime_type: Some("video/*".to_string()),
            size: None,
            filename: None,
            width: None,
            height: None,
            duration: None,
            thumbnail_url: None,
            caption: None,
            metadata: HashMap::new(),
        }
    }

    pub fn audio(url: &str) -> Self {
        Self {
            id: None,
            message_type: MessageType::Audio,
            url: Some(url.to_string()),
            mime_type: Some("audio/*".to_string()),
            size: None,
            filename: None,
            width: None,
            height: None,
            duration: None,
            thumbnail_url: None,
            caption: None,
            metadata: HashMap::new(),
        }
    }

    pub fn file(url: &str, filename: &str) -> Self {
        Self {
            id: None,
            message_type: MessageType::File,
            url: Some(url.to_string()),
            mime_type: None,
            size: None,
            filename: Some(filename.to_string()),
            width: None,
            height: None,
            duration: None,
            thumbnail_url: None,
            caption: None,
            metadata: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageContent {
    pub message_type: MessageType,
    pub text: Option<String>,
    pub html: Option<String>,
    pub markdown: Option<String>,
    pub attachments: Vec<Attachment>,
    pub metadata: HashMap<String, String>,
}

impl MessageContent {
    pub fn text(text: &str) -> Self {
        Self {
            message_type: MessageType::Text,
            text: Some(text.to_string()),
            html: None,
            markdown: None,
            attachments: Vec::new(),
            metadata: HashMap::new(),
        }
    }

    pub fn with_html(mut self, html: &str) -> Self {
        self.html = Some(html.to_string());
        self
    }

    pub fn with_attachment(mut self, attachment: Attachment) -> Self {
        self.attachments.push(attachment);
        self
    }

    pub fn with_metadata(mut self, key: &str, value: &str) -> Self {
        self.metadata.insert(key.to_string(), value.to_string());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: String,
    pub channel_id: String,
    pub from: MessageSender,
    pub to: Option<MessageRecipient>,
    pub group_id: Option<String>,
    pub content: MessageContent,
    pub timestamp: i64,
    pub thread_id: Option<String>,
    pub reply_to: Option<String>,
    pub mentions: Vec<String>,
    pub reactions: Vec<MessageReaction>,
    pub is_forwarded: bool,
    pub is_reply: bool,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageSender {
    pub id: String,
    pub name: Option<String>,
    pub avatar_url: Option<String>,
    pub is_bot: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[serde(untagged)]
pub enum MessageRecipient {
    User { id: String },
    Group { id: String },
    Channel { id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageReaction {
    pub emoji: String,
    pub user_ids: Vec<String>,
    pub count: i32,
}

impl Message {
    pub fn new(channel_id: &str, from: MessageSender, content: MessageContent) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            channel_id: channel_id.to_string(),
            from,
            to: None,
            group_id: None,
            content,
            timestamp: chrono::Utc::now().timestamp_millis(),
            thread_id: None,
            reply_to: None,
            mentions: Vec::new(),
            reactions: Vec::new(),
            is_forwarded: false,
            is_reply: false,
            metadata: HashMap::new(),
        }
    }

    pub fn to_user(mut self, user_id: &str) -> Self {
        self.to = Some(MessageRecipient::User { id: user_id.to_string() });
        self
    }

    pub fn to_group(mut self, group_id: &str) -> Self {
        self.group_id = Some(group_id.to_string());
        self.to = Some(MessageRecipient::Group { id: group_id.to_string() });
        self
    }

    pub fn in_thread(mut self, thread_id: &str) -> Self {
        self.thread_id = Some(thread_id.to_string());
        self
    }

    pub fn reply_to(mut self, message_id: &str) -> Self {
        self.reply_to = Some(message_id.to_string());
        self.is_reply = true;
        self
    }

    pub fn mention(mut self, user_id: &str) -> Self {
        self.mentions.push(user_id.to_string());
        self
    }
}

pub struct MessageQueue {
    queue: Arc<RwLock<Vec<Message>>>,
    max_size: usize,
}

impl MessageQueue {
    pub fn new(max_size: usize) -> Self {
        Self {
            queue: Arc::new(RwLock::new(Vec::new())),
            max_size,
        }
    }

    pub async fn enqueue(&self, message: Message) -> Result<(), QueueError> {
        let mut queue = self.queue.write().await;
        if queue.len() >= self.max_size {
            return Err(QueueError::QueueFull);
        }
        queue.push(message);
        Ok(())
    }

    pub async fn dequeue(&self) -> Option<Message> {
        let mut queue = self.queue.write().await;
        queue.pop()
    }

    pub async fn peek(&self) -> Option<Message> {
        let queue = self.queue.read().await;
        queue.last().cloned()
    }

    pub async fn len(&self) -> usize {
        let queue = self.queue.read().await;
        queue.len()
    }

    pub async fn clear(&self) {
        let mut queue = self.queue.write().await;
        queue.clear();
    }
}

#[derive(Debug, thiserror::Error)]
pub enum QueueError {
    #[error("Queue is full")]
    QueueFull,
    #[error("Queue is empty")]
    QueueEmpty,
}

pub struct MessageRouter {
    routes: Arc<RwLock<HashMap<String, Vec<String>>>>,
    default_channel: Arc<RwLock<Option<String>>>,
}

impl MessageRouter {
    pub fn new() -> Self {
        Self {
            routes: Arc::new(RwLock::new(HashMap::new())),
            default_channel: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn add_route(&self, pattern: &str, channel_ids: Vec<String>) {
        let mut routes = self.routes.write().await;
        routes.insert(pattern.to_string(), channel_ids);
    }

    pub async fn remove_route(&self, pattern: &str) -> Option<Vec<String>> {
        let mut routes = self.routes.write().await;
        routes.remove(pattern)
    }

    pub async fn set_default_channel(&self, channel_id: &str) {
        let mut default = self.default_channel.write().await;
        *default = Some(channel_id.to_string());
    }

    pub async fn route(&self, message: &Message) -> Vec<String> {
        let routes = self.routes.read().await;
        let default = self.default_channel.read().await;

        for (pattern, channel_ids) in routes.iter() {
            if self.matches_pattern(&message.content.text, pattern) {
                return channel_ids.clone();
            }
        }

        default.clone().map(|c| vec![c]).unwrap_or_default()
    }

    fn matches_pattern(&self, text: &Option<String>, pattern: &str) -> bool {
        if let Some(text) = text {
            if pattern.starts_with("regex:") {
                let regex_pattern = &pattern[6..];
                if let Ok(re) = regex::Regex::new(regex_pattern) {
                    return re.is_match(text);
                }
            }
            if pattern.starts_with("contains:") {
                let keyword = &pattern[9..];
                return text.contains(keyword);
            }
            if pattern.starts_with("starts:") {
                let prefix = &pattern[7..];
                return text.starts_with(prefix);
            }
            if pattern.starts_with("ends:") {
                let suffix = &pattern[5..];
                return text.ends_with(suffix);
            }
            text == pattern
        } else {
            false
        }
    }

    pub async fn list_routes(&self) -> HashMap<String, Vec<String>> {
        let routes = self.routes.read().await;
        routes.clone()
    }
}

impl Default for MessageRouter {
    fn default() -> Self {
        Self::new()
    }
}

pub fn normalize_message(raw: serde_json::Value, channel_id: &str) -> Message {
    let from = MessageSender {
        id: raw.get("from")
            .or_else(|| raw.get("user_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string(),
        name: raw.get("name")
            .or_else(|| raw.get("sender_name"))
            .and_then(|v| v.as_str())
            .map(String::from),
        avatar_url: raw.get("avatar_url")
            .and_then(|v| v.as_str())
            .map(String::from),
        is_bot: raw.get("is_bot")
            .and_then(|v| v.as_bool())
            .unwrap_or(false),
    };

    let content = if let Some(text) = raw.get("text").and_then(|v| v.as_str()) {
        MessageContent::text(text)
    } else if let Some(html) = raw.get("html").and_then(|v| v.as_str()) {
        MessageContent::text(html).with_html(html)
    } else {
        MessageContent::text("")
    };

    let mut message = Message::new(channel_id, from, content);
    message.metadata.insert("raw".to_string(), raw.to_string());
    message
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_creation() {
        let sender = MessageSender {
            id: "user_123".to_string(),
            name: Some("John".to_string()),
            avatar_url: None,
            is_bot: false,
        };

        let message = Message::new("telegram", sender, MessageContent::text("Hello"))
            .to_user("user_456");

        assert_eq!(message.channel_id, "telegram");
        assert!(message.to.is_some());
    }

    #[test]
    fn test_attachment_creation() {
        let img = Attachment::image("https://example.com/image.jpg");
        assert_eq!(img.message_type, MessageType::Image);
        assert!(img.url.is_some());
    }

    #[tokio::test]
    async fn test_message_router_pattern() {
        let router = MessageRouter::new();
        
        let message = Message::new("test", MessageSender {
            id: "user".to_string(),
            name: None,
            avatar_url: None,
            is_bot: false,
        }, MessageContent::text("hello world"));

        let routes = router.route(&message).await;
        assert!(routes.is_empty());
    }

    #[tokio::test]
    async fn test_message_queue() {
        let queue = MessageQueue::new(2);
        
        let msg = Message::new("test", MessageSender {
            id: "user".to_string(),
            name: None,
            avatar_url: None,
            is_bot: false,
        }, MessageContent::text("test"));

        assert!(queue.enqueue(msg.clone()).await.is_ok());
        assert!(queue.enqueue(msg.clone()).await.is_ok());
        assert!(queue.enqueue(msg).await.is_err());
    }
}
