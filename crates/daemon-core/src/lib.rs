//! Daemon Core - Background service management for OpenClaw
//!
//! Provides daemon process management, service monitoring, and background task scheduling.

pub mod monitor;
pub mod process;
pub mod service;
pub mod signal;

pub use monitor::{DiskInfo, SystemMonitor, SystemStats};
pub use process::{ProcessInfo, ProcessManager, ProcessStatus};
pub use service::{DaemonService, ServiceConfig, ServiceManager, ServiceState};
pub use signal::{Signal, SignalEvent, SignalHandler};

pub type DaemonResult<T> = Result<T, DaemonError>;

#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("Service error: {0}")]
    ServiceError(String),

    #[error("Process error: {0}")]
    ProcessError(String),

    #[error("Signal error: {0}")]
    SignalError(String),

    #[error("Monitor error: {0}")]
    MonitorError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Permission error: {0}")]
    PermissionError(String),

    #[error("Not found: {0}")]
    NotFound(String),
}

impl serde::Serialize for DaemonError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
