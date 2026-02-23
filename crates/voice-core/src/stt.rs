use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::STTEngine;

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum STTProvider {
    OpenAIWhisper,
    GoogleCloudSpeech,
    AmazonTranscribe,
    MicrosoftAzure,
    Deepgram,
    Local,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct STTResult {
    pub text: String,
    pub language: String,
    pub confidence: f32,
    pub duration_ms: u64,
}

#[allow(dead_code)]
pub struct STTConfig {
    pub provider: STTProvider,
    pub api_key: Option<String>,
    pub language: String,
    pub model: Option<String>,
}

impl Default for STTConfig {
    fn default() -> Self {
        Self {
            provider: STTProvider::OpenAIWhisper,
            api_key: None,
            language: "en".to_string(),
            model: None,
        }
    }
}

#[allow(dead_code)]
pub struct STTClient {
    config: STTConfig,
}

impl STTClient {
    pub fn new(config: STTConfig) -> Self {
        Self { config }
    }

    pub fn with_api_key(mut self, api_key: String) -> Self {
        self.config.api_key = Some(api_key);
        self
    }

    pub fn with_language(mut self, language: &str) -> Self {
        self.config.language = language.to_string();
        self
    }

    pub fn with_model(mut self, model: &str) -> Self {
        self.config.model = Some(model.to_string());
        self
    }
}

#[async_trait]
impl STTEngine for STTClient {
    async fn transcribe(&self, audio: &[u8]) -> Result<STTResult> {
        match self.config.provider {
            STTProvider::OpenAIWhisper => {
                self.whisper_transcribe(audio).await
            }
            STTProvider::Deepgram => {
                self.deepgram_transcribe(audio).await
            }
            _ => {
                self.default_transcribe(audio).await
            }
        }
    }

    fn provider(&self) -> STTProvider {
        self.config.provider
    }

    fn language(&self) -> &str {
        &self.config.language
    }

    fn set_language(&mut self, lang: &str) {
        self.config.language = lang.to_string();
    }
}

#[allow(dead_code)]
impl STTClient {
    async fn whisper_transcribe(&self, audio: &[u8]) -> Result<STTResult> {
        if let Some(ref api_key) = self.config.api_key {
            let client = reqwest::Client::new();

            let boundary = uuid::Uuid::new_v4().to_string();
            let mut body = Vec::new();

            body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
            body.extend_from_slice(b"Content-Disposition: form-data; name=\"model\"\r\n\r\n");
            body.extend_from_slice(self.config.model.as_deref().unwrap_or("whisper-1").as_bytes());
            body.extend_from_slice(b"\r\n");

            body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
            body.extend_from_slice(format!("Content-Disposition: form-data; name=\"language\"\r\n\r\n{}\r\n", self.config.language).as_bytes());

            body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
            body.extend_from_slice(b"Content-Disposition: form-data; name=\"file\"; filename=\"audio.mp3\"\r\nContent-Type: audio/mp3\r\n\r\n");
            body.extend_from_slice(audio);
            body.extend_from_slice(b"\r\n");

            body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

            let response = client
                .post("https://api.openai.com/v1/audio/transcriptions")
                .header("Authorization", format!("Bearer {}", api_key))
                .header("Content-Type", format!("multipart/form-data; boundary={}", boundary))
                .body(body)
                .send()
                .await?;

            #[derive(Deserialize)]
            struct WhisperResponse {
                text: String,
            }

            let result: WhisperResponse = response.json().await?;

            Ok(STTResult {
                text: result.text,
                language: self.config.language.clone(),
                confidence: 0.9,
                duration_ms: 0,
            })
        } else {
            self.default_transcribe(audio).await
        }
    }

    async fn deepgram_transcribe(&self, audio: &[u8]) -> Result<STTResult> {
        if let Some(ref api_key) = self.config.api_key {
            let client = reqwest::Client::new();
            
            let response = client
                .post(format!(
                    "https://api.deepgram.com/v1/listen?language={}&model=nova-2",
                    self.config.language
                ))
                .header("Authorization", format!("Token {}", api_key))
                .body(audio.to_vec())
                .header("Content-Type", "audio/mp3")
                .send()
                .await?;

            #[derive(Deserialize)]
            struct DeepgramResponse {
                results: DeepgramResults,
            }

            #[derive(Deserialize)]
            struct DeepgramResults {
                channels: Vec<DeepgramChannel>,
            }

            #[derive(Deserialize)]
            struct DeepgramChannel {
                alternatives: Vec<DeepgramAlternative>,
            }

            #[derive(Deserialize)]
            struct DeepgramAlternative {
                transcript: String,
            }

            let result: DeepgramResponse = response.json().await?;
            
            let text = result
                .results
                .channels
                .first()
                .and_then(|c| c.alternatives.first())
                .map(|a| a.transcript.clone())
                .unwrap_or_default();

            Ok(STTResult {
                text,
                language: self.config.language.clone(),
                confidence: 0.9,
                duration_ms: 0,
            })
        } else {
            self.default_transcribe(audio).await
        }
    }

    async fn default_transcribe(&self, _audio: &[u8]) -> Result<STTResult> {
        Ok(STTResult {
            text: String::new(),
            language: self.config.language.clone(),
            confidence: 0.0,
            duration_ms: 0,
        })
    }
}
