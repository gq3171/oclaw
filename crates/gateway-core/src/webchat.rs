use axum::{
    extract::{State, WebSocketUpgrade},
    extract::ws::{WebSocket, Message},
    response::IntoResponse,
    routing::get,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;
use tracing::{info, warn, error};
use oclaw_agent_core::Transcript;

use crate::http::HttpState;
use crate::http::agent_bridge::{self, ToolRegistryExecutor};
use crate::pipeline;

// ── Data types ──────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub role: String,
    pub content: String,
    pub timestamp: i64,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatHistory {
    pub session_id: String,
    pub messages: Vec<ChatMessage>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct WebChatState {
    pub sessions: Arc<RwLock<Vec<ChatHistory>>>,
    pub max_history: usize,
    pub transcripts: Arc<RwLock<HashMap<String, Transcript>>>,
}

impl Default for WebChatState {
    fn default() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(Vec::new())),
            max_history: 100,
            transcripts: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl WebChatState {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn add_message(&self, session_id: &str, message: ChatMessage) {
        // Persist to transcript
        {
            let mut transcripts = self.transcripts.write().await;
            let transcript = transcripts
                .entry(session_id.to_string())
                .or_insert_with(|| Transcript::new(session_id));
            let llm_msg = oclaw_llm_core::chat::ChatMessage {
                role: match message.role.as_str() {
                    "assistant" => oclaw_llm_core::chat::MessageRole::Assistant,
                    "system" => oclaw_llm_core::chat::MessageRole::System,
                    _ => oclaw_llm_core::chat::MessageRole::User,
                },
                content: message.content.clone(),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            };
            if let Err(e) = transcript.append(&llm_msg).await {
                warn!("Failed to persist webchat message: {}", e);
            }
        }

        let mut sessions = self.sessions.write().await;

        if let Some(session) = sessions.iter_mut().find(|s| s.session_id == session_id) {
            session.messages.push(message);
            session.updated_at = chrono::Utc::now().timestamp();

            if session.messages.len() > self.max_history {
                session.messages.remove(0);
            }
        } else {
            sessions.push(ChatHistory {
                session_id: session_id.to_string(),
                messages: vec![message],
                created_at: chrono::Utc::now().timestamp(),
                updated_at: chrono::Utc::now().timestamp(),
            });
        }
    }

    pub async fn get_history(&self, session_id: &str) -> Option<ChatHistory> {
        let sessions = self.sessions.read().await;
        sessions.iter().find(|s| s.session_id == session_id).cloned()
    }

    pub async fn clear_history(&self, session_id: &str) {
        let mut sessions = self.sessions.write().await;
        sessions.retain(|s| s.session_id != session_id);
    }

    pub async fn list_sessions(&self) -> Vec<serde_json::Value> {
        let sessions = self.sessions.read().await;
        sessions.iter().map(|s| {
            serde_json::json!({
                "id": s.session_id,
                "messages": s.messages.len(),
                "created_at": s.created_at,
                "updated_at": s.updated_at,
            })
        }).collect()
    }
}

// ── WebSocket message types ─────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct WsIncoming {
    #[serde(rename = "type")]
    msg_type: String,
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

// ── Router ──────────────────────────────────────────────────────────

pub fn create_webchat_router(state: Arc<HttpState>) -> Router {
    Router::new()
        .route("/ws", get(websocket_handler))
        .with_state(state)
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<HttpState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

// ── Socket handler ──────────────────────────────────────────────────

/// Fixed session ID — webchat is a personal assistant, no session switching needed.
const WEBCHAT_SESSION: &str = "default";

async fn handle_socket(socket: WebSocket, state: Arc<HttpState>) {
    let (mut sender, mut receiver) = socket.split();
    let webchat = WebChatState::new();

    // Determine initial model name
    let model_name = state.llm_provider.as_ref()
        .map(|p| p.default_model().to_string())
        .unwrap_or_else(|| "none".to_string());
    let current_model = Arc::new(RwLock::new(model_name.clone()));

    // Send connected message
    let _ = send_json(&mut sender, &serde_json::json!({
        "type": "connected",
        "session": WEBCHAT_SESSION,
        "model": model_name,
    })).await;

    // Load transcript history and send to client
    load_and_send_history(&mut sender, WEBCHAT_SESSION).await;

    // Cancellation flag for abort
    let abort_flag = Arc::new(tokio::sync::Notify::new());

    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                let incoming: WsIncoming = match serde_json::from_str(&text) {
                    Ok(v) => v,
                    Err(_) => {
                        // Treat plain text as a message for backwards compat
                        WsIncoming {
                            msg_type: "message".to_string(),
                            content: Some(text.to_string()),
                            model: None,
                        }
                    }
                };

                match incoming.msg_type.as_str() {
                    "message" => {
                        handle_user_message(
                            &mut sender,
                            &state,
                            &webchat,
                            &current_model,
                            incoming.content.unwrap_or_default(),
                            &abort_flag,
                        ).await;
                    }
                    "abort" => {
                        abort_flag.notify_one();
                    }
                    "history" => {
                        let history = webchat.get_history(WEBCHAT_SESSION).await;
                        let messages = history.map(|h| {
                            h.messages.iter().map(|m| serde_json::json!({
                                "id": m.id, "role": m.role,
                                "content": m.content, "timestamp": m.timestamp,
                            })).collect::<Vec<_>>()
                        }).unwrap_or_default();
                        let _ = send_json(&mut sender, &serde_json::json!({
                            "type": "history", "messages": messages,
                        })).await;
                    }
                    "models" => {
                        let models = state.llm_provider.as_ref()
                            .map(|p| p.supported_models())
                            .unwrap_or_default();
                        let _ = send_json(&mut sender, &serde_json::json!({
                            "type": "models", "models": models,
                        })).await;
                    }
                    "set_model" => {
                        if let Some(m) = incoming.model {
                            *current_model.write().await = m.clone();
                            let _ = send_json(&mut sender, &serde_json::json!({
                                "type": "connected",
                                "session": WEBCHAT_SESSION,
                                "model": m,
                            })).await;
                        }
                    }
                    "clear" => {
                        webchat.clear_history(WEBCHAT_SESSION).await;
                        let _ = send_json(&mut sender, &serde_json::json!({
                            "type": "history", "messages": [],
                        })).await;
                    }
                    _ => {}
                }
            }
            Ok(Message::Close(_)) => break,
            Err(_) => break,
            _ => {}
        }
    }
}

// ── Message handling ────────────────────────────────────────────────

async fn handle_user_message(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    state: &Arc<HttpState>,
    webchat: &WebChatState,
    current_model: &Arc<RwLock<String>>,
    content: String,
    abort_flag: &Arc<tokio::sync::Notify>,
) {
    let session_id = WEBCHAT_SESSION;

    // Store user message
    webchat.add_message(session_id, ChatMessage {
        id: Uuid::new_v4().to_string(),
        role: "user".to_string(),
        content: content.clone(),
        timestamp: chrono::Utc::now().timestamp(),
        metadata: None,
    }).await;

    // Send typing indicator
    let _ = send_json(sender, &serde_json::json!({"type": "typing"})).await;

    // Check for LLM provider
    let Some(provider) = &state.llm_provider else {
        let _ = send_json(sender, &serde_json::json!({
            "type": "error",
            "content": "No LLM provider configured",
        })).await;
        return;
    };

    // Build tool executor
    let tool_executor = match &state.tool_registry {
        Some(reg) => {
            let mut exec = ToolRegistryExecutor::new(reg.clone());
            if let Some(regs) = &state.plugin_registrations {
                exec = exec.with_plugin_registrations(regs.clone());
            }
            Some(exec)
        }
        None => None,
    };

    // Build dynamic system prompt from workspace (SOUL.md, IDENTITY.md, etc.)
    let system_prompt = build_system_prompt(state, provider).await;
    let sid = format!("webchat_{}", session_id);

    // Run agent with abort race
    let reply_fut = async {
        if let Some(ref executor) = tool_executor {
            agent_bridge::agent_reply_with_prompt(
                provider, executor, &content, Some(&sid), &system_prompt,
            ).await
        } else {
            // No tools — direct LLM call
            let request = oclaw_llm_core::chat::ChatRequest {
                model: current_model.read().await.clone(),
                messages: vec![oclaw_llm_core::chat::ChatMessage {
                    role: oclaw_llm_core::chat::MessageRole::User,
                    content: content.clone(),
                    name: None, tool_calls: None, tool_call_id: None,
                }],
                temperature: None, top_p: None, max_tokens: None,
                stop: None, tools: None, tool_choice: None,
                stream: None, response_format: None,
            };
            match provider.chat(request).await {
                Ok(c) => Ok(c.choices.first()
                    .map(|ch| ch.message.content.clone())
                    .unwrap_or_default()),
                Err(e) => Err(e.to_string()),
            }
        }
    };

    let result = tokio::select! {
        res = reply_fut => res,
        _ = abort_flag.notified() => Err("Aborted by user".to_string()),
    };

    match result {
        Ok(reply) => {
            // Store assistant message
            webchat.add_message(session_id, ChatMessage {
                id: Uuid::new_v4().to_string(),
                role: "assistant".to_string(),
                content: reply.clone(),
                timestamp: chrono::Utc::now().timestamp(),
                metadata: None,
            }).await;

            let _ = send_json(sender, &serde_json::json!({
                "type": "done",
                "content": reply,
            })).await;

            // Memory flush — write durable memories to workspace files
            if state.workspace.is_some() && state.tool_registry.is_some() {
                let content_clone = content.clone();
                let reply_clone = reply.clone();
                let state_clone = state.clone();
                let sid_clone = sid.clone();
                tokio::spawn(async move {
                    pipeline::try_memory_flush(
                        state_clone.llm_provider.as_ref().unwrap(),
                        &state_clone,
                        &sid_clone,
                        0,
                        0,
                    ).await;
                    let _ = (&content_clone, &reply_clone); // consumed for side-effects
                });
            }
        }
        Err(e) => {
            error!("Webchat LLM error: {}", e);
            let _ = send_json(sender, &serde_json::json!({
                "type": "error",
                "content": e,
            })).await;
        }
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Load transcript history for the given session and send recent messages to the client.
async fn load_and_send_history(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    session_id: &str,
) {
    let transcript_key = format!("webchat_{}", session_id);
    let transcript = Transcript::new(&transcript_key);
    if !transcript.exists().await {
        return;
    }

    let messages = transcript.load().await;
    if messages.is_empty() {
        return;
    }

    // Send last 50 messages as history
    let recent: Vec<_> = messages.iter().rev().take(50).rev().collect();
    let history: Vec<serde_json::Value> = recent.iter().map(|m| {
        serde_json::json!({
            "role": match m.role {
                oclaw_llm_core::chat::MessageRole::Assistant => "assistant",
                oclaw_llm_core::chat::MessageRole::System => "system",
                _ => "user",
            },
            "content": m.content,
        })
    }).collect();

    if !history.is_empty() {
        info!("[webchat] loaded {} history messages for session {}", history.len(), session_id);
        let _ = send_json(sender, &serde_json::json!({
            "type": "history",
            "messages": history,
        })).await;
    }
}

/// Build dynamic system prompt from workspace files (SOUL.md, IDENTITY.md, etc.),
/// matching the channel pipeline behavior.
async fn build_system_prompt(
    state: &Arc<HttpState>,
    provider: &Arc<dyn oclaw_llm_core::providers::LlmProvider>,
) -> String {
    use oclaw_workspace_core::system_prompt::{self, RuntimeInfo};

    let model = provider.default_model().to_string();
    let tool_names: Vec<String> = state.tool_registry.as_ref()
        .map(|r| r.list_for_llm().iter()
            .filter_map(|v| v["name"].as_str().map(|s| s.to_string()))
            .collect())
        .unwrap_or_default();

    let runtime = RuntimeInfo {
        agent_id: Some("webchat-agent".to_string()),
        model: Some(model.clone()),
        default_model: Some(model),
        os: Some(std::env::consts::OS.to_string()),
        arch: Some(std::env::consts::ARCH.to_string()),
        host: std::env::var("HOSTNAME").or_else(|_| std::env::var("COMPUTERNAME")).ok(),
        shell: std::env::var("SHELL").ok(),
        channel: Some("webchat".to_string()),
        workspace_dir: state.workspace.as_ref().map(|ws| ws.root().to_string_lossy().to_string()),
        version: Some(env!("CARGO_PKG_VERSION").to_string()),
    };

    // Check hatching mode
    let is_hatching = state.needs_hatching.load(std::sync::atomic::Ordering::Relaxed);
    if is_hatching {
        return oclaw_workspace_core::bootstrap::BootstrapRunner::hatching_system_prompt().to_string();
    }

    // Load from workspace
    if let Some(ref ws) = state.workspace
        && let Ok(prompt) = system_prompt::load_and_build_with_runtime(
            ws, None, false, Some(runtime), &tool_names,
        ).await
    {
        return prompt;
    }

    // Fallback
    format!(
        "You are a helpful assistant with tools: {}. Respond in the user's language.",
        tool_names.join(", ")
    )
}

async fn send_json(
    sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    value: &serde_json::Value,
) -> Result<(), axum::Error> {
    if let Ok(json) = serde_json::to_string(value) {
        sender.send(Message::Text(json.into())).await
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_webchat_state() {
        let state = WebChatState::new();

        let msg = ChatMessage {
            id: "1".to_string(),
            role: "user".to_string(),
            content: "Hello".to_string(),
            timestamp: 1000,
            metadata: None,
        };

        state.add_message("session-1", msg).await;

        let history = state.get_history("session-1").await;
        assert!(history.is_some());
        assert_eq!(history.unwrap().messages.len(), 1);
    }

    #[tokio::test]
    async fn test_clear_history() {
        let state = WebChatState::new();

        let msg = ChatMessage {
            id: "1".to_string(),
            role: "user".to_string(),
            content: "Hello".to_string(),
            timestamp: 1000,
            metadata: None,
        };

        state.add_message("session-1", msg).await;
        state.clear_history("session-1").await;

        let history = state.get_history("session-1").await;
        assert!(history.is_none());
    }

    #[tokio::test]
    async fn test_list_sessions() {
        let state = WebChatState::new();

        state.add_message("s1", ChatMessage {
            id: "1".to_string(),
            role: "user".to_string(),
            content: "Hello".to_string(),
            timestamp: 1000,
            metadata: None,
        }).await;

        let sessions = state.list_sessions().await;
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["id"], "s1");
    }
}
