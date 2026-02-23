use crate::traits::*;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct MatrixChannel {
    homeserver: Option<String>,
    access_token: Option<String>,
    user_id: Option<String>,
    device_id: Option<String>,
    client: Option<Client>,
    status: ChannelStatus,
    room_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[allow(dead_code)]
struct MatrixSendMessage {
    msgtype: String,
    body: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct MatrixResponse {
    #[serde(default)]
    room_id: Option<String>,
    #[serde(default)]
    event_id: Option<String>,
    #[serde(default)]
    access_token: Option<String>,
    errcode: Option<String>,
    error: Option<String>,
}

impl MatrixChannel {
    pub fn new() -> Self {
        Self {
            homeserver: None,
            access_token: None,
            user_id: None,
            device_id: None,
            client: None,
            status: ChannelStatus::Disconnected,
            room_id: None,
        }
    }

    pub fn with_config(mut self, homeserver: &str, user_id: &str, access_token: &str) -> Self {
        self.homeserver = Some(homeserver.to_string());
        self.user_id = Some(user_id.to_string());
        self.access_token = Some(access_token.to_string());
        self
    }

    pub fn with_device_id(mut self, device_id: &str) -> Self {
        self.device_id = Some(device_id.to_string());
        self
    }

    pub fn with_room(mut self, room_id: &str) -> Self {
        self.room_id = Some(room_id.to_string());
        self
    }

    async fn send_api_request(
        &self,
        method: &str,
        path: &str,
        body: Option<&serde_json::Value>,
    ) -> ChannelResult<serde_json::Value> {
        let homeserver = self.homeserver.as_ref()
            .ok_or_else(|| ChannelError::ConfigurationError("Homeserver not set".to_string()))?;

        let url = format!("{}{}", homeserver, path);

        let client = self.client.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".to_string()))?;

        let http_method = reqwest::Method::from_bytes(method.as_bytes())
            .map_err(|e| ChannelError::ConfigurationError(format!("Invalid HTTP method '{}': {}", method, e)))?;
        let mut request = client.request(http_method, &url);

        if let Some(token) = &self.access_token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }

        if let Some(body) = body {
            request = request.json(body);
        }

        let response = request.send().await
            .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;

        let matrix_resp: MatrixResponse = response.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        if matrix_resp.errcode.is_some() || matrix_resp.error.is_some() {
            return Err(ChannelError::MessageError(
                matrix_resp.error.unwrap_or_else(|| matrix_resp.errcode.unwrap_or_default())
            ));
        }

        Ok(serde_json::json!({
            "room_id": matrix_resp.room_id,
            "event_id": matrix_resp.event_id
        }))
    }
}

impl Default for MatrixChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Channel for MatrixChannel {
    fn channel_type(&self) -> &str {
        "matrix"
    }

    async fn connect(&mut self) -> ChannelResult<()> {
        if self.homeserver.is_none() || self.access_token.is_none() {
            return Err(ChannelError::AuthenticationError(
                "Homeserver and access token required".to_string(),
            ));
        }

        self.status = ChannelStatus::Connecting;
        self.client = Some(Client::new());

        let _: serde_json::Value = self.send_api_request("GET", "/_matrix/client/v3/account/whoami", None).await?;

        tracing::info!("Matrix channel connected");

        self.status = ChannelStatus::Connected;
        Ok(())
    }

    async fn disconnect(&mut self) -> ChannelResult<()> {
        self.client = None;
        self.status = ChannelStatus::Disconnected;
        tracing::info!("Matrix channel disconnected");
        Ok(())
    }

    fn status(&self) -> ChannelStatus {
        self.status
    }

    async fn send_message(&self, message: &ChannelMessage) -> ChannelResult<String> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let room_id = message.metadata.get("room_id")
            .or(self.room_id.as_ref())
            .cloned()
            .ok_or_else(|| ChannelError::MessageError("Room ID not specified".to_string()))?;

        let txn_id = uuid::Uuid::new_v4().to_string();

        let body = serde_json::json!({
            "msgtype": "m.text",
            "body": message.content.clone()
        });

        let path = format!(
            "/_matrix/client/v3/rooms/{}/send/m.room.message/{}?access_token={}",
            room_id,
            txn_id,
            self.access_token.as_ref()
                .ok_or_else(|| ChannelError::AuthenticationError("Access token not set".to_string()))?,
        );

        let response: serde_json::Value = self.send_api_request("PUT", &path, Some(&body)).await?;

        let event_id = response.get("event_id")
            .and_then(|e| e.as_str())
            .map(|s| s.to_string())
            .ok_or_else(|| ChannelError::MessageError("No event ID returned".to_string()))?;

        Ok(event_id)
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        let account = ChannelAccount {
            id: self.user_id.clone().unwrap_or_default(),
            name: self.user_id.clone().unwrap_or_default(),
            channel: "matrix".to_string(),
            avatar: None,
            status: Some("active".to_string()),
        };

        Ok(vec![account])
    }

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()> {
        tracing::debug!("Received Matrix event: {:?}", event);
        
        if event.event_type.as_str() == "m.room.message" {
            tracing::info!("Received Matrix message: {:?}", event.payload);
        }
        
        Ok(())
    }

    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>> {
        Ok(Box::new(MatrixSender {
            channel: Arc::new(RwLock::new(self.clone())),
        }))
    }
}

impl Clone for MatrixChannel {
    fn clone(&self) -> Self {
        Self {
            homeserver: self.homeserver.clone(),
            access_token: self.access_token.clone(),
            user_id: self.user_id.clone(),
            device_id: self.device_id.clone(),
            client: self.client.clone(),
            status: self.status,
            room_id: self.room_id.clone(),
        }
    }
}

struct MatrixSender {
    channel: Arc<RwLock<MatrixChannel>>,
}

impl MessageSender for MatrixSender {
    fn send<'a>(&'a self, content: &'a str, metadata: HashMap<String, String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = ChannelResult<String>> + Send + 'a>> {
        let channel = self.channel.clone();
        let content = content.to_string();
        let metadata = metadata.clone();
        
        Box::pin(async move {
            let message = ChannelMessage {
                id: uuid::Uuid::new_v4().to_string(),
                channel: "matrix".to_string(),
                sender: String::new(),
                content,
                timestamp: chrono::Utc::now().timestamp_millis(),
                metadata,
            };
            
            channel.read().await.send_message(&message).await
        })
    }
}
