use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use std::sync::Arc;
use tracing::{info, error};

use crate::http::HttpState;
use oclaws_channel_core::traits::ChannelEvent;

async fn get_channel_and_forward(
    state: &HttpState,
    channel_name: &str,
    event: ChannelEvent,
) -> Response {
    let manager = match &state.channel_manager {
        Some(m) => m,
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": "No channel manager configured"}))).into_response(),
    };
    let mgr = manager.read().await;
    let channel = match mgr.get(channel_name).await {
        Some(c) => c,
        None => return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": format!("Channel '{}' not found", channel_name)}))).into_response(),
    };
    let ch = channel.read().await;
    match ch.handle_event(event).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => {
            error!("Webhook event handling failed: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e.to_string()}))).into_response()
        }
    }
}

pub async fn telegram_webhook(
    State(state): State<Arc<HttpState>>,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    let event_type = if payload.get("callback_query").is_some() {
        "callback_query"
    } else {
        "message"
    };
    info!("Telegram webhook: {}", event_type);
    let event = ChannelEvent {
        event_type: event_type.to_string(),
        channel: "telegram".to_string(),
        payload,
    };
    get_channel_and_forward(&state, "telegram", event).await
}

pub async fn slack_webhook(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(mut payload): Json<serde_json::Value>,
) -> Response {
    // Handle Slack URL verification challenge
    if payload.get("type").and_then(|t| t.as_str()) == Some("url_verification") {
        let challenge = payload["challenge"].as_str().unwrap_or("");
        return Json(serde_json::json!({"challenge": challenge})).into_response();
    }

    // Inject Slack signature headers into payload for downstream HMAC verification
    if let Some(ts) = headers.get("x-slack-request-timestamp").and_then(|v| v.to_str().ok()) {
        payload["_slack_timestamp"] = serde_json::json!(ts);
    }
    if let Some(sig) = headers.get("x-slack-signature").and_then(|v| v.to_str().ok()) {
        payload["_slack_signature"] = serde_json::json!(sig);
    }

    let event_type = payload.get("event").and_then(|e| e.get("type")).and_then(|t| t.as_str()).unwrap_or("event");
    info!("Slack webhook: {}", event_type);
    let event = ChannelEvent {
        event_type: event_type.to_string(),
        channel: "slack".to_string(),
        payload,
    };
    get_channel_and_forward(&state, "slack", event).await
}

pub async fn discord_webhook(
    State(state): State<Arc<HttpState>>,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    // Handle Discord PING interaction (type 1)
    if payload.get("type").and_then(|t| t.as_u64()) == Some(1) {
        return Json(serde_json::json!({"type": 1})).into_response();
    }

    let event_type = payload.get("type").and_then(|t| t.as_u64()).map(|t| format!("interaction_{}", t)).unwrap_or_else(|| "interaction".to_string());
    info!("Discord webhook: {}", event_type);
    let event = ChannelEvent {
        event_type,
        channel: "discord".to_string(),
        payload,
    };
    get_channel_and_forward(&state, "discord", event).await
}

pub async fn generic_webhook(
    State(state): State<Arc<HttpState>>,
    Path(channel_name): Path<String>,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    info!("Generic webhook for channel: {}", channel_name);
    let event = ChannelEvent {
        event_type: "webhook".to_string(),
        channel: channel_name.clone(),
        payload,
    };
    get_channel_and_forward(&state, &channel_name, event).await
}
