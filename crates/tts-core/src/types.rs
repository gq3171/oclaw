//! Core types for TTS.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TtsAutoMode {
    #[default]
    Off,
    Always,
    Inbound,
    Tagged,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TtsProvider {
    OpenAI,
    ElevenLabs,
    #[default]
    Edge,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsRequest {
    pub text: String,
    pub provider: TtsProvider,
    pub voice: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsResult {
    pub success: bool,
    pub audio_path: Option<PathBuf>,
    pub error: Option<String>,
    pub latency_ms: Option<u64>,
    pub provider: TtsProvider,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsConfig {
    pub enabled: bool,
    pub auto_mode: TtsAutoMode,
    pub default_provider: TtsProvider,
    pub default_voice: Option<String>,
    pub output_dir: PathBuf,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_mode: TtsAutoMode::Off,
            default_provider: TtsProvider::Edge,
            default_voice: None,
            output_dir: PathBuf::from("tts_output"),
        }
    }
}
