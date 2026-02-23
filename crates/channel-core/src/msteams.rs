use crate::traits::*;
use async_trait::async_trait;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct MsTeamsChannel {
    bot_id: Option<String>,
    bot_password: Option<String>,
    tenant_id: Option<String>,
    access_token: Option<String>,
    client: Option<Client>,
    status: ChannelStatus,
}

impl MsTeamsChannel {
    pub fn new() -> Self {
        Self {
            bot_id: None, bot_password: None, tenant_id: None,
            access_token: None, client: None, status: ChannelStatus::Disconnected,
        }
    }

    pub fn with_config(mut self, bot_id: &str, bot_password: &str, tenant_id: Option<&str>) -> Self {
        self.bot_id = Some(bot_id.into());
        self.bot_password = Some(bot_password.into());
        self.tenant_id = tenant_id.map(Into::into);
        self
    }

    async fn get_token(&mut self) -> ChannelResult<String> {
        let client = self.client.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".into()))?;
        let bot_id = self.bot_id.as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("Bot ID not set".into()))?;
        let bot_password = self.bot_password.as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("Bot password not set".into()))?;

        let params = [
            ("grant_type", "client_credentials"),
            ("client_id", bot_id),
            ("client_secret", bot_password),
            ("scope", "https://api.botframework.com/.default"),
        ];

        let resp = client.post("https://login.microsoftonline.com/botframework.com/oauth2/v2.0/token")
            .form(&params).send().await
            .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;

        let json: serde_json::Value = resp.json().await
            .map_err(|e| ChannelError::AuthenticationError(e.to_string()))?;

        let token = json["access_token"].as_str()
            .ok_or_else(|| ChannelError::AuthenticationError("No access_token in response".into()))?
            .to_string();

        self.access_token = Some(token.clone());
        Ok(token)
    }
}

impl Default for MsTeamsChannel {
    fn default() -> Self { Self::new() }
}

impl Clone for MsTeamsChannel {
    fn clone(&self) -> Self {
        Self {
            bot_id: self.bot_id.clone(), bot_password: self.bot_password.clone(),
            tenant_id: self.tenant_id.clone(), access_token: self.access_token.clone(),
            client: None, status: self.status,
        }
    }
}

#[async_trait]
impl Channel for MsTeamsChannel {
    fn channel_type(&self) -> &str { "msteams" }

    async fn connect(&mut self) -> ChannelResult<()> {
        self.status = ChannelStatus::Connecting;
        self.client = Some(Client::new());
        self.get_token().await?;
        self.status = ChannelStatus::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> ChannelResult<()> {
        self.client = None;
        self.access_token = None;
        self.status = ChannelStatus::Disconnected;
        Ok(())
    }

    fn status(&self) -> ChannelStatus { self.status }

    async fn send_message(&self, message: &ChannelMessage) -> ChannelResult<String> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".into()));
        }
        let token = self.access_token.as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("No token".into()))?;
        let service_url = message.metadata.get("service_url")
            .ok_or_else(|| ChannelError::MessageError("service_url required".into()))?;
        let conversation_id = message.metadata.get("conversation_id")
            .ok_or_else(|| ChannelError::MessageError("conversation_id required".into()))?;

        let url = format!("{}/v3/conversations/{}/activities", service_url, conversation_id);
        let body = serde_json::json!({
            "type": "message",
            "text": message.content,
        });

        let resp = self.client.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".into()))?
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body).send().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        let json: serde_json::Value = resp.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        Ok(json["id"].as_str().unwrap_or("sent").to_string())
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>> {
        Ok(vec![])
    }

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()> {
        tracing::debug!("MS Teams event: {:?}", event);
        Ok(())
    }

    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>> {
        Ok(Box::new(MsTeamsSender { channel: Arc::new(RwLock::new(self.clone())) }))
    }
}

struct MsTeamsSender {
    channel: Arc<RwLock<MsTeamsChannel>>,
}

impl MessageSender for MsTeamsSender {
    fn send<'a>(&'a self, content: &'a str, metadata: HashMap<String, String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = ChannelResult<String>> + Send + 'a>> {
        let channel = self.channel.clone();
        let content = content.to_string();
        Box::pin(async move {
            let msg = ChannelMessage {
                id: uuid::Uuid::new_v4().to_string(),
                channel: "msteams".into(), sender: String::new(),
                content, timestamp: chrono::Utc::now().timestamp_millis(),
                metadata,
            };
            channel.read().await.send_message(&msg).await
        })
    }
}
