use thiserror::Error;

#[derive(Error, Debug)]
pub enum BrowserError {
    #[error("Connection error: {0}")]
    ConnectionError(String),

    #[error("Protocol error: {0}")]
    ProtocolError(String),

    #[error("Target not found: {0}")]
    TargetNotFound(String),

    #[error("Page error: {0}")]
    PageError(String),

    #[error("Execution error: {0}")]
    ExecutionError(String),

    #[error("Navigation error: {0}")]
    NavigationError(String),

    #[error("Timeout: {0}")]
    Timeout(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

pub type BrowserResult<T> = Result<T, BrowserError>;
