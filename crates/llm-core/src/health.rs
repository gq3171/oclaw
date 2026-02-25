//! Provider health check utilities.

use crate::providers::ProviderType;
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ProviderHealth {
    pub provider: ProviderType,
    pub is_healthy: bool,
    pub latency_ms: Option<u64>,
    pub last_check: u64,
    pub error: Option<String>,
}

impl ProviderHealth {
    pub fn healthy(provider: ProviderType, latency_ms: u64) -> Self {
        Self {
            provider,
            is_healthy: true,
            latency_ms: Some(latency_ms),
            last_check: chrono::Utc::now().timestamp_millis() as u64,
            error: None,
        }
    }

    pub fn unhealthy(provider: ProviderType, error: String) -> Self {
        Self {
            provider,
            is_healthy: false,
            latency_ms: None,
            last_check: chrono::Utc::now().timestamp_millis() as u64,
            error: Some(error),
        }
    }
}
