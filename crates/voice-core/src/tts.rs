use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::TTSEngine;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TTSProvider {
    OpenAI,
    GoogleCloud,
    AmazonPolly,
    MicrosoftAzure,
    ElevenLabs,
    Local,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TTSVoice {
    pub id: String,
    pub name: String,
    pub provider: TTSProvider,
    pub language: String,
    pub neural: bool,
}

pub struct TTSConfig {
    pub provider: TTSProvider,
    pub api_key: Option<String>,
    pub voice_id: Option<String>,
    pub model: Option<String>,
}

impl Default for TTSConfig {
    fn default() -> Self {
        Self {
            provider: TTSProvider::OpenAI,
            api_key: None,
            voice_id: None,
            model: None,
        }
    }
}

pub struct TTSClient {
    config: TTSConfig,
    voices: Vec<TTSVoice>,
}

impl TTSClient {
    pub fn new(config: TTSConfig) -> Self {
        let voices = vec![
            TTSVoice {
                id: "alloy".to_string(),
                name: "Alloy".to_string(),
                provider: config.provider,
                language: "en".to_string(),
                neural: true,
            },
            TTSVoice {
                id: "echo".to_string(),
                name: "Echo".to_string(),
                provider: config.provider,
                language: "en".to_string(),
                neural: true,
            },
            TTSVoice {
                id: "fable".to_string(),
                name: "Fable".to_string(),
                provider: config.provider,
                language: "en".to_string(),
                neural: true,
            },
            TTSVoice {
                id: "onyx".to_string(),
                name: "Onyx".to_string(),
                provider: config.provider,
                language: "en".to_string(),
                neural: true,
            },
            TTSVoice {
                id: "nova".to_string(),
                name: "Nova".to_string(),
                provider: config.provider,
                language: "en".to_string(),
                neural: true,
            },
            TTSVoice {
                id: "shimmer".to_string(),
                name: "Shimmer".to_string(),
                provider: config.provider,
                language: "en".to_string(),
                neural: true,
            },
        ];

        Self { config, voices }
    }

    pub fn with_api_key(mut self, api_key: String) -> Self {
        self.config.api_key = Some(api_key);
        self
    }

    pub fn with_voice(mut self, voice_id: &str) -> Self {
        self.config.voice_id = Some(voice_id.to_string());
        self
    }

    pub fn with_model(mut self, model: &str) -> Self {
        self.config.model = Some(model.to_string());
        self
    }
}

#[async_trait]
impl TTSEngine for TTSClient {
    async fn speak(&self, text: &str, voice: &TTSVoice) -> Result<Vec<u8>> {
        match self.config.provider {
            TTSProvider::OpenAI => {
                self.openai_tts(text, voice).await
            }
            TTSProvider::ElevenLabs => {
                self.elevenlabs_tts(text, voice).await
            }
            _ => {
                self.default_tts(text, voice).await
            }
        }
    }

    fn list_voices(&self) -> Vec<TTSVoice> {
        self.voices.clone()
    }

    fn provider(&self) -> TTSProvider {
        self.config.provider
    }
}

impl TTSClient {
    async fn openai_tts(&self, text: &str, voice: &TTSVoice) -> Result<Vec<u8>> {
        if let Some(ref api_key) = self.config.api_key {
            let client = reqwest::Client::new();
            let response = client
                .post("https://api.openai.com/v1/audio/speech")
                .header("Authorization", format!("Bearer {}", api_key))
                .json(&serde_json::json!({
                    "model": self.config.model.as_deref().unwrap_or("tts-1"),
                    "voice": voice.id,
                    "input": text,
                    "response_format": "mp3"
                }))
                .send()
                .await?;

            let audio = response.bytes().await?.to_vec();
            Ok(audio)
        } else {
            self.default_tts(text, voice).await
        }
    }

    async fn elevenlabs_tts(&self, text: &str, voice: &TTSVoice) -> Result<Vec<u8>> {
        if let Some(ref api_key) = self.config.api_key {
            let voice_id = self.config.voice_id.as_deref().unwrap_or(&voice.id);
            let client = reqwest::Client::new();
            let response = client
                .post(format!(
                    "https://api.elevenlabs.io/v1/text-to-speech/{}",
                    voice_id
                ))
                .header("xi-api-key", api_key)
                .json(&serde_json::json!({
                    "text": text,
                    "voice_settings": {
                        "stability": 0.5,
                        "similarity_boost": 0.75
                    }
                }))
                .send()
                .await?;

            let audio = response.bytes().await?.to_vec();
            Ok(audio)
        } else {
            self.default_tts(text, voice).await
        }
    }

    async fn default_tts(&self, text: &str, _voice: &TTSVoice) -> Result<Vec<u8>> {
        let silence: Vec<u8> = vec![0u8; 1000];
        Ok(silence)
    }
}
