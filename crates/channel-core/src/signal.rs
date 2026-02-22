use crate::traits::*;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct SignalChannel {
    phone_number: Option<String>,
    signal_cli_path: Option<String>,
    recipient: Option<String>,
    client: Option<Client>,
    status: ChannelStatus,
    api_url: Option<String>,
}

#[derive(Debug, Serialize)]
struct SignalSendMessage {
    message: String,
    recipients: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct SignalResponse {
    #[serde(default)]
    results: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    error: Option<String>,
}

impl SignalChannel {
    pub fn new() -> Self {
        Self {
            phone_number: None,
            signal_cli_path: None,
            recipient: None,
            client: None,
            status: ChannelStatus::Disconnected,
            api_url: None,
        }
    }

    pub fn with_config(phone_number: &str, signal_cli_path: Option<&str>, api_url: Option<&str>) -> Self {
        Self {
            phone_number: Some(phone_number.to_string()),
            signal_cli_path: signal_cli_path.map(|s| s.to_string()),
            recipient: None,
            client: Some(Client::new()),
            status: ChannelStatus::Disconnected,
            api_url: api_url.map(|s| format!("{}/v1/send", s)),
        }
    }

    pub fn with_recipient(mut self, recipient: &str) -> Self {
        self.recipient = Some(recipient.to_string());
        self
    }

    async fn send_via_api(&self, message: &str, recipient: &str) -> ChannelResult<String> {
        let api_url = self.api_url.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("API URL not configured".to_string()))?;

        let client = self.client.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".to_string()))?;

        let body = SignalSendMessage {
            message: message.to_string(),
            recipients: vec![recipient.to_string()],
        };

        let response = client
            .post(api_url)
            .json(&body)
            .send()
            .await
            .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;

        let signal_resp: SignalResponse = response.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        if signal_resp.error.is_some() {
            return Err(ChannelError::MessageError(
                signal_resp.error.unwrap()
            ));
        }

        Ok(format!("{}_{}", recipient, uuid::Uuid::new_v4()))
    }
}

impl Default for SignalChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Channel for SignalChannel {
    fn channel_type(&self) -> &str {
        "signal"
    }

    async fn connect(&mut self) -> ChannelResult<()> {
        if self.phone_number.is_none() {
            return Err(ChannelError::AuthenticationError(
                "Phone number required".to_string(),
            ));
        }

        self.status = ChannelStatus::Connecting;
        self.client = Some(Client::new());

        tracing::info!("Signal channel connecting with phone: {:?}", self.phone_number);

        self.status = ChannelStatus::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> ChannelResult<()> {
        self.client = None;
        self.status = ChannelStatus::Disconnected;
        tracing::info!("Signal channel disconnected");
        Ok(())
    }

    fn status(&self) -> ChannelStatus {
        self.status
    }

    async fn send_message(&self, message: &ChannelMessage) -> ChannelResult<String> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let recipient = message.metadata.get("recipient")
            .or_else(|| self.recipient.as_ref())
            .cloned()
            .ok_or_else(|| ChannelError::MessageError("Recipient not specified".to_string()))?;

        if let Some(_api_url) = &self.api_url {
            self.send_via_api(&message.content, &recipient).await
        } else {
            Err(ChannelError::ConfigError("Signal CLI or API URL required".to_string()))
        }
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let account = ChannelAccount {
            id: self.phone_number.clone().unwrap_or_default(),
            name: self.phone_number.clone().unwrap_or_default(),
            channel: "signal".to_string(),
            avatar: None,
            status: Some("active".to_string()),
        };

        Ok(vec![account])
    }

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()> {
        tracing::debug!("Received Signal event: {:?}", event);
        Ok(())
    }

    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>> {
        Ok(Box::new(SignalSender {
            channel: Arc::new(RwLock::new(self.clone())),
        }))
    }
}

impl Clone for SignalChannel {
    fn clone(&self) -> Self {
        Self {
            phone_number: self.phone_number.clone(),
            signal_cli_path: self.signal_cli_path.clone(),
            recipient: self.recipient.clone(),
            client: self.client.clone(),
            status: self.status,
            api_url: self.api_url.clone(),
        }
    }
}

struct SignalSender {
    channel: Arc<RwLock<SignalChannel>>,
}

impl MessageSender for SignalSender {
    fn send<'a>(&'a self, content: &'a str, metadata: HashMap<String, String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = ChannelResult<String>> + Send + 'a>> {
        let channel = self.channel.clone();
        let content = content.to_string();
        let metadata = metadata.clone();
        
        Box::pin(async move {
            let message = ChannelMessage {
                id: uuid::Uuid::new_v4().to_string(),
                channel: "signal".to_string(),
                sender: String::new(),
                content,
                timestamp: chrono::Utc::now().timestamp_millis(),
                metadata,
            };
            
            channel.read().await.send_message(&message).await
        })
    }
}
