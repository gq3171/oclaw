use axum::{extract::State, response::IntoResponse};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::http::HttpState;

pub struct AppMetrics {
    pub request_count: AtomicU64,
    pub error_count: AtomicU64,
    pub active_connections: AtomicU64,
    pub latency_sum_us: AtomicU64,
    pub start_time: std::time::Instant,
}

impl Default for AppMetrics {
    fn default() -> Self {
        Self {
            request_count: AtomicU64::new(0),
            error_count: AtomicU64::new(0),
            active_connections: AtomicU64::new(0),
            latency_sum_us: AtomicU64::new(0),
            start_time: std::time::Instant::now(),
        }
    }
}

impl AppMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_request(&self, status: u16, elapsed_us: u64) {
        self.request_count.fetch_add(1, Ordering::Relaxed);
        self.latency_sum_us.fetch_add(elapsed_us, Ordering::Relaxed);
        if status >= 500 {
            self.error_count.fetch_add(1, Ordering::Relaxed);
        }
    }
}

pub async fn metrics_handler(
    State(state): State<Arc<HttpState>>,
) -> impl IntoResponse {
    let metrics = &state.metrics;
    let requests = metrics.request_count.load(Ordering::Relaxed);
    let errors = metrics.error_count.load(Ordering::Relaxed);
    let connections = metrics.active_connections.load(Ordering::Relaxed);
    let latency_sum_us = metrics.latency_sum_us.load(Ordering::Relaxed);
    let avg_latency_ms = if requests > 0 { latency_sum_us as f64 / requests as f64 / 1000.0 } else { 0.0 };
    let uptime = metrics.start_time.elapsed().as_secs();

    let session_count = state.gateway_server.session_manager.read().await
        .list_sessions().map(|s| s.len()).unwrap_or(0);

    let mut channel_lines = String::new();
    if let Some(ref cm) = state.channel_manager {
        let mgr = cm.read().await;
        for (name, status) in mgr.get_status().await {
            let val = match status {
                oclaws_channel_core::traits::ChannelStatus::Connected => 1,
                _ => 0,
            };
            channel_lines.push_str(&format!(
                "oclaws_channel_status{{channel=\"{}\"}} {}\n", name, val
            ));
        }
    }

    let body = format!(
        "# HELP oclaws_requests_total Total HTTP requests\n\
         # TYPE oclaws_requests_total counter\n\
         oclaws_requests_total {requests}\n\
         # HELP oclaws_errors_total Total 5xx errors\n\
         # TYPE oclaws_errors_total counter\n\
         oclaws_errors_total {errors}\n\
         # HELP oclaws_request_latency_avg_ms Average request latency in milliseconds\n\
         # TYPE oclaws_request_latency_avg_ms gauge\n\
         oclaws_request_latency_avg_ms {avg_latency_ms:.3}\n\
         # HELP oclaws_active_connections Current active connections\n\
         # TYPE oclaws_active_connections gauge\n\
         oclaws_active_connections {connections}\n\
         # HELP oclaws_sessions_total Current session count\n\
         # TYPE oclaws_sessions_total gauge\n\
         oclaws_sessions_total {session_count}\n\
         # HELP oclaws_uptime_seconds Server uptime in seconds\n\
         # TYPE oclaws_uptime_seconds gauge\n\
         oclaws_uptime_seconds {uptime}\n\
         {channel_lines}",
    );

    (
        [(axum::http::header::CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    )
}
