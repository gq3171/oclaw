//! Media processing pipeline — routes attachments to providers.

use tracing::{debug, warn};

use crate::cache::MediaAttachmentCache;
use crate::providers::{
    AudioRequest, ImageRequest, MediaProvider, MediaProviderError, VideoRequest,
};
use crate::types::{
    MediaAttachment, MediaCapability, MediaConfig, MediaOutputKind, MediaUnderstandingOutput,
};

pub struct MediaPipeline {
    providers: Vec<Box<dyn MediaProvider>>,
    cache: MediaAttachmentCache,
    config: MediaConfig,
}

impl MediaPipeline {
    pub fn new(config: MediaConfig) -> Self {
        Self {
            providers: Vec::new(),
            cache: MediaAttachmentCache::default(),
            config,
        }
    }

    pub fn add_provider(&mut self, provider: Box<dyn MediaProvider>) {
        self.providers.push(provider);
    }

    pub fn with_cache_size(mut self, max_entries: usize) -> Self {
        self.cache = MediaAttachmentCache::new(max_entries);
        self
    }

    /// Process all attachments, returning understanding outputs.
    pub async fn process(
        &self,
        attachments: &[MediaAttachment],
    ) -> Vec<Result<MediaUnderstandingOutput, MediaProviderError>> {
        let mut results = Vec::new();

        for (idx, attachment) in attachments.iter().enumerate() {
            let Some(cap) = attachment.capability() else {
                debug!(mime = %attachment.mime_type, "Skipping unsupported media type");
                continue;
            };

            if !self.check_size_limit(attachment, &cap) {
                warn!(
                    mime = %attachment.mime_type,
                    size = attachment.size_bytes,
                    "Attachment exceeds size limit"
                );
                continue;
            }

            let result = self.process_single(idx, attachment, &cap).await;
            results.push(result);
        }

        results
    }

    async fn process_single(
        &self,
        index: usize,
        attachment: &MediaAttachment,
        capability: &MediaCapability,
    ) -> Result<MediaUnderstandingOutput, MediaProviderError> {
        // Read file data
        let data = tokio::fs::read(&attachment.path).await.map_err(|e| {
            MediaProviderError::Api(format!("Failed to read {}: {}", attachment.path, e))
        })?;

        // Check cache
        let hash = MediaAttachmentCache::content_hash(&data);
        if let Some(cached) = self.cache.get(&hash) {
            debug!(hash = %hash, "Cache hit for media attachment");
            return Ok(MediaUnderstandingOutput {
                kind: capability_to_output_kind(capability),
                attachment_index: index,
                text: cached,
                provider: "cache".to_string(),
                model: None,
            });
        }

        // Find a provider that supports this capability
        let provider = self.find_provider(capability).ok_or_else(|| {
            MediaProviderError::Unsupported(format!(
                "No provider for {:?}",
                capability
            ))
        })?;

        let text = match capability {
            MediaCapability::Image => {
                let req = ImageRequest {
                    image_data: data,
                    mime_type: attachment.mime_type.clone(),
                    prompt: None,
                };
                provider.describe_image(&req).await?
            }
            MediaCapability::Audio => {
                let req = AudioRequest {
                    audio_data: data,
                    mime_type: attachment.mime_type.clone(),
                    language: None,
                };
                provider.transcribe_audio(&req).await?
            }
            MediaCapability::Video => {
                let req = VideoRequest {
                    video_data: data,
                    mime_type: attachment.mime_type.clone(),
                    prompt: None,
                };
                provider.describe_video(&req).await?
            }
        };

        // Cache the result
        self.cache.put(hash, text.clone());

        Ok(MediaUnderstandingOutput {
            kind: capability_to_output_kind(capability),
            attachment_index: index,
            text,
            provider: provider.id().to_string(),
            model: None,
        })
    }

    fn find_provider(&self, capability: &MediaCapability) -> Option<&dyn MediaProvider> {
        self.providers
            .iter()
            .find(|p| p.capabilities().contains(capability))
            .map(|p| p.as_ref())
    }

    fn check_size_limit(&self, attachment: &MediaAttachment, cap: &MediaCapability) -> bool {
        match cap {
            MediaCapability::Image => attachment.size_bytes <= self.config.max_image_size_bytes,
            MediaCapability::Audio => true, // duration check would need metadata
            MediaCapability::Video => true, // duration check would need metadata
        }
    }
}

fn capability_to_output_kind(cap: &MediaCapability) -> MediaOutputKind {
    match cap {
        MediaCapability::Image => MediaOutputKind::ImageDescription,
        MediaCapability::Audio => MediaOutputKind::AudioTranscription,
        MediaCapability::Video => MediaOutputKind::VideoDescription,
    }
}
