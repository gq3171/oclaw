//! Media understanding — image/audio/video processing pipeline.

pub mod cache;
pub mod pipeline;
pub mod providers;
pub mod types;

pub use pipeline::MediaPipeline;
pub use types::{
    MediaAttachment, MediaAttachmentDecision, MediaCapability, MediaCapabilityDecision,
    MediaConfig, MediaDecisionOutcome, MediaModelDecision, MediaModelDecisionOutcome,
    MediaModelDecisionType, MediaOutputKind, MediaUnderstandingOutput,
};
