use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WebhookEvent {
    MessageReceived,
    MessageSent,
    MessageEdited,
    MessageDeleted,
    PresenceChanged,
    SessionStarted,
    SessionEnded,
    ChannelConnected,
    ChannelDisconnected,
    Error,
}

impl WebhookEvent {
    pub fn as_str(&self) -> &str {
        match self {
            WebhookEvent::MessageReceived => "message.received",
            WebhookEvent::MessageSent => "message.sent",
            WebhookEvent::MessageEdited => "message.edited",
            WebhookEvent::MessageDeleted => "message.deleted",
            WebhookEvent::PresenceChanged => "presence.changed",
            WebhookEvent::SessionStarted => "session.started",
            WebhookEvent::SessionEnded => "session.ended",
            WebhookEvent::ChannelConnected => "channel.connected",
            WebhookEvent::ChannelDisconnected => "channel.disconnected",
            WebhookEvent::Error => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookPayload {
    pub event: WebhookEvent,
    pub timestamp: i64,
    pub data: serde_json::Value,
    pub source: Option<String>,
}

impl WebhookPayload {
    pub fn new(event: WebhookEvent, data: serde_json::Value) -> Self {
        Self {
            event,
            timestamp: chrono::Utc::now().timestamp_millis(),
            data,
            source: None,
        }
    }

    pub fn with_source(mut self, source: &str) -> Self {
        self.source = Some(source.to_string());
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WebhookRegistration {
    pub id: String,
    pub url: String,
    pub events: Vec<WebhookEvent>,
    pub secret: Option<String>,
    pub headers: HashMap<String, String>,
    pub enabled: bool,
    pub retry_policy: Option<RetryPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryPolicy {
    pub max_retries: i32,
    pub initial_delay_ms: i64,
    pub max_delay_ms: i64,
    pub backoff_multiplier: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay_ms: 1000,
            max_delay_ms: 30000,
            backoff_multiplier: 2.0,
        }
    }
}

impl WebhookRegistration {
    pub fn new(url: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            url: url.to_string(),
            events: Vec::new(),
            secret: None,
            headers: HashMap::new(),
            enabled: true,
            retry_policy: None,
        }
    }

    pub fn with_events(mut self, events: Vec<WebhookEvent>) -> Self {
        self.events = events;
        self
    }

    pub fn with_secret(mut self, secret: &str) -> Self {
        self.secret = Some(secret.to_string());
        self
    }

    pub fn with_header(mut self, key: &str, value: &str) -> Self {
        self.headers.insert(key.to_string(), value.to_string());
        self
    }

    pub fn with_retry_policy(mut self, policy: RetryPolicy) -> Self {
        self.retry_policy = Some(policy);
        self
    }

    pub fn subscribes_to(&self, event: &WebhookEvent) -> bool {
        self.events.is_empty() || self.events.contains(event)
    }
}

pub struct WebhookManager {
    webhooks: Arc<RwLock<HashMap<String, WebhookRegistration>>>,
    http_client: reqwest::Client,
}

impl WebhookManager {
    pub fn new() -> Self {
        Self {
            webhooks: Arc::new(RwLock::new(HashMap::new())),
            http_client: reqwest::Client::new(),
        }
    }

    pub async fn register(&self, webhook: WebhookRegistration) -> String {
        let id = webhook.id.clone();
        let mut webhooks = self.webhooks.write().await;
        webhooks.insert(id.clone(), webhook);
        id
    }

    pub async fn unregister(&self, id: &str) -> Option<WebhookRegistration> {
        let mut webhooks = self.webhooks.write().await;
        webhooks.remove(id)
    }

    pub async fn get(&self, id: &str) -> Option<WebhookRegistration> {
        let webhooks = self.webhooks.read().await;
        webhooks.get(id).cloned()
    }

    pub async fn list(&self) -> Vec<WebhookRegistration> {
        let webhooks = self.webhooks.read().await;
        webhooks.values().cloned().collect()
    }

    pub async fn list_by_event(&self, event: &WebhookEvent) -> Vec<WebhookRegistration> {
        let webhooks = self.webhooks.read().await;
        webhooks.values()
            .filter(|w| w.enabled && w.subscribes_to(event))
            .cloned()
            .collect()
    }

    pub async fn update(&self, id: &str, webhook: WebhookRegistration) -> bool {
        let mut webhooks = self.webhooks.write().await;
        if webhooks.contains_key(id) {
            webhooks.insert(id.to_string(), webhook);
            true
        } else {
            false
        }
    }

    pub async fn toggle(&self, id: &str, enabled: bool) -> bool {
        let mut webhooks = self.webhooks.write().await;
        if let Some(webhook) = webhooks.get_mut(id) {
            webhook.enabled = enabled;
            true
        } else {
            false
        }
    }

    pub async fn trigger(&self, event: WebhookEvent, data: serde_json::Value) {
        let subscribers = self.list_by_event(&event).await;
        
        for webhook in subscribers {
            let payload = WebhookPayload::new(event, data.clone());
            let http_client = self.http_client.clone();
            let webhook_url = webhook.url.clone();
            let secret = webhook.secret.clone();
            let headers = webhook.headers.clone();
            
            tokio::spawn(async move {
                if let Err(e) = send_webhook(&http_client, &webhook_url, payload, secret.as_deref(), &headers).await {
                    tracing::error!("Failed to send webhook to {}: {}", webhook_url, e);
                }
            });
        }
    }

    pub async fn count(&self) -> usize {
        let webhooks = self.webhooks.read().await;
        webhooks.len()
    }
}

impl Default for WebhookManager {
    fn default() -> Self {
        Self::new()
    }
}

async fn send_webhook(
    client: &reqwest::Client,
    url: &str,
    payload: WebhookPayload,
    secret: Option<&str>,
    headers: &HashMap<String, String>,
) -> Result<(), WebhookError> {
    let mut request = client.post(url);
    
    for (key, value) in headers {
        request = request.header(key, value);
    }
    
    if let Some(secret) = secret {
        let payload_str = serde_json::to_string(&payload).map_err(WebhookError::SerializationError)?;
        let signature = calculate_hmac_sha256(secret, &payload_str);
        request = request.header("X-Webhook-Signature", signature);
    }
    
    request = request.header("Content-Type", "application/json");
    
    let response = request
        .json(&payload)
        .send()
        .await
        .map_err(WebhookError::RequestError)?;
    
    if !response.status().is_success() {
        return Err(WebhookError::ResponseError(format!(
            "HTTP {}",
            response.status()
        )));
    }
    
    Ok(())
}

fn calculate_hmac_sha256(secret: &str, message: &str) -> String {
    use hmac::{Hmac, Mac};
    type HmacSha256 = Hmac<sha2::Sha256>;
    
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes())
        .expect("HMAC can take key of any size");
    mac.update(message.as_bytes());
    
    let result = mac.finalize();
    hex::encode(result.into_bytes())
}

#[derive(Debug, thiserror::Error)]
pub enum WebhookError {
    #[error("Request error: {0}")]
    RequestError(#[from] reqwest::Error),
    
    #[error("Response error: {0}")]
    ResponseError(String),
    
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_webhook_registration() {
        let webhook = WebhookRegistration::new("https://example.com/webhook")
            .with_events(vec![WebhookEvent::MessageReceived, WebhookEvent::MessageSent])
            .with_secret("my_secret");
        
        assert!(webhook.subscribes_to(&WebhookEvent::MessageReceived));
        assert!(webhook.subscribes_to(&WebhookEvent::MessageSent));
        assert!(!webhook.subscribes_to(&WebhookEvent::PresenceChanged));
    }

    #[test]
    fn test_webhook_event_string() {
        assert_eq!(WebhookEvent::MessageReceived.as_str(), "message.received");
        assert_eq!(WebhookEvent::SessionStarted.as_str(), "session.started");
    }

    #[test]
    fn test_retry_policy_default() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_retries, 3);
        assert_eq!(policy.initial_delay_ms, 1000);
    }

    #[test]
    fn test_webhook_payload() {
        let payload = WebhookPayload::new(
            WebhookEvent::MessageReceived,
            serde_json::json!({"text": "hello"})
        );
        
        assert_eq!(payload.event, WebhookEvent::MessageReceived);
        assert!(payload.source.is_none());
    }
}
