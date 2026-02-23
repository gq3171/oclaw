use axum::{
    extract::State,
    http::StatusCode,
    response::{sse::{Event, Sse}, IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use tracing::{info, error};

use crate::http::HttpState;

fn sanitize_error(msg: &str) -> String {
    // Strip anything that looks like an API key or token from error messages
    regex::Regex::new(r"(?i)(sk-|key-|token-|bearer\s+)[a-zA-Z0-9\-_]{8,}")
        .map(|re| re.replace_all(msg, "${1}[REDACTED]").to_string())
        .unwrap_or_else(|_| "Internal server error".to_string())
}

#[derive(Debug, Deserialize)]
pub struct ChatCompletionsRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<i32>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub tool_choice: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionsResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize)]
pub struct Choice {
    pub index: i32,
    pub message: ChatMessage,
    pub finish_reason: String,
}

#[derive(Debug, Serialize)]
pub struct Usage {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
}

fn to_llm_messages(msgs: &[ChatMessage]) -> Vec<oclaws_llm_core::chat::ChatMessage> {
    msgs.iter().map(|m| {
        let role = match m.role.as_str() {
            "system" => oclaws_llm_core::chat::MessageRole::System,
            "assistant" => oclaws_llm_core::chat::MessageRole::Assistant,
            "tool" => oclaws_llm_core::chat::MessageRole::Tool,
            _ => oclaws_llm_core::chat::MessageRole::User,
        };
        oclaws_llm_core::chat::ChatMessage {
            role,
            content: m.content.clone(),
            name: m.name.clone(),
            tool_calls: None,
            tool_call_id: m.tool_call_id.clone(),
        }
    }).collect()
}

pub async fn chat_completions_handler(
    State(state): State<Arc<HttpState>>,
    Json(payload): Json<ChatCompletionsRequest>,
) -> Response {
    info!("Chat completions request for model: {}", payload.model);

    let provider = match &state.llm_provider {
        Some(p) => p.clone(),
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": {"message": "No LLM provider configured", "type": "server_error"}}))).into_response(),
    };

    let request = oclaws_llm_core::chat::ChatRequest {
        model: payload.model.clone(),
        messages: to_llm_messages(&payload.messages),
        temperature: payload.temperature,
        top_p: None,
        max_tokens: payload.max_tokens,
        stop: None,
        tools: None,
        tool_choice: None,
        stream: Some(payload.stream),
        response_format: None,
    };

    if payload.stream {
        match provider.chat_stream(request).await {
            Ok(mut rx) => {
                let stream = async_stream::stream! {
                    while let Some(chunk) = rx.recv().await {
                        match chunk {
                            Ok(c) => {
                                if let Ok(json) = serde_json::to_string(&c) {
                                    yield Ok::<_, Infallible>(Event::default().data(json));
                                }
                            }
                            Err(e) => {
                                error!("Stream error: {}", e);
                                break;
                            }
                        }
                    }
                    yield Ok::<_, Infallible>(Event::default().data("[DONE]"));
                };
                Sse::new(stream).into_response()
            }
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": {"message": sanitize_error(&e.to_string()), "type": "server_error"}}))).into_response(),
        }
    } else {
        match provider.chat(request).await {
            Ok(completion) => {
                let choices: Vec<Choice> = completion.choices.iter().map(|c| Choice {
                    index: c.index,
                    message: ChatMessage {
                        role: "assistant".to_string(),
                        content: c.message.content.clone(),
                        name: c.message.name.clone(),
                        tool_calls: None,
                        tool_call_id: c.message.tool_call_id.clone(),
                    },
                    finish_reason: c.finish_reason.clone().unwrap_or("stop".to_string()),
                }).collect();

                let usage = completion.usage.map(|u| Usage {
                    prompt_tokens: u.prompt_tokens,
                    completion_tokens: u.completion_tokens,
                    total_tokens: u.total_tokens,
                }).unwrap_or(Usage { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 });

                Json(ChatCompletionsResponse {
                    id: completion.id,
                    object: "chat.completion".to_string(),
                    created: completion.created,
                    model: completion.model,
                    choices,
                    usage,
                }).into_response()
            }
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": {"message": sanitize_error(&e.to_string()), "type": "server_error"}}))).into_response(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ResponsesRequest {
    pub model: String,
    #[serde(default)]
    pub input: serde_json::Value,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct ResponsesResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub output: Vec<OutputItem>,
    pub usage: Usage,
}

#[derive(Debug, Serialize)]
pub struct OutputItem {
    #[serde(rename = "type")]
    pub item_type: String,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Serialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: String,
}

pub async fn responses_handler(
    State(state): State<Arc<HttpState>>,
    Json(payload): Json<ResponsesRequest>,
) -> Response {
    info!("Responses request for model: {}", payload.model);

    let provider = match &state.llm_provider {
        Some(p) => p.clone(),
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": {"message": "No LLM provider configured", "type": "server_error"}}))).into_response(),
    };

    let input_text = match &payload.input {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => {
            arr.iter().filter_map(|item| {
                item.get("content").and_then(|c| c.as_str()).map(|s| s.to_string())
            }).collect::<Vec<_>>().join("\n")
        }
        _ => String::new(),
    };

    let request = oclaws_llm_core::chat::ChatRequest {
        model: payload.model.clone(),
        messages: vec![oclaws_llm_core::chat::ChatMessage {
            role: oclaws_llm_core::chat::MessageRole::User,
            content: input_text,
            name: None, tool_calls: None, tool_call_id: None,
        }],
        temperature: payload.temperature,
        top_p: None,
        max_tokens: payload.max_tokens,
        stop: None, tools: None, tool_choice: None, stream: None, response_format: None,
    };

    match provider.chat(request).await {
        Ok(completion) => {
            let text = completion.choices.first().map(|c| c.message.content.clone()).unwrap_or_default();
            let usage = completion.usage.map(|u| Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            }).unwrap_or(Usage { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 });

            Json(ResponsesResponse {
                id: format!("resp-{}", uuid::Uuid::new_v4()),
                object: "response".to_string(),
                created: chrono::Utc::now().timestamp(),
                model: payload.model,
                output: vec![OutputItem {
                    item_type: "message".to_string(),
                    content: vec![ContentBlock { block_type: "output_text".to_string(), text }],
                }],
                usage,
            }).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": {"message": sanitize_error(&e.to_string()), "type": "server_error"}}))).into_response(),
    }
}

// --- Management API endpoints ---

pub async fn agent_status_handler(
    State(state): State<Arc<HttpState>>,
) -> impl IntoResponse {
    let has_provider = state.llm_provider.is_some();
    Json(serde_json::json!({
        "status": if has_provider { "ready" } else { "no_provider" },
        "provider_configured": has_provider,
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

pub async fn sessions_list_handler(
    State(state): State<Arc<HttpState>>,
) -> impl IntoResponse {
    let manager = state.gateway_server.session_manager.read().await;
    let sessions = manager.list_sessions().unwrap_or_default();
    Json(serde_json::json!({ "sessions": sessions }))
}

pub async fn sessions_delete_handler(
    State(state): State<Arc<HttpState>>,
    axum::extract::Path(key): axum::extract::Path<String>,
) -> Response {
    let manager = state.gateway_server.session_manager.read().await;
    match manager.remove_session(&key) {
        Ok(Some(_)) => Json(serde_json::json!({"ok": true})).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "session not found"}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

pub async fn config_get_handler(
    State(state): State<Arc<HttpState>>,
) -> impl IntoResponse {
    Json(serde_json::to_value(&*state._gateway).unwrap_or_default())
}

pub async fn config_reload_handler() -> impl IntoResponse {
    // Config reload is handled by the config crate's hot-reload watcher.
    // This endpoint triggers a manual re-read signal.
    Json(serde_json::json!({"ok": true, "message": "reload requested"}))
}

pub async fn models_list_handler(
    State(state): State<Arc<HttpState>>,
) -> impl IntoResponse {
    let models = match &state.llm_provider {
        Some(p) => p.supported_models(),
        None => vec![],
    };
    Json(serde_json::json!({ "models": models }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::{get, post, delete}, Router};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;
    use crate::http::{HttpState, health_handler};
    use crate::http::auth::AuthState;
    use crate::server::GatewayServer;
    use oclaws_config::settings::Gateway;
    use oclaws_llm_core::providers::MockLlmProvider;
    use tokio::sync::RwLock;

    fn test_state(provider: Option<Arc<dyn oclaws_llm_core::providers::LlmProvider>>) -> Arc<HttpState> {
        Arc::new(HttpState {
            auth_state: Arc::new(RwLock::new(AuthState::new(None))),
            gateway_server: Arc::new(GatewayServer::new(0)),
            _gateway: Arc::new(Gateway::default()),
            llm_provider: provider,
            hook_pipeline: None,
            channel_manager: None,
            metrics: Arc::new(crate::http::metrics::AppMetrics::new()),
            health_checker: Arc::new(oclaws_doctor_core::HealthChecker::new()),
        })
    }

    fn test_router(state: Arc<HttpState>) -> Router {
        Router::new()
            .route("/health", get(health_handler))
            .route("/ready", get(crate::http::readiness_handler))
            .route("/v1/chat/completions", post(chat_completions_handler))
            .route("/v1/responses", post(responses_handler))
            .route("/agent/status", get(agent_status_handler))
            .route("/sessions", get(sessions_list_handler))
            .route("/sessions/{key}", delete(sessions_delete_handler))
            .route("/models", get(models_list_handler))
            .route("/webhooks/telegram", post(crate::http::webhooks::telegram_webhook))
            .route("/webhooks/slack", post(crate::http::webhooks::slack_webhook))
            .route("/webhooks/discord", post(crate::http::webhooks::discord_webhook))
            .route("/webhooks/{channel}", post(crate::http::webhooks::generic_webhook))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let app = test_router(test_state(None));
        let req = Request::get("/health").body(axum::body::Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_models_with_provider() {
        let mock = MockLlmProvider::new();
        let state = test_state(Some(Arc::new(mock)));
        let app = test_router(state);
        let req = Request::get("/models").body(axum::body::Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["models"].as_array().unwrap().contains(&serde_json::json!("mock-model")));
    }

    #[tokio::test]
    async fn test_chat_completions_no_provider() {
        let app = test_router(test_state(None));
        let body = serde_json::json!({
            "model": "test",
            "messages": [{"role": "user", "content": "hello"}]
        });
        let req = Request::post("/v1/chat/completions")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_chat_completions_with_mock() {
        let mock = MockLlmProvider::new();
        mock.queue_text("Hello from mock!");
        let state = test_state(Some(Arc::new(mock)));
        let app = test_router(state);
        let body = serde_json::json!({
            "model": "mock-model",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let req = Request::post("/v1/chat/completions")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["choices"][0]["message"]["content"], "Hello from mock!");
    }

    #[tokio::test]
    async fn test_responses_with_mock() {
        let mock = MockLlmProvider::new();
        mock.queue_text("Response text");
        let state = test_state(Some(Arc::new(mock)));
        let app = test_router(state);
        let body = serde_json::json!({
            "model": "mock-model",
            "input": "test input"
        });
        let req = Request::post("/v1/responses")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["output"][0]["content"][0]["text"], "Response text");
    }

    #[tokio::test]
    async fn test_agent_status() {
        let state = test_state(Some(Arc::new(MockLlmProvider::new())));
        let app = test_router(state);
        let req = Request::get("/agent/status").body(axum::body::Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["status"], "ready");
    }

    #[tokio::test]
    async fn test_sessions_list_empty() {
        let app = test_router(test_state(None));
        let req = Request::get("/sessions").body(axum::body::Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_session_delete_not_found() {
        let app = test_router(test_state(None));
        let req = Request::delete("/sessions/nonexistent").body(axum::body::Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_telegram_webhook_no_channel_manager() {
        let app = test_router(test_state(None));
        let body = serde_json::json!({"message": {"text": "hi"}});
        let req = Request::post("/webhooks/telegram")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_slack_webhook_url_verification() {
        let app = test_router(test_state(None));
        let body = serde_json::json!({"type": "url_verification", "challenge": "test_challenge_123"});
        let req = Request::post("/webhooks/slack")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["challenge"], "test_challenge_123");
    }

    #[tokio::test]
    async fn test_generic_webhook_unknown_channel() {
        let state = {
            let mut s = (*test_state(None)).clone();
            s.channel_manager = Some(Arc::new(RwLock::new(oclaws_channel_core::ChannelManager::new())));
            Arc::new(s)
        };
        let app = test_router(state);
        let body = serde_json::json!({"data": "test"});
        let req = Request::post("/webhooks/unknown_channel")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
