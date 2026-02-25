use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use std::sync::Arc;
use tracing::{info, warn, error};

use crate::http::HttpState;
use oclaws_channel_core::traits::ChannelEvent;
use oclaws_channel_core::group_gate;

// ── Webhook signature verification ──────────────────────────────────────

/// Verify Slack webhook signature (HMAC-SHA256).
fn verify_slack_signature(signing_secret: &str, timestamp: &str, body: &str, expected_sig: &str) -> bool {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let base = format!("v0:{}:{}", timestamp, body);
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(signing_secret.as_bytes()) else {
        return false;
    };
    mac.update(base.as_bytes());
    let computed = format!("v0={}", hex::encode(mac.finalize().into_bytes()));
    constant_time_eq(computed.as_bytes(), expected_sig.as_bytes())
}

/// Verify Feishu/Lark webhook signature.
fn verify_feishu_signature(encrypt_key: &str, timestamp: &str, body: &str, expected_sig: &str) -> bool {
    use sha2::{Sha256, Digest};
    let to_sign = format!("{}\n{}\n{}", timestamp, encrypt_key, body);
    let hash = hex::encode(Sha256::digest(to_sign.as_bytes()));
    constant_time_eq(hash.as_bytes(), expected_sig.as_bytes())
}

/// Verify Discord webhook signature (Ed25519).
fn verify_discord_signature(public_key_hex: &str, signature_hex: &str, timestamp: &str, body: &str) -> bool {
    use ed25519_dalek::{Signature, VerifyingKey, Verifier};

    let Ok(key_bytes) = hex::decode(public_key_hex) else { return false };
    let Ok(sig_bytes) = hex::decode(signature_hex) else { return false };

    let key_arr: [u8; 32] = match key_bytes.try_into() {
        Ok(a) => a,
        Err(_) => return false,
    };
    let Ok(verifying_key) = VerifyingKey::from_bytes(&key_arr) else { return false };
    let Ok(signature) = Signature::from_slice(&sig_bytes) else { return false };

    let message = format!("{}{}", timestamp, body);
    verifying_key.verify(message.as_bytes(), &signature).is_ok()
}

/// Verify Telegram secret_token header.
fn verify_telegram_secret(expected: &str, actual: &str) -> bool {
    constant_time_eq(expected.as_bytes(), actual.as_bytes())
}

/// Constant-time byte comparison to prevent timing attacks.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ── Webhook handlers ────────────────────────────────────────────────────

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
        None => {
            tracing::warn!("Webhook for unregistered channel '{}'", channel_name);
            return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": format!("Channel '{}' not found", channel_name)}))).into_response();
        }
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
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    // Verify Telegram secret_token if configured
    if let Ok(secret) = std::env::var("TELEGRAM_WEBHOOK_SECRET") {
        let header_token = headers
            .get("x-telegram-bot-api-secret-token")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !verify_telegram_secret(&secret, header_token) {
            warn!("Telegram webhook: invalid secret token");
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Invalid secret token"}))).into_response();
        }
    }

    let event_type = if payload.get("callback_query").is_some() {
        "callback_query"
    } else {
        "message"
    };
    info!("Telegram webhook: {}", event_type);

    // Extract chat_id, text, and chat type from Telegram update
    let (chat_id, text, is_group) = if event_type == "message" {
        let cid = payload.pointer("/message/chat/id").and_then(|v| v.as_i64());
        let txt = payload.pointer("/message/text").and_then(|v| v.as_str());
        let chat_type = payload.pointer("/message/chat/type").and_then(|v| v.as_str()).unwrap_or("private");
        let group = chat_type == "group" || chat_type == "supergroup";
        (cid, txt.map(|s| s.to_string()), group)
    } else {
        (None, None, false)
    };

    if let (Some(chat_id), Some(text)) = (chat_id, text) {
        let is_echo = state.echo_tracker.lock().await.has(&text);
        // Telegram: check for @bot mention via entities (type=mention or bot_command)
        // This is more accurate than naive text.contains('@') which matches any @ symbol
        let has_mention = payload.pointer("/message/entities")
            .and_then(|v| v.as_array())
            .is_some_and(|arr| arr.iter().any(|e| {
                let t = e["type"].as_str().unwrap_or("");
                t == "mention" || t == "bot_command"
            }));
        let should = group_gate::should_process(is_group, state.group_activation, has_mention);

        if !is_echo && should {
            let state_clone = state.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_telegram_reply(&state_clone, chat_id, &text).await {
                    error!("Failed to handle Telegram reply: {}", e);
                }
            });
        }
    }

    // Still forward event for logging
    let event = ChannelEvent {
        event_type: event_type.to_string(),
        channel: "telegram".to_string(),
        payload,
    };
    get_channel_and_forward(&state, "telegram", event).await
}

async fn handle_telegram_reply(
    state: &HttpState,
    chat_id: i64,
    text: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let provider = state.llm_provider.as_ref()
        .ok_or("No LLM provider configured")?;

    let session_id = format!("telegram_{}", chat_id);
    let reply = if let Some(ref registry) = state.tool_registry {
        let executor = crate::http::agent_bridge::ToolRegistryExecutor::new(registry.clone());
        crate::http::agent_bridge::agent_reply_with_session(provider, &executor, text, Some(&session_id)).await
            .unwrap_or_else(|e| format!("Agent error: {}", e))
    } else {
        // Fallback: direct LLM call without tools
        let request = oclaws_llm_core::chat::ChatRequest {
            model: provider.default_model().to_string(),
            messages: vec![oclaws_llm_core::chat::ChatMessage {
                role: oclaws_llm_core::chat::MessageRole::User,
                content: text.to_string(),
                name: None, tool_calls: None, tool_call_id: None,
            }],
            temperature: None, top_p: None, max_tokens: None,
            stop: None, tools: None, tool_choice: None,
            stream: None, response_format: None,
        };
        provider.chat(request).await
            .map(|c| c.choices.first().map(|ch| ch.message.content.clone()).unwrap_or_default())
            .unwrap_or_else(|e| format!("LLM error: {}", e))
    };

    // Echo tracking — remember our reply so we skip it on re-receive
    state.echo_tracker.lock().await.remember(&reply);

    // Send reply via channel
    let manager = state.channel_manager.as_ref()
        .ok_or("No channel manager")?;
    let mgr = manager.read().await;
    let channel = mgr.get("telegram").await
        .ok_or("Telegram channel not found")?;

    let mut metadata = std::collections::HashMap::new();
    metadata.insert("chat_id".to_string(), chat_id.to_string());

    let msg = oclaws_channel_core::traits::ChannelMessage {
        id: uuid::Uuid::new_v4().to_string(),
        channel: "telegram".to_string(),
        sender: "bot".to_string(),
        content: reply.to_string(),
        timestamp: chrono::Utc::now().timestamp_millis(),
        metadata,
    };

    let ch = channel.read().await;
    ch.send_message(&msg).await
        .map_err(|e| format!("Send error: {}", e))?;

    info!("Replied to Telegram chat {}", chat_id);
    Ok(())
}

pub async fn slack_webhook(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    raw_body: axum::body::Bytes,
) -> Response {
    let body_str = String::from_utf8_lossy(&raw_body).to_string();

    // Verify Slack signature if signing secret is configured
    if let Ok(signing_secret) = std::env::var("SLACK_SIGNING_SECRET") {
        let timestamp = headers
            .get("x-slack-request-timestamp")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let signature = headers
            .get("x-slack-signature")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        // Reject requests older than 5 minutes to prevent replay attacks
        if let Ok(ts) = timestamp.parse::<i64>() {
            let now = chrono::Utc::now().timestamp();
            if (now - ts).unsigned_abs() > 300 {
                warn!("Slack webhook: timestamp too old (replay attack?)");
                return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Request too old"}))).into_response();
            }
        } else {
            warn!("Slack webhook: missing or invalid timestamp");
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Invalid timestamp"}))).into_response();
        }

        if !verify_slack_signature(&signing_secret, timestamp, &body_str, signature) {
            warn!("Slack webhook: invalid signature");
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Invalid signature"}))).into_response();
        }
    }

    let mut payload: serde_json::Value = match serde_json::from_str(&body_str) {
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response();
        }
    };

    // Handle Slack URL verification challenge
    if payload.get("type").and_then(|t| t.as_str()) == Some("url_verification") {
        let challenge = payload["challenge"].as_str().unwrap_or("");
        return Json(serde_json::json!({"challenge": challenge})).into_response();
    }

    // Inject Slack signature headers + raw body into payload for downstream verification
    if let Some(ts) = headers.get("x-slack-request-timestamp").and_then(|v| v.to_str().ok()) {
        payload["_slack_timestamp"] = serde_json::json!(ts);
    }
    if let Some(sig) = headers.get("x-slack-signature").and_then(|v| v.to_str().ok()) {
        payload["_slack_signature"] = serde_json::json!(sig);
    }
    payload["_slack_raw_body"] = serde_json::json!(body_str);

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
    headers: HeaderMap,
    raw_body: axum::body::Bytes,
) -> Response {
    let body_str = String::from_utf8_lossy(&raw_body).to_string();

    // Verify Discord signature (Ed25519) if public key is configured
    if let Ok(public_key) = std::env::var("DISCORD_PUBLIC_KEY") {
        let signature = headers
            .get("x-signature-ed25519")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let timestamp = headers
            .get("x-signature-timestamp")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");

        if signature.is_empty() || timestamp.is_empty() {
            warn!("Discord webhook: missing signature or timestamp");
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Missing signature"}))).into_response();
        }

        if !verify_discord_signature(&public_key, signature, timestamp, &body_str) {
            warn!("Discord webhook: invalid signature");
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Invalid signature"}))).into_response();
        }
    }

    let payload: serde_json::Value = match serde_json::from_str(&body_str) {
        Ok(v) => v,
        Err(e) => {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response();
        }
    };

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

pub async fn feishu_webhook(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(payload): Json<serde_json::Value>,
) -> Response {
    // Verify Feishu signature if encrypt key is configured
    if let Ok(encrypt_key) = std::env::var("FEISHU_ENCRYPT_KEY") {
        let timestamp = headers
            .get("x-lark-request-timestamp")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let signature = headers
            .get("x-lark-signature")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        let body_str = serde_json::to_string(&payload).unwrap_or_default();

        if signature.is_empty() || timestamp.is_empty() {
            warn!("Feishu webhook: missing signature or timestamp");
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Missing signature"}))).into_response();
        }
        if !verify_feishu_signature(&encrypt_key, timestamp, &body_str, signature) {
            warn!("Feishu webhook: invalid signature");
            return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"error": "Invalid signature"}))).into_response();
        }
    }

    // Handle Feishu URL verification challenge
    if payload.get("type").and_then(|t| t.as_str()) == Some("url_verification") {
        let challenge = payload["challenge"].as_str().unwrap_or("");
        return Json(serde_json::json!({"challenge": challenge})).into_response();
    }

    // Extract event type
    let event_type = payload.pointer("/header/event_type")
        .and_then(|t| t.as_str())
        .unwrap_or("event");
    info!("Feishu webhook: {}", event_type);

    // Handle im.message.receive_v1
    if event_type == "im.message.receive_v1" {
        let chat_id = payload.pointer("/event/message/chat_id").and_then(|v| v.as_str()).map(|s| s.to_string());
        let message_id = payload.pointer("/event/message/message_id").and_then(|v| v.as_str()).map(|s| s.to_string());
        let msg_type = payload.pointer("/event/message/message_type").and_then(|v| v.as_str()).unwrap_or("text");
        let chat_type = payload.pointer("/event/message/chat_type").and_then(|v| v.as_str()).unwrap_or("p2p");
        let is_group = chat_type == "group";
        let has_mention = payload.pointer("/event/message/mentions")
            .and_then(|v| v.as_array())
            .is_some_and(|arr| !arr.is_empty());

        let text = if msg_type == "text" {
            payload.pointer("/event/message/content")
                .and_then(|v| v.as_str())
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                .and_then(|v| v["text"].as_str().map(|s| s.to_string()))
        } else {
            Some(format!("[{}消息]", msg_type))
        };

        if let (Some(chat_id), Some(text)) = (chat_id, text) {
            // Echo detection — skip own messages
            let is_echo = state.echo_tracker.lock().await.has(&text);
            // Group gating
            let should = group_gate::should_process(is_group, state.group_activation, has_mention);

            if !is_echo && should {
                let state_clone = state.clone();
                let message_id = message_id.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_feishu_reply(&state_clone, &chat_id, message_id.as_deref(), &text).await {
                        error!("Failed to handle Feishu reply: {}", e);
                    }
                });
            } else if !should {
                info!("Feishu: skipping group message (not mentioned)");
            }
        }
    }

    let event = ChannelEvent {
        event_type: event_type.to_string(),
        channel: "feishu".to_string(),
        payload,
    };
    get_channel_and_forward(&state, "feishu", event).await
}

async fn handle_feishu_reply(
    state: &HttpState,
    chat_id: &str,
    message_id: Option<&str>,
    text: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let provider = state.llm_provider.as_ref()
        .ok_or("No LLM provider configured")?;

    let session_id = format!("feishu_{}", chat_id);
    let reply = if let Some(ref registry) = state.tool_registry {
        let executor = crate::http::agent_bridge::ToolRegistryExecutor::new(registry.clone());
        crate::http::agent_bridge::agent_reply_with_session(provider, &executor, text, Some(&session_id)).await
            .unwrap_or_else(|e| format!("Agent error: {}", e))
    } else {
        let request = oclaws_llm_core::chat::ChatRequest {
            model: provider.default_model().to_string(),
            messages: vec![oclaws_llm_core::chat::ChatMessage {
                role: oclaws_llm_core::chat::MessageRole::User,
                content: text.to_string(),
                name: None, tool_calls: None, tool_call_id: None,
            }],
            temperature: None, top_p: None, max_tokens: None,
            stop: None, tools: None, tool_choice: None,
            stream: None, response_format: None,
        };
        provider.chat(request).await
            .map(|c| c.choices.first().map(|ch| ch.message.content.clone()).unwrap_or_default())
            .unwrap_or_else(|e| format!("LLM error: {}", e))
    };

    state.echo_tracker.lock().await.remember(&reply);

    let manager = state.channel_manager.as_ref().ok_or("No channel manager")?;
    let mgr = manager.read().await;
    let channel = mgr.get("feishu").await.ok_or("Feishu channel not found")?;

    let mut metadata = std::collections::HashMap::new();
    metadata.insert("chat_id".to_string(), chat_id.to_string());
    if let Some(mid) = message_id {
        metadata.insert("message_id".to_string(), mid.to_string());
    }

    let msg = oclaws_channel_core::traits::ChannelMessage {
        id: uuid::Uuid::new_v4().to_string(),
        channel: "feishu".to_string(),
        sender: "bot".to_string(),
        content: reply,
        timestamp: chrono::Utc::now().timestamp_millis(),
        metadata,
    };

    let ch = channel.read().await;
    ch.send_message(&msg).await.map_err(|e| format!("Send error: {}", e))?;
    info!("Replied to Feishu chat {}", chat_id);
    Ok(())
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
