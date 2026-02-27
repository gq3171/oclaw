//! Deepgram audio transcription provider.

use async_trait::async_trait;
use reqwest::Client;
use tracing::debug;

use super::{AudioRequest, ImageRequest, MediaProvider, MediaProviderError, VideoRequest};
use crate::types::MediaCapability;

pub struct DeepgramMediaProvider {
    client: Client,
    api_key: String,
    base_url: String,
}

impl DeepgramMediaProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url: "https://api.deepgram.com/v1".to_string(),
        }
    }

    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }
}

#[async_trait]
impl MediaProvider for DeepgramMediaProvider {
    fn id(&self) -> &str {
        "deepgram"
    }

    fn capabilities(&self) -> Vec<MediaCapability> {
        vec![MediaCapability::Audio]
    }

    fn model_for(&self, capability: MediaCapability) -> Option<String> {
        match capability {
            MediaCapability::Audio => Some("nova-2".to_string()),
            MediaCapability::Image | MediaCapability::Video => None,
        }
    }

    async fn describe_image(&self, _req: &ImageRequest) -> Result<String, MediaProviderError> {
        Err(MediaProviderError::Unsupported(
            "Deepgram does not support image description".to_string(),
        ))
    }

    async fn transcribe_audio(&self, req: &AudioRequest) -> Result<String, MediaProviderError> {
        debug!(provider = "deepgram", "Sending transcription request");

        let resp = self
            .client
            .post(format!(
                "{}/listen?model=nova-2&smart_format=true",
                self.base_url
            ))
            .header("Authorization", format!("Token {}", self.api_key))
            .header("Content-Type", &req.mime_type)
            .body(req.audio_data.clone())
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(MediaProviderError::Api(format!("{}: {}", status, text)));
        }

        let json: serde_json::Value = resp.json().await?;
        let text = json["results"]["channels"][0]["alternatives"][0]["transcript"]
            .as_str()
            .unwrap_or("")
            .to_string();
        Ok(text)
    }

    async fn describe_video(&self, _req: &VideoRequest) -> Result<String, MediaProviderError> {
        Err(MediaProviderError::Unsupported(
            "Deepgram does not support video description".to_string(),
        ))
    }
}
