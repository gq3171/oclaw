use axum::{
    extract::State,
    http::{Request, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use crate::http::HttpState;

/// Auth middleware: validates Bearer token or password on protected routes.
pub async fn auth_middleware(
    State(state): State<Arc<HttpState>>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let path = req.uri().path();

    // Skip auth for health and root
    if path == "/health" || path == "/" {
        return next.run(req).await;
    }

    let auth_header = req.headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let auth_state = state.auth_state.read().await;

    // If no auth is configured, allow all requests
    if !auth_state.has_auth_config() {
        drop(auth_state);
        return next.run(req).await;
    }

    if let Some(header) = &auth_header {
        if let Some(token) = header.strip_prefix("Bearer ")
            && auth_state.authenticate_token(token).await.is_some()
        {
            drop(auth_state);
            return next.run(req).await;
        }
        if auth_state.validate_password(header).await {
            drop(auth_state);
            return next.run(req).await;
        }
    }

    drop(auth_state);
    (StatusCode::UNAUTHORIZED, "Unauthorized").into_response()
}

/// Request ID middleware: generates a unique ID, tracks latency and error metrics.
pub async fn request_id_middleware(
    State(state): State<Arc<HttpState>>,
    mut req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let start = std::time::Instant::now();
    let request_id = uuid::Uuid::new_v4().to_string();
    let header_value = axum::http::HeaderValue::from_str(&request_id)
        .unwrap_or_else(|_| axum::http::HeaderValue::from_static("unknown"));
    req.headers_mut().insert("x-request-id", header_value.clone());
    let mut resp = next.run(req).await;
    resp.headers_mut().insert("x-request-id", header_value);
    state.metrics.record_request(resp.status().as_u16(), start.elapsed().as_micros() as u64);
    resp
}

/// Security headers middleware: adds standard security headers to all responses.
pub async fn security_headers_middleware(
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert("x-content-type-options", "nosniff".parse().unwrap());
    h.insert("x-frame-options", "DENY".parse().unwrap());
    h.insert("x-xss-protection", "1; mode=block".parse().unwrap());
    h.insert("referrer-policy", "strict-origin-when-cross-origin".parse().unwrap());
    h.insert("strict-transport-security", "max-age=63072000; includeSubDomains".parse().unwrap());
    resp
}

/// Hook middleware: runs before_request / after_response hooks if pipeline is present.
pub async fn hook_middleware(
    State(state): State<Arc<HttpState>>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if let Some(ref pipeline) = state.hook_pipeline {
        let _ = pipeline.before_request(req.uri().path()).await;
    }

    let response = next.run(req).await;

    if let Some(ref pipeline) = state.hook_pipeline {
        let status = response.status().as_u16().to_string();
        let _ = pipeline.after_response(&status).await;
    }

    response
}
