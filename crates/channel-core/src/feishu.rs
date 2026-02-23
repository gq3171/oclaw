use crate::traits::*;
use async_trait::async_trait;
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct FeishuChannel {
    app_id: Option<String>,
    app_secret: Option<String>,
    tenant_access_token: Option<String>,
    client: Option<Client>,
    status: ChannelStatus,
}

impl FeishuChannel {
    pub fn new() -> Self {
        Self {
            app_id: None, app_secret: None, tenant_access_token: None,
            client: None, status: ChannelStatus::Disconnected,
        }
    }

    pub fn with_config(mut self, app_id: &str, app_secret: &str) -> Self {
        self.app_id = Some(app_id.into());
        self.app_secret = Some(app_secret.into());
        self
    }

    async fn get_tenant_token(&mut self) -> ChannelResult<String> {
        let client = self.client.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".into()))?;
        let body = serde_json::json!({
            "app_id": self.app_id.as_deref().unwrap_or(""),
            "app_secret": self.app_secret.as_deref().unwrap_or(""),
        });

        let resp = client.post("https://open.feishu.cn/open-apis/auth/v3/tenant_access_token/internal")
            .json(&body).send().await
            .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;

        let json: serde_json::Value = resp.json().await
            .map_err(|e| ChannelError::AuthenticationError(e.to_string()))?;

        let token = json["tenant_access_token"].as_str()
            .ok_or_else(|| ChannelError::AuthenticationError("No tenant_access_token".into()))?
            .to_string();

        self.tenant_access_token = Some(token.clone());
        Ok(token)
    }
}

impl Default for FeishuChannel {
    fn default() -> Self { Self::new() }
}

impl Clone for FeishuChannel {
    fn clone(&self) -> Self {
        Self {
            app_id: self.app_id.clone(), app_secret: self.app_secret.clone(),
            tenant_access_token: self.tenant_access_token.clone(),
            client: None, status: self.status,
        }
    }
}

#[async_trait]
impl Channel for FeishuChannel {
    fn channel_type(&self) -> &str { "feishu" }

    async fn connect(&mut self) -> ChannelResult<()> {
        self.status = ChannelStatus::Connecting;
        self.client = Some(Client::new());
        self.get_tenant_token().await?;
        self.status = ChannelStatus::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> ChannelResult<()> {
        self.client = None;
        self.tenant_access_token = None;
        self.status = ChannelStatus::Disconnected;
        Ok(())
    }

    fn status(&self) -> ChannelStatus { self.status }

    async fn send_message(&self, message: &ChannelMessage) -> ChannelResult<String> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".into()));
        }
        let token = self.tenant_access_token.as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("No token".into()))?;
        let receive_id = message.metadata.get("receive_id")
            .ok_or_else(|| ChannelError::MessageError("receive_id required".into()))?;
        let receive_id_type = message.metadata.get("receive_id_type").map(|s| s.as_str()).unwrap_or("chat_id");

        let url = format!("https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type={}", receive_id_type);
        let body = serde_json::json!({
            "receive_id": receive_id,
            "msg_type": "text",
            "content": serde_json::json!({"text": message.content}).to_string(),
        });

        let resp = self.client.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".into()))?
            .post(&url)
            .header("Authorization", format!("Bearer {}", token))
            .json(&body).send().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        let json: serde_json::Value = resp.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        Ok(json["data"]["message_id"].as_str().unwrap_or("sent").to_string())
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>> {
        Ok(vec![])
    }

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()> {
        tracing::debug!("Feishu event: {:?}", event);
        Ok(())
    }

    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>> {
        Ok(Box::new(FeishuSender { channel: Arc::new(RwLock::new(self.clone())) }))
    }
}

struct FeishuSender {
    channel: Arc<RwLock<FeishuChannel>>,
}

impl MessageSender for FeishuSender {
    fn send<'a>(&'a self, content: &'a str, metadata: HashMap<String, String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = ChannelResult<String>> + Send + 'a>> {
        let channel = self.channel.clone();
        let content = content.to_string();
        Box::pin(async move {
            let msg = ChannelMessage {
                id: uuid::Uuid::new_v4().to_string(),
                channel: "feishu".into(), sender: String::new(),
                content, timestamp: chrono::Utc::now().timestamp_millis(),
                metadata,
            };
            channel.read().await.send_message(&msg).await
        })
    }
}
