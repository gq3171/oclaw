//! Core types for media understanding.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaDecisionOutcome {
    Success,
    Skipped,
    Disabled,
    NoAttachment,
    ScopeDeny,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaModelDecisionType {
    Provider,
    Cli,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaModelDecisionOutcome {
    Success,
    Skipped,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaModelDecision {
    #[serde(rename = "type")]
    pub decision_type: MediaModelDecisionType,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub outcome: MediaModelDecisionOutcome,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaAttachmentDecision {
    pub attachment_index: usize,
    pub attempts: Vec<MediaModelDecision>,
    pub chosen: Option<MediaModelDecision>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaCapabilityDecision {
    pub capability: MediaCapability,
    pub outcome: MediaDecisionOutcome,
    pub attachments: Vec<MediaAttachmentDecision>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaConfig {
    #[serde(default = "default_true")]
    pub image_enabled: bool,
    #[serde(default = "default_true")]
    pub audio_enabled: bool,
    #[serde(default = "default_true")]
    pub video_enabled: bool,
    pub max_image_size_bytes: u64,
    pub max_audio_duration_secs: u64,
    pub max_video_duration_secs: u64,
    pub default_image_provider: String,
    pub default_audio_provider: String,
    #[serde(default = "default_video_provider")]
    pub default_video_provider: String,
    #[serde(default)]
    pub image_fallback_providers: Vec<String>,
    #[serde(default)]
    pub audio_fallback_providers: Vec<String>,
    #[serde(default)]
    pub video_fallback_providers: Vec<String>,
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            image_enabled: true,
            audio_enabled: true,
            video_enabled: true,
            max_image_size_bytes: 20 * 1024 * 1024,
            max_audio_duration_secs: 300,
            max_video_duration_secs: 120,
            default_image_provider: "openai".to_string(),
            default_audio_provider: "openai".to_string(),
            default_video_provider: default_video_provider(),
            image_fallback_providers: vec!["anthropic".to_string(), "google".to_string()],
            audio_fallback_providers: vec!["deepgram".to_string(), "google".to_string()],
            video_fallback_providers: vec![],
        }
    }
}

fn default_video_provider() -> String {
    "google".to_string()
}

fn default_true() -> bool {
    true
}
