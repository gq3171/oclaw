//! Google Gemini vision provider.

use async_trait::async_trait;
use base64::Engine;
use reqwest::Client;
use tracing::debug;

use super::{AudioRequest, ImageRequest, MediaProvider, MediaProviderError, VideoRequest};
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
        vec![
            MediaCapability::Image,
            MediaCapability::Audio,
            MediaCapability::Video,
        ]
    }

    fn model_for(&self, capability: MediaCapability) -> Option<String> {
        match capability {
            MediaCapability::Image | MediaCapability::Audio | MediaCapability::Video => {
                Some(self.model.clone())
            }
        }
    }

    async fn describe_image(&self, req: &ImageRequest) -> Result<String, MediaProviderError> {
        let prompt = req
            .prompt
            .as_deref()
            .unwrap_or("Describe this image concisely.");
        self.generate_inline_data_text(&req.mime_type, &req.image_data, prompt, "vision")
            .await
    }

    async fn transcribe_audio(&self, req: &AudioRequest) -> Result<String, MediaProviderError> {
        let prompt = req
            .language
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map_or("Transcribe the audio.".to_string(), |lang| {
                format!("Transcribe the audio in {}.", lang)
            });
        self.generate_inline_data_text(&req.mime_type, &req.audio_data, &prompt, "audio")
            .await
    }

    async fn describe_video(&self, req: &VideoRequest) -> Result<String, MediaProviderError> {
        let prompt = req
            .prompt
            .as_deref()
            .unwrap_or("Describe this video concisely.");
        self.generate_inline_data_text(&req.mime_type, &req.video_data, prompt, "video")
            .await
    }
}

impl GoogleMediaProvider {
    async fn generate_inline_data_text(
        &self,
        mime_type: &str,
        bytes: &[u8],
        prompt: &str,
        mode: &str,
    ) -> Result<String, MediaProviderError> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        let body = serde_json::json!({
            "contents": [{
                "parts": [
                    {"text": prompt},
                    {
                        "inline_data": {
                            "mime_type": mime_type,
                            "data": b64
                        }
                    }
                ]
            }]
        });

        debug!(
            provider = "google",
            mode, "Sending Gemini inline-data request"
        );

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
        extract_text_from_candidates(&json)
            .ok_or_else(|| MediaProviderError::Api("Gemini response missing text".to_string()))
    }
}

fn extract_text_from_candidates(json: &serde_json::Value) -> Option<String> {
    let parts = json
        .get("candidates")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.get("content"))
        .and_then(|v| v.get("parts"))
        .and_then(|v| v.as_array())?;

    let text = parts
        .iter()
        .filter_map(|part| part.get("text").and_then(|v| v.as_str()))
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>()
        .join("\n");

    if text.is_empty() { None } else { Some(text) }
}

#[cfg(test)]
mod tests {
    use super::extract_text_from_candidates;

    #[test]
    fn extract_text_from_candidates_joins_multiple_text_parts() {
        let payload = serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [
                        {"text": "line one"},
                        {"inline_data": {"mime_type": "audio/wav", "data": "abc"}},
                        {"text": "line two"}
                    ]
                }
            }]
        });
        assert_eq!(
            extract_text_from_candidates(&payload).as_deref(),
            Some("line one\nline two")
        );
    }

    #[test]
    fn extract_text_from_candidates_returns_none_without_text() {
        let payload = serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [
                        {"inline_data": {"mime_type": "audio/wav", "data": "abc"}}
                    ]
                }
            }]
        });
        assert!(extract_text_from_candidates(&payload).is_none());
    }
}
