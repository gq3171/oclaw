//! Media processing pipeline — routes attachments to providers.

use std::collections::HashSet;

use tracing::{debug, warn};

use crate::cache::MediaAttachmentCache;
use crate::providers::{
    AudioRequest, ImageRequest, MediaProvider, MediaProviderError, VideoRequest,
};
use crate::types::{
    MediaAttachment, MediaAttachmentDecision, MediaCapability, MediaCapabilityDecision,
    MediaConfig, MediaDecisionOutcome, MediaModelDecision, MediaModelDecisionOutcome,
    MediaModelDecisionType, MediaOutputKind, MediaUnderstandingOutput,
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
        self.process_with_decisions(attachments).await.0
    }

    /// Process all attachments and return Node-style attempt decisions.
    pub async fn process_with_decisions(
        &self,
        attachments: &[MediaAttachment],
    ) -> (
        Vec<Result<MediaUnderstandingOutput, MediaProviderError>>,
        Vec<MediaCapabilityDecision>,
    ) {
        let mut results = Vec::new();
        let mut image_decisions = Vec::new();
        let mut audio_decisions = Vec::new();
        let mut video_decisions = Vec::new();

        for (idx, attachment) in attachments.iter().enumerate() {
            let Some(cap) = attachment.capability() else {
                debug!(mime = %attachment.mime_type, "Skipping unsupported media type");
                continue;
            };
            if !self.capability_enabled(cap) {
                debug!(capability = ?cap, "Capability disabled; skipping attachment");
                continue;
            }

            if !self.check_size_limit(attachment, &cap) {
                warn!(
                    mime = %attachment.mime_type,
                    size = attachment.size_bytes,
                    "Attachment exceeds size limit"
                );
                push_capability_decision(
                    &mut image_decisions,
                    &mut audio_decisions,
                    &mut video_decisions,
                    cap,
                    MediaAttachmentDecision {
                        attachment_index: idx,
                        attempts: vec![MediaModelDecision {
                            decision_type: MediaModelDecisionType::Provider,
                            provider: None,
                            model: None,
                            outcome: MediaModelDecisionOutcome::Skipped,
                            reason: Some(
                                "maxBytes: attachment exceeds configured size limit".to_string(),
                            ),
                        }],
                        chosen: None,
                    },
                );
                continue;
            }

            let processed = self.process_single(idx, attachment, &cap).await;
            results.push(processed.result);
            push_capability_decision(
                &mut image_decisions,
                &mut audio_decisions,
                &mut video_decisions,
                cap,
                processed.decision,
            );
        }

        let mut decisions = Vec::new();
        append_capability_decision_with_state(
            &mut decisions,
            MediaCapability::Image,
            image_decisions,
            self.config.image_enabled,
        );
        append_capability_decision_with_state(
            &mut decisions,
            MediaCapability::Audio,
            audio_decisions,
            self.config.audio_enabled,
        );
        append_capability_decision_with_state(
            &mut decisions,
            MediaCapability::Video,
            video_decisions,
            self.config.video_enabled,
        );

        (results, decisions)
    }

    fn capability_enabled(&self, cap: MediaCapability) -> bool {
        match cap {
            MediaCapability::Image => self.config.image_enabled,
            MediaCapability::Audio => self.config.audio_enabled,
            MediaCapability::Video => self.config.video_enabled,
        }
    }

    async fn process_single(
        &self,
        index: usize,
        attachment: &MediaAttachment,
        capability: &MediaCapability,
    ) -> ProcessSingleOutcome {
        let data = tokio::fs::read(&attachment.path).await.map_err(|e| {
            MediaProviderError::Api(format!("Failed to read {}: {}", attachment.path, e))
        });
        let data = match data {
            Ok(data) => data,
            Err(err) => {
                return ProcessSingleOutcome {
                    result: Err(err),
                    decision: MediaAttachmentDecision {
                        attachment_index: index,
                        attempts: vec![],
                        chosen: None,
                    },
                };
            }
        };

        let hash = MediaAttachmentCache::content_hash(&data);
        if let Some(cached) = self.cache.get(&hash) {
            debug!(hash = %hash, "Cache hit for media attachment");
            let chosen = MediaModelDecision {
                decision_type: MediaModelDecisionType::Provider,
                provider: Some("cache".to_string()),
                model: None,
                outcome: MediaModelDecisionOutcome::Success,
                reason: Some("cache hit".to_string()),
            };
            return ProcessSingleOutcome {
                result: Ok(MediaUnderstandingOutput {
                    kind: capability_to_output_kind(capability),
                    attachment_index: index,
                    text: cached,
                    provider: "cache".to_string(),
                    model: None,
                }),
                decision: MediaAttachmentDecision {
                    attachment_index: index,
                    attempts: vec![chosen.clone()],
                    chosen: Some(chosen),
                },
            };
        }

        let candidates = self.ordered_providers(capability);
        if candidates.is_empty() {
            return ProcessSingleOutcome {
                result: Err(MediaProviderError::Unsupported(format!(
                    "No provider configured for {:?}",
                    capability
                ))),
                decision: MediaAttachmentDecision {
                    attachment_index: index,
                    attempts: vec![MediaModelDecision {
                        decision_type: MediaModelDecisionType::Provider,
                        provider: None,
                        model: None,
                        outcome: MediaModelDecisionOutcome::Skipped,
                        reason: Some(format!("no provider configured for {:?}", capability)),
                    }],
                    chosen: None,
                },
            };
        }

        let mut failures = Vec::new();
        let mut unsupported = Vec::new();
        let mut attempts = Vec::new();

        for provider in candidates {
            let provider_id = provider.id().to_string();
            let provider_model = provider.model_for(*capability);
            let attempt = match capability {
                MediaCapability::Image => {
                    let req = ImageRequest {
                        image_data: data.clone(),
                        mime_type: attachment.mime_type.clone(),
                        prompt: None,
                    };
                    provider.describe_image(&req).await
                }
                MediaCapability::Audio => {
                    let req = AudioRequest {
                        audio_data: data.clone(),
                        mime_type: attachment.mime_type.clone(),
                        language: None,
                    };
                    provider.transcribe_audio(&req).await
                }
                MediaCapability::Video => {
                    let req = VideoRequest {
                        video_data: data.clone(),
                        mime_type: attachment.mime_type.clone(),
                        prompt: None,
                    };
                    provider.describe_video(&req).await
                }
            };

            match attempt {
                Ok(text) => {
                    self.cache.put(hash.clone(), text.clone());
                    let chosen = MediaModelDecision {
                        decision_type: MediaModelDecisionType::Provider,
                        provider: Some(provider_id.clone()),
                        model: provider_model.clone(),
                        outcome: MediaModelDecisionOutcome::Success,
                        reason: None,
                    };
                    attempts.push(chosen.clone());
                    return ProcessSingleOutcome {
                        result: Ok(MediaUnderstandingOutput {
                            kind: capability_to_output_kind(capability),
                            attachment_index: index,
                            text,
                            provider: provider_id,
                            model: provider_model,
                        }),
                        decision: MediaAttachmentDecision {
                            attachment_index: index,
                            attempts,
                            chosen: Some(chosen),
                        },
                    };
                }
                Err(MediaProviderError::Unsupported(reason)) => {
                    debug!(
                        provider = %provider_id,
                        capability = ?capability,
                        reason = %reason,
                        "Provider capability unsupported, trying next"
                    );
                    attempts.push(MediaModelDecision {
                        decision_type: MediaModelDecisionType::Provider,
                        provider: Some(provider_id.clone()),
                        model: provider_model.clone(),
                        outcome: MediaModelDecisionOutcome::Skipped,
                        reason: Some(reason.clone()),
                    });
                    unsupported.push(format!("{}: {}", provider_id, reason));
                }
                Err(err) => {
                    warn!(
                        provider = %provider_id,
                        capability = ?capability,
                        error = %err,
                        "Provider attempt failed, trying next"
                    );
                    attempts.push(MediaModelDecision {
                        decision_type: MediaModelDecisionType::Provider,
                        provider: Some(provider_id.clone()),
                        model: provider_model,
                        outcome: MediaModelDecisionOutcome::Failed,
                        reason: Some(err.to_string()),
                    });
                    failures.push(format!("{}: {}", provider_id, err));
                }
            }
        }

        if !failures.is_empty() {
            let mut details = failures;
            details.extend(unsupported);
            return ProcessSingleOutcome {
                result: Err(MediaProviderError::Api(format!(
                    "All providers failed for {:?}: {}",
                    capability,
                    details.join(" | ")
                ))),
                decision: MediaAttachmentDecision {
                    attachment_index: index,
                    attempts,
                    chosen: None,
                },
            };
        }

        ProcessSingleOutcome {
            result: Err(MediaProviderError::Unsupported(format!(
                "No provider succeeded for {:?}: {}",
                capability,
                unsupported.join(" | ")
            ))),
            decision: MediaAttachmentDecision {
                attachment_index: index,
                attempts,
                chosen: None,
            },
        }
    }

    fn ordered_providers(&self, capability: &MediaCapability) -> Vec<&dyn MediaProvider> {
        let configured_ids = self.configured_provider_chain(capability);
        let mut out = Vec::new();
        let mut seen = HashSet::new();

        for cfg_id in configured_ids {
            if seen.contains(&cfg_id) {
                continue;
            }
            if let Some(p) = self.providers.iter().find(|p| {
                p.capabilities().contains(capability)
                    && normalize_provider_id(p.id()) == Some(cfg_id.clone())
            }) {
                out.push(p.as_ref());
                seen.insert(cfg_id);
            }
        }

        for provider in &self.providers {
            if !provider.capabilities().contains(capability) {
                continue;
            }
            let Some(pid) = normalize_provider_id(provider.id()) else {
                continue;
            };
            if seen.insert(pid) {
                out.push(provider.as_ref());
            }
        }

        out
    }

    fn configured_provider_chain(&self, capability: &MediaCapability) -> Vec<String> {
        let mut out = Vec::new();
        let push_norm = |list: &mut Vec<String>, raw: &str| {
            if let Some(pid) = normalize_provider_id(raw) {
                list.push(pid);
            }
        };

        match capability {
            MediaCapability::Image => {
                push_norm(&mut out, &self.config.default_image_provider);
                for pid in &self.config.image_fallback_providers {
                    push_norm(&mut out, pid);
                }
            }
            MediaCapability::Audio => {
                push_norm(&mut out, &self.config.default_audio_provider);
                for pid in &self.config.audio_fallback_providers {
                    push_norm(&mut out, pid);
                }
            }
            MediaCapability::Video => {
                push_norm(&mut out, &self.config.default_video_provider);
                for pid in &self.config.video_fallback_providers {
                    push_norm(&mut out, pid);
                }
            }
        }

        out
    }

    fn check_size_limit(&self, attachment: &MediaAttachment, cap: &MediaCapability) -> bool {
        match cap {
            MediaCapability::Image => attachment.size_bytes <= self.config.max_image_size_bytes,
            MediaCapability::Audio => true,
            MediaCapability::Video => true,
        }
    }
}

struct ProcessSingleOutcome {
    result: Result<MediaUnderstandingOutput, MediaProviderError>,
    decision: MediaAttachmentDecision,
}

fn push_capability_decision(
    image: &mut Vec<MediaAttachmentDecision>,
    audio: &mut Vec<MediaAttachmentDecision>,
    video: &mut Vec<MediaAttachmentDecision>,
    capability: MediaCapability,
    decision: MediaAttachmentDecision,
) {
    match capability {
        MediaCapability::Image => image.push(decision),
        MediaCapability::Audio => audio.push(decision),
        MediaCapability::Video => video.push(decision),
    }
}

fn append_capability_decision_with_state(
    out: &mut Vec<MediaCapabilityDecision>,
    capability: MediaCapability,
    attachments: Vec<MediaAttachmentDecision>,
    enabled: bool,
) {
    if !enabled {
        out.push(MediaCapabilityDecision {
            capability,
            outcome: MediaDecisionOutcome::Disabled,
            attachments: vec![],
        });
        return;
    }
    if attachments.is_empty() {
        out.push(MediaCapabilityDecision {
            capability,
            outcome: MediaDecisionOutcome::NoAttachment,
            attachments,
        });
        return;
    }
    let has_success = attachments.iter().any(|d| {
        d.chosen
            .as_ref()
            .map(|c| c.outcome == MediaModelDecisionOutcome::Success)
            .unwrap_or(false)
    });
    out.push(MediaCapabilityDecision {
        capability,
        outcome: if has_success {
            MediaDecisionOutcome::Success
        } else {
            MediaDecisionOutcome::Skipped
        },
        attachments,
    });
}

fn normalize_provider_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed == "gemini" {
        Some("google".to_string())
    } else {
        Some(trimmed)
    }
}

fn capability_to_output_kind(cap: &MediaCapability) -> MediaOutputKind {
    match cap {
        MediaCapability::Image => MediaOutputKind::ImageDescription,
        MediaCapability::Audio => MediaOutputKind::AudioTranscription,
        MediaCapability::Video => MediaOutputKind::VideoDescription,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::{Arc, Mutex};

    struct MockProvider {
        id: &'static str,
        caps: Vec<MediaCapability>,
        image_result: Result<&'static str, &'static str>,
        audio_result: Result<&'static str, &'static str>,
        video_result: Result<&'static str, &'static str>,
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl MediaProvider for MockProvider {
        fn id(&self) -> &str {
            self.id
        }

        fn capabilities(&self) -> Vec<MediaCapability> {
            self.caps.clone()
        }

        fn model_for(&self, capability: MediaCapability) -> Option<String> {
            if self.caps.contains(&capability) {
                Some(format!("{}-model", self.id))
            } else {
                None
            }
        }

        async fn describe_image(&self, _req: &ImageRequest) -> Result<String, MediaProviderError> {
            self.calls
                .lock()
                .expect("calls lock")
                .push(self.id.to_string());
            match self.image_result {
                Ok(v) => Ok(v.to_string()),
                Err("unsupported") => {
                    Err(MediaProviderError::Unsupported("unsupported".to_string()))
                }
                Err(e) => Err(MediaProviderError::Api(e.to_string())),
            }
        }

        async fn transcribe_audio(
            &self,
            _req: &AudioRequest,
        ) -> Result<String, MediaProviderError> {
            self.calls
                .lock()
                .expect("calls lock")
                .push(self.id.to_string());
            match self.audio_result {
                Ok(v) => Ok(v.to_string()),
                Err("unsupported") => {
                    Err(MediaProviderError::Unsupported("unsupported".to_string()))
                }
                Err(e) => Err(MediaProviderError::Api(e.to_string())),
            }
        }

        async fn describe_video(&self, _req: &VideoRequest) -> Result<String, MediaProviderError> {
            self.calls
                .lock()
                .expect("calls lock")
                .push(self.id.to_string());
            match self.video_result {
                Ok(v) => Ok(v.to_string()),
                Err("unsupported") => {
                    Err(MediaProviderError::Unsupported("unsupported".to_string()))
                }
                Err(e) => Err(MediaProviderError::Api(e.to_string())),
            }
        }
    }

    fn write_temp_file(bytes: &[u8]) -> String {
        static NEXT_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let seq = NEXT_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "oclaw-media-test-{}-{}.bin",
            std::process::id(),
            seq
        ));
        std::fs::write(&path, bytes).expect("write temp media file");
        path.to_string_lossy().to_string()
    }

    #[tokio::test]
    async fn tries_fallback_provider_when_primary_fails() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut pipeline = MediaPipeline::new(MediaConfig::default());
        pipeline.add_provider(Box::new(MockProvider {
            id: "openai",
            caps: vec![MediaCapability::Image],
            image_result: Err("upstream down"),
            audio_result: Err("unsupported"),
            video_result: Err("unsupported"),
            calls: calls.clone(),
        }));
        pipeline.add_provider(Box::new(MockProvider {
            id: "anthropic",
            caps: vec![MediaCapability::Image],
            image_result: Ok("from anthropic"),
            audio_result: Err("unsupported"),
            video_result: Err("unsupported"),
            calls: calls.clone(),
        }));

        let path = write_temp_file(b"image-bytes");
        let attachments = vec![MediaAttachment {
            path: path.clone(),
            mime_type: "image/png".to_string(),
            size_bytes: 11,
            original_name: None,
        }];

        let out = pipeline.process(&attachments).await;
        std::fs::remove_file(path).ok();

        assert_eq!(out.len(), 1);
        let output = out[0].as_ref().expect("fallback success");
        assert_eq!(output.provider, "anthropic");
        assert_eq!(output.text, "from anthropic");

        let calls = calls.lock().expect("calls lock").clone();
        assert_eq!(calls, vec!["openai".to_string(), "anthropic".to_string()]);
    }

    #[tokio::test]
    async fn skips_unsupported_and_uses_next_provider() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut cfg = MediaConfig::default();
        cfg.default_audio_provider = "openai".to_string();
        cfg.audio_fallback_providers = vec!["google".to_string()];

        let mut pipeline = MediaPipeline::new(cfg);
        pipeline.add_provider(Box::new(MockProvider {
            id: "openai",
            caps: vec![MediaCapability::Audio],
            image_result: Err("unsupported"),
            audio_result: Err("unsupported"),
            video_result: Err("unsupported"),
            calls: calls.clone(),
        }));
        pipeline.add_provider(Box::new(MockProvider {
            id: "google",
            caps: vec![MediaCapability::Audio],
            image_result: Err("unsupported"),
            audio_result: Ok("audio text"),
            video_result: Err("unsupported"),
            calls: calls.clone(),
        }));

        let path = write_temp_file(b"audio-bytes");
        let attachments = vec![MediaAttachment {
            path: path.clone(),
            mime_type: "audio/wav".to_string(),
            size_bytes: 11,
            original_name: None,
        }];

        let out = pipeline.process(&attachments).await;
        std::fs::remove_file(path).ok();

        let output = out[0].as_ref().expect("fallback success");
        assert_eq!(output.provider, "google");

        let calls = calls.lock().expect("calls lock").clone();
        assert_eq!(calls, vec!["openai".to_string(), "google".to_string()]);
    }

    #[tokio::test]
    async fn falls_back_to_registered_order_when_default_unknown() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let mut cfg = MediaConfig::default();
        cfg.default_image_provider = "non-existent".to_string();
        cfg.image_fallback_providers = vec![];

        let mut pipeline = MediaPipeline::new(cfg);
        pipeline.add_provider(Box::new(MockProvider {
            id: "anthropic",
            caps: vec![MediaCapability::Image],
            image_result: Ok("anthropic ok"),
            audio_result: Err("unsupported"),
            video_result: Err("unsupported"),
            calls: calls.clone(),
        }));
        pipeline.add_provider(Box::new(MockProvider {
            id: "openai",
            caps: vec![MediaCapability::Image],
            image_result: Ok("openai ok"),
            audio_result: Err("unsupported"),
            video_result: Err("unsupported"),
            calls: calls.clone(),
        }));

        let path = write_temp_file(b"img");
        let attachments = vec![MediaAttachment {
            path: path.clone(),
            mime_type: "image/jpeg".to_string(),
            size_bytes: 3,
            original_name: None,
        }];

        let out = pipeline.process(&attachments).await;
        std::fs::remove_file(path).ok();

        let output = out[0].as_ref().expect("first provider should be used");
        assert_eq!(output.provider, "anthropic");
        let calls = calls.lock().expect("calls lock").clone();
        assert_eq!(calls, vec!["anthropic".to_string()]);
    }

    #[tokio::test]
    async fn process_with_decisions_records_attempts_and_chosen() {
        let mut pipeline = MediaPipeline::new(MediaConfig::default());
        pipeline.add_provider(Box::new(MockProvider {
            id: "openai",
            caps: vec![MediaCapability::Image],
            image_result: Err("upstream down"),
            audio_result: Err("unsupported"),
            video_result: Err("unsupported"),
            calls: Arc::new(Mutex::new(Vec::new())),
        }));
        pipeline.add_provider(Box::new(MockProvider {
            id: "anthropic",
            caps: vec![MediaCapability::Image],
            image_result: Ok("ok"),
            audio_result: Err("unsupported"),
            video_result: Err("unsupported"),
            calls: Arc::new(Mutex::new(Vec::new())),
        }));

        let path = write_temp_file(b"image");
        let attachments = vec![MediaAttachment {
            path: path.clone(),
            mime_type: "image/png".to_string(),
            size_bytes: 5,
            original_name: None,
        }];

        let (results, decisions) = pipeline.process_with_decisions(&attachments).await;
        std::fs::remove_file(path).ok();

        assert_eq!(results.len(), 1);
        let output = results[0].as_ref().expect("image output");
        assert_eq!(output.model.as_deref(), Some("anthropic-model"));
        assert_eq!(decisions.len(), 3);
        let cap = decisions
            .iter()
            .find(|d| d.capability == MediaCapability::Image)
            .expect("image decision");
        assert_eq!(cap.capability, MediaCapability::Image);
        assert_eq!(cap.outcome, MediaDecisionOutcome::Success);
        assert_eq!(cap.attachments.len(), 1);
        let att = &cap.attachments[0];
        assert_eq!(att.attempts.len(), 2);
        assert_eq!(att.attempts[0].provider.as_deref(), Some("openai"));
        assert_eq!(att.attempts[0].outcome, MediaModelDecisionOutcome::Failed);
        assert_eq!(att.attempts[0].model.as_deref(), Some("openai-model"));
        assert_eq!(att.attempts[1].provider.as_deref(), Some("anthropic"));
        assert_eq!(att.attempts[1].outcome, MediaModelDecisionOutcome::Success);
        assert_eq!(att.attempts[1].model.as_deref(), Some("anthropic-model"));
        assert_eq!(
            att.chosen.as_ref().and_then(|c| c.provider.as_deref()),
            Some("anthropic")
        );
        assert_eq!(
            att.chosen.as_ref().and_then(|c| c.model.as_deref()),
            Some("anthropic-model")
        );
        let audio = decisions
            .iter()
            .find(|d| d.capability == MediaCapability::Audio)
            .expect("audio decision");
        assert_eq!(audio.outcome, MediaDecisionOutcome::NoAttachment);
        assert!(audio.attachments.is_empty());
        let video = decisions
            .iter()
            .find(|d| d.capability == MediaCapability::Video)
            .expect("video decision");
        assert_eq!(video.outcome, MediaDecisionOutcome::NoAttachment);
        assert!(video.attachments.is_empty());
    }

    #[tokio::test]
    async fn no_provider_configured_is_recorded_as_skipped_attempt() {
        let mut cfg = MediaConfig::default();
        cfg.default_image_provider = "unknown".to_string();
        cfg.image_fallback_providers = vec![];
        let pipeline = MediaPipeline::new(cfg);

        let path = write_temp_file(b"image");
        let attachments = vec![MediaAttachment {
            path: path.clone(),
            mime_type: "image/png".to_string(),
            size_bytes: 5,
            original_name: None,
        }];

        let (results, decisions) = pipeline.process_with_decisions(&attachments).await;
        std::fs::remove_file(path).ok();

        assert_eq!(results.len(), 1);
        assert!(results[0].is_err());
        let image = decisions
            .iter()
            .find(|d| d.capability == MediaCapability::Image)
            .expect("image decision");
        assert_eq!(image.outcome, MediaDecisionOutcome::Skipped);
        assert_eq!(image.attachments.len(), 1);
        let attempts = &image.attachments[0].attempts;
        assert_eq!(attempts.len(), 1);
        assert_eq!(attempts[0].outcome, MediaModelDecisionOutcome::Skipped);
        assert!(
            attempts[0]
                .reason
                .as_deref()
                .unwrap_or("")
                .contains("no provider configured")
        );
    }

    #[tokio::test]
    async fn disabled_capability_returns_disabled_decision_without_attempts() {
        let mut cfg = MediaConfig::default();
        cfg.image_enabled = false;

        let mut pipeline = MediaPipeline::new(cfg);
        pipeline.add_provider(Box::new(MockProvider {
            id: "openai",
            caps: vec![MediaCapability::Image],
            image_result: Ok("unused"),
            audio_result: Err("unsupported"),
            video_result: Err("unsupported"),
            calls: Arc::new(Mutex::new(Vec::new())),
        }));

        let path = write_temp_file(b"image");
        let attachments = vec![MediaAttachment {
            path: path.clone(),
            mime_type: "image/png".to_string(),
            size_bytes: 5,
            original_name: None,
        }];

        let (results, decisions) = pipeline.process_with_decisions(&attachments).await;
        std::fs::remove_file(path).ok();

        assert!(results.is_empty());
        let image = decisions
            .iter()
            .find(|d| d.capability == MediaCapability::Image)
            .expect("image decision");
        assert_eq!(image.outcome, MediaDecisionOutcome::Disabled);
        assert!(image.attachments.is_empty());
    }
}
