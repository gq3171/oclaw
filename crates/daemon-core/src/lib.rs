//! Daemon Core - Background service management for OpenClaw
//! 
//! Provides daemon process management, service monitoring, and background task scheduling.

pub mod service;
pub mod process;
pub mod signal;
pub mod monitor;

pub use service::{DaemonService, ServiceConfig, ServiceState, ServiceManager};
pub use process::{ProcessManager, ProcessInfo, ProcessStatus};
pub use signal::{SignalHandler, Signal, SignalEvent};
pub use monitor::{SystemMonitor, SystemStats, DiskInfo};

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
