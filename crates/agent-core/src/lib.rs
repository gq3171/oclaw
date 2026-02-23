pub mod agent;
pub mod subagent;
pub mod model_fallback;
pub mod auth;
pub mod loop_detect;

pub use agent::{Agent, AgentConfig, AgentState, ToolExecutor};
pub use subagent::{Subagent, SubagentRegistry, SubagentStatus};
pub use model_fallback::{ModelFallback, ModelChain, FallbackConfig};
pub use auth::{AuthManager, AuthProvider, ProviderCredentials};
pub use loop_detect::LoopDetector;

pub type AgentResult<T> = Result<T, AgentError>;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("Initialization error: {0}")]
    InitError(String),
    
    #[error("Execution error: {0}")]
    ExecutionError(String),
    
    #[error("Model error: {0}")]
    ModelError(String),
    
    #[error("Authentication error: {0}")]
    AuthError(String),
    
    #[error("Subagent error: {0}")]
    SubagentError(String),
    
    #[error("Not found: {0}")]
    NotFound(String),
    
    #[error("Timeout: {0}")]
    Timeout(String),
    
    #[error("Rate limited: {0}")]
    RateLimited(String),
}

impl serde::Serialize for AgentError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
