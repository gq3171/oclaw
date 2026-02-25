//! Google Gemini vision provider.

use async_trait::async_trait;
use base64::Engine;
use reqwest::Client;
use tracing::debug;

use super::{
    AudioRequest, ImageRequest, MediaProvider, MediaProviderError, VideoRequest,
};
use crate::types::MediaCapability;

pub struct GoogleMediaProvider {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl GoogleMediaProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url: "https://generativelanguage.googleapis.com/v1beta".to_string(),
            model: "gemini-2.0-flash".to_string(),
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
impl MediaProvider for GoogleMediaProvider {
    fn id(&self) -> &str {
        "google"
    }

    fn capabilities(&self) -> Vec<MediaCapability> {
        vec![MediaCapability::Image, MediaCapability::Video]
    }

    async fn describe_image(&self, req: &ImageRequest) -> Result<String, MediaProviderError> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&req.image_data);
        let prompt = req.prompt.as_deref().unwrap_or("Describe this image concisely.");

        let body = serde_json::json!({
            "contents": [{
                "parts": [
                    {"text": prompt},
                    {
                        "inline_data": {
                            "mime_type": req.mime_type,
                            "data": b64
                        }
                    }
                ]
            }]
        });

        debug!(provider = "google", "Sending Gemini vision request");

        let url = format!(
            "{}/models/{}:generateContent?key={}",
            self.base_url, self.model, self.api_key
        );

        let resp = self.client.post(&url).json(&body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(MediaProviderError::Api(format!("{}: {}", status, text)));
        }

        let json: serde_json::Value = resp.json().await?;
        let text = json["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();
        Ok(text)
    }

    async fn transcribe_audio(&self, _req: &AudioRequest) -> Result<String, MediaProviderError> {
        Err(MediaProviderError::Unsupported(
            "Google Gemini audio transcription not yet implemented".to_string(),
        ))
    }

    async fn describe_video(&self, req: &VideoRequest) -> Result<String, MediaProviderError> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&req.video_data);
        let prompt = req.prompt.as_deref().unwrap_or("Describe this video concisely.");

        let body = serde_json::json!({
            "contents": [{
                "parts": [
                    {"text": prompt},
                    {
                        "inline_data": {
                            "mime_type": req.mime_type,
                            "data": b64
                        }
                    }
                ]
            }]
        });

        debug!(provider = "google", "Sending Gemini video request");

        let url = format!(
            "{}/models/{}:generateContent?key={}",
            self.base_url, self.model, self.api_key
        );

        let resp = self.client.post(&url).json(&body).send().await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(MediaProviderError::Api(format!("{}: {}", status, text)));
        }

        let json: serde_json::Value = resp.json().await?;
        let text = json["candidates"][0]["content"]["parts"][0]["text"]
            .as_str()
            .unwrap_or("")
            .to_string();
        Ok(text)
    }
}
