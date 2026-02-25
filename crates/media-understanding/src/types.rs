//! Core types for media understanding.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaCapability {
    Image,
    Audio,
    Video,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaAttachment {
    pub path: String,
    pub mime_type: String,
    pub size_bytes: u64,
    pub original_name: Option<String>,
}

impl MediaAttachment {
    pub fn capability(&self) -> Option<MediaCapability> {
        if self.mime_type.starts_with("image/") {
            Some(MediaCapability::Image)
        } else if self.mime_type.starts_with("audio/") {
            Some(MediaCapability::Audio)
        } else if self.mime_type.starts_with("video/") {
            Some(MediaCapability::Video)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaUnderstandingOutput {
    pub kind: MediaOutputKind,
    pub attachment_index: usize,
    pub text: String,
    pub provider: String,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaOutputKind {
    ImageDescription,
    AudioTranscription,
    VideoDescription,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaConfig {
    pub max_image_size_bytes: u64,
    pub max_audio_duration_secs: u64,
    pub max_video_duration_secs: u64,
    pub default_image_provider: String,
    pub default_audio_provider: String,
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            max_image_size_bytes: 20 * 1024 * 1024,
            max_audio_duration_secs: 300,
            max_video_duration_secs: 120,
            default_image_provider: "openai".to_string(),
            default_audio_provider: "openai".to_string(),
        }
    }
}
