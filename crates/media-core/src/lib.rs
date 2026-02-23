//! Media Core - Media processing pipeline for OpenClaw
//! 
//! Provides image and audio processing, MIME detection, and media transformations.

pub mod mime_detector;
pub mod media_store;

#[cfg(feature = "image")]
pub mod image_processor;

#[cfg(feature = "audio")]
pub mod audio_processor;

#[cfg(feature = "image")]
pub use image_processor::{ImageProcessor, MediaImageFormat, ImageConfig, ResizeOptions};

#[cfg(feature = "audio")]
pub use audio_processor::{AudioProcessor, AudioFormat, AudioConfig, AudioCodec};

pub use mime_detector::{MimeDetector, MimeType};
pub use media_store::{MediaStore, MediaMetadata, MediaRef};

pub type MediaResult<T> = Result<T, MediaError>;

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("Image error: {0}")]
    ImageError(String),
    
    #[error("Audio error: {0}")]
    AudioError(String),
    
    #[error("Detection error: {0}")]
    DetectionError(String),
    
    #[error("Storage error: {0}")]
    StorageError(String),
    
    #[error("Conversion error: {0}")]
    ConversionError(String),
    
    #[error("Unsupported format: {0}")]
    UnsupportedFormat(String),
    
    #[error("IO error: {0}")]
    IoError(String),
}

impl serde::Serialize for MediaError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
