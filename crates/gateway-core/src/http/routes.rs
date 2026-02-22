use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::info;

use crate::http::HttpState;

#[derive(Debug, Deserialize)]
pub struct ChatCompletionsRequest {
    model: String,
    #[serde(default)]
    _messages: Vec<ChatMessage>,
    #[serde(default)]
    _temperature: f64,
    #[serde(default)]
    _max_tokens: Option<i32>,
    #[serde(default)]
    stream: bool,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionsResponse {
    id: String,
    object: String,
    created: i64,
    model: String,
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Debug, Serialize)]
pub struct Choice {
    index: i32,
    message: ChatMessage,
    finish_reason: String,
}

#[derive(Debug, Serialize)]
pub struct Usage {
    prompt_tokens: i32,
    completion_tokens: i32,
    total_tokens: i32,
}

pub async fn chat_completions_handler(
    State(_state): State<Arc<HttpState>>,
    Json(payload): Json<ChatCompletionsRequest>,
) -> Response {
    info!("Chat completions request for model: {}", payload.model);

    if payload.stream {
        return (StatusCode::NOT_IMPLEMENTED, "Streaming not implemented yet").into_response();
    }

    let response = ChatCompletionsResponse {
        id: format!("chatcmpl-{}", uuid::Uuid::new_v4()),
        object: "chat.completion".to_string(),
        created: chrono::Utc::now().timestamp(),
        model: payload.model.clone(),
        choices: vec![Choice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content: "This is a placeholder response. LLM integration coming soon.".to_string(),
            },
            finish_reason: "stop".to_string(),
        }],
        usage: Usage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        },
    };

    Json(response).into_response()
}

#[derive(Debug, Deserialize)]
pub struct ResponsesRequest {
    model: String,
    #[serde(default)]
    _input: Vec<serde_json::Value>,
    #[serde(default)]
    _temperature: f64,
    #[serde(default)]
    _max_tokens: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct ResponsesResponse {
    id: String,
    object: String,
    created: i64,
    model: String,
    output: Vec<OutputItem>,
    usage: Usage,
}

#[derive(Debug, Serialize)]
pub struct OutputItem {
    #[serde(rename = "type")]
    item_type: String,
    content: Vec<ContentBlock>,
}

#[derive(Debug, Serialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: String,
}

pub async fn responses_handler(
    State(_state): State<Arc<HttpState>>,
    Json(payload): Json<ResponsesRequest>,
) -> Response {
    info!("Responses request for model: {}", payload.model);

    let response = ResponsesResponse {
        id: format!("resp-{}", uuid::Uuid::new_v4()),
        object: "response".to_string(),
        created: chrono::Utc::now().timestamp(),
        model: payload.model.clone(),
        output: vec![OutputItem {
            item_type: "message".to_string(),
            content: vec![ContentBlock {
                block_type: "output_text".to_string(),
                text: "This is a placeholder response. LLM integration coming soon.".to_string(),
            }],
        }],
        usage: Usage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        },
    };

    Json(response).into_response()
}
