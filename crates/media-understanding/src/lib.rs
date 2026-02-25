//! Media understanding — image/audio/video processing pipeline.

pub mod cache;
pub mod pipeline;
pub mod providers;
pub mod types;

pub use pipeline::MediaPipeline;
pub use types::{MediaAttachment, MediaCapability, MediaConfig, MediaOutputKind, MediaUnderstandingOutput};
