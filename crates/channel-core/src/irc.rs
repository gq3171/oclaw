use crate::traits::*;
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::RwLock;

pub struct IrcChannel {
    server: Option<String>,
    port: u16,
    nick: Option<String>,
    username: Option<String>,
    realname: Option<String>,
    password: Option<String>,
    channels: Vec<String>,
    status: ChannelStatus,
    stream: Option<Arc<RwLock<TcpStream>>>,
}

impl IrcChannel {
    pub fn new() -> Self {
        Self {
            server: None,
            port: 6667,
            nick: None,
            username: None,
            realname: None,
            password: None,
            channels: Vec::new(),
            status: ChannelStatus::Disconnected,
            stream: None,
        }
    }

    pub fn with_config(mut self, server: &str, nick: &str) -> Self {
        self.server = Some(server.to_string());
        self.nick = Some(nick.to_string());
        self.username = Some(nick.to_string());
        self.realname = Some(nick.to_string());
        self
    }

    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn with_password(mut self, password: &str) -> Self {
        self.password = Some(password.to_string());
        self
    }

    pub fn join_channel(mut self, channel: &str) -> Self {
        self.channels.push(channel.to_string());
        self
    }

    async fn send_raw(&mut self, command: &str) -> ChannelResult<()> {
        let stream = self.stream.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Not connected".to_string()))?;

        let mut guard = stream.write().await;
        guard.write_all(format!("{}\r\n", command).as_bytes()).await
            .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;
        
        Ok(())
    }
}

impl Default for IrcChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Channel for IrcChannel {
    fn channel_type(&self) -> &str {
        "irc"
    }

    async fn connect(&mut self) -> ChannelResult<()> {
        let server = self.server.as_ref()
            .ok_or_else(|| ChannelError::ConfigError("Server not configured".to_string()))?
            .clone();
        
        let nick = self.nick.as_ref()
            .ok_or_else(|| ChannelError::ConfigError("Nick not configured".to_string()))?
            .clone();

        let username = self.username.clone();
        let realname = self.realname.clone();
        let password = self.password.clone();
        let channels = self.channels.clone();

        self.status = ChannelStatus::Connecting;

        let addr = format!("{}:{}", server, self.port);
        let stream = TcpStream::connect(&addr).await
            .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;
        
        self.stream = Some(Arc::new(RwLock::new(stream)));

        if let Some(pass) = password {
            self.send_raw(&format!("PASS {}", pass)).await?;
        }

        self.send_raw(&format!("NICK {}", nick)).await?;
        self.send_raw(&format!("USER {} 0 * :{}", username.as_deref().unwrap_or(&nick), realname.as_deref().unwrap_or(&nick))).await?;

        for channel in &channels {
            self.send_raw(&format!("JOIN {}", channel)).await?;
        }

        tracing::info!("IRC channel connected to {}", server);

        self.status = ChannelStatus::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> ChannelResult<()> {
        if let Some(stream) = self.stream.take() {
            let mut s = stream.write().await;
            s.shutdown().await.ok();
        }
        self.status = ChannelStatus::Disconnected;
        tracing::info!("IRC channel disconnected");
        Ok(())
    }

    fn status(&self) -> ChannelStatus {
        self.status
    }

    async fn send_message(&self, message: &ChannelMessage) -> ChannelResult<String> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let target = message.metadata.get("target")
            .or_else(|| message.metadata.get("channel"))
            .cloned()
            .ok_or_else(|| ChannelError::MessageError("Target channel not specified".to_string()))?;

        let response = format!("PRIVMSG {} :{}", target, message.content);

        let stream = self.stream.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Not connected".to_string()))?;

        let mut s = stream.write().await;
        s.write_all(format!("{}\r\n", response).as_bytes()).await
            .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;

        Ok(format!("{}_{}", target, uuid::Uuid::new_v4()))
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let account = ChannelAccount {
            id: self.nick.clone().unwrap_or_default(),
            name: self.nick.clone().unwrap_or_default(),
            channel: "irc".to_string(),
            avatar: None,
            status: Some("active".to_string()),
        };

        Ok(vec![account])
    }

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()> {
        tracing::debug!("Received IRC event: {:?}", event);
        
        match event.event_type.as_str() {
            "PRIVMSG" => {
                tracing::info!("Received IRC message: {:?}", event.payload);
            }
            "JOIN" => {
                tracing::info!("User joined: {:?}", event.payload);
            }
            "PART" => {
                tracing::info!("User left: {:?}", event.payload);
            }
            _ => {}
        }
        
        Ok(())
    }

    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>> {
        Ok(Box::new(IrcSender {
            channel: Arc::new(RwLock::new(self.clone())),
        }))
    }

    fn parse_webhook(&self, payload: &serde_json::Value) -> Option<WebhookMessage> {
        // IRC bridge: /message, /nick, /channel
        let text = payload.get("message")
            .and_then(|v| v.as_str())?;
        let chat_id = payload.get("channel")
            .or_else(|| payload.get("nick"))
            .and_then(|v| v.as_str())
            .unwrap_or("default");
        let is_group = payload.get("channel").is_some();
        Some(WebhookMessage {
            text: text.to_string(),
            chat_id: chat_id.to_string(),
            is_group,
            has_mention: false,
            metadata: HashMap::new(),
        })
    }
}

impl Clone for IrcChannel {
    fn clone(&self) -> Self {
        Self {
            server: self.server.clone(),
            port: self.port,
            nick: self.nick.clone(),
            username: self.username.clone(),
            realname: self.realname.clone(),
            password: self.password.clone(),
            channels: self.channels.clone(),
            status: self.status,
            stream: None,
        }
    }
}

struct IrcSender {
    channel: Arc<RwLock<IrcChannel>>,
}

impl MessageSender for IrcSender {
    fn send<'a>(&'a self, content: &'a str, metadata: HashMap<String, String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = ChannelResult<String>> + Send + 'a>> {
        let channel = self.channel.clone();
        let content = content.to_string();
        let metadata = metadata.clone();
        
        Box::pin(async move {
            let message = ChannelMessage {
                id: uuid::Uuid::new_v4().to_string(),
                channel: "irc".to_string(),
                sender: String::new(),
                content,
                timestamp: chrono::Utc::now().timestamp_millis(),
                metadata,
            };
            
            channel.read().await.send_message(&message).await
        })
    }
}
