use crate::traits::*;
use async_trait::async_trait;
use std::collections::HashMap;
use std::pin::Pin;

pub struct WebchatChannel {
    _enabled: bool,
    username: Option<String>,
    password: Option<String>,
    status: ChannelStatus,
}

impl WebchatChannel {
    pub fn new() -> Self {
        Self {
            _enabled: true,
            username: None,
            password: None,
            status: ChannelStatus::Disconnected,
        }
    }

    pub fn with_credentials(mut self, username: &str, password: &str) -> Self {
        self.username = Some(username.to_string());
        self.password = Some(password.to_string());
        self
    }
}

impl Default for WebchatChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Channel for WebchatChannel {
    fn channel_type(&self) -> &str {
        "webchat"
    }

    async fn connect(&mut self) -> ChannelResult<()> {
        self.status = ChannelStatus::Connecting;

        if let (Some(_), Some(_)) = (&self.username, &self.password) {
            self.status = ChannelStatus::Connected;
            Ok(())
        } else {
            self.status = ChannelStatus::Error;
            Err(ChannelError::AuthenticationError(
                "Username and password required".to_string(),
            ))
        }
    }

    async fn disconnect(&mut self) -> ChannelResult<()> {
        self.status = ChannelStatus::Disconnected;
        Ok(())
    }

    fn status(&self) -> ChannelStatus {
        self.status
    }

    async fn send_message(&self, message: &ChannelMessage) -> ChannelResult<String> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        Ok(message.id.clone())
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>> {
        Ok(vec![ChannelAccount {
            id: "default".to_string(),
            name: self.username.clone().unwrap_or_default(),
            channel: "webchat".to_string(),
            avatar: None,
            status: Some("online".to_string()),
        }])
    }

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()> {
        tracing::debug!("Received event: {:?}", event);
        Ok(())
    }

    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>> {
        Ok(Box::new(WebchatSender {
            _channel: "webchat".to_string(),
        }))
    }
}

struct WebchatSender {
    _channel: String,
}

impl MessageSender for WebchatSender {
    fn send<'a>(
        &'a self,
        _content: &'a str,
        _metadata: HashMap<String, String>,
    ) -> Pin<Box<dyn std::future::Future<Output = ChannelResult<String>> + Send + 'a>> {
        Box::pin(async move { Ok(format!("message-{}", uuid::Uuid::new_v4())) })
    }
}
