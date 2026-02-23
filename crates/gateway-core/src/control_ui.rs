use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Json,
    routing::get,
    Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use chrono::Utc;

#[derive(Debug, Serialize, Deserialize)]
pub struct HealthStatus {
    pub status: String,
    pub uptime_seconds: i64,
    pub timestamp: String,
    pub version: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ChannelStatusResponse {
    pub channel: String,
    pub status: String,
    pub connected_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SessionStatusResponse {
    pub session_id: String,
    pub agent_id: String,
    pub status: String,
    pub created_at: String,
    pub message_count: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GatewayStats {
    pub total_sessions: usize,
    pub active_sessions: usize,
    pub total_channels: usize,
    pub uptime_seconds: i64,
    pub memory_usage_bytes: Option<u64>,
}

pub struct ControlUiState {
    pub start_time: i64,
}

impl Default for ControlUiState {
    fn default() -> Self {
        Self {
            start_time: Utc::now().timestamp(),
        }
    }
}

impl ControlUiState {
    pub fn new() -> Self {
        Self::default()
    }
}

pub fn create_control_ui_router(state: Arc<ControlUiState>) -> Router {
    Router::new()
        .route("/health", get(health_handler))
        .route("/stats", get(stats_handler))
        .route("/channels", get(list_channels_handler))
        .route("/sessions", get(list_sessions_handler))
        .route("/sessions/:id", get(session_detail_handler))
        .with_state(state)
}

async fn health_handler(State(state): State<Arc<ControlUiState>>) -> Json<HealthStatus> {
    let uptime = Utc::now().timestamp() - state.start_time;
    
    Json(HealthStatus {
        status: "healthy".to_string(),
        uptime_seconds: uptime,
        timestamp: Utc::now().to_rfc3339(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

async fn stats_handler() -> Json<GatewayStats> {
    Json(GatewayStats {
        total_sessions: 0,
        active_sessions: 0,
        total_channels: 0,
        uptime_seconds: 0,
        memory_usage_bytes: None,
    })
}

async fn list_channels_handler() -> Json<Vec<ChannelStatusResponse>> {
    Json(vec![])
}

async fn list_sessions_handler() -> Json<Vec<SessionStatusResponse>> {
    Json(vec![])
}

async fn session_detail_handler(Path(session_id): Path<String>) -> Result<Json<SessionStatusResponse>, StatusCode> {
    Ok(Json(SessionStatusResponse {
        session_id,
        agent_id: "default".to_string(),
        status: "running".to_string(),
        created_at: Utc::now().to_rfc3339(),
        message_count: 0,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_handler() {
        let state = Arc::new(ControlUiState::new());
        let result = health_handler(State(state)).await;
        
        assert_eq!(result.status, "healthy");
    }

    #[tokio::test]
    async fn test_stats_handler() {
        let result = stats_handler().await;
        
        assert_eq!(result.total_sessions, 0);
    }

    #[tokio::test]
    async fn test_session_detail() {
        let result = session_detail_handler(Path("test-123".to_string())).await.unwrap();
        
        assert_eq!(result.session_id, "test-123");
    }
}
