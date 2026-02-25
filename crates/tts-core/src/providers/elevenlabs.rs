//! ElevenLabs TTS provider.

use async_trait::async_trait;
use reqwest::Client;
use std::path::Path;
use tracing::debug;

use super::{SynthesizeResult, TtsError, TtsProviderBackend};

pub struct ElevenLabsTts {
    client: Client,
    api_key: String,
    base_url: String,
}

impl ElevenLabsTts {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
            base_url: "https://api.elevenlabs.io/v1".to_string(),
        }
    }
}

#[async_trait]
impl TtsProviderBackend for ElevenLabsTts {
    fn id(&self) -> &str {
        "elevenlabs"
    }

    fn default_voice(&self) -> &str {
        "21m00Tcm4TlvDq8ikWAM" // Rachel
    }

    fn available_voices(&self) -> Vec<&str> {
        vec![
            "21m00Tcm4TlvDq8ikWAM", // Rachel
            "AZnzlk1XvdvUeBnXmlld", // Domi
            "EXAVITQu4vr4xnSDxMaL", // Bella
            "MF3mGyEYCl7XYWbV9V6O", // Elli
        ]
    }

    async fn synthesize(
        &self,
        text: &str,
        voice: Option<&str>,
        output_path: &Path,
    ) -> Result<SynthesizeResult, TtsError> {
        let voice_id = voice.unwrap_or(self.default_voice());
        debug!(provider = "elevenlabs", voice_id, "Synthesizing speech");

        let body = serde_json::json!({
            "text": text,
            "model_id": "eleven_monolingual_v1",
            "voice_settings": {
                "stability": 0.5,
                "similarity_boost": 0.5
            }
        });

        let resp = self.client
            .post(format!("{}/text-to-speech/{}", self.base_url, voice_id))
            .header("xi-api-key", &self.api_key)
            .header("Accept", "audio/mpeg")
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
