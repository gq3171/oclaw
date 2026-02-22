use thiserror::Error;

#[derive(Error, Debug)]
pub enum LlmError {
    #[error("API error: {0}")]
    ApiError(String),

    #[error("Authentication error: {0}")]
    AuthError(String),

    #[error("Rate limit error: {0}")]
    RateLimitError(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Parse error: {0}")]
    ParseError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("Unsupported model: {0}")]
    UnsupportedModel(String),
}

pub type LlmResult<T> = Result<T, LlmError>;
