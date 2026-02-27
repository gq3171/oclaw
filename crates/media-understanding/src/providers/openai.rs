//! OpenAI vision + Whisper provider.

use async_trait::async_trait;
use base64::Engine;
use reqwest::Client;
use tracing::debug;

use super::{AudioRequest, ImageRequest, MediaProvider, MediaProviderError, VideoRequest};
use crate::types::MediaCapability;

pub struct OpenAiMediaProvider {
    client: Client,
    api_key: String,
    base_url: String,
    vision_model: String,
    whisper_model: String,
}

impl OpenAiMediaProvider {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url: "https://api.openai.com/v1".to_string(),
            vision_model: "gpt-4o".to_string(),
            whisper_model: "whisper-1".to_string(),
        }
    }

    pub fn with_base_url(mut self, url: String) -> Self {
        self.base_url = url;
        self
    }

    pub fn with_vision_model(mut self, model: String) -> Self {
        self.vision_model = model;
        self
    }
}

#[async_trait]
impl MediaProvider for OpenAiMediaProvider {
    fn id(&self) -> &str {
        "openai"
    }

    fn capabilities(&self) -> Vec<MediaCapability> {
        vec![MediaCapability::Image, MediaCapability::Audio]
    }

    fn model_for(&self, capability: MediaCapability) -> Option<String> {
        match capability {
            MediaCapability::Image => Some(self.vision_model.clone()),
            MediaCapability::Audio => Some(self.whisper_model.clone()),
            MediaCapability::Video => None,
        }
    }

    async fn describe_image(&self, req: &ImageRequest) -> Result<String, MediaProviderError> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&req.image_data);
        let data_url = format!("data:{};base64,{}", req.mime_type, b64);
        let prompt = req
            .prompt
            .as_deref()
            .unwrap_or("Describe this image concisely.");

        let body = serde_json::json!({
            "model": self.vision_model,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "text", "text": prompt},
                    {"type": "image_url", "image_url": {"url": data_url}}
                ]
            }],
            "max_tokens": 1024
        });

        debug!(provider = "openai", "Sending vision request");

        let resp = self
            .client
            .post(format!("{}/chat/completions", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(MediaProviderError::Api(format!("{}: {}", status, text)));
        }

        let json: serde_json::Value = resp.json().await?;
        let text = json["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        Ok(text)
    }

    async fn transcribe_audio(&self, req: &AudioRequest) -> Result<String, MediaProviderError> {
        let ext = match req.mime_type.as_str() {
            "audio/mp3" | "audio/mpeg" => "mp3",
            "audio/wav" => "wav",
            "audio/ogg" => "ogg",
            "audio/flac" => "flac",
            "audio/webm" => "webm",
            "audio/mp4" | "audio/m4a" => "m4a",
            _ => "mp3",
        };

        let file_part = reqwest::multipart::Part::bytes(req.audio_data.clone())
            .file_name(format!("audio.{}", ext))
            .mime_str(&req.mime_type)
            .map_err(|e| MediaProviderError::Encoding(e.to_string()))?;

        let mut form = reqwest::multipart::Form::new()
            .text("model", self.whisper_model.clone())
            .part("file", file_part);

        if let Some(ref lang) = req.language {
            form = form.text("language", lang.clone());
        }

        debug!(provider = "openai", "Sending whisper transcription request");

        let resp = self
            .client
            .post(format!("{}/audio/transcriptions", self.base_url))
            .bearer_auth(&self.api_key)
            .multipart(form)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(MediaProviderError::Api(format!("{}: {}", status, text)));
        }

        let json: serde_json::Value = resp.json().await?;
        let text = json["text"].as_str().unwrap_or("").to_string();
        Ok(text)
    }

    async fn describe_video(&self, _req: &VideoRequest) -> Result<String, MediaProviderError> {
        Err(MediaProviderError::Unsupported(
            "OpenAI does not support direct video description".to_string(),
        ))
    }
}
