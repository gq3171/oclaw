//! Media provider trait and registry.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::types::MediaCapability;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageRequest {
    pub image_data: Vec<u8>,
    pub mime_type: String,
    pub prompt: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioRequest {
    pub audio_data: Vec<u8>,
    pub mime_type: String,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoRequest {
    pub video_data: Vec<u8>,
    pub mime_type: String,
    pub prompt: Option<String>,
}

#[async_trait]
pub trait MediaProvider: Send + Sync {
    fn id(&self) -> &str;
    fn capabilities(&self) -> Vec<MediaCapability>;
    async fn describe_image(&self, req: &ImageRequest) -> Result<String, MediaProviderError>;
    async fn transcribe_audio(&self, req: &AudioRequest) -> Result<String, MediaProviderError>;
    async fn describe_video(&self, req: &VideoRequest) -> Result<String, MediaProviderError>;
}

#[derive(Debug, thiserror::Error)]
pub enum MediaProviderError {
    #[error("unsupported capability: {0}")]
    Unsupported(String),
    #[error("API error: {0}")]
    Api(String),
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),
    #[error("encoding error: {0}")]
    Encoding(String),
}

pub mod anthropic;
pub mod deepgram;
pub mod google;
pub mod openai;
