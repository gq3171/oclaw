//! TTS provider trait and error types.

use async_trait::async_trait;
use std::path::{Path, PathBuf};

pub mod edge;
pub mod elevenlabs;
pub mod openai;

#[async_trait]
pub trait TtsProviderBackend: Send + Sync {
    fn id(&self) -> &str;
    fn default_voice(&self) -> &str;
    fn available_voices(&self) -> Vec<&str>;
    async fn synthesize(
        &self,
        text: &str,
        voice: Option<&str>,
        output_path: &Path,
    ) -> Result<SynthesizeResult, TtsError>;
}

#[derive(Debug, Clone)]
pub struct SynthesizeResult {
    pub output_path: PathBuf,
    pub duration_ms: Option<u64>,
    pub bytes_written: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum TtsError {
    #[error("API error: {0}")]
    Api(String),
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("provider not configured: {0}")]
    NotConfigured(String),
}
