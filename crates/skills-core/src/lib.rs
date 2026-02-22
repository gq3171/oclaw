pub type SkillResult<T> = Result<T, SkillError>;

#[derive(Debug, thiserror::Error)]
pub enum SkillError {
    #[error("Execution error: {0}")]
    ExecutionError(String),
    
    #[error("Validation error: {0}")]
    ValidationError(String),
    
    #[error("Not found: {0}")]
    NotFound(String),
    
    #[error("Timeout: {0}")]
    Timeout(String),
    
    #[error("Permission denied: {0}")]
    PermissionDenied(String),
}

impl serde::Serialize for SkillError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub mod skill;
pub mod registry;
pub mod builtin;

pub use skill::{Skill, SkillInput, SkillOutput, SkillContext};
pub use registry::SkillRegistry;
pub use builtin::*;
