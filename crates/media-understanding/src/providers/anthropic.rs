//! Anthropic Claude vision provider.

use async_trait::async_trait;
use base64::Engine;
use reqwest::Client;
use tracing::debug;

use super::{AudioRequest, ImageRequest, MediaProvider, MediaProviderError, VideoRequest};
use crate::types::MediaCapability;

pub struct AnthropicMediaProvider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl AnthropicMediaProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url: "https://api.anthropic.com/v1".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
        }
    }

    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }

    pub fn with_model(mut self, model: String) -> Self {
        self.model = model;
        self
    }
}

#[async_trait]
impl MediaProvider for AnthropicMediaProvider {
    fn id(&self) -> &str {
        "anthropic"
    }

    fn capabilities(&self) -> Vec<MediaCapability> {
        vec![MediaCapability::Image]
    }

    fn model_for(&self, capability: MediaCapability) -> Option<String> {
        match capability {
            MediaCapability::Image => Some(self.model.clone()),
            MediaCapability::Audio | MediaCapability::Video => None,
        }
    }

    async fn describe_image(&self, req: &ImageRequest) -> Result<String, MediaProviderError> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&req.image_data);
        let media_type = match req.mime_type.as_str() {
            "image/png" => "image/png",
            "image/gif" => "image/gif",
            "image/webp" => "image/webp",
            _ => "image/jpeg",
        };
        let prompt = req
            .prompt
            .as_deref()
            .unwrap_or("Describe this image concisely.");

        let body = serde_json::json!({
            "model": self.model,
            "max_tokens": 1024,
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": media_type,
                            "data": b64
                        }
                    },
                    {"type": "text", "text": prompt}
                ]
            }]
        });

        debug!(provider = "anthropic", "Sending vision request");

        let resp = self
            .client
            .post(format!("{}/messages", self.base_url))
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(MediaProviderError::Api(format!("{}: {}", status, text)));
        }

        let json: serde_json::Value = resp.json().await?;
        let text = json["content"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();
        Ok(text)
    }

    async fn transcribe_audio(&self, _req: &AudioRequest) -> Result<String, MediaProviderError> {
        Err(MediaProviderError::Unsupported(
            "Anthropic does not support audio transcription".to_string(),
        ))
    }

    async fn describe_video(&self, _req: &VideoRequest) -> Result<String, MediaProviderError> {
        Err(MediaProviderError::Unsupported(
            "Anthropic does not support direct video description".to_string(),
        ))
    }
}
