//! OpenAI TTS provider.

use async_trait::async_trait;
use reqwest::Client;
use std::path::Path;
use tracing::debug;

use super::{SynthesizeResult, TtsError, TtsProviderBackend};

pub struct OpenAiTts {
    client: Client,
    api_key: String,
    base_url: String,
    model: String,
}

impl OpenAiTts {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url: "https://api.openai.com/v1".to_string(),
            model: "tts-1".to_string(),
        }
    }

    pub fn with_model(mut self, model: String) -> Self {
        self.model = model;
        self
    }
}

#[async_trait]
impl TtsProviderBackend for OpenAiTts {
    fn id(&self) -> &str {
        "openai"
    }

    fn default_voice(&self) -> &str {
        "alloy"
    }

    fn available_voices(&self) -> Vec<&str> {
        vec!["alloy", "echo", "fable", "onyx", "nova", "shimmer"]
    }

    async fn synthesize(
        &self,
        text: &str,
        voice: Option<&str>,
        output_path: &Path,
    ) -> Result<SynthesizeResult, TtsError> {
        let voice = voice.unwrap_or(self.default_voice());
        debug!(provider = "openai", voice, "Synthesizing speech");

        let body = serde_json::json!({
            "model": self.model,
            "input": text,
            "voice": voice,
            "response_format": "mp3"
        });

        let resp = self
            .client
            .post(format!("{}/audio/speech", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let err = resp.text().await.unwrap_or_default();
            return Err(TtsError::Api(format!("{}: {}", status, err)));
        }

        let bytes = resp.bytes().await?;
        if let Some(parent) = output_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(output_path, &bytes).await?;

        Ok(SynthesizeResult {
            output_path: output_path.to_path_buf(),
            duration_ms: None,
            bytes_written: bytes.len() as u64,
        })
    }
}
