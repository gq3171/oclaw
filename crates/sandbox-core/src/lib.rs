pub mod container;
pub mod image;
pub mod volume;
pub mod network;

pub use container::{Container, ContainerConfig, ContainerStatus, ContainerManager};
pub use image::{Image, ImageManager};
pub use volume::{Volume, VolumeManager};
pub use network::{Network, NetworkManager};

pub type SandboxResult<T> = Result<T, SandboxError>;

#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("Container error: {0}")]
    ContainerError(String),
    
    #[error("Image error: {0}")]
    ImageError(String),
    
    #[error("Network error: {0}")]
    NetworkError(String),
    
    #[error("Volume error: {0}")]
    VolumeError(String),
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
    
    #[error("Not found: {0}")]
    NotFound(String),
    
    #[error("Timeout: {0}")]
    Timeout(String),
    
    #[error("Resource limit: {0}")]
    ResourceLimit(String),
}

impl serde::Serialize for SandboxError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
