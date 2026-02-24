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
use tracing::{warn, error};
use oclaws_agent_core::Transcript;

use crate::http::HttpState;
use crate::http::agent_bridge::{self, ToolRegistryExecutor};

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
            let llm_msg = oclaws_llm_core::chat::ChatMessage {
                role: match message.role.as_str() {
                    "assistant" => oclaws_llm_core::chat::MessageRole::Assistant,
                    "system" => oclaws_llm_core::chat::MessageRole::System,
                    _ => oclaws_llm_core::chat::MessageRole::User,
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
    session: Option<String>,
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

async fn handle_socket(socket: WebSocket, state: Arc<HttpState>) {
    let (mut sender, mut receiver) = socket.split();
    let session_id = Uuid::new_v4().to_string();
    let webchat = WebChatState::new();

    // Determine initial model name
    let model_name = state.llm_provider.as_ref()
        .map(|p| p.default_model().to_string())
        .unwrap_or_else(|| "none".to_string());
    let current_model = Arc::new(RwLock::new(model_name.clone()));
    let current_session = Arc::new(RwLock::new(session_id.clone()));

    // Send connected message
    let _ = send_json(&mut sender, &serde_json::json!({
        "type": "connected",
        "session": session_id,
        "model": model_name,
    })).await;

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
                            session: None,
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
                            &current_session,
                            &current_model,
                            incoming.content.unwrap_or_default(),
                            &abort_flag,
                        ).await;
                    }
                    "abort" => {
                        abort_flag.notify_one();
                    }
                    "history" => {
                        let sid = incoming.session
                            .unwrap_or_else(|| current_session.blocking_read().clone());
                        let history = webchat.get_history(&sid).await;
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
                    "sessions" => {
                        let sessions = webchat.list_sessions().await;
                        let _ = send_json(&mut sender, &serde_json::json!({
                            "type": "sessions", "sessions": sessions,
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
                                "session": *current_session.read().await,
                                "model": m,
                            })).await;
                        }
                    }
                    "set_session" => {
                        if let Some(sid) = incoming.session {
                            *current_session.write().await = sid.clone();
                            let _ = send_json(&mut sender, &serde_json::json!({
                                "type": "connected",
                                "session": sid,
                                "model": *current_model.read().await,
                            })).await;
                        }
                    }
                    "clear" => {
                        let sid = incoming.session
                            .unwrap_or_else(|| current_session.blocking_read().clone());
                        webchat.clear_history(&sid).await;
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
    current_session: &Arc<RwLock<String>>,
    current_model: &Arc<RwLock<String>>,
    content: String,
    abort_flag: &Arc<tokio::sync::Notify>,
) {
    let session_id = current_session.read().await.clone();

    // Store user message
    webchat.add_message(&session_id, ChatMessage {
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

    // Run agent with abort race
    let reply_fut = async {
        if let Some(ref executor) = tool_executor {
            let sid = format!("webchat_{}", session_id);
            agent_bridge::agent_reply_with_session(provider, executor, &content, Some(&sid)).await
        } else {
            // No tools — direct LLM call
            let request = oclaws_llm_core::chat::ChatRequest {
                model: current_model.read().await.clone(),
                messages: vec![oclaws_llm_core::chat::ChatMessage {
                    role: oclaws_llm_core::chat::MessageRole::User,
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
            webchat.add_message(&session_id, ChatMessage {
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
