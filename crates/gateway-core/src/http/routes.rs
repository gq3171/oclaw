use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{
        IntoResponse, Response,
        sse::{Event, Sse},
    },
};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use tracing::{error, info, warn};

use crate::http::HttpState;
use oclaw_memory_core::MemoryManager;

/// Fire-and-forget: store a user↔assistant exchange into long-term memory.
fn spawn_memory_capture(mm: Arc<MemoryManager>, user_text: String, assistant_text: String) {
    if user_text.is_empty() || assistant_text.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let content = format!("User: {}\nAssistant: {}", user_text, assistant_text);
        match mm.add_memory(&content, "chat_api").await {
            Ok(id) => info!("Memory captured: {}", id),
            Err(e) => warn!("Memory capture failed: {}", e),
        }
    });
}

fn header_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
}

fn normalize_agent_id(raw: &str) -> String {
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return "main".to_string();
    }

    let mut out = String::with_capacity(trimmed.len());
    let mut prev_dash = false;
    for ch in trimmed.chars() {
        let valid = ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-';
        if valid {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "main".to_string()
    } else {
        out.chars().take(64).collect()
    }
}

fn resolve_agent_id_from_model(model: &str) -> Option<String> {
    let raw = model.trim();
    if raw.is_empty() {
        return None;
    }

    if let Some(rest) = raw.strip_prefix("openclaw:") {
        return Some(normalize_agent_id(rest));
    }
    if let Some(rest) = raw.strip_prefix("openclaw/") {
        return Some(normalize_agent_id(rest));
    }
    if let Some(rest) = raw.strip_prefix("agent:") {
        return Some(normalize_agent_id(rest));
    }
    None
}

fn resolve_agent_id_for_request(headers: &HeaderMap, model: &str) -> String {
    if let Some(v) = header_value(headers, "x-openclaw-agent-id")
        .or_else(|| header_value(headers, "x-openclaw-agent"))
    {
        return normalize_agent_id(&v);
    }
    resolve_agent_id_from_model(model).unwrap_or_else(|| "main".to_string())
}

fn sanitize_transcript_session_id(raw: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    raw.hash(&mut hasher);
    let digest = format!("{:016x}", hasher.finish());

    let mut safe = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' || ch == '.' {
            safe.push(ch.to_ascii_lowercase());
        } else {
            safe.push('_');
        }
    }

    while safe.contains("__") {
        safe = safe.replace("__", "_");
    }

    let safe = safe.trim_matches('_');
    if safe.is_empty() {
        format!("session_{}", digest)
    } else {
        let head: String = safe.chars().take(96).collect();
        format!("{}_{}", head, digest)
    }
}

fn resolve_chat_completions_session_id(
    headers: &HeaderMap,
    model: &str,
    user: Option<&str>,
) -> Option<String> {
    if let Some(explicit) = header_value(headers, "x-openclaw-session-key") {
        return Some(sanitize_transcript_session_id(&explicit));
    }

    let user = user
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_lowercase)?;

    let agent_id = resolve_agent_id_for_request(headers, model);
    let key = format!("agent-{}-openai-user-{}", agent_id, user);
    Some(sanitize_transcript_session_id(&key))
}

fn payload_has_explicit_history(messages: &[ChatMessage]) -> bool {
    messages.iter().any(|m| {
        matches!(
            m.role.as_str(),
            "assistant" | "tool" | "function" | "developer"
        )
    })
}

async fn persist_transcript_turn(session_id: Option<&str>, user_text: &str, assistant_text: &str) {
    let Some(sid) = session_id.map(str::trim).filter(|s| !s.is_empty()) else {
        return;
    };
    let user_text = user_text.trim();
    let assistant_text = assistant_text.trim();
    if user_text.is_empty() || assistant_text.is_empty() {
        return;
    }

    let transcript = oclaw_agent_core::Transcript::new(sid);
    let user_msg = oclaw_llm_core::chat::ChatMessage {
        role: oclaw_llm_core::chat::MessageRole::User,
        content: user_text.to_string(),
        name: None,
        tool_calls: None,
        tool_call_id: None,
    };
    if let Err(e) = transcript.append(&user_msg).await {
        warn!("Transcript append user failed for {}: {}", sid, e);
        return;
    }

    let assistant_msg = oclaw_llm_core::chat::ChatMessage {
        role: oclaw_llm_core::chat::MessageRole::Assistant,
        content: assistant_text.to_string(),
        name: None,
        tool_calls: None,
        tool_call_id: None,
    };
    if let Err(e) = transcript.append(&assistant_msg).await {
        warn!("Transcript append assistant failed for {}: {}", sid, e);
    }
}

fn sanitize_error(msg: &str) -> String {
    // Strip anything that looks like an API key or token from error messages.
    // Regex is compiled once via LazyLock to avoid per-call overhead.
    static RE: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)(sk-|key-|token-|bearer\s+)[a-zA-Z0-9\-_]{8,}").unwrap()
    });
    RE.replace_all(msg, "${1}[REDACTED]").to_string()
}

#[derive(Debug, Deserialize)]
pub struct ChatCompletionsRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub user: Option<String>,
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

/// OpenAI-compatible streaming chunk format.
#[derive(Debug, Serialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
}

#[derive(Debug, Serialize)]
pub struct ChunkChoice {
    pub index: i32,
    pub delta: ChunkDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChunkDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

fn to_llm_messages(msgs: &[ChatMessage]) -> Vec<oclaw_llm_core::chat::ChatMessage> {
    msgs.iter()
        .map(|m| {
            let role = match m.role.as_str() {
                "system" => oclaw_llm_core::chat::MessageRole::System,
                "assistant" => oclaw_llm_core::chat::MessageRole::Assistant,
                "tool" => oclaw_llm_core::chat::MessageRole::Tool,
                _ => oclaw_llm_core::chat::MessageRole::User,
            };
            oclaw_llm_core::chat::ChatMessage {
                role,
                content: m.content.clone(),
                name: m.name.clone(),
                tool_calls: None,
                tool_call_id: m.tool_call_id.clone(),
            }
        })
        .collect()
}

pub async fn chat_completions_handler(
    State(state): State<Arc<HttpState>>,
    headers: HeaderMap,
    Json(payload): Json<ChatCompletionsRequest>,
) -> Response {
    let is_hatching = state
        .needs_hatching
        .load(std::sync::atomic::Ordering::Relaxed);
    info!(
        "Chat completions: model={}, messages={}, hatching={}",
        payload.model,
        payload.messages.len(),
        is_hatching
    );
    // Log message roles for debugging hatching conversation flow
    if is_hatching {
        for (i, m) in payload.messages.iter().enumerate() {
            let preview: String = m.content.chars().take(60).collect();
            info!("  [{}] {}: {}", i, m.role, preview);
        }
    }

    let provider = match &state.llm_provider {
        Some(p) => p.clone(),
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": {"message": "No LLM provider configured", "type": "server_error"}}))).into_response(),
    };

    let session_id =
        resolve_chat_completions_session_id(&headers, &payload.model, payload.user.as_deref());

    let mut messages = to_llm_messages(&payload.messages);
    if let Some(ref sid) = session_id
        && !payload_has_explicit_history(&payload.messages)
    {
        let transcript = oclaw_agent_core::Transcript::new(sid);
        let mut recalled = transcript.load().await;
        if !recalled.is_empty() {
            let insert_pos = messages
                .iter()
                .position(|m| m.role != oclaw_llm_core::chat::MessageRole::System)
                .unwrap_or(messages.len());
            info!(
                "chat.completions replayed {} transcript messages for session {}",
                recalled.len(),
                sid
            );
            messages.splice(insert_pos..insert_pos, recalled.drain(..));
        }
    }

    // Inject hatching system prompt if first run
    if is_hatching {
        let hatching_prompt =
            oclaw_workspace_core::bootstrap::BootstrapRunner::hatching_system_prompt();
        messages.insert(
            0,
            oclaw_llm_core::chat::ChatMessage {
                role: oclaw_llm_core::chat::MessageRole::System,
                content: hatching_prompt.to_string(),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
        );
    }

    // Memory recall: inject relevant context from long-term memory
    if let Some(ref mm) = state.memory_manager {
        let query = payload
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user")
            .map(|m| m.content.clone())
            .unwrap_or_default();
        if !query.is_empty() {
            let recaller = crate::memory_bridge::MemoryManagerRecaller::new(mm.clone());
            let results =
                oclaw_agent_core::auto_recall::MemoryRecaller::recall(&recaller, &query, 5, 0.3)
                    .await;
            if let Some(ctx_msg) = oclaw_agent_core::auto_recall::format_recall_context(&results) {
                // Insert recall context right after any system messages
                let insert_pos = messages
                    .iter()
                    .position(|m| m.role != oclaw_llm_core::chat::MessageRole::System)
                    .unwrap_or(0);
                messages.insert(insert_pos, ctx_msg);
            }
        }
    }

    // Extract last user message for memory capture after LLM responds
    let user_query_for_capture = payload
        .messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.clone())
        .unwrap_or_default();

    // Build tool schemas for hatching mode
    let hatching_tools: Option<Vec<oclaw_llm_core::chat::Tool>> = if is_hatching {
        state.tool_registry.as_ref().map(|r| {
            r.list_for_llm()
                .into_iter()
                .filter_map(|v| {
                    Some(oclaw_llm_core::chat::Tool {
                        type_: "function".to_string(),
                        function: oclaw_llm_core::chat::ToolFunction {
                            name: v["name"].as_str()?.to_string(),
                            description: v["description"].as_str().unwrap_or("").to_string(),
                            parameters: v["parameters"].clone(),
                        },
                    })
                })
                .collect()
        })
    } else {
        None
    };

    // During hatching, force non-streaming so we can run the tool loop server-side
    let force_non_stream = is_hatching;

    let request = oclaw_llm_core::chat::ChatRequest {
        model: payload.model.clone(),
        messages,
        temperature: payload.temperature,
        top_p: None,
        max_tokens: payload.max_tokens,
        stop: None,
        tools: hatching_tools.clone(),
        tool_choice: None,
        stream: Some(payload.stream && !force_non_stream),
        response_format: None,
    };

    if payload.stream && !force_non_stream {
        let model_name = payload.model.clone();
        let mm_for_stream = state.memory_manager.clone();
        let user_q_for_stream = user_query_for_capture.clone();
        let session_for_stream = session_id.clone();
        match provider.chat_stream(request).await {
            Ok(mut rx) => {
                let stream = async_stream::stream! {
                    let chunk_id = format!("chatcmpl-{}", uuid::Uuid::new_v4().simple());
                    let created = chrono::Utc::now().timestamp();
                    let mut first = true;
                    let mut full_text = String::new();

                    while let Some(chunk) = rx.recv().await {
                        match chunk {
                            Ok(c) => {
                                let content = if c.choices.is_empty() {
                                    None
                                } else {
                                    c.choices[0].delta.as_ref().map(|d| d.content.clone())
                                };

                                // Accumulate for memory capture
                                if let Some(ref t) = content {
                                    full_text.push_str(t);
                                }

                                let delta = if first {
                                    first = false;
                                    ChunkDelta { role: Some("assistant".into()), content }
                                } else {
                                    ChunkDelta { role: None, content }
                                };

                                let chunk_resp = ChatCompletionChunk {
                                    id: chunk_id.clone(),
                                    object: "chat.completion.chunk".into(),
                                    created,
                                    model: model_name.clone(),
                                    choices: vec![ChunkChoice {
                                        index: 0,
                                        delta,
                                        finish_reason: None,
                                    }],
                                };
                                if let Ok(json) = serde_json::to_string(&chunk_resp) {
                                    yield Ok::<_, Infallible>(Event::default().data(json));
                                }
                            }
                            Err(e) => {
                                error!("Stream error: {}", e);
                                break;
                            }
                        }
                    }

                    // Final chunk with finish_reason
                    let final_chunk = ChatCompletionChunk {
                        id: chunk_id,
                        object: "chat.completion.chunk".into(),
                        created,
                        model: model_name,
                        choices: vec![ChunkChoice {
                            index: 0,
                            delta: ChunkDelta { role: None, content: None },
                            finish_reason: Some("stop".into()),
                        }],
                    };
                    if let Ok(json) = serde_json::to_string(&final_chunk) {
                        yield Ok::<_, Infallible>(Event::default().data(json));
                    }
                    yield Ok::<_, Infallible>(Event::default().data("[DONE]"));

                    // Memory capture after stream completes
                    if let Some(mm) = mm_for_stream {
                        spawn_memory_capture(mm, user_q_for_stream.clone(), full_text.clone());
                    }
                    persist_transcript_turn(
                        session_for_stream.as_deref(),
                        &user_q_for_stream,
                        &full_text,
                    ).await;
                };
                Sse::new(stream).into_response()
            }
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": {"message": sanitize_error(&e.to_string()), "type": "server_error"}}))).into_response(),
        }
    } else if is_hatching && state.tool_registry.is_some() {
        // Hatching mode: inline tool execution loop
        hatching_tool_loop(
            &provider,
            &state,
            &payload,
            request,
            hatching_tools,
            &user_query_for_capture,
            session_id.as_deref(),
        )
        .await
    } else {
        // Standard non-streaming path
        non_streaming_response(
            &provider,
            &state,
            request,
            &user_query_for_capture,
            session_id.as_deref(),
        )
        .await
    }
}

/// Hatching mode: run LLM with tool execution loop until we get a pure-text reply.
async fn hatching_tool_loop(
    provider: &Arc<dyn oclaw_llm_core::providers::LlmProvider>,
    state: &Arc<HttpState>,
    payload: &ChatCompletionsRequest,
    initial_request: oclaw_llm_core::chat::ChatRequest,
    tools: Option<Vec<oclaw_llm_core::chat::Tool>>,
    user_query: &str,
    session_id: Option<&str>,
) -> Response {
    let registry = state.tool_registry.as_ref().unwrap();
    let mut loop_messages = initial_request.messages.clone();
    let mut final_content = String::new();
    let mut last_model = payload.model.clone();
    let mut last_id = String::new();

    for _round in 0..10 {
        let req = oclaw_llm_core::chat::ChatRequest {
            model: payload.model.clone(),
            messages: loop_messages.clone(),
            temperature: payload.temperature,
            top_p: None,
            max_tokens: payload.max_tokens,
            stop: None,
            tools: tools.clone(),
            tool_choice: None,
            stream: Some(false),
            response_format: None,
        };

        let completion = match provider.chat(req).await {
            Ok(c) => c,
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": {"message": sanitize_error(&e.to_string()), "type": "server_error"}})),
                ).into_response();
            }
        };

        last_model = completion.model.clone();
        last_id = completion.id.clone();

        let choice = match completion.choices.first() {
            Some(c) => c,
            None => break,
        };

        // No tool_calls → we have a final text reply
        if choice.message.tool_calls.is_none()
            || choice
                .message
                .tool_calls
                .as_ref()
                .is_some_and(|tc| tc.is_empty())
        {
            final_content = choice.message.content.clone();
            info!(
                "[hatching] round {}: text reply ({} chars), no tool calls",
                _round,
                final_content.len()
            );
            break;
        }

        // Append assistant message (with tool_calls) to conversation
        loop_messages.push(choice.message.clone());

        // Execute each tool call and append results
        let tool_calls = choice.message.tool_calls.as_ref().unwrap();
        info!(
            "[hatching] round {}: {} tool call(s)",
            _round,
            tool_calls.len()
        );
        for tc in tool_calls {
            let args: serde_json::Value =
                serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::Value::Null);
            let call = oclaw_tools_core::tool::ToolCall {
                id: tc.id.clone(),
                name: tc.function.name.clone(),
                arguments: args,
            };
            let resp = registry.execute_call(call).await;
            let result_text = if let Some(err) = resp.error {
                format!("Error: {}", err)
            } else {
                resp.result.to_string()
            };
            loop_messages.push(oclaw_llm_core::chat::ChatMessage {
                role: oclaw_llm_core::chat::MessageRole::Tool,
                content: result_text,
                name: None,
                tool_calls: None,
                tool_call_id: Some(tc.id.clone()),
            });
        }
    }

    // Check if hatching is complete (identity personalized)
    check_hatching_complete(state).await;

    // Memory capture
    if let Some(ref mm) = state.memory_manager {
        spawn_memory_capture(mm.clone(), user_query.to_string(), final_content.clone());
    }
    persist_transcript_turn(session_id, user_query, &final_content).await;

    // Return final text as a standard ChatCompletionsResponse
    Json(ChatCompletionsResponse {
        id: last_id,
        object: "chat.completion".to_string(),
        created: chrono::Utc::now().timestamp(),
        model: last_model,
        choices: vec![Choice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content: final_content,
                name: None,
                tool_calls: None,
                tool_call_id: None,
            },
            finish_reason: "stop".to_string(),
        }],
        usage: Usage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        },
    })
    .into_response()
}

/// Check if the agent identity has been fully personalized and clear the hatching flag.
/// Requires name + at least one of (emoji, creature, vibe) to prevent premature completion
/// when the agent writes the name before finishing the full hatching conversation.
async fn check_hatching_complete(state: &Arc<HttpState>) {
    if !state
        .needs_hatching
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        return;
    }
    if let Some(ref ws) = state.workspace
        && let Ok(Some(identity)) = oclaw_workspace_core::identity::AgentIdentity::load(ws).await
    {
        let has_name = identity.name.is_some();
        let has_extras =
            identity.emoji.is_some() || identity.creature.is_some() || identity.vibe.is_some();
        if has_name && has_extras {
            state
                .needs_hatching
                .store(false, std::sync::atomic::Ordering::Relaxed);
            info!(
                "Hatching complete — identity personalized: {}",
                identity.display_name()
            );

            // Clear all old session transcripts so every channel
            // picks up the new identity on next message.
            if let Err(e) = oclaw_agent_core::transcript::Transcript::clear_all_sessions().await {
                warn!("Failed to clear old session transcripts: {}", e);
            } else {
                info!("Cleared old session transcripts after hatching");
            }
        }
    }
}

/// Standard non-streaming chat completion (extracted for readability).
async fn non_streaming_response(
    provider: &Arc<dyn oclaw_llm_core::providers::LlmProvider>,
    state: &Arc<HttpState>,
    request: oclaw_llm_core::chat::ChatRequest,
    user_query: &str,
    session_id: Option<&str>,
) -> Response {
    match provider.chat(request).await {
        Ok(completion) => {
            let assistant_text = completion.choices.first()
                .map(|c| c.message.content.clone())
                .unwrap_or_default();
            if let Some(ref mm) = state.memory_manager {
                spawn_memory_capture(mm.clone(), user_query.to_string(), assistant_text.clone());
            }
            persist_transcript_turn(session_id, user_query, &assistant_text).await;

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
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": {"message": sanitize_error(&e.to_string()), "type": "server_error"}})),
        ).into_response(),
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
        serde_json::Value::Array(arr) => arr
            .iter()
            .filter_map(|item| {
                item.get("content")
                    .and_then(|c| c.as_str())
                    .map(|s| s.to_string())
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };

    let request = oclaw_llm_core::chat::ChatRequest {
        model: payload.model.clone(),
        messages: vec![oclaw_llm_core::chat::ChatMessage {
            role: oclaw_llm_core::chat::MessageRole::User,
            content: input_text,
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }],
        temperature: payload.temperature,
        top_p: None,
        max_tokens: payload.max_tokens,
        stop: None,
        tools: None,
        tool_choice: None,
        stream: None,
        response_format: None,
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

pub async fn agent_status_handler(State(state): State<Arc<HttpState>>) -> impl IntoResponse {
    let has_provider = state.llm_provider.is_some();
    let needs_hatching = state
        .needs_hatching
        .load(std::sync::atomic::Ordering::Relaxed);
    Json(serde_json::json!({
        "status": if has_provider { "ready" } else { "no_provider" },
        "provider_configured": has_provider,
        "needs_hatching": needs_hatching,
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

/// Read recent transcript history for a given session key.
/// Returns the last N messages from the JSONL transcript file.
pub async fn transcript_history_handler(
    State(_state): State<Arc<HttpState>>,
    axum::extract::Path(session_key): axum::extract::Path<String>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(30);

    let transcript = oclaw_agent_core::Transcript::new(&session_key);
    if !transcript.exists().await {
        return Json(serde_json::json!({ "messages": [], "total": 0 }));
    }

    let all = transcript.load().await;
    let total = all.len();
    let recent: Vec<serde_json::Value> = all
        .iter()
        .rev()
        .take(limit)
        .rev()
        .map(|m| {
            serde_json::json!({
                "role": match m.role {
                    oclaw_llm_core::chat::MessageRole::Assistant => "assistant",
                    oclaw_llm_core::chat::MessageRole::System => "system",
                    _ => "user",
                },
                "content": m.content,
            })
        })
        .collect();

    Json(serde_json::json!({ "messages": recent, "total": total }))
}

pub async fn sessions_list_handler(State(state): State<Arc<HttpState>>) -> impl IntoResponse {
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
        Ok(Some(_)) => {
            state.clear_session_counters(&key);
            Json(serde_json::json!({"ok": true})).into_response()
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "session not found"})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e})),
        )
            .into_response(),
    }
}

pub async fn config_get_handler(State(state): State<Arc<HttpState>>) -> impl IntoResponse {
    Json(serde_json::to_value(&*state._gateway).unwrap_or_default())
}

pub async fn config_reload_handler() -> impl IntoResponse {
    // Config reload is handled by the config crate's hot-reload watcher.
    // This endpoint triggers a manual re-read signal.
    Json(serde_json::json!({"ok": true, "message": "reload requested"}))
}

pub async fn models_list_handler(State(state): State<Arc<HttpState>>) -> impl IntoResponse {
    let models = match &state.llm_provider {
        Some(p) => p
            .list_models()
            .await
            .unwrap_or_else(|_| p.supported_models()),
        None => vec![],
    };
    Json(serde_json::json!({ "models": models }))
}

pub async fn config_full_get_handler(State(state): State<Arc<HttpState>>) -> Response {
    match &state.full_config {
        Some(cfg) => {
            let cfg = cfg.read().await;
            Json(serde_json::to_value(&*cfg).unwrap_or_default()).into_response()
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "full config not available"})),
        )
            .into_response(),
    }
}

pub async fn config_full_put_handler(
    State(state): State<Arc<HttpState>>,
    Json(new_config): Json<oclaw_config::settings::Config>,
) -> Response {
    let errors = new_config.validate();
    if !errors.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"errors": errors})),
        )
            .into_response();
    }
    let Some(ref full_config) = state.full_config else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "full config not available"})),
        )
            .into_response();
    };
    if let Some(ref path) = state.config_path {
        match serde_json::to_string_pretty(&new_config) {
            Ok(content) => {
                if let Err(e) = tokio::fs::write(path, content).await {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({"error": format!("write failed: {}", e)})),
                    )
                        .into_response();
                }
            }
            Err(e) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": format!("serialize failed: {}", e)})),
                )
                    .into_response();
            }
        }
    }
    *full_config.write().await = new_config;
    Json(serde_json::json!({"ok": true})).into_response()
}

pub async fn config_ui_handler() -> axum::response::Html<&'static str> {
    axum::response::Html(CONFIG_UI_HTML)
}

pub async fn webchat_ui_handler() -> axum::response::Html<&'static str> {
    axum::response::Html(WEBCHAT_HTML)
}

const CONFIG_UI_HTML: &str = concat!(
    r##"<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>OpenClaw Config</title>
<style>
*{box-sizing:border-box;margin:0;padding:0}
:root{--bg:#0f0f1a;--bg2:#161d30;--bg3:#1e2d4a;--bg4:#141825;--bg5:#19223a;--accent:#00d4ff;--accent2:#0f3460;--border:#2a3a5c;--text:#e0e0e0;--text2:#8899aa;--text3:#6b7f99;--ok:#4caf50;--err:#f44336;--warn:#ff9800}
body{font-family:system-ui,-apple-system,sans-serif;background:var(--bg);color:var(--text);min-height:100vh;display:flex;flex-direction:column}
a{color:var(--accent)}
.topbar{display:flex;align-items:center;gap:12px;padding:12px 20px;background:var(--bg2);border-bottom:1px solid var(--bg3);flex-shrink:0}
.topbar .brand{font-weight:700;color:var(--accent);font-size:16px}
.topbar .spacer{flex:1}
.topbar-btn{padding:5px 12px;background:var(--bg3);border:1px solid var(--border);border-radius:6px;color:var(--text2);cursor:pointer;font-size:12px;transition:all .2s}
.topbar-btn:hover{color:var(--accent);border-color:var(--accent)}
.layout{display:flex;flex:1;overflow:hidden}
.sidebar{width:220px;background:var(--bg2);border-right:1px solid var(--bg3);overflow-y:auto;flex-shrink:0;padding:8px 0}
.sidebar::-webkit-scrollbar{width:4px}
.sidebar::-webkit-scrollbar-thumb{background:var(--accent2);border-radius:2px}
.nav-item{display:flex;align-items:center;gap:10px;padding:10px 16px;cursor:pointer;color:var(--text2);font-size:13px;transition:all .15s;border-left:3px solid transparent}
.nav-item:hover{background:rgba(0,212,255,.04);color:var(--text)}
.nav-item.active{background:rgba(0,212,255,.08);color:var(--accent);border-left-color:var(--accent)}
.nav-item .icon{font-size:16px;width:20px;text-align:center}
.nav-item .badge{margin-left:auto;background:var(--accent2);color:var(--text3);font-size:10px;padding:1px 6px;border-radius:8px}
.main{flex:1;overflow-y:auto;padding:24px 32px}
.main::-webkit-scrollbar{width:6px}
.main::-webkit-scrollbar-thumb{background:var(--accent2);border-radius:3px}
.page{display:none}
.page.active{display:block}
.page-title{font-size:18px;font-weight:600;color:var(--accent);margin-bottom:4px}
.page-desc{font-size:13px;color:var(--text3);margin-bottom:20px}
.card{background:var(--bg5);border:1px solid var(--border);border-radius:10px;padding:16px;margin-bottom:16px;transition:box-shadow .2s}
.card:hover{box-shadow:0 2px 12px rgba(0,212,255,.06)}
.card-header{display:flex;align-items:center;gap:10px;margin-bottom:12px}
.card-title{font-size:14px;font-weight:600;color:var(--text)}
.card-desc{font-size:12px;color:var(--text3)}
.card-badge{font-size:11px;padding:2px 8px;border-radius:4px;font-weight:500}
.card-badge.on{background:#1a3a2e;color:var(--ok)}
.card-badge.off{background:#2a2020;color:var(--text3)}
.field{margin-bottom:14px}
.field-row{display:flex;gap:12px;margin-bottom:14px}
.field-row .field{flex:1;margin-bottom:0}
.field label{display:block;font-size:12px;color:var(--text3);margin-bottom:4px;letter-spacing:.3px}
.field .hint{font-size:11px;color:var(--text3);margin-top:2px;opacity:.7}
.field input,.field textarea,.field select{width:100%;padding:8px 12px;background:var(--bg4);border:1px solid var(--border);border-radius:6px;color:var(--text);font-family:'SF Mono',Monaco,monospace;font-size:13px;transition:border-color .2s}
.field input:focus,.field textarea:focus,.field select:focus{outline:none;border-color:var(--accent)}
.field input.err{border-color:var(--err)}
.field textarea{min-height:60px;resize:vertical}
.field select{cursor:pointer;appearance:none;background-image:url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='12' height='12' viewBox='0 0 12 12'%3E%3Cpath fill='%236b7f99' d='M6 8L1 3h10z'/%3E%3C/svg%3E");background-repeat:no-repeat;background-position:right 10px center}
.toggle-wrap{display:flex;align-items:center;gap:10px;margin-bottom:14px}
.toggle-wrap label{margin-bottom:0;font-size:13px;color:var(--text);cursor:pointer}
.toggle{position:relative;width:40px;height:22px;flex-shrink:0}
.toggle input{opacity:0;width:0;height:0}
.toggle .slider{position:absolute;inset:0;background:var(--border);border-radius:11px;cursor:pointer;transition:.2s}
.toggle .slider:before{content:'';position:absolute;height:16px;width:16px;left:3px;bottom:3px;background:#888;border-radius:50%;transition:.2s}
.toggle input:checked+.slider{background:var(--accent)}
.toggle input:checked+.slider:before{transform:translateX(18px);background:#fff}
.pwd-wrap{position:relative}
.pwd-wrap input{padding-right:36px}
.pwd-toggle{position:absolute;right:8px;top:50%;transform:translateY(-50%);background:none;border:none;color:var(--text3);cursor:pointer;font-size:15px;padding:2px 4px}
.pwd-toggle:hover{color:var(--accent)}
.tags{display:flex;flex-wrap:wrap;gap:4px;margin-top:4px}
.tag{display:inline-flex;align-items:center;gap:4px;padding:3px 8px;background:var(--accent2);border-radius:4px;font-size:12px;color:var(--text2)}
.tag .x{cursor:pointer;color:var(--text3);font-size:14px}
.tag .x:hover{color:var(--err)}
.btn{padding:8px 16px;border-radius:6px;cursor:pointer;font-size:13px;border:1px solid var(--border);transition:all .2s}
.btn-primary{background:linear-gradient(135deg,#00b8d4,var(--accent));color:var(--bg);border:none;font-weight:600}
.btn-primary:hover{box-shadow:0 2px 12px rgba(0,212,255,.3);transform:translateY(-1px)}
.btn-primary:disabled{opacity:.5;cursor:not-allowed;transform:none}
.btn-secondary{background:var(--accent2);color:var(--accent);border-color:rgba(0,212,255,.2)}
.btn-secondary:hover{background:#1a4a80}
.btn-danger{background:#3a1a1a;color:var(--err);border-color:rgba(244,67,54,.3)}
.btn-danger:hover{background:#4a2020}
.btn-ghost{background:transparent;color:var(--text2);border-color:transparent}
.btn-ghost:hover{color:var(--accent);background:rgba(0,212,255,.05)}
.btn-row{display:flex;gap:8px;margin-top:12px}
.provider-grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(320px,1fr));gap:16px}
.channel-grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(300px,1fr));gap:12px}
.ch-card{background:var(--bg4);border:1px solid var(--border);border-radius:8px;padding:14px;transition:all .2s}
.ch-card:hover{border-color:rgba(0,212,255,.3)}
.ch-card.enabled{border-color:rgba(76,175,80,.3)}
.ch-card-head{display:flex;align-items:center;gap:10px;margin-bottom:10px}
.ch-card-head .ch-icon{font-size:20px;width:28px;text-align:center}
.ch-card-head .ch-name{font-weight:600;font-size:14px;color:var(--text)}
.ch-card-head .spacer{flex:1}
.section-divider{border:none;border-top:1px solid var(--border);margin:20px 0}
.empty-state{text-align:center;padding:40px 20px;color:var(--text3)}
.empty-state .icon{font-size:32px;margin-bottom:8px;opacity:.5}
.empty-state p{font-size:14px}
.footer-bar{display:flex;align-items:center;justify-content:space-between;padding:12px 20px;background:var(--bg2);border-top:1px solid var(--bg3);flex-shrink:0}
.footer-bar .status{font-size:12px;color:var(--text3)}
#toast{position:fixed;top:20px;right:20px;z-index:999;pointer-events:none}
.toast-item{padding:12px 20px;border-radius:8px;margin-bottom:8px;font-size:13px;animation:slideIn .3s ease;pointer-events:auto;backdrop-filter:blur(8px)}
.toast-ok{background:rgba(26,58,46,.95);border:1px solid var(--ok);color:var(--ok)}
.toast-err{background:rgba(58,26,26,.95);border:1px solid var(--err);color:var(--err)}
@keyframes slideIn{from{opacity:0;transform:translateX(40px)}to{opacity:1;transform:translateX(0)}}
@keyframes slideOut{from{opacity:1}to{opacity:0;transform:translateX(40px)}}
.modal-bg{position:fixed;top:0;left:0;width:100%;height:100%;background:rgba(0,0,0,.6);display:flex;align-items:center;justify-content:center;z-index:100;backdrop-filter:blur(2px)}
.modal{background:var(--bg3);border:1px solid var(--accent2);border-radius:12px;padding:24px;min-width:380px;max-width:90vw;max-height:80vh;overflow-y:auto}
.modal h3{color:var(--accent);margin-bottom:16px;font-size:16px}
.modal .field{margin-bottom:12px}
.modal-btns{display:flex;gap:8px;justify-content:flex-end;margin-top:20px}
@media(max-width:768px){.sidebar{display:none}.layout{flex-direction:column}.main{padding:16px}}
</style></head><body>
<div class="topbar">
  <span class="brand">OpenClaw</span>
  <span class="spacer"></span>
  <button class="topbar-btn" id="lang-btn" onclick="toggleLang()">中文</button>
  <button class="topbar-btn" id="export-btn" onclick="exportCfg()">Export</button>
  <button class="topbar-btn" id="import-btn" onclick="document.getElementById('import-file').click()">Import</button>
  <input type="file" id="import-file" accept=".json" style="display:none" onchange="importCfg(event)">
  <button class="btn btn-primary" id="save-btn" onclick="save()">Save</button>
</div>
<div class="layout">
<div class="sidebar" id="sidebar"></div>
<div class="main" id="main"></div>
</div>
<div class="footer-bar"><span class="status" id="status-bar">Loading...</span></div>
<div id="toast"></div>
"##,
    // --- CONFIG_UI script part 1: i18n + schema ---
    r##"<script>
let lang='en',cfg={},currentPage='gateway';
const I={en:{
title:'OpenClaw Configuration',save:'Save',saving:'Saving...',saved:'Configuration saved',
cancel:'Cancel',confirm:'OK',delete:'Delete',enable:'Enable',disable:'Disable',
addProvider:'Add Provider',removeProvider:'Remove',providerName:'Provider Name',
providerType:'Provider Type',selectType:'Select type...',
errPrefix:'Error: ',loadErr:'Failed to load config: ',
noProviders:'No providers configured yet',noChannels:'No channels enabled',
exportOk:'Config exported',importOk:'Config imported',importErr:'Invalid config file',
export:'Export',import:'Import',
nav:{gateway:'Gateway',models:'Models',channels:'Channels',session:'Session',browser:'Browser',cron:'Cron Jobs',memory:'Memory',logging:'Logging',advanced:'Advanced'},
navDesc:{gateway:'Server, auth, TLS, proxy',models:'LLM providers and fallback',channels:'Messaging integrations',session:'History and compaction',browser:'Browser automation',cron:'Scheduled tasks',memory:'Memory and embeddings',logging:'Log levels and output',advanced:'Diagnostics, voice, plugins'},
providerTypes:['anthropic','openai','google','cohere','ollama','vllm','litellm','bedrock','openrouter','together','minimax'],
channelNames:{webchat:'Webchat',whatsapp:'WhatsApp',telegram:'Telegram',discord:'Discord',slack:'Slack',signal:'Signal',line:'LINE',matrix:'Matrix',nostr:'Nostr',irc:'IRC',google_chat:'Google Chat',mattermost:'Mattermost',feishu:'Feishu',msteams:'MS Teams',twitch:'Twitch',zalo:'Zalo',nextcloud:'Nextcloud',synology:'Synology',bluebubbles:'BlueBubbles'},
},zh:{
title:'OpenClaw 配置管理',save:'保存',saving:'保存中...',saved:'配置已保存',
cancel:'取消',confirm:'确定',delete:'删除',enable:'启用',disable:'禁用',
addProvider:'添加供应商',removeProvider:'移除',providerName:'供应商名称',
providerType:'供应商类型',selectType:'选择类型...',
errPrefix:'错误：',loadErr:'加载配置失败：',
noProviders:'尚未配置任何供应商',noChannels:'尚未启用任何频道',
exportOk:'配置已导出',importOk:'配置已导入',importErr:'无效的配置文件',
export:'导出',import:'导入',
nav:{gateway:'网关',models:'模型',channels:'频道',session:'会话',browser:'浏览器',cron:'定时任务',memory:'记忆',logging:'日志',advanced:'高级设置'},
navDesc:{gateway:'服务器、认证、TLS、代理',models:'LLM 供应商与降级链',channels:'消息平台集成',session:'历史记录与压缩',browser:'浏览器自动化',cron:'定时任务调度',memory:'记忆与向量搜索',logging:'日志级别与输出',advanced:'诊断、语音、插件'},
providerTypes:['anthropic','openai','google','cohere','ollama','vllm','litellm','bedrock','openrouter','together','minimax'],
channelNames:{webchat:'网页聊天',whatsapp:'WhatsApp',telegram:'Telegram',discord:'Discord',slack:'Slack',signal:'Signal',line:'LINE',matrix:'Matrix',nostr:'Nostr',irc:'IRC',google_chat:'Google Chat',mattermost:'Mattermost',feishu:'飞书',msteams:'MS Teams',twitch:'Twitch',zalo:'Zalo',nextcloud:'Nextcloud',synology:'Synology',bluebubbles:'BlueBubbles'},
labels:{
'Server':'服务器','Authentication':'认证','TLS / HTTPS':'TLS / HTTPS','Control UI':'控制面板','Tailscale':'Tailscale',
'Fallback Settings':'降级设置','Session Settings':'会话设置','Compaction':'上下文压缩','Pruning':'消息裁剪','Auto Reset':'自动重置',
'Browser Automation':'浏览器自动化','SSRF Policy':'SSRF 策略','Cron Settings':'定时任务设置','Long-term Memory':'长期记忆',
'Logging':'日志','Diagnostics / OpenTelemetry':'诊断 / OpenTelemetry','Voice (Talk)':'语音 (Talk)','Web / Reconnect':'Web / 重连',
'UI Settings':'界面设置','Service Discovery':'服务发现','Plugins':'插件','Update':'更新',
'Port':'端口','Bind Address':'绑定地址','Mode':'模式','Custom Bind Host':'自定义绑定主机',
'Auth Mode':'认证模式','Token':'令牌','Password':'密码','Allow Tailscale':'允许 Tailscale',
'Enable TLS':'启用 TLS','Auto-generate Certificate':'自动生成证书','Certificate Path':'证书路径','Key Path':'密钥路径','CA Path':'CA 路径',
'Enable Control UI':'启用控制面板','Base Path':'基础路径','Allowed Origins':'允许的来源','Reset on Exit':'退出时重置',
'Provider Type':'供应商类型','Default Provider':'默认供应商','API Key':'API 密钥','Base URL':'基础 URL','Default Model':'默认模型',
'Max Tokens':'最大令牌数','Temperature':'温度','Max Concurrency':'最大并发数','Fallback Chain':'降级链',
'Enable Fallback':'启用降级','Cooldown (seconds)':'冷却时间（秒）','Retry Delay (ms)':'重试延迟（毫秒）',
'Username':'用户名','Bot Token':'机器人令牌','API URL':'API 地址','Guild ID':'服务器 ID','Channel IDs':'频道 ID',
'Signing Secret':'签名密钥','Webhook URL':'Webhook 地址',
'Phone Number ID':'电话号码 ID','API Token':'API 令牌','Business Account ID':'商业账户 ID','Webhook Verify Token':'Webhook 验证令牌',
'Phone Number':'电话号码','Signal CLI Path':'Signal CLI 路径',
'Channel Access Token':'频道访问令牌','Channel Secret':'频道密钥','User ID':'用户 ID',
'Homeserver':'主服务器','Access Token':'访问令牌','Device ID':'设备 ID','Room ID':'房间 ID',
'Relay URLs':'中继地址','Private Key':'私钥','Public Key':'公钥',
'Server':'服务器','Nickname':'昵称','Channels':'频道',
'Space Name':'空间名称','Service Account JSON':'服务账户 JSON',
'Server URL':'服务器地址','Team ID':'团队 ID','Channel ID':'频道 ID',
'App ID':'应用 ID','App Secret':'应用密钥','Verification Token':'验证令牌','Encrypt Key':'加密密钥',
'History Limit':'历史记录上限','Persist Sessions':'持久化会话',
'Enable Compaction':'启用压缩','Reserve Tokens':'保留令牌数','Keep Recent Tokens':'保留最近令牌数',
'Enable Pruning':'启用裁剪','Soft Trim Max Chars':'软裁剪最大字符数','Hard Clear Max Chars':'硬清除最大字符数','Keep Last N Assistants':'保留最近 N 条助手消息',
'Idle Reset (minutes)':'空闲重置（分钟）',
'Enable Browser':'启用浏览器','Headless Mode':'无头模式','Foreground Window':'前台窗口','No Sandbox':'无沙箱','Attach Only':'仅附加','Enable Evaluate':'启用执行',
'CDP URL':'CDP 地址','Executable Path':'可执行文件路径','Remote CDP Timeout (ms)':'远程 CDP 超时（毫秒）','Default Profile':'默认配置',
'Allow Private Network':'允许私有网络','Allowed Hostnames':'允许的主机名',
'Enable Cron':'启用定时任务','Store Path':'存储路径','Max Concurrent Runs':'最大并发数','Webhook Token':'Webhook 令牌',
'Enable Memory':'启用记忆','Embedding Provider':'嵌入供应商','Embedding Model':'嵌入模型','Database Path':'数据库路径',
'Vector Weight':'向量权重','Text Weight':'文本权重','Auto Index':'自动索引',
'Log Level':'日志级别','Console Level':'控制台级别','Console Style':'控制台样式','Log File Path':'日志文件路径',
'Redact Sensitive':'脱敏处理','Redact Patterns':'脱敏规则',
'Enable Diagnostics':'启用诊断','Enable OTel':'启用 OTel','OTel Endpoint':'OTel 端点','Traces':'链路追踪','Metrics':'指标','Logs':'日志',
'Service Name':'服务名称','Sample Rate':'采样率',
'Voice ID':'语音 ID','Model ID':'模型 ID','Output Format':'输出格式','Interrupt on Speech':'语音打断',
'Enable Web':'启用 Web','Heartbeat (seconds)':'心跳间隔（秒）','Initial (ms)':'初始值（毫秒）','Max (ms)':'最大值（毫秒）',
'Factor':'退避因子','Max Attempts':'最大重试次数',
'Seam Color':'主题色','Assistant Name':'助手名称','Assistant Avatar':'助手头像',
'Wide Area Discovery':'广域发现','mDNS Mode':'mDNS 模式',
'Enable Plugins':'启用插件','Allow List':'允许列表','Deny List':'拒绝列表',
'Update Channel':'更新通道','Check on Start':'启动时检查',
'Bot ID':'机器人 ID','Bot Password':'机器人密码','Tenant ID':'租户 ID',
'Client ID':'客户端 ID','Channel Name':'频道名称','Webhook Secret':'Webhook 密钥',
'Rate Limiting':'速率限制','Max Attempts':'最大尝试次数','Window (ms)':'窗口（毫秒）','Lockout (ms)':'锁定（毫秒）','Exempt Loopback':'豁免回环',
'Trusted Proxy':'可信代理','User Header':'用户头','Required Headers':'必需头','Allow Users':'允许用户',
'Remote Connection':'远程连接','Transport':'传输协议','TLS Fingerprint':'TLS 指纹','SSH Target':'SSH 目标','SSH Identity':'SSH 身份',
'Config Reload':'配置重载','Debounce (ms)':'防抖（毫秒）','HTTP Endpoints':'HTTP 端点','Chat Completions':'聊天补全','Responses':'响应',
'Tool Allow/Deny':'工具允许/拒绝','Allow List':'允许列表','Deny List':'拒绝列表','Health Check Interval (min)':'健康检查间隔（分钟）',
'Canvas Host':'Canvas 主机','Live Reload':'实时重载','Root Path':'根路径',
'Preserve Filenames':'保留文件名','Shell Environment':'Shell 环境','Timeout (ms)':'超时（毫秒）',
},
}};
const t=k=>{const parts=k.split('.');let v=I[lang];for(const p of parts){v=v?.[p];if(v===undefined)return k}return v};
const tl=s=>{if(lang==='zh'&&I.zh.labels[s])return I.zh.labels[s];return s};
const NAV_ICONS={gateway:'\u2699',models:'\u2B22',channels:'\u260E',session:'\u23F3',browser:'\u{1F310}',cron:'\u23F0',memory:'\u{1F9E0}',logging:'\u{1F4CB}',advanced:'\u2699'};
const CH_ICONS={webchat:'\u{1F4AC}',whatsapp:'\u{1F4F1}',telegram:'\u2708',discord:'\u{1F3AE}',slack:'\u{1F4E8}',signal:'\u{1F510}',line:'\u{1F49A}',matrix:'\u{1F504}',nostr:'\u{1F4E1}',irc:'\u{1F4BB}',google_chat:'\u{1F4AC}',mattermost:'\u{1F4AC}',feishu:'\u{1F426}',msteams:'\u{1F4BC}',twitch:'\u{1F3AE}',zalo:'\u{1F4AC}',nextcloud:'\u2601',synology:'\u{1F4E6}',bluebubbles:'\u{1F4AC}'};
function toggleLang(){lang=lang==='en'?'zh':'en';document.getElementById('lang-btn').textContent=lang==='en'?'中文':'EN';renderAll()}
function toast(msg,ok){const d=document.getElementById('toast'),el=document.createElement('div');el.className='toast-item '+(ok?'toast-ok':'toast-err');el.textContent=msg;d.appendChild(el);setTimeout(()=>{el.style.animation='slideOut .3s ease forwards';setTimeout(()=>el.remove(),300)},2500)}
function isSensitive(k){return/token|key|password|secret|api_key|signing/i.test(k)}
function gp(obj,path){const parts=path.split('.');let c=obj;for(const p of parts){if(c==null)return undefined;c=c[p]}return c}
function sp(obj,path,val){const parts=path.split('.');let c=obj;for(let i=0;i<parts.length-1;i++){if(c[parts[i]]==null)c[parts[i]]={};c=c[parts[i]]}c[parts[parts.length-1]]=val}
function dp(obj,path){const parts=path.split('.');let c=obj;for(let i=0;i<parts.length-1;i++){if(c==null)return;c=c[parts[i]]}if(c)delete c[parts[parts.length-1]]}
function mkInput(path,label,type,opts){
const d=document.createElement('div');d.className='field';
if(label){const l=document.createElement('label');l.textContent=tl(label);d.appendChild(l)}
const val=gp(cfg,path);
if(type==='toggle'){
const w=document.createElement('div');w.className='toggle-wrap';
const tog=document.createElement('label');tog.className='toggle';
const inp=document.createElement('input');inp.type='checkbox';inp.checked=!!val;inp.dataset.path=path;inp.dataset.type='bool';
const sl=document.createElement('span');sl.className='slider';
tog.appendChild(inp);tog.appendChild(sl);w.appendChild(tog);
const lb=document.createElement('label');lb.textContent=tl(label)||'';lb.style.cursor='pointer';lb.onclick=()=>{inp.checked=!inp.checked};
w.appendChild(lb);d.innerHTML='';d.appendChild(w);
}else if(type==='select'){
const sel=document.createElement('select');sel.dataset.path=path;
(opts?.options||[]).forEach(o=>{const op=document.createElement('option');op.value=o;op.textContent=o;if(val===o)op.selected=true;sel.appendChild(op)});
d.appendChild(sel);
}else if(type==='password'){
const w=document.createElement('div');w.className='pwd-wrap';
const inp=document.createElement('input');inp.type='password';inp.value=val||'';inp.dataset.path=path;inp.placeholder=opts?.placeholder||'';
const btn=document.createElement('button');btn.type='button';btn.className='pwd-toggle';btn.innerHTML='&#128065;';
btn.onclick=()=>{inp.type=inp.type==='password'?'text':'password'};
w.appendChild(inp);w.appendChild(btn);d.appendChild(w);
}else if(type==='number'){
const inp=document.createElement('input');inp.type='number';inp.value=val!=null?val:'';inp.step=opts?.step||'any';inp.dataset.path=path;inp.placeholder=opts?.placeholder||'';
d.appendChild(inp);
}else if(type==='textarea'){
const ta=document.createElement('textarea');ta.value=val!=null?(typeof val==='string'?val:JSON.stringify(val,null,2)):'';ta.dataset.path=path;ta.dataset.type=opts?.arrayMode?'array':'text';ta.placeholder=opts?.placeholder||'';
d.appendChild(ta);
}else{
const inp=document.createElement('input');inp.type='text';inp.value=val||'';inp.dataset.path=path;inp.placeholder=opts?.placeholder||'';
d.appendChild(inp);
}
if(opts?.hint){const h=document.createElement('div');h.className='hint';h.textContent=opts.hint;d.appendChild(h)}
return d;
}
function mkRow(fields){const r=document.createElement('div');r.className='field-row';fields.forEach(f=>r.appendChild(f));return r}
function showModal(title,fields,onOk){
const bg=document.createElement('div');bg.className='modal-bg';
const m=document.createElement('div');m.className='modal';
const h=document.createElement('h3');h.textContent=title;m.appendChild(h);
const inputs={};
fields.forEach(f=>{
const d=document.createElement('div');d.className='field';
const l=document.createElement('label');l.textContent=f.label;d.appendChild(l);
if(f.type==='select'){
const sel=document.createElement('select');(f.options||[]).forEach(o=>{const op=document.createElement('option');op.value=o;op.textContent=o;sel.appendChild(op)});
inputs[f.key]=sel;d.appendChild(sel);
}else{const i=document.createElement('input');i.type=f.type||'text';i.placeholder=f.placeholder||'';inputs[f.key]=i;d.appendChild(i)}
m.appendChild(d);
});
const btns=document.createElement('div');btns.className='modal-btns';
const cBtn=document.createElement('button');cBtn.className='btn btn-ghost';cBtn.textContent=t('cancel');cBtn.onclick=()=>bg.remove();
const oBtn=document.createElement('button');oBtn.className='btn btn-primary';oBtn.textContent=t('confirm');
oBtn.onclick=()=>{const vals={};for(const[k,i] of Object.entries(inputs))vals[k]=(i.value||'').trim();if(onOk(vals))bg.remove()};
btns.appendChild(cBtn);btns.appendChild(oBtn);m.appendChild(btns);
bg.appendChild(m);bg.onclick=e=>{if(e.target===bg)bg.remove()};
document.body.appendChild(bg);
}
// --- Sidebar + page rendering ---
function renderSidebar(){
const sb=document.getElementById('sidebar');sb.innerHTML='';
const pages=['gateway','models','channels','session','browser','cron','memory','logging','advanced'];
pages.forEach(p=>{
const el=document.createElement('div');el.className='nav-item'+(p===currentPage?' active':'');
el.innerHTML='<span class="icon">'+(NAV_ICONS[p]||'')+'</span><span>'+t('nav.'+p)+'</span>';
el.onclick=()=>{currentPage=p;renderSidebar();renderPage()};
sb.appendChild(el);
});
}
function renderPage(){
const main=document.getElementById('main');main.innerHTML='';
const pg=document.createElement('div');pg.className='page active';
const renderers={gateway:pgGateway,models:pgModels,channels:pgChannels,session:pgSession,browser:pgBrowser,cron:pgCron,memory:pgMemory,logging:pgLogging,advanced:pgAdvanced};
const fn=renderers[currentPage];if(fn)fn(pg);
main.appendChild(pg);
document.getElementById('status-bar').textContent=t('nav.'+currentPage)+' — '+t('navDesc.'+currentPage);
}
function renderAll(){renderSidebar();renderPage();document.getElementById('save-btn').textContent=t('save');document.getElementById('export-btn').textContent=t('export');document.getElementById('import-btn').textContent=t('import')}
function pgTitle(pg,key){
const h=document.createElement('div');h.className='page-title';h.textContent=t('nav.'+key);pg.appendChild(h);
const d=document.createElement('div');d.className='page-desc';d.textContent=t('navDesc.'+key);pg.appendChild(d);
}
// --- Page: Gateway ---
function pgGateway(pg){
pgTitle(pg,'gateway');
if(!cfg.gateway)cfg.gateway={};
const c=document.createElement('div');c.className='card';
const ch=document.createElement('div');ch.className='card-header';
const ct=document.createElement('div');ct.className='card-title';ct.textContent=tl('Server');c.appendChild(ch);ch.appendChild(ct);
c.appendChild(mkRow([mkInput('gateway.port','Port','number',{placeholder:'8080'}),mkInput('gateway.bind','Bind Address','text',{placeholder:'0.0.0.0'})]));
c.appendChild(mkInput('gateway.mode','Mode','select',{options:['normal','http-only','ws-only']}));
c.appendChild(mkInput('gateway.customBindHost','Custom Bind Host','text',{placeholder:'Optional custom hostname'}));
pg.appendChild(c);
// Auth
const ac=document.createElement('div');ac.className='card';
const ah=document.createElement('div');ah.className='card-header';
const at=document.createElement('div');at.className='card-title';at.textContent=tl('Authentication');ac.appendChild(ah);ah.appendChild(at);
if(!cfg.gateway.auth)cfg.gateway.auth={};
ac.appendChild(mkInput('gateway.auth.mode','Auth Mode','select',{options:['none','token','password','device']}));
ac.appendChild(mkInput('gateway.auth.token','Token','password',{placeholder:'Bearer token for API access'}));
ac.appendChild(mkInput('gateway.auth.password','Password','password',{placeholder:'Password for password auth mode'}));
ac.appendChild(mkInput('gateway.auth.allowTailscale','Allow Tailscale','toggle'));
pg.appendChild(ac);
// TLS
const tc=document.createElement('div');tc.className='card';
const th2=document.createElement('div');th2.className='card-header';
const tt=document.createElement('div');tt.className='card-title';tt.textContent=tl('TLS / HTTPS');tc.appendChild(th2);th2.appendChild(tt);
if(!cfg.gateway.tls)cfg.gateway.tls={};
tc.appendChild(mkInput('gateway.tls.enabled','Enable TLS','toggle'));
tc.appendChild(mkInput('gateway.tls.autoGenerate','Auto-generate Certificate','toggle'));
tc.appendChild(mkRow([mkInput('gateway.tls.certPath','Certificate Path','text',{placeholder:'/path/to/cert.pem'}),mkInput('gateway.tls.keyPath','Key Path','text',{placeholder:'/path/to/key.pem'})]));
tc.appendChild(mkInput('gateway.tls.caPath','CA Path','text',{placeholder:'Optional CA bundle path'}));
pg.appendChild(tc);
// Control UI
const uc=document.createElement('div');uc.className='card';
const uh=document.createElement('div');uh.className='card-header';
const ut2=document.createElement('div');ut2.className='card-title';ut2.textContent=tl('Control UI');uc.appendChild(uh);uh.appendChild(ut2);
if(!cfg.gateway.controlUi)cfg.gateway.controlUi={};
uc.appendChild(mkInput('gateway.controlUi.enabled','Enable Control UI','toggle'));
uc.appendChild(mkInput('gateway.controlUi.basePath','Base Path','text',{placeholder:'/ui'}));
uc.appendChild(mkInput('gateway.controlUi.allowedOrigins','Allowed Origins','textarea',{placeholder:'["http://localhost:3000"]',arrayMode:true}));
pg.appendChild(uc);
// Tailscale
const tsc=document.createElement('div');tsc.className='card';
const tsh=document.createElement('div');tsh.className='card-header';
const tst=document.createElement('div');tst.className='card-title';tst.textContent=tl('Tailscale');tsc.appendChild(tsh);tsh.appendChild(tst);
if(!cfg.gateway.tailscale)cfg.gateway.tailscale={};
tsc.appendChild(mkInput('gateway.tailscale.mode','Mode','select',{options:['','funnel','serve']}));
tsc.appendChild(mkInput('gateway.tailscale.resetOnExit','Reset on Exit','toggle'));
pg.appendChild(tsc);
// Rate Limiting
const rlc=document.createElement('div');rlc.className='card';
const rlh=document.createElement('div');rlh.className='card-header';
const rlt=document.createElement('div');rlt.className='card-title';rlt.textContent=tl('Rate Limiting');rlc.appendChild(rlh);rlh.appendChild(rlt);
if(!cfg.gateway.auth.rateLimit)cfg.gateway.auth.rateLimit={};
rlc.appendChild(mkRow([mkInput('gateway.auth.rateLimit.maxAttempts','Max Attempts','number',{placeholder:'5'}),mkInput('gateway.auth.rateLimit.windowMs','Window (ms)','number',{placeholder:'60000'})]));
rlc.appendChild(mkRow([mkInput('gateway.auth.rateLimit.lockoutMs','Lockout (ms)','number',{placeholder:'300000'}),mkInput('gateway.auth.rateLimit.exemptLoopback','Exempt Loopback','toggle')]));
pg.appendChild(rlc);
// Remote
const rmc=document.createElement('div');rmc.className='card';
const rmh=document.createElement('div');rmh.className='card-header';
const rmt=document.createElement('div');rmt.className='card-title';rmt.textContent=tl('Remote Connection');rmc.appendChild(rmh);rmh.appendChild(rmt);
if(!cfg.gateway.remote)cfg.gateway.remote={};
rmc.appendChild(mkInput('gateway.remote.url','URL','text',{placeholder:'wss://remote-host:8080'}));
rmc.appendChild(mkInput('gateway.remote.transport','Transport','select',{options:['','ws','ssh']}));
rmc.appendChild(mkRow([mkInput('gateway.remote.token','Token','password'),mkInput('gateway.remote.password','Password','password')]));
pg.appendChild(rmc);
// Reload
const rldc=document.createElement('div');rldc.className='card';
const rldh=document.createElement('div');rldh.className='card-header';
const rldt=document.createElement('div');rldt.className='card-title';rldt.textContent=tl('Config Reload');rldc.appendChild(rldh);rldh.appendChild(rldt);
if(!cfg.gateway.reload)cfg.gateway.reload={};
rldc.appendChild(mkInput('gateway.reload.mode','Mode','select',{options:['','watch','manual']}));
rldc.appendChild(mkInput('gateway.reload.debounceMs','Debounce (ms)','number',{placeholder:'1000'}));
pg.appendChild(rldc);
// HTTP Endpoints
const hec=document.createElement('div');hec.className='card';
const heh=document.createElement('div');heh.className='card-header';
const het=document.createElement('div');het.className='card-title';het.textContent=tl('HTTP Endpoints');hec.appendChild(heh);heh.appendChild(het);
if(!cfg.gateway.http)cfg.gateway.http={};if(!cfg.gateway.http.endpoints)cfg.gateway.http.endpoints={};
if(!cfg.gateway.http.endpoints.chatCompletions)cfg.gateway.http.endpoints.chatCompletions={};
if(!cfg.gateway.http.endpoints.responses)cfg.gateway.http.endpoints.responses={};
hec.appendChild(mkInput('gateway.http.endpoints.chatCompletions.enabled','Chat Completions','toggle'));
hec.appendChild(mkInput('gateway.http.endpoints.responses.enabled','Responses','toggle'));
pg.appendChild(hec);
// Tools + Health Check
const gtc=document.createElement('div');gtc.className='card';
const gth=document.createElement('div');gth.className='card-header';
const gtt=document.createElement('div');gtt.className='card-title';gtt.textContent=tl('Tool Allow/Deny');gtc.appendChild(gth);gth.appendChild(gtt);
if(!cfg.gateway.tools)cfg.gateway.tools={};
gtc.appendChild(mkInput('gateway.tools.allow','Allow List','textarea',{placeholder:'["tool_a"]',arrayMode:true}));
gtc.appendChild(mkInput('gateway.tools.deny','Deny List','textarea',{placeholder:'["tool_b"]',arrayMode:true}));
gtc.appendChild(mkInput('gateway.channelHealthCheckMinutes','Health Check Interval (min)','number',{placeholder:'5'}));
pg.appendChild(gtc);
}
// --- Page: Models ---
function pgModels(pg){
pgTitle(pg,'models');
if(!cfg.models)cfg.models={};
if(!cfg.models.providers)cfg.models.providers={};
const providers=cfg.models.providers;
const providerNames=Object.keys(providers).sort();
pg.appendChild(mkInput('models.defaultProvider','Default Provider','select',{options:[''].concat(providerNames)}));
const grid=document.createElement('div');grid.className='provider-grid';
for(const[name,prov] of Object.entries(providers)){
const c=document.createElement('div');c.className='card';
const ch2=document.createElement('div');ch2.className='card-header';
const ct2=document.createElement('div');ct2.className='card-title';ct2.textContent=name;
const badge=document.createElement('span');badge.className='card-badge on';badge.textContent=prov.provider||'unknown';
const spacer=document.createElement('div');spacer.style.flex='1';
const delBtn=document.createElement('button');delBtn.className='btn btn-danger';delBtn.textContent=t('removeProvider');delBtn.style.fontSize='11px';delBtn.style.padding='4px 10px';
delBtn.onclick=()=>{delete providers[name];if(cfg.models.defaultProvider===name)cfg.models.defaultProvider='';renderAll()};
ch2.appendChild(ct2);ch2.appendChild(badge);ch2.appendChild(spacer);ch2.appendChild(delBtn);c.appendChild(ch2);
const base='models.providers.'+name;
c.appendChild(mkInput(base+'.provider','Provider Type','select',{options:t('providerTypes')}));
c.appendChild(mkInput(base+'.apiKey','API Key','password',{placeholder:'sk-...'}));
c.appendChild(mkInput(base+'.baseUrl','Base URL','text',{placeholder:'Optional custom endpoint'}));
c.appendChild(mkInput(base+'.model','Default Model','text',{placeholder:'e.g. gpt-4, claude-3-opus'}));
c.appendChild(mkRow([mkInput(base+'.maxTokens','Max Tokens','number',{placeholder:'4096'}),mkInput(base+'.temperature','Temperature','number',{placeholder:'0.7',step:'0.1'})]));
c.appendChild(mkInput(base+'.maxConcurrency','Max Concurrency','number',{placeholder:'10'}));
c.appendChild(mkInput(base+'.fallback','Fallback Chain','textarea',{placeholder:'["provider2","provider3"]',arrayMode:true}));
grid.appendChild(c);
}
pg.appendChild(grid);
if(!Object.keys(providers).length){
const empty=document.createElement('div');empty.className='empty-state';empty.innerHTML='<div class="icon">\u2B22</div><p>'+t('noProviders')+'</p>';
pg.appendChild(empty);
}
const addBtn=document.createElement('button');addBtn.className='btn btn-secondary';addBtn.textContent='+ '+t('addProvider');addBtn.style.marginTop='12px';
addBtn.onclick=()=>showModal(t('addProvider'),[
{key:'name',label:t('providerName'),placeholder:'e.g. my-openai'},
{key:'type',label:t('providerType'),type:'select',options:t('providerTypes')}
],vals=>{
if(!vals.name)return false;
if(providers[vals.name]){toast(t('providerName')+(lang==='zh'?' 已存在':' exists'),false);return false}
providers[vals.name]={provider:vals.type||'openai'};
if(!cfg.models.defaultProvider)cfg.models.defaultProvider=vals.name;
renderAll();return true;
});
pg.appendChild(addBtn);
// Fallback settings
const fc=document.createElement('div');fc.className='card';fc.style.marginTop='16px';
const fh=document.createElement('div');fh.className='card-header';
const ft=document.createElement('div');ft.className='card-title';ft.textContent=tl('Fallback Settings');fc.appendChild(fh);fh.appendChild(ft);
if(!cfg.models.fallback)cfg.models.fallback={};
fc.appendChild(mkInput('models.fallback.enabled','Enable Fallback','toggle'));
fc.appendChild(mkRow([mkInput('models.fallback.cooldownSecs','Cooldown (seconds)','number',{placeholder:'60'}),mkInput('models.fallback.retryDelayMs','Retry Delay (ms)','number',{placeholder:'1000'})]));
pg.appendChild(fc);
}
"##,
    // --- CONFIG_UI script part 2: channels + session pages ---
    r##"
// --- Page: Channels ---
function pgChannels(pg){
pgTitle(pg,'channels');
if(!cfg.channels)cfg.channels={};
const CHSCHEMA={
webchat:{fields:[{k:'auth.username',l:'Username',t:'text'},{k:'auth.password',l:'Password',t:'password'}]},
telegram:{fields:[{k:'botToken',l:'Bot Token',t:'password'},{k:'apiUrl',l:'API URL',t:'text',p:'https://api.telegram.org'}]},
discord:{fields:[{k:'botToken',l:'Bot Token',t:'password'},{k:'guildId',l:'Guild ID',t:'text'},{k:'channelIds',l:'Channel IDs',t:'textarea',arr:true}]},
slack:{fields:[{k:'botToken',l:'Bot Token',t:'password'},{k:'signingSecret',l:'Signing Secret',t:'password'},{k:'channelIds',l:'Channel IDs',t:'textarea',arr:true},{k:'webhookUrl',l:'Webhook URL',t:'text'}]},
whatsapp:{fields:[{k:'phoneNumberId',l:'Phone Number ID',t:'text'},{k:'apiToken',l:'API Token',t:'password'},{k:'businessAccountId',l:'Business Account ID',t:'text'},{k:'webhookVerifyToken',l:'Webhook Verify Token',t:'password'}]},
signal:{fields:[{k:'phoneNumber',l:'Phone Number',t:'text'},{k:'apiUrl',l:'API URL',t:'text'},{k:'signalCliPath',l:'Signal CLI Path',t:'text'}]},
line:{fields:[{k:'channelAccessToken',l:'Channel Access Token',t:'password'},{k:'channelSecret',l:'Channel Secret',t:'password'},{k:'userId',l:'User ID',t:'text'}]},
matrix:{fields:[{k:'homeserver',l:'Homeserver',t:'text',p:'https://matrix.org'},{k:'userId',l:'User ID',t:'text'},{k:'accessToken',l:'Access Token',t:'password'},{k:'deviceId',l:'Device ID',t:'text'},{k:'roomId',l:'Room ID',t:'text'}]},
nostr:{fields:[{k:'relayUrls',l:'Relay URLs',t:'textarea',arr:true},{k:'privateKey',l:'Private Key',t:'password'},{k:'publicKey',l:'Public Key',t:'text'}]},
irc:{fields:[{k:'server',l:'Server',t:'text'},{k:'port',l:'Port',t:'number'},{k:'nick',l:'Nickname',t:'text'},{k:'password',l:'Password',t:'password'},{k:'channels',l:'Channels',t:'textarea',arr:true}]},
google_chat:{fields:[{k:'spaceName',l:'Space Name',t:'text'},{k:'serviceAccountJson',l:'Service Account JSON',t:'password'}]},
mattermost:{fields:[{k:'serverUrl',l:'Server URL',t:'text'},{k:'accessToken',l:'Access Token',t:'password'},{k:'teamId',l:'Team ID',t:'text'},{k:'channelId',l:'Channel ID',t:'text'}]},
feishu:{fields:[{k:'appId',l:'App ID',t:'text'},{k:'appSecret',l:'App Secret',t:'password'},{k:'verificationToken',l:'Verification Token',t:'password'},{k:'encryptKey',l:'Encrypt Key',t:'password'}]},
msteams:{fields:[{k:'botId',l:'Bot ID',t:'text'},{k:'botPassword',l:'Bot Password',t:'password'},{k:'tenantId',l:'Tenant ID',t:'text'}]},
twitch:{fields:[{k:'clientId',l:'Client ID',t:'text'},{k:'accessToken',l:'Access Token',t:'password'},{k:'channelName',l:'Channel Name',t:'text'}]},
zalo:{fields:[{k:'appId',l:'App ID',t:'text'},{k:'accessToken',l:'Access Token',t:'password'},{k:'webhookSecret',l:'Webhook Secret',t:'password'}]},
nextcloud:{fields:[{k:'serverUrl',l:'Server URL',t:'text'},{k:'token',l:'Token',t:'password'},{k:'secret',l:'Secret',t:'password'}]},
synology:{fields:[{k:'serverUrl',l:'Server URL',t:'text'},{k:'token',l:'Token',t:'password'}]},
bluebubbles:{fields:[{k:'serverUrl',l:'Server URL',t:'text'},{k:'password',l:'Password',t:'password'}]}
};
const grid=document.createElement('div');grid.className='channel-grid';
for(const[chKey,schema] of Object.entries(CHSCHEMA)){
const chCfg=cfg.channels[chKey];
const enabled=chCfg?.enabled;
const card=document.createElement('div');card.className='ch-card'+(enabled?' enabled':'');
const head=document.createElement('div');head.className='ch-card-head';
head.innerHTML='<span class="ch-icon">'+(CH_ICONS[chKey]||'')+'</span><span class="ch-name">'+(t('channelNames.'+chKey)||chKey)+'</span><span class="spacer"></span>';
const tog=document.createElement('label');tog.className='toggle';
const inp=document.createElement('input');inp.type='checkbox';inp.checked=!!enabled;
inp.onchange=()=>{
if(inp.checked){if(!cfg.channels[chKey])cfg.channels[chKey]={};cfg.channels[chKey].enabled=true}
else if(cfg.channels[chKey])cfg.channels[chKey].enabled=false;
renderAll();
};
const sl=document.createElement('span');sl.className='slider';
tog.appendChild(inp);tog.appendChild(sl);head.appendChild(tog);card.appendChild(head);
if(enabled){
const base='channels.'+chKey;
schema.fields.forEach(f=>{
const fType=f.t==='password'?'password':f.t==='number'?'number':f.t==='textarea'?'textarea':'text';
card.appendChild(mkInput(base+'.'+f.k,tl(f.l),fType,{placeholder:f.p||'',arrayMode:f.arr}));
});
}
grid.appendChild(card);
}
pg.appendChild(grid);
}
// --- Page: Session ---
function pgSession(pg){
pgTitle(pg,'session');
if(!cfg.session)cfg.session={};
const c=document.createElement('div');c.className='card';
const ch=document.createElement('div');ch.className='card-header';
const ct=document.createElement('div');ct.className='card-title';ct.textContent=tl('Session Settings');c.appendChild(ch);ch.appendChild(ct);
c.appendChild(mkInput('session.historyLimit','History Limit','number',{placeholder:'100'}));
c.appendChild(mkInput('session.persist','Persist Sessions','toggle'));
pg.appendChild(c);
// Compaction
if(!cfg.session.compaction)cfg.session.compaction={};
const cc=document.createElement('div');cc.className='card';
const cch=document.createElement('div');cch.className='card-header';
const cct=document.createElement('div');cct.className='card-title';cct.textContent=tl('Compaction');cc.appendChild(cch);cch.appendChild(cct);
cc.appendChild(mkInput('session.compaction.enabled','Enable Compaction','toggle'));
cc.appendChild(mkRow([mkInput('session.compaction.reserveTokens','Reserve Tokens','number',{placeholder:'2000'}),mkInput('session.compaction.keepRecentTokens','Keep Recent Tokens','number',{placeholder:'4000'})]));
pg.appendChild(cc);
// Pruning
if(!cfg.session.pruning)cfg.session.pruning={};
const pc=document.createElement('div');pc.className='card';
const pch=document.createElement('div');pch.className='card-header';
const pct=document.createElement('div');pct.className='card-title';pct.textContent=tl('Pruning');pc.appendChild(pch);pch.appendChild(pct);
pc.appendChild(mkInput('session.pruning.enabled','Enable Pruning','toggle'));
pc.appendChild(mkRow([mkInput('session.pruning.softTrimMaxChars','Soft Trim Max Chars','number',{placeholder:'50000'}),mkInput('session.pruning.hardClearMaxChars','Hard Clear Max Chars','number',{placeholder:'100000'})]));
pc.appendChild(mkInput('session.pruning.keepLastAssistants','Keep Last N Assistants','number',{placeholder:'3'}));
pg.appendChild(pc);
// Reset
if(!cfg.session.reset)cfg.session.reset={};
const rc=document.createElement('div');rc.className='card';
const rch=document.createElement('div');rch.className='card-header';
const rct=document.createElement('div');rct.className='card-title';rct.textContent=tl('Auto Reset');rc.appendChild(rch);rch.appendChild(rct);
rc.appendChild(mkInput('session.reset.idleMinutes','Idle Reset (minutes)','number',{placeholder:'30'}));
pg.appendChild(rc);
}
"##,
    // --- CONFIG_UI script part 3: browser + cron + memory + logging pages ---
    r##"
// --- Page: Browser ---
function pgBrowser(pg){
pgTitle(pg,'browser');
if(!cfg.browser)cfg.browser={};
const c=document.createElement('div');c.className='card';
const ch=document.createElement('div');ch.className='card-header';
const ct=document.createElement('div');ct.className='card-title';ct.textContent=tl('Browser Automation');c.appendChild(ch);ch.appendChild(ct);
c.appendChild(mkInput('browser.enabled','Enable Browser','toggle'));
c.appendChild(mkInput('browser.headless','Headless Mode','toggle'));
c.appendChild(mkInput('browser.foreground','Foreground Window','toggle'));
c.appendChild(mkInput('browser.noSandbox','No Sandbox','toggle'));
c.appendChild(mkInput('browser.attachOnly','Attach Only','toggle'));
c.appendChild(mkInput('browser.evaluateEnabled','Enable Evaluate','toggle'));
c.appendChild(mkInput('browser.cdpUrl','CDP URL','text',{placeholder:'ws://localhost:9222'}));
c.appendChild(mkInput('browser.executablePath','Executable Path','text',{placeholder:'/usr/bin/chromium'}));
c.appendChild(mkInput('browser.remoteCdpTimeoutMs','Remote CDP Timeout (ms)','number',{placeholder:'30000'}));
c.appendChild(mkInput('browser.defaultProfile','Default Profile','text'));
pg.appendChild(c);
// SSRF Policy
if(!cfg.browser.ssrfPolicy)cfg.browser.ssrfPolicy={};
const sc=document.createElement('div');sc.className='card';
const sh=document.createElement('div');sh.className='card-header';
const st=document.createElement('div');st.className='card-title';st.textContent=tl('SSRF Policy');sc.appendChild(sh);sh.appendChild(st);
sc.appendChild(mkInput('browser.ssrfPolicy.allowPrivateNetwork','Allow Private Network','toggle'));
sc.appendChild(mkInput('browser.ssrfPolicy.allowedHostnames','Allowed Hostnames','textarea',{placeholder:'["example.com"]',arrayMode:true}));
pg.appendChild(sc);
}
// --- Page: Cron ---
function pgCron(pg){
pgTitle(pg,'cron');
if(!cfg.cron)cfg.cron={};
const c=document.createElement('div');c.className='card';
const ch=document.createElement('div');ch.className='card-header';
const ct=document.createElement('div');ct.className='card-title';ct.textContent=tl('Cron Settings');c.appendChild(ch);ch.appendChild(ct);
c.appendChild(mkInput('cron.enabled','Enable Cron','toggle'));
c.appendChild(mkInput('cron.store','Store Path','text',{placeholder:'~/.oclaw/cron.json'}));
c.appendChild(mkInput('cron.maxConcurrentRuns','Max Concurrent Runs','number',{placeholder:'5'}));
c.appendChild(mkInput('cron.webhook','Webhook URL','text',{placeholder:'https://...'}));
c.appendChild(mkInput('cron.webhookToken','Webhook Token','password'));
pg.appendChild(c);
}
// --- Page: Memory ---
function pgMemory(pg){
pgTitle(pg,'memory');
if(!cfg.memory)cfg.memory={};
const c=document.createElement('div');c.className='card';
const ch=document.createElement('div');ch.className='card-header';
const ct=document.createElement('div');ct.className='card-title';ct.textContent=tl('Long-term Memory');c.appendChild(ch);ch.appendChild(ct);
c.appendChild(mkInput('memory.enabled','Enable Memory','toggle'));
c.appendChild(mkInput('memory.provider','Embedding Provider','select',{options:['','openai','anthropic','cohere','ollama']}));
c.appendChild(mkInput('memory.apiKey','API Key','password',{placeholder:'Key for embedding provider'}));
c.appendChild(mkInput('memory.model','Embedding Model','text',{placeholder:'text-embedding-3-small'}));
c.appendChild(mkInput('memory.dbPath','Database Path','text',{placeholder:'~/.oclaw/memory.db'}));
c.appendChild(mkRow([mkInput('memory.vectorWeight','Vector Weight','number',{placeholder:'0.7',step:'0.1'}),mkInput('memory.textWeight','Text Weight','number',{placeholder:'0.3',step:'0.1'})]));
c.appendChild(mkInput('memory.autoIndex','Auto Index','toggle'));
pg.appendChild(c);
}
// --- Page: Logging ---
function pgLogging(pg){
pgTitle(pg,'logging');
if(!cfg.logging)cfg.logging={};
const c=document.createElement('div');c.className='card';
const ch=document.createElement('div');ch.className='card-header';
const ct=document.createElement('div');ct.className='card-title';ct.textContent=tl('Logging');c.appendChild(ch);ch.appendChild(ct);
c.appendChild(mkInput('logging.level','Log Level','select',{options:['trace','debug','info','warn','error']}));
c.appendChild(mkInput('logging.consoleLevel','Console Level','select',{options:['','trace','debug','info','warn','error']}));
c.appendChild(mkInput('logging.consoleStyle','Console Style','select',{options:['','text','json','pretty']}));
c.appendChild(mkInput('logging.file','Log File Path','text',{placeholder:'/var/log/oclaw.log'}));
c.appendChild(mkInput('logging.redactSensitive','Redact Sensitive','select',{options:['','true','false','partial']}));
c.appendChild(mkInput('logging.redactPatterns','Redact Patterns','textarea',{placeholder:'["sk-.*","token-.*"]',arrayMode:true}));
pg.appendChild(c);
}
"##,
    // --- CONFIG_UI script part 4: advanced page + save/load/init ---
    r##"
// --- Page: Advanced ---
function pgAdvanced(pg){
pgTitle(pg,'advanced');
// Diagnostics
if(!cfg.diagnostics)cfg.diagnostics={};
const dc=document.createElement('div');dc.className='card';
const dh=document.createElement('div');dh.className='card-header';
const dt=document.createElement('div');dt.className='card-title';dt.textContent=tl('Diagnostics / OpenTelemetry');dc.appendChild(dh);dh.appendChild(dt);
dc.appendChild(mkInput('diagnostics.enabled','Enable Diagnostics','toggle'));
if(!cfg.diagnostics.otel)cfg.diagnostics.otel={};
dc.appendChild(mkInput('diagnostics.otel.enabled','Enable OTel','toggle'));
dc.appendChild(mkInput('diagnostics.otel.endpoint','OTel Endpoint','text',{placeholder:'http://localhost:4318'}));
dc.appendChild(mkRow([mkInput('diagnostics.otel.traces','Traces','toggle'),mkInput('diagnostics.otel.metrics','Metrics','toggle'),mkInput('diagnostics.otel.logs','Logs','toggle')]));
dc.appendChild(mkInput('diagnostics.otel.serviceName','Service Name','text',{placeholder:'oclaw'}));
dc.appendChild(mkInput('diagnostics.otel.sampleRate','Sample Rate','number',{placeholder:'1.0',step:'0.1'}));
pg.appendChild(dc);
// Talk / Voice
if(!cfg.talk)cfg.talk={};
const tc=document.createElement('div');tc.className='card';
const th=document.createElement('div');th.className='card-header';
const tt=document.createElement('div');tt.className='card-title';tt.textContent=tl('Voice (Talk)');tc.appendChild(th);th.appendChild(tt);
tc.appendChild(mkInput('talk.voiceId','Voice ID','text'));
tc.appendChild(mkInput('talk.modelId','Model ID','text'));
tc.appendChild(mkInput('talk.outputFormat','Output Format','text',{placeholder:'mp3'}));
tc.appendChild(mkInput('talk.apiKey','API Key','password'));
tc.appendChild(mkInput('talk.interruptOnSpeech','Interrupt on Speech','toggle'));
pg.appendChild(tc);
// Web / Reconnect
if(!cfg.web)cfg.web={};
const wc=document.createElement('div');wc.className='card';
const wh=document.createElement('div');wh.className='card-header';
const wt=document.createElement('div');wt.className='card-title';wt.textContent=tl('Web / Reconnect');wc.appendChild(wh);wh.appendChild(wt);
wc.appendChild(mkInput('web.enabled','Enable Web','toggle'));
wc.appendChild(mkInput('web.heartbeatSeconds','Heartbeat (seconds)','number',{placeholder:'30'}));
if(!cfg.web.reconnect)cfg.web.reconnect={};
wc.appendChild(mkRow([mkInput('web.reconnect.initialMs','Initial (ms)','number',{placeholder:'1000'}),mkInput('web.reconnect.maxMs','Max (ms)','number',{placeholder:'60000'})]));
wc.appendChild(mkRow([mkInput('web.reconnect.factor','Factor','number',{placeholder:'2.0',step:'0.1'}),mkInput('web.reconnect.maxAttempts','Max Attempts','number',{placeholder:'10'})]));
pg.appendChild(wc);
// UI
if(!cfg.ui)cfg.ui={};
const uic=document.createElement('div');uic.className='card';
const uih=document.createElement('div');uih.className='card-header';
const uit=document.createElement('div');uit.className='card-title';uit.textContent=tl('UI Settings');uic.appendChild(uih);uih.appendChild(uit);
uic.appendChild(mkInput('ui.seamColor','Seam Color','text',{placeholder:'#00d4ff'}));
if(!cfg.ui.assistant)cfg.ui.assistant={};
uic.appendChild(mkInput('ui.assistant.name','Assistant Name','text',{placeholder:'OpenClaw'}));
uic.appendChild(mkInput('ui.assistant.avatar','Assistant Avatar','text',{placeholder:'URL or emoji'}));
pg.appendChild(uic);
// Discovery
if(!cfg.discovery)cfg.discovery={};
const disc=document.createElement('div');disc.className='card';
const dish=document.createElement('div');dish.className='card-header';
const dist=document.createElement('div');dist.className='card-title';dist.textContent=tl('Service Discovery');disc.appendChild(dish);dish.appendChild(dist);
if(!cfg.discovery.wideArea)cfg.discovery.wideArea={};
if(!cfg.discovery.mdns)cfg.discovery.mdns={};
disc.appendChild(mkInput('discovery.wideArea.enabled','Wide Area Discovery','toggle'));
disc.appendChild(mkInput('discovery.mdns.mode','mDNS Mode','select',{options:['','off','listen','announce']}));
pg.appendChild(disc);
// Plugins
if(!cfg.plugins)cfg.plugins={};
const plc=document.createElement('div');plc.className='card';
const plh=document.createElement('div');plh.className='card-header';
const plt=document.createElement('div');plt.className='card-title';plt.textContent=tl('Plugins');plc.appendChild(plh);plh.appendChild(plt);
plc.appendChild(mkInput('plugins.enabled','Enable Plugins','toggle'));
plc.appendChild(mkInput('plugins.allow','Allow List','textarea',{placeholder:'["plugin-a","plugin-b"]',arrayMode:true}));
plc.appendChild(mkInput('plugins.deny','Deny List','textarea',{placeholder:'["plugin-x"]',arrayMode:true}));
pg.appendChild(plc);
// Update
if(!cfg.update)cfg.update={};
const upc=document.createElement('div');upc.className='card';
const uph=document.createElement('div');uph.className='card-header';
const upt=document.createElement('div');upt.className='card-title';upt.textContent=tl('Update');upc.appendChild(uph);uph.appendChild(upt);
upc.appendChild(mkInput('update.channel','Update Channel','select',{options:['','stable','beta','nightly']}));
upc.appendChild(mkInput('update.checkOnStart','Check on Start','toggle'));
pg.appendChild(upc);
// Canvas Host
if(!cfg.canvasHost)cfg.canvasHost={};
const chc=document.createElement('div');chc.className='card';
const chh=document.createElement('div');chh.className='card-header';
const cht=document.createElement('div');cht.className='card-title';cht.textContent=tl('Canvas Host');chc.appendChild(chh);chh.appendChild(cht);
chc.appendChild(mkInput('canvasHost.enabled','Enable','toggle'));
chc.appendChild(mkInput('canvasHost.root','Root Path','text',{placeholder:'./canvas'}));
chc.appendChild(mkRow([mkInput('canvasHost.port','Port','number',{placeholder:'3000'}),mkInput('canvasHost.liveReload','Live Reload','toggle')]));
pg.appendChild(chc);
// Media
if(!cfg.media)cfg.media={};
const mdc=document.createElement('div');mdc.className='card';
const mdh=document.createElement('div');mdh.className='card-header';
const mdt=document.createElement('div');mdt.className='card-title';mdt.textContent='Media';mdc.appendChild(mdh);mdh.appendChild(mdt);
mdc.appendChild(mkInput('media.preserveFilenames','Preserve Filenames','toggle'));
pg.appendChild(mdc);
// Env
if(!cfg.env)cfg.env={};
const enc=document.createElement('div');enc.className='card';
const enh=document.createElement('div');enh.className='card-header';
const ent=document.createElement('div');ent.className='card-title';ent.textContent=tl('Shell Environment');enc.appendChild(enh);enh.appendChild(ent);
if(!cfg.env.shellEnv)cfg.env.shellEnv={};
enc.appendChild(mkInput('env.shellEnv.enabled','Enable','toggle'));
enc.appendChild(mkInput('env.shellEnv.timeoutMs','Timeout (ms)','number',{placeholder:'5000'}));
pg.appendChild(enc);
}
// --- Collect form values into cfg ---
function collect(){
const out=JSON.parse(JSON.stringify(cfg));
document.querySelectorAll('[data-path]').forEach(el=>{
const p=el.dataset.path;let v;
if(el.dataset.type==='bool'||el.type==='checkbox')v=el.checked;
else if(el.type==='number')v=el.value===''?undefined:Number(el.value);
else if(el.dataset.type==='array'){try{v=JSON.parse(el.value)}catch{v=el.value.split('\n').map(s=>s.trim()).filter(Boolean)}}
else v=el.value===''?undefined:el.value;
if(v!==undefined)sp(out,p,v);
});
// Clean empty objects
function prune(o){if(!o||typeof o!=='object')return o;for(const k of Object.keys(o)){o[k]=prune(o[k]);if(o[k]&&typeof o[k]==='object'&&!Array.isArray(o[k])&&!Object.keys(o[k]).length)delete o[k]}return o}
return prune(out);
}
// --- Save ---
async function save(){
const btn=document.getElementById('save-btn');
btn.disabled=true;btn.textContent=t('saving');
try{
const data=collect();
const r=await fetch('/api/config/full',{method:'PUT',headers:{'Content-Type':'application/json'},body:JSON.stringify(data)});
const j=await r.json();
if(r.ok){cfg=data;toast(t('saved'),true)}
else{toast(t('errPrefix')+(j.errors||[j.error]).join(', '),false)}
}catch(e){toast(t('errPrefix')+e,false)}
btn.disabled=false;btn.textContent=t('save');
}
// --- Export / Import ---
function exportCfg(){
const data=JSON.stringify(collect(),null,2);
const blob=new Blob([data],{type:'application/json'});
const a=document.createElement('a');a.href=URL.createObjectURL(blob);a.download='oclaw-config.json';a.click();
URL.revokeObjectURL(a.href);toast(t('exportOk'),true);
}
function importCfg(e){
const file=e.target.files[0];if(!file)return;
const reader=new FileReader();
reader.onload=ev=>{
try{cfg=JSON.parse(ev.target.result);renderAll();toast(t('importOk'),true)}
catch{toast(t('importErr'),false)}
};
reader.readAsText(file);e.target.value='';
}
// --- Init ---
fetch('/api/config/full').then(r=>r.json()).then(j=>{cfg=j;renderAll();document.getElementById('status-bar').textContent=lang==='zh'?'配置已加载':'Config loaded'}).catch(e=>toast(t('loadErr')+e,false));
</script></body></html>"##,
);

const WEBCHAT_HTML: &str = concat!(
    r##"<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>OpenClaw Chat</title>
<style>
*{box-sizing:border-box;margin:0;padding:0}
body{font-family:system-ui,-apple-system,sans-serif;background:#0f0f1a;color:#e0e0e0;height:100vh;display:flex;flex-direction:column;overflow:hidden}
a{color:#00d4ff}
.header{display:flex;align-items:center;gap:12px;padding:10px 16px;background:#161d30;border-bottom:1px solid #1e2d4a;flex-shrink:0}
.header .dot{width:10px;height:10px;border-radius:50%;background:#555;flex-shrink:0;transition:background .3s}
.header .dot.on{background:#4caf50}
.header .dot.err{background:#f44336}
.header .brand{font-weight:700;color:#00d4ff;font-size:15px;white-space:nowrap}
.header select{background:#1e2d4a;color:#c0c0c0;border:1px solid #2a3a5c;border-radius:6px;padding:4px 8px;font-size:12px;cursor:pointer;max-width:180px}
.header select:focus{outline:none;border-color:#00d4ff}
.header .spacer{flex:1}
.header .status-text{font-size:11px;color:#6b7f99;white-space:nowrap}
.chat-thread{flex:1;overflow-y:auto;padding:16px;display:flex;flex-direction:column;gap:4px}
.chat-thread::-webkit-scrollbar{width:6px}
.chat-thread::-webkit-scrollbar-thumb{background:#2a3a5c;border-radius:3px}
.chat-group{display:flex;gap:12px;max-width:900px;width:100%;margin:0 auto;padding:8px 0}
.chat-group.user{flex-direction:row-reverse}
.chat-avatar{width:34px;height:34px;border-radius:8px;display:flex;align-items:center;justify-content:center;font-size:16px;flex-shrink:0;margin-top:2px}
.chat-group:not(.user) .chat-avatar{background:#1e2d4a;color:#00d4ff}
.chat-group.user .chat-avatar{background:#0f3460;color:#7eb8da}
.chat-messages{display:flex;flex-direction:column;gap:6px;min-width:0;max-width:calc(100% - 50px)}
.chat-text{font-size:14px;line-height:1.6;color:#d0d0d0;word-wrap:break-word;overflow-wrap:break-word}
.chat-group.user .chat-text{color:#b0c4de}
.chat-text p{margin:0 0 8px}
.chat-text p:last-child{margin-bottom:0}
.chat-text strong{color:#e8e8e8;font-weight:600}
.chat-text em{color:#a0b8d0;font-style:italic}
.chat-text code{background:#141825;padding:1px 5px;border-radius:4px;font-family:'SF Mono',Monaco,'Cascadia Code',monospace;font-size:13px;color:#7ee8fa}
.chat-text pre{background:#0d0d18;border:1px solid #1e2d4a;border-radius:8px;padding:12px;margin:8px 0;overflow-x:auto;position:relative}
.chat-text pre code{background:none;padding:0;color:#c8d8e8;font-size:13px;line-height:1.5}
.chat-text blockquote{border-left:3px solid #0f3460;padding:4px 12px;margin:6px 0;color:#8899aa;background:#141825;border-radius:0 6px 6px 0}
.chat-text ul,.chat-text ol{margin:6px 0 6px 20px}
.chat-text li{margin:2px 0}
.chat-text hr{border:none;border-top:1px solid #1e2d4a;margin:10px 0}
.chat-text a{color:#00d4ff;text-decoration:none}
.chat-text a:hover{text-decoration:underline}
.chat-text .copy-btn{position:absolute;top:6px;right:6px;background:#1e2d4a;border:1px solid #2a3a5c;border-radius:4px;color:#6b7f99;cursor:pointer;font-size:11px;padding:2px 8px}
.chat-text .copy-btn:hover{color:#00d4ff;border-color:#00d4ff}
.cursor-blink{display:inline-block;width:2px;height:1em;background:#00d4ff;margin-left:2px;animation:blink 1s step-end infinite;vertical-align:text-bottom}
@keyframes blink{50%{opacity:0}}
.tool-card{background:#141825;border:1px solid #1e2d4a;border-radius:8px;margin:6px 0;overflow:hidden}
.tool-card-header{display:flex;align-items:center;gap:8px;padding:8px 12px;cursor:pointer;font-size:13px;color:#8899aa;user-select:none}
.tool-card-header:hover{background:#1a2240}
.tool-card-icon{font-size:14px}
.tool-card-name{font-weight:600;color:#7eb8da}
.tool-card-preview{color:#6b7f99;font-size:12px;margin-left:auto;max-width:200px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.tool-card-body{display:none;padding:8px 12px;border-top:1px solid #1e2d4a;font-size:12px;font-family:'SF Mono',Monaco,monospace;color:#8899aa;max-height:200px;overflow-y:auto;white-space:pre-wrap;word-break:break-all}
.tool-card.open .tool-card-body{display:block}
.chat-compose{flex-shrink:0;padding:12px 16px;background:linear-gradient(to top,#0f0f1a 60%,transparent);border-top:1px solid #1e2d4a;position:relative}
.compose-wrap{max-width:900px;margin:0 auto;display:flex;gap:8px;align-items:flex-end;position:relative}
.compose-wrap textarea{flex:1;background:#141825;border:1px solid #2a3a5c;border-radius:10px;padding:10px 14px;color:#e0e0e0;font-family:system-ui,sans-serif;font-size:14px;resize:none;min-height:42px;max-height:150px;line-height:1.4;overflow-y:auto}
.compose-wrap textarea:focus{outline:none;border-color:#00d4ff}
.compose-wrap textarea::placeholder{color:#4a5a70}
.send-btn{width:38px;height:38px;border-radius:10px;background:#00d4ff;border:none;color:#0f0f1a;cursor:pointer;display:flex;align-items:center;justify-content:center;font-size:18px;flex-shrink:0;transition:opacity .2s}
.send-btn:hover{opacity:.85}
.send-btn:disabled{opacity:.4;cursor:not-allowed}
.slash-popup{position:absolute;bottom:100%;left:0;right:60px;background:#1e2d4a;border:1px solid #2a3a5c;border-radius:8px;margin-bottom:4px;display:none;max-height:240px;overflow-y:auto;z-index:10}
.slash-popup.show{display:block}
.slash-item{padding:8px 14px;cursor:pointer;font-size:13px;display:flex;gap:10px;align-items:center}
.slash-item:hover,.slash-item.active{background:#253050}
.slash-item .cmd{color:#00d4ff;font-weight:600;min-width:80px}
.slash-item .desc{color:#6b7f99;font-size:12px}
.welcome{text-align:center;color:#4a5a70;margin:auto;padding:40px 20px}
.welcome h2{color:#2a3a5c;font-size:20px;margin-bottom:8px}
.welcome p{font-size:14px}
@media(max-width:600px){.header{padding:8px 10px;gap:8px}.header .brand{font-size:13px}.chat-thread{padding:10px}.compose-wrap textarea{font-size:13px}}
</style></head>
<body>
"##,
    // --- WEBCHAT_HTML body ---
    r##"<div class="header">
  <div class="dot" id="dot"></div>
  <span class="brand">OpenClaw</span>
  <select id="model-sel" title="Model"><option>loading...</option></select>
  <select id="session-sel" title="Session"><option>loading...</option></select>
  <div class="spacer"></div>
  <span class="status-text" id="status-text">Connecting...</span>
</div>
<div class="chat-thread" id="thread">
  <div class="welcome"><h2>OpenClaw Chat</h2><p>Send a message to get started</p></div>
</div>
<div class="chat-compose">
  <div class="compose-wrap">
    <div class="slash-popup" id="slash-popup"></div>
    <textarea id="input" rows="1" placeholder="Message OpenClaw..." autofocus></textarea>
    <button class="send-btn" id="send-btn" title="Send">&#9654;</button>
  </div>
</div>
"##,
    // --- WEBCHAT_HTML script ---
    r##"<script>
const $=s=>document.getElementById(s);
const thread=$('thread'),input=$('input'),sendBtn=$('send-btn'),dot=$('dot'),
      statusText=$('status-text'),modelSel=$('model-sel'),sessionSel=$('session-sel'),
      slashPopup=$('slash-popup');
let ws,sessionId='',currentModel='',messages=[],streaming=false,userScrolled=false,
    reconnectDelay=1000,slashIdx=-1;

const SLASH_CMDS=[
  {cmd:'/help',desc:'Show available commands'},
  {cmd:'/clear',desc:'Clear chat history'},
  {cmd:'/model',desc:'Switch model'},
  {cmd:'/session',desc:'Switch session'},
  {cmd:'/status',desc:'Show connection status'},
  {cmd:'/think',desc:'Enable thinking mode'},
  {cmd:'/verbose',desc:'Toggle verbose output'},
  {cmd:'/abort',desc:'Abort current generation'}
];

// --- WebSocket ---
function connect(){
  const proto=location.protocol==='https:'?'wss:':'ws:';
  ws=new WebSocket(proto+'//'+location.host+'/webchat/ws');
  ws.onopen=()=>{dot.className='dot on';statusText.textContent='Connected';reconnectDelay=1000};
  ws.onclose=()=>{
    dot.className='dot err';statusText.textContent='Disconnected';
    setTimeout(connect,reconnectDelay);reconnectDelay=Math.min(reconnectDelay*1.5,15000);
  };
  ws.onerror=()=>{dot.className='dot err'};
  ws.onmessage=e=>{
    let d;try{d=JSON.parse(e.data)}catch{return}
    handleMsg(d);
  };
}

function wsSend(obj){if(ws&&ws.readyState===1)ws.send(JSON.stringify(obj))}

// --- Message handling ---
function handleMsg(d){
  switch(d.type){
    case 'connected':
      sessionId=d.session||sessionId;currentModel=d.model||'';
      statusText.textContent='Connected - '+currentModel;
      wsSend({type:'models'});wsSend({type:'sessions'});
      break;
    case 'typing':
      streaming=true;sendBtn.disabled=true;
      statusText.textContent='Thinking...';
      break;
    case 'chunk':
      if(d.content){
        let last=messages[messages.length-1];
        if(!last||last.role!=='assistant'||last.done){
          messages.push({role:'assistant',content:d.content,done:false,tools:[]});
        }else{last.content+=d.content}
        renderMessages();
      }
      break;
    case 'tool_call':
      if(messages.length&&messages[messages.length-1].role==='assistant'){
        messages[messages.length-1].tools.push({name:d.name,args:d.args,status:'running',result:null});
        renderMessages();
      }
      break;
    case 'tool_result':
      if(messages.length&&messages[messages.length-1].role==='assistant'){
        let tools=messages[messages.length-1].tools;
        let tc=tools.find(t=>t.name===d.name&&t.status==='running');
        if(tc){tc.status=d.status||'success';tc.result=d.result||''}
        renderMessages();
      }
      break;
    case 'done':
      streaming=false;sendBtn.disabled=false;
      statusText.textContent='Connected - '+currentModel;
      if(d.content){
        let last=messages[messages.length-1];
        if(!last||last.role!=='assistant'){
          messages.push({role:'assistant',content:d.content,done:true,tools:[]});
        }else{last.content=d.content;last.done=true}
      }else if(messages.length&&messages[messages.length-1].role==='assistant'){
        messages[messages.length-1].done=true;
      }
      renderMessages();
      break;
    case 'error':
      streaming=false;sendBtn.disabled=false;
      statusText.textContent='Connected - '+currentModel;
      messages.push({role:'assistant',content:'**Error:** '+(d.content||'Unknown error'),done:true,tools:[]});
      renderMessages();
      break;
    case 'history':
      messages=(d.messages||[]).map(m=>({role:m.role,content:m.content,done:true,tools:[]}));
      renderMessages();
      break;
    case 'sessions':
      renderSessionSelect(d.sessions||[]);
      break;
    case 'models':
      renderModelSelect(d.models||[]);
      break;
  }
}
"##,
    // --- WEBCHAT_HTML markdown renderer ---
    r##"
// --- Markdown renderer ---
function md(text){
  if(!text)return'';
  let h=text;
  // Escape HTML
  h=h.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
  // Code blocks
  h=h.replace(/```(\w*)\n([\s\S]*?)```/g,function(_,lang,code){
    const id='cb'+Math.random().toString(36).slice(2,8);
    return'<pre><code data-lang="'+(lang||'')+'" id="'+id+'">'+code.trim()+'</code><button class="copy-btn" onclick="copyCode(\''+id+'\')">Copy</button></pre>';
  });
  // Inline code
  h=h.replace(/`([^`\n]+)`/g,'<code>$1</code>');
  // Bold
  h=h.replace(/\*\*(.+?)\*\*/g,'<strong>$1</strong>');
  // Italic
  h=h.replace(/\*(.+?)\*/g,'<em>$1</em>');
  // Links
  h=h.replace(/\[([^\]]+)\]\(([^)]+)\)/g,'<a href="$2" target="_blank" rel="noopener">$1</a>');
  // Blockquotes
  h=h.replace(/^&gt;\s?(.*)$/gm,'<blockquote>$1</blockquote>');
  // Merge adjacent blockquotes
  h=h.replace(/<\/blockquote>\n<blockquote>/g,'\n');
  // Horizontal rules
  h=h.replace(/^---$/gm,'<hr>');
  // Unordered lists
  h=h.replace(/^[\-\*]\s+(.*)$/gm,'<li>$1</li>');
  h=h.replace(/((?:<li>.*<\/li>\n?)+)/g,'<ul>$1</ul>');
  // Ordered lists
  h=h.replace(/^\d+\.\s+(.*)$/gm,'<li>$1</li>');
  // Headings
  h=h.replace(/^### (.*)$/gm,'<strong style="font-size:15px">$1</strong>');
  h=h.replace(/^## (.*)$/gm,'<strong style="font-size:16px">$1</strong>');
  h=h.replace(/^# (.*)$/gm,'<strong style="font-size:18px">$1</strong>');
  // Paragraphs
  h=h.replace(/\n{2,}/g,'</p><p>');
  h=h.replace(/\n/g,'<br>');
  h='<p>'+h+'</p>';
  // Clean empty paragraphs
  h=h.replace(/<p>\s*<\/p>/g,'');
  return h;
}
function copyCode(id){
  const el=document.getElementById(id);
  if(el)navigator.clipboard.writeText(el.textContent).catch(()=>{});
}
"##,
    // --- WEBCHAT_HTML UI rendering ---
    r##"
// --- Render messages ---
function renderMessages(){
  const wasAtBottom=!userScrolled;
  thread.innerHTML='';
  if(!messages.length){
    thread.innerHTML='<div class="welcome"><h2>OpenClaw Chat</h2><p>Send a message to get started</p></div>';
    return;
  }
  let lastRole='';
  let group=null;
  let msgsDiv=null;
  messages.forEach((m,i)=>{
    if(m.role!==lastRole){
      group=document.createElement('div');
      group.className='chat-group'+(m.role==='user'?' user':'');
      const av=document.createElement('div');
      av.className='chat-avatar';
      av.textContent=m.role==='user'?'\u{1F464}':'\u{2726}';
      const msgs=document.createElement('div');
      msgs.className='chat-messages';
      group.appendChild(av);group.appendChild(msgs);
      thread.appendChild(group);
      msgsDiv=msgs;lastRole=m.role;
    }
    const txt=document.createElement('div');
    txt.className='chat-text';
    if(m.role==='user'){
      txt.textContent=m.content;
    }else{
      txt.innerHTML=md(m.content);
      if(!m.done){
        const cur=document.createElement('span');
        cur.className='cursor-blink';
        txt.appendChild(cur);
      }
    }
    msgsDiv.appendChild(txt);
    // Tool cards
    if(m.tools&&m.tools.length){
      m.tools.forEach(tc=>{
        msgsDiv.appendChild(makeToolCard(tc));
      });
    }
  });
  if(wasAtBottom)scrollBottom();
}
function scrollBottom(){
  thread.scrollTop=thread.scrollHeight;
}
thread.addEventListener('scroll',()=>{
  const diff=thread.scrollHeight-thread.scrollTop-thread.clientHeight;
  userScrolled=diff>60;
});
"##,
    // --- WEBCHAT_HTML tool cards + selects ---
    r##"
// --- Tool cards ---
function makeToolCard(tc){
  const card=document.createElement('div');
  card.className='tool-card'+(tc.status==='success'||tc.status==='error'?' ':'');
  const hdr=document.createElement('div');
  hdr.className='tool-card-header';
  const icons={pending:'\u25CC',running:'\u27F3',success:'\u2713',error:'\u2717'};
  hdr.innerHTML='<span class="tool-card-icon">'+(icons[tc.status]||'\u25CC')+'</span>'
    +'<span class="tool-card-name">'+esc(tc.name)+'</span>'
    +'<span class="tool-card-preview">'+(tc.result?esc(tc.result).slice(0,60):'')+'</span>';
  hdr.onclick=()=>card.classList.toggle('open');
  const body=document.createElement('div');
  body.className='tool-card-body';
  body.textContent=tc.result||JSON.stringify(tc.args,null,2);
  card.appendChild(hdr);card.appendChild(body);
  return card;
}
function esc(s){return s?s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;'):''}

// --- Select renderers ---
function renderModelSelect(models){
  modelSel.innerHTML='';
  models.forEach(m=>{
    const o=document.createElement('option');o.value=m;o.textContent=m;
    if(m===currentModel)o.selected=true;
    modelSel.appendChild(o);
  });
}
modelSel.onchange=()=>{wsSend({type:'set_model',model:modelSel.value})};

function renderSessionSelect(sessions){
  sessionSel.innerHTML='';
  const cur=document.createElement('option');
  cur.value=sessionId;cur.textContent=sessionId.slice(0,8)+'... (current)';cur.selected=true;
  sessionSel.appendChild(cur);
  sessions.forEach(s=>{
    if(s.id===sessionId)return;
    const o=document.createElement('option');o.value=s.id;
    o.textContent=s.id.slice(0,8)+'... ('+s.messages+' msgs)';
    sessionSel.appendChild(o);
  });
  const nw=document.createElement('option');nw.value='__new__';nw.textContent='+ New session';
  sessionSel.appendChild(nw);
}
sessionSel.onchange=()=>{
  if(sessionSel.value==='__new__'){
    messages=[];renderMessages();
    wsSend({type:'set_session',session:crypto.randomUUID()});
  }else{
    wsSend({type:'set_session',session:sessionSel.value});
    wsSend({type:'history',session:sessionSel.value});
  }
};
"##,
    // --- WEBCHAT_HTML send + slash commands ---
    r##"
// --- Send message ---
function sendMessage(){
  const text=input.value.trim();
  if(!text||streaming)return;
  messages.push({role:'user',content:text,done:true,tools:[]});
  renderMessages();scrollBottom();
  wsSend({type:'message',content:text,session:sessionId});
  input.value='';autoResize();
  hideSlash();
}
sendBtn.onclick=sendMessage;

// --- Slash commands ---
function showSlash(filter){
  const f=filter.toLowerCase();
  const matched=SLASH_CMDS.filter(c=>c.cmd.startsWith(f));
  if(!matched.length){hideSlash();return}
  slashPopup.innerHTML='';
  matched.forEach((c,i)=>{
    const el=document.createElement('div');
    el.className='slash-item'+(i===0?' active':'');
    el.innerHTML='<span class="cmd">'+c.cmd+'</span><span class="desc">'+c.desc+'</span>';
    el.onclick=()=>{applySlash(c.cmd)};
    slashPopup.appendChild(el);
  });
  slashPopup.classList.add('show');
  slashIdx=0;
}
function hideSlash(){slashPopup.classList.remove('show');slashPopup.innerHTML='';slashIdx=-1}
function applySlash(cmd){
  input.value=cmd+' ';hideSlash();input.focus();
}
function slashNav(dir){
  const items=slashPopup.querySelectorAll('.slash-item');
  if(!items.length)return;
  items[slashIdx]?.classList.remove('active');
  slashIdx=(slashIdx+dir+items.length)%items.length;
  items[slashIdx]?.classList.add('active');
  items[slashIdx]?.scrollIntoView({block:'nearest'});
}
"##,
    // --- WEBCHAT_HTML keyboard + init ---
    r##"
// --- Keyboard shortcuts ---
input.addEventListener('keydown',e=>{
  if(slashPopup.classList.contains('show')){
    if(e.key==='ArrowDown'){e.preventDefault();slashNav(1);return}
    if(e.key==='ArrowUp'){e.preventDefault();slashNav(-1);return}
    if(e.key==='Enter'){
      e.preventDefault();
      const active=slashPopup.querySelector('.slash-item.active .cmd');
      if(active)applySlash(active.textContent);
      return;
    }
    if(e.key==='Escape'){hideSlash();return}
  }
  if(e.key==='Enter'&&!e.shiftKey){e.preventDefault();sendMessage();return}
  if(e.key==='Escape'&&streaming){wsSend({type:'abort'});return}
});
input.addEventListener('input',()=>{
  autoResize();
  const v=input.value;
  if(v.startsWith('/')&&!v.includes(' ')){showSlash(v)}
  else{hideSlash()}
});
document.addEventListener('keydown',e=>{
  if(e.ctrlKey&&e.key==='c'&&streaming){wsSend({type:'abort'})}
});

// --- Auto-resize textarea ---
function autoResize(){
  input.style.height='auto';
  input.style.height=Math.min(input.scrollHeight,150)+'px';
}

// --- Init ---
connect();
</script></body></html>"##,
);

// --- Cron REST endpoints ---

pub async fn cron_list_handler(State(state): State<Arc<HttpState>>) -> Response {
    let Some(ref svc) = state.cron_service else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "cron not enabled"})),
        )
            .into_response();
    };
    let jobs = svc.list().await;
    Json(serde_json::json!({ "jobs": jobs })).into_response()
}

pub async fn cron_create_handler(
    State(state): State<Arc<HttpState>>,
    Json(payload): Json<oclaw_cron_core::CronJob>,
) -> Response {
    let Some(ref svc) = state.cron_service else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "cron not enabled"})),
        )
            .into_response();
    };
    match svc.add(payload).await {
        Ok(job) => (StatusCode::CREATED, Json(serde_json::json!(job))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn cron_delete_handler(
    State(state): State<Arc<HttpState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    let Some(ref svc) = state.cron_service else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "cron not enabled"})),
        )
            .into_response();
    };
    match svc.remove(&id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn cron_trigger_handler(
    State(state): State<Arc<HttpState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    let Some(ref svc) = state.cron_service else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "cron not enabled"})),
        )
            .into_response();
    };
    match svc.trigger(&id).await {
        Ok(_) => Json(serde_json::json!({"triggered": true})).into_response(),
        Err(e) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn cron_logs_handler(
    State(state): State<Arc<HttpState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    let Some(ref run_log) = state.cron_run_log else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "cron run log not available"})),
        )
            .into_response();
    };
    match run_log.read(&id, 50).await {
        Ok(entries) => Json(serde_json::json!({"logs": entries})).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn cron_status_handler(State(state): State<Arc<HttpState>>) -> Response {
    let scheduler_running = state
        .cron_scheduler
        .as_ref()
        .map(|s| s.is_running())
        .unwrap_or(false);
    let job_count = match &state.cron_service {
        Some(svc) => svc.list().await.len(),
        None => 0,
    };
    Json(serde_json::json!({
        "scheduler_running": scheduler_running,
        "job_count": job_count,
    }))
    .into_response()
}

pub async fn canvas_ui_handler() -> axum::response::Html<&'static str> {
    axum::response::Html(CANVAS_HTML)
}

const CANVAS_HTML: &str = concat!(
    r##"<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>OpenClaw Canvas</title>
<style>
*{box-sizing:border-box;margin:0;padding:0}
:root{--bg:#0f0f1a;--bg2:#161d30;--accent:#00d4ff;--border:#2a3a5c;--text:#e0e0e0;--text2:#8899aa}
body{font-family:system-ui,sans-serif;background:var(--bg);color:var(--text);height:100vh;display:flex;flex-direction:column}
.topbar{display:flex;align-items:center;gap:12px;padding:10px 20px;background:var(--bg2);border-bottom:1px solid var(--border)}
.topbar .brand{font-weight:700;color:var(--accent);font-size:16px}
.topbar .status{margin-left:auto;font-size:12px;color:var(--text2)}
.topbar .dot{width:8px;height:8px;border-radius:50%;display:inline-block;margin-right:4px}
.dot.on{background:#4caf50}.dot.off{background:#f44336}
.canvas-wrap{flex:1;display:flex;align-items:center;justify-content:center;overflow:hidden;position:relative}
canvas{background:#1a1a2e;border:1px solid var(--border);cursor:crosshair}
.overlay{position:absolute;top:10px;right:10px;background:rgba(22,29,48,.9);border:1px solid var(--border);border-radius:8px;padding:8px 12px;font-size:11px;color:var(--text2)}
</style></head><body>
<div class="topbar"><span class="brand">OpenClaw Canvas</span>
<span class="status"><span class="dot off" id="dot"></span><span id="st">Disconnected</span></span></div>
<div class="canvas-wrap"><canvas id="cv"></canvas>
<div class="overlay" id="info">Ready</div></div>
"##,
    r##"<script>
const cv=document.getElementById('cv'),ctx=cv.getContext('2d'),dot=document.getElementById('dot'),st=document.getElementById('st'),info=document.getElementById('info');
let ws,visible=true;
function resize(){cv.width=window.innerWidth;cv.height=window.innerHeight-44;ctx.fillStyle='#1a1a2e';ctx.fillRect(0,0,cv.width,cv.height)}
resize();window.addEventListener('resize',resize);
function connect(){
const proto=location.protocol==='https:'?'wss':'ws';
ws=new WebSocket(proto+'://'+location.host+'/webchat/ws');
ws.onopen=()=>{dot.className='dot on';st.textContent='Connected';ws.send(JSON.stringify({type:'canvas.hello',width:cv.width,height:cv.height}))};
ws.onclose=()=>{dot.className='dot off';st.textContent='Reconnecting...';setTimeout(connect,2000)};
ws.onerror=()=>ws.close();
ws.onmessage=(e)=>{try{handleCmd(JSON.parse(e.data))}catch(err){console.error(err)}};
}
function handleCmd(msg){
if(!msg.type)return;
const d=msg.data||{};
switch(msg.type){
case'canvas.clear':ctx.fillStyle=d.color||'#1a1a2e';ctx.fillRect(0,0,cv.width,cv.height);info.textContent='Cleared';break;
case'canvas.rect':ctx.fillStyle=d.color||'#00d4ff';ctx.fillRect(d.x||0,d.y||0,d.w||100,d.h||100);break;
case'canvas.circle':ctx.beginPath();ctx.arc(d.x||0,d.y||0,d.r||50,0,Math.PI*2);ctx.fillStyle=d.color||'#00d4ff';ctx.fill();break;
case'canvas.line':ctx.beginPath();ctx.moveTo(d.x1||0,d.y1||0);ctx.lineTo(d.x2||0,d.y2||0);ctx.strokeStyle=d.color||'#00d4ff';ctx.lineWidth=d.width||2;ctx.stroke();break;
case'canvas.text':ctx.font=(d.size||16)+'px system-ui';ctx.fillStyle=d.color||'#e0e0e0';ctx.fillText(d.text||'',d.x||0,d.y||0);break;
case'canvas.image':if(d.src){const img=new Image();img.onload=()=>ctx.drawImage(img,d.x||0,d.y||0,d.w||img.width,d.h||img.height);img.src=d.src}break;
case'canvas.eval':try{const r=eval(d.code);if(ws.readyState===1)ws.send(JSON.stringify({type:'canvas.eval.result',result:String(r)}))}catch(err){if(ws.readyState===1)ws.send(JSON.stringify({type:'canvas.eval.error',error:err.message}))}break;
case'canvas.snapshot':cv.toBlob(b=>{const reader=new FileReader();reader.onload=()=>{if(ws.readyState===1)ws.send(JSON.stringify({type:'canvas.snapshot.result',data:reader.result}))};reader.readAsDataURL(b)},d.format||'image/png',d.quality||0.9);break;
case'canvas.navigate':if(d.url)window.location.href=d.url;break;
case'canvas.present':visible=true;cv.style.display='block';info.textContent='Presenting';break;
case'canvas.hide':visible=false;cv.style.display='none';info.textContent='Hidden';break;
default:console.log('Unknown canvas cmd:',msg.type);
}}
connect();
</script></body></html>"##
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::auth::AuthState;
    use crate::http::{HttpState, health_handler};
    use crate::server::GatewayServer;
    use axum::http::{HeaderMap, HeaderValue, Request, StatusCode};
    use axum::{
        Router,
        routing::{delete, get, post},
    };
    use oclaw_config::settings::Gateway;
    use oclaw_llm_core::providers::MockLlmProvider;
    use tokio::sync::RwLock;
    use tower::ServiceExt;

    fn test_state(
        provider: Option<Arc<dyn oclaw_llm_core::providers::LlmProvider>>,
    ) -> Arc<HttpState> {
        Arc::new(HttpState {
            auth_state: Arc::new(RwLock::new(AuthState::new(None))),
            gateway_server: Arc::new(GatewayServer::new(0)),
            _gateway: Arc::new(Gateway::default()),
            llm_provider: provider,
            hook_pipeline: None,
            channel_manager: None,
            tool_registry: None,
            skill_registry: None,
            approval_gate: None,
            plugin_registrations: None,
            cron_service: None,
            cron_scheduler: None,
            cron_events: None,
            cron_run_log: None,
            memory_manager: None,
            workspace: None,
            metrics: Arc::new(crate::http::metrics::AppMetrics::new()),
            health_checker: Arc::new(oclaw_doctor_core::HealthChecker::new()),
            full_config: None,
            config_path: None,
            echo_tracker: Arc::new(tokio::sync::Mutex::new(
                oclaw_agent_core::EchoTracker::default(),
            )),
            group_activation: oclaw_channel_core::group_gate::GroupActivation::default(),
            dm_scope: crate::session_key::DmScope::default(),
            identity_links: None,
            needs_hatching: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            pipeline_config: Arc::new(crate::pipeline::PipelineConfig::default()),
            flush_tracker: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            session_usage_tokens: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            session_turn_counts: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            session_rate_limiter: Arc::new(oclaw_acp::SessionRateLimiter::default_session_limiter()),
            session_queues: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            session_run_locks: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            auto_capture_config: Arc::new(oclaw_memory_core::AutoCaptureConfig::default()),
            auto_capture_counts: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            skill_overrides: Arc::new(RwLock::new(std::collections::HashMap::new())),
            wizard_sessions: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            tts_runtime: Arc::new(tokio::sync::RwLock::new(Default::default())),
            exec_approvals_snapshot: Arc::new(tokio::sync::RwLock::new(Default::default())),
            node_pairing_store: Arc::new(tokio::sync::Mutex::new(
                oclaw_pairing::PairingStore::default(),
            )),
            node_pairs: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            node_pair_index: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            node_exec_approvals: Arc::new(tokio::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
            node_invocations: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            node_connected: Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::new())),
            agent_runs: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            agent_idempotency: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            agent_idempotency_gates: Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            chat_runs: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            chat_abort_handles: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            chat_dedupe: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            chat_idempotency_gates: Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            send_dedupe: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
            send_idempotency_gates: Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            voicewake_triggers: Arc::new(tokio::sync::RwLock::new(Vec::new())),
            talk_mode: Arc::new(tokio::sync::RwLock::new(Default::default())),
            heartbeats_enabled: Arc::new(tokio::sync::RwLock::new(true)),
            last_heartbeat_event: Arc::new(tokio::sync::RwLock::new(None)),
            usage_snapshot: Arc::new(tokio::sync::RwLock::new(
                crate::http::GatewayUsageSnapshot::default(),
            )),
            event_tx: tokio::sync::broadcast::channel(16).0,
            device_pair_pending: Arc::new(
                tokio::sync::Mutex::new(std::collections::HashMap::new()),
            ),
            device_pair_pending_index: Arc::new(tokio::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            device_paired: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
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
            .route(
                "/webhooks/telegram",
                post(crate::http::webhooks::telegram_webhook),
            )
            .route(
                "/webhooks/slack",
                post(crate::http::webhooks::slack_webhook),
            )
            .route(
                "/webhooks/discord",
                post(crate::http::webhooks::discord_webhook),
            )
            .route(
                "/webhooks/whatsapp",
                post(crate::http::webhooks::whatsapp_webhook),
            )
            .route(
                "/webhooks/{channel}",
                post(crate::http::webhooks::generic_webhook),
            )
            .with_state(state)
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let app = test_router(test_state(None));
        let req = Request::get("/health")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_models_with_provider() {
        let mock = MockLlmProvider::new();
        let state = test_state(Some(Arc::new(mock)));
        let app = test_router(state);
        let req = Request::get("/models")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(
            json["models"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("mock-model"))
        );
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
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["choices"][0]["message"]["content"], "Hello from mock!");
    }

    #[test]
    fn test_chat_completions_session_id_from_user() {
        let headers = HeaderMap::new();
        let sid = resolve_chat_completions_session_id(&headers, "openclaw:main", Some("Alice-01"))
            .unwrap();
        assert!(sid.contains("agent-main-openai-user-alice-01"));
    }

    #[test]
    fn test_chat_completions_session_id_header_overrides_user() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-openclaw-session-key",
            HeaderValue::from_static("my_custom_session"),
        );
        let sid = resolve_chat_completions_session_id(&headers, "openclaw:main", Some("Alice-01"))
            .unwrap();
        assert!(sid.contains("my_custom_session"));
        assert!(!sid.contains("openai-user"));
    }

    #[tokio::test]
    async fn test_chat_completions_replays_transcript_with_stable_session() {
        let mock = Arc::new(MockLlmProvider::new());
        mock.queue_text("first-answer");
        mock.queue_text("second-answer");

        let provider: Arc<dyn oclaw_llm_core::providers::LlmProvider> = mock.clone();
        let state = test_state(Some(provider));
        let app = test_router(state);

        let mut headers = HeaderMap::new();
        let raw_session = format!("test-openai-{}", uuid::Uuid::new_v4().simple());
        headers.insert(
            "x-openclaw-session-key",
            HeaderValue::from_str(&raw_session).unwrap(),
        );
        let session_id = resolve_chat_completions_session_id(&headers, "openclaw", None).unwrap();
        let transcript = oclaw_agent_core::Transcript::new(&session_id);
        let _ = transcript.clear().await;

        let body1 = serde_json::json!({
            "model": "openclaw",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let req1 = Request::post("/v1/chat/completions")
            .header("content-type", "application/json")
            .header("x-openclaw-session-key", raw_session.clone())
            .body(axum::body::Body::from(serde_json::to_vec(&body1).unwrap()))
            .unwrap();
        let resp1 = app.clone().oneshot(req1).await.unwrap();
        assert_eq!(resp1.status(), StatusCode::OK);

        let body2 = serde_json::json!({
            "model": "openclaw",
            "messages": [{"role": "user", "content": "what next?"}]
        });
        let req2 = Request::post("/v1/chat/completions")
            .header("content-type", "application/json")
            .header("x-openclaw-session-key", raw_session)
            .body(axum::body::Body::from(serde_json::to_vec(&body2).unwrap()))
            .unwrap();
        let resp2 = app.oneshot(req2).await.unwrap();
        assert_eq!(resp2.status(), StatusCode::OK);

        let calls = mock.recorded_calls();
        assert!(calls.len() >= 2);
        let second = &calls[1];
        assert!(second.messages.iter().any(|m| {
            m.role == oclaw_llm_core::chat::MessageRole::Assistant && m.content == "first-answer"
        }));
        assert!(
            second
                .messages
                .iter()
                .any(|m| m.role == oclaw_llm_core::chat::MessageRole::User && m.content == "hi")
        );

        let _ = transcript.clear().await;
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
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["output"][0]["content"][0]["text"], "Response text");
    }

    #[tokio::test]
    async fn test_agent_status() {
        let state = test_state(Some(Arc::new(MockLlmProvider::new())));
        let app = test_router(state);
        let req = Request::get("/agent/status")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["status"], "ready");
    }

    #[tokio::test]
    async fn test_sessions_list_empty() {
        let app = test_router(test_state(None));
        let req = Request::get("/sessions")
            .body(axum::body::Body::empty())
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_session_delete_not_found() {
        let app = test_router(test_state(None));
        let req = Request::delete("/sessions/nonexistent")
            .body(axum::body::Body::empty())
            .unwrap();
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
        let body =
            serde_json::json!({"type": "url_verification", "challenge": "test_challenge_123"});
        let req = Request::post("/webhooks/slack")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["challenge"], "test_challenge_123");
    }

    #[tokio::test]
    async fn test_generic_webhook_unknown_channel() {
        let state = {
            let mut s = (*test_state(None)).clone();
            s.channel_manager = Some(Arc::new(RwLock::new(
                oclaw_channel_core::ChannelManager::new(),
            )));
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
