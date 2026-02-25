use crate::traits::*;
use crate::types::*;
use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct WhatsAppChannel {
    phone_number_id: Option<String>,
    api_token: Option<String>,
    api_url: Option<String>,
    client: Option<Client>,
    status: ChannelStatus,
    business_account_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct WhatsAppMessage {
    messaging_product: String,
    to: String,
    #[serde(rename = "type")]
    message_type: String,
    text: Option<WhatsAppText>,
    image: Option<WhatsAppImage>,
}

#[derive(Debug, Serialize)]
struct WhatsAppText {
    body: String,
}

#[derive(Debug, Serialize)]
struct WhatsAppImage {
    id: Option<String>,
    link: Option<String>,
    caption: Option<String>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct WhatsAppResponse {
    messages: Option<Vec<WhatsAppMessageResponse>>,
    error: Option<WhatsAppError>,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct WhatsAppMessageResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct WhatsAppError {
    message: String,
    #[serde(rename = "type")]
    error_type: String,
    code: i32,
    fbtrace_id: Option<String>,
}

impl WhatsAppChannel {
    pub fn new() -> Self {
        Self {
            phone_number_id: None,
            api_token: None,
            api_url: None,
            client: None,
            status: ChannelStatus::Disconnected,
            business_account_id: None,
        }
    }

    pub fn with_config(
        mut self,
        phone_number_id: &str,
        api_token: &str,
        business_account_id: Option<&str>,
    ) -> Self {
        self.phone_number_id = Some(phone_number_id.to_string());
        self.api_token = Some(api_token.to_string());
        self.business_account_id = business_account_id.map(|s| s.to_string());
        self.api_url = Some("https://graph.facebook.com/v18.0".to_string());
        self
    }

    fn get_base_url(&self) -> String {
        self.api_url.clone().unwrap_or_else(|| "https://graph.facebook.com/v18.0".to_string())
    }

    async fn send_api_request(
        &self,
        method: &str,
        endpoint: &str,
        body: Option<&serde_json::Value>,
    ) -> ChannelResult<serde_json::Value> {
        let token = self.api_token.as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("API token not set".to_string()))?;
        
        let url = format!("{}{}", self.get_base_url(), endpoint);
        
        let client = self.client.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".to_string()))?;

        let mut request = match method.to_uppercase().as_str() {
            "GET" => client.get(&url),
            "POST" => client.post(&url),
            "DELETE" => client.delete(&url),
            _ => return Err(ChannelError::ConnectionError("Invalid method".to_string())),
        };

        request = request
            .header("Authorization", format!("Bearer {}", token))
            .header("Content-Type", "application/json");

        if let Some(body) = body {
            request = request.json(body);
        }

        let response = request.send().await
            .map_err(|e| ChannelError::ConnectionError(e.to_string()))?;

        let status = response.status();
        let json: serde_json::Value = response.json().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;

        if status.is_success() {
            Ok(json)
        } else {
            let error_msg = json.get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            
            Err(ChannelError::MessageError(error_msg.to_string()))
        }
    }
}

impl Default for WhatsAppChannel {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Channel for WhatsAppChannel {
    fn channel_type(&self) -> &str {
        "whatsapp"
    }

    async fn connect(&mut self) -> ChannelResult<()> {
        if self.phone_number_id.is_none() || self.api_token.is_none() {
            return Err(ChannelError::AuthenticationError(
                "Phone number ID and API token required".to_string(),
            ));
        }

        self.status = ChannelStatus::Connecting;

        self.client = Some(Client::new());

        self.status = ChannelStatus::Connected;
        tracing::info!("WhatsApp channel connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> ChannelResult<()> {
        self.client = None;
        self.status = ChannelStatus::Disconnected;
        tracing::info!("WhatsApp channel disconnected");
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
            .cloned()
            .ok_or_else(|| ChannelError::MessageError("Recipient not specified".to_string()))?;

        let whatsapp_msg = WhatsAppMessage {
            messaging_product: "whatsapp".to_string(),
            to: recipient,
            message_type: "text".to_string(),
            text: Some(WhatsAppText {
                body: message.content.clone(),
            }),
            image: None,
        };

        let phone_number_id = self.phone_number_id.as_ref()
            .ok_or_else(|| ChannelError::ConfigError("Phone number ID not set".to_string()))?;

        let endpoint = format!("/{}/messages", phone_number_id);

        let json = self.send_api_request(
            "POST",
            &endpoint,
            Some(&serde_json::to_value(&whatsapp_msg).map_err(|e| ChannelError::MessageError(e.to_string()))?),
        )
        .await?;

        let message_id = json.get("messages")
            .and_then(|m| m.as_array())
            .and_then(|arr| arr.first())
            .and_then(|m| m.get("id"))
            .and_then(|id| id.as_str())
            .map(|id| id.to_string())
            .ok_or_else(|| ChannelError::MessageError("No message ID returned".to_string()))?;

        Ok(message_id)
    }

    async fn list_accounts(&self) -> ChannelResult<Vec<ChannelAccount>> {
        if self.status != ChannelStatus::Connected {
            return Err(ChannelError::ConnectionError("Not connected".to_string()));
        }

        Ok(vec![ChannelAccount {
            id: self.phone_number_id.clone().unwrap_or_default(),
            name: self.phone_number_id.clone().unwrap_or_default(),
            channel: "whatsapp".to_string(),
            avatar: None,
            status: Some("active".to_string()),
        }])
    }

    async fn handle_event(&self, event: ChannelEvent) -> ChannelResult<()> {
        tracing::debug!("Received WhatsApp event: {:?}", event);
        
        match event.event_type.as_str() {
            "message" => {
                tracing::info!("Received WhatsApp message: {:?}", event.payload);
            }
            "status" => {
                tracing::info!("WhatsApp status update: {:?}", event.payload);
            }
            _ => {}
        }
        
        Ok(())
    }

    fn capabilities(&self) -> ChannelCapabilities {
        ChannelCapabilities {
            reactions: true,
            media: true,
            polls: true,
            ..Default::default()
        }
    }

    fn parse_webhook(&self, payload: &serde_json::Value) -> Option<WebhookMessage> {
        // WhatsApp Cloud API: /entry/0/changes/0/value/messages/0
        let msg = payload.pointer("/entry/0/changes/0/value/messages/0")?;
        let from = msg.get("from").and_then(|v| v.as_str()).unwrap_or("unknown");
        let msg_type = msg.get("type").and_then(|v| v.as_str()).unwrap_or("text");
        let text = if msg_type == "text" {
            msg.pointer("/text/body").and_then(|v| v.as_str()).map(|s| s.to_string())
        } else if msg_type == "interactive" {
            msg.pointer("/interactive/button_reply/title")
                .or_else(|| msg.pointer("/interactive/list_reply/title"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        } else {
            Some(format!("[{}]", msg_type))
        }?;
        let mut metadata = HashMap::new();
        metadata.insert("recipient".to_string(), from.to_string());
        Some(WebhookMessage {
            text,
            chat_id: from.to_string(),
            is_group: false,
            has_mention: false,
            metadata,
        })
    }

    async fn send_reaction(&self, message_id: &str, emoji: &str, _metadata: &HashMap<String, String>) -> ChannelResult<()> {
        let recipient = _metadata.get("recipient")
            .ok_or_else(|| ChannelError::MessageError("Recipient required for reaction".into()))?;
        let phone_number_id = self.phone_number_id.as_ref()
            .ok_or_else(|| ChannelError::ConfigError("Phone number ID not set".into()))?;
        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "recipient_type": "individual",
            "to": recipient,
            "type": "reaction",
            "reaction": {
                "message_id": message_id,
                "emoji": emoji,
            }
        });
        self.send_api_request("POST", &format!("/{}/messages", phone_number_id), Some(&body)).await?;
        Ok(())
    }

    async fn send_media(&self, target: &str, media: &ChannelMedia) -> ChannelResult<String> {
        let phone_number_id = self.phone_number_id.as_ref()
            .ok_or_else(|| ChannelError::ConfigError("Phone number ID not set".into()))?;
        let media_type_str = match media.media_type {
            MediaType::Photo => "image",
            MediaType::Audio => "audio",
            MediaType::Voice => "audio",
            MediaType::Video => "video",
            MediaType::Document | MediaType::File => "document",
            MediaType::Sticker => "sticker",
        };
        let media_obj = match &media.data {
            MediaData::Url(u) => serde_json::json!({"link": u}),
            MediaData::FileId(id) => serde_json::json!({"id": id}),
            MediaData::Bytes(_) => return Err(ChannelError::UnsupportedOperation(
                "WhatsApp send_media with raw bytes not supported".into(),
            )),
        };
        let mut body = serde_json::json!({
            "messaging_product": "whatsapp",
            "to": target,
            "type": media_type_str,
            media_type_str: media_obj,
        });
        if let Some(caption) = &media.caption
            && let Some(obj) = body.get_mut(media_type_str).and_then(|o| o.as_object_mut())
        {
            obj.insert("caption".to_string(), serde_json::Value::String(caption.clone()));
        }
        let resp = self.send_api_request("POST", &format!("/{}/messages", phone_number_id), Some(&body)).await?;
        Ok(resp.get("messages")
            .and_then(|m| m.as_array())
            .and_then(|a| a.first())
            .and_then(|m| m.get("id"))
            .and_then(|id| id.as_str())
            .unwrap_or_default()
            .to_string())
    }

    async fn download_media(&self, media_id: &str) -> ChannelResult<Vec<u8>> {
        let info = self.send_api_request("GET", &format!("/{}", media_id), None).await?;
        let url = info.get("url").and_then(|u| u.as_str())
            .ok_or_else(|| ChannelError::MessageError("No download URL in response".into()))?;
        let token = self.api_token.as_ref()
            .ok_or_else(|| ChannelError::AuthenticationError("API token not set".into()))?;
        let client = self.client.as_ref()
            .ok_or_else(|| ChannelError::ConnectionError("Client not initialized".into()))?;
        let bytes = client.get(url)
            .header("Authorization", format!("Bearer {}", token))
            .send().await
            .map_err(|e| ChannelError::ConnectionError(e.to_string()))?
            .bytes().await
            .map_err(|e| ChannelError::MessageError(e.to_string()))?;
        Ok(bytes.to_vec())
    }

    async fn send_poll(&self, target: &str, poll: &PollRequest) -> ChannelResult<String> {
        let phone_number_id = self.phone_number_id.as_ref()
            .ok_or_else(|| ChannelError::ConfigError("Phone number ID not set".into()))?;
        let buttons: Vec<serde_json::Value> = poll.options.iter()
            .take(3)
            .map(|o| serde_json::json!({"type": "reply", "reply": {"id": o, "title": o}}))
            .collect();
        let body = serde_json::json!({
            "messaging_product": "whatsapp",
            "to": target,
            "type": "interactive",
            "interactive": {
                "type": "button",
                "body": {"text": poll.question},
                "action": {"buttons": buttons},
            }
        });
        let resp = self.send_api_request("POST", &format!("/{}/messages", phone_number_id), Some(&body)).await?;
        Ok(resp.get("messages")
            .and_then(|m| m.as_array())
            .and_then(|a| a.first())
            .and_then(|m| m.get("id"))
            .and_then(|id| id.as_str())
            .unwrap_or_default()
            .to_string())
    }

    fn get_message_sender(&self) -> ChannelResult<Box<dyn MessageSender>> {
        Ok(Box::new(WhatsAppSender {
            channel: Arc::new(RwLock::new(self.clone())),
        }))
    }
}

impl Clone for WhatsAppChannel {
    fn clone(&self) -> Self {
        Self {
            phone_number_id: self.phone_number_id.clone(),
            api_token: self.api_token.clone(),
            api_url: self.api_url.clone(),
            client: None,
            status: self.status,
            business_account_id: self.business_account_id.clone(),
        }
    }
}

struct WhatsAppSender {
    channel: Arc<RwLock<WhatsAppChannel>>,
}

impl MessageSender for WhatsAppSender {
    fn send<'a>(&'a self, content: &'a str, metadata: HashMap<String, String>) -> Pin<Box<dyn std::future::Future<Output = ChannelResult<String>> + Send + 'a>> {
        let channel = self.channel.clone();
        let content = content.to_string();
        let metadata = metadata.clone();
        
        Box::pin(async move {
            let message = ChannelMessage {
                id: uuid::Uuid::new_v4().to_string(),
                channel: "whatsapp".to_string(),
                sender: String::new(),
                content,
                timestamp: chrono::Utc::now().timestamp_millis(),
                metadata,
            };
            
            channel.read().await.send_message(&message).await
        })
    }
}
