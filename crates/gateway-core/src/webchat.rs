use axum::{
    extract::{State, WebSocketUpgrade, Path},
    extract::ws::{WebSocket, Message},
    response::IntoResponse,
    routing::{get, delete},
    Json,
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

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
}

impl WebChatState {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(Vec::new())),
            max_history: 100,
        }
    }

    pub async fn add_message(&self, session_id: &str, message: ChatMessage) {
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
}

pub fn create_webchat_router(state: Arc<WebChatState>) -> Router {
    Router::new()
        .route("/ws", get(websocket_handler))
        .route("/history/:session_id", get(history_handler))
        .route("/history/:session_id", delete(clear_history_handler))
        .with_state(state)
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<WebChatState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: Arc<WebChatState>) {
    let (mut sender, mut receiver) = socket.split();
    let session_id = Uuid::new_v4().to_string();
    
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                let chat_msg = ChatMessage {
                    id: Uuid::new_v4().to_string(),
                    role: "user".to_string(),
                    content: text.to_string(),
                    timestamp: chrono::Utc::now().timestamp(),
                    metadata: None,
                };
                
                state.add_message(&session_id, chat_msg).await;
                
                if let Ok(response) = serde_json::to_string(&serde_json::json!({
                    "type": "message",
                    "content": "Echo: ".to_string() + &text,
                })) {
                    let _ = sender.send(Message::Text(response.into())).await;
                }
            }
            Ok(Message::Close(_)) => {
                break;
            }
            Err(_) => {
                break;
            }
            _ => {}
        }
    }
}

async fn history_handler(
    Path(session_id): Path<String>,
    State(state): State<Arc<WebChatState>>,
) -> (StatusCode, Json<serde_json::Value>) {
    if let Some(history) = state.get_history(&session_id).await {
        (StatusCode::OK, Json(serde_json::to_value(history).unwrap_or_default()))
    } else {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({
            "error": "Session not found"
        })))
    }
}

async fn clear_history_handler(
    Path(session_id): Path<String>,
    State(state): State<Arc<WebChatState>>,
) -> impl IntoResponse {
    state.clear_history(&session_id).await;
    (StatusCode::NO_CONTENT, ())
}

use axum::http::StatusCode;

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
}
