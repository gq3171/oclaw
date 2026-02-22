use thiserror::Error;

#[derive(Error, Debug)]
pub enum GatewayError {
    #[error("Server error: {0}")]
    ServerError(String),

    #[error("WebSocket error: {0}")]
    WebSocketError(String),

    #[error("Authentication error: {0}")]
    AuthenticationError(String),

    #[error("Invalid frame: {0}")]
    InvalidFrame(String),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("Connection error: {0}")]
    ConnectionError(String),

    #[error("Config error: {0}")]
    ConfigError(String),

    #[error("Session error: {0}")]
    SessionError(String),
}

pub type GatewayResult<T> = Result<T, GatewayError>;
