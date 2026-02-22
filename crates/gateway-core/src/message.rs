use crate::{GatewayError, GatewayResult};
use oclaws_protocol::frames::{EventFrame, GatewayFrame, RequestFrame, ResponseFrame};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

pub struct MessageHandler;

impl MessageHandler {
    pub fn new_request(method: &str, params: Option<serde_json::Value>) -> RequestFrame {
        RequestFrame {
            frame_type: oclaws_protocol::frames::RequestFrameType::Req,
            id: Uuid::new_v4().to_string(),
            method: method.to_string(),
            params,
        }
    }

    pub fn new_response(
        id: &str,
        ok: bool,
        payload: Option<serde_json::Value>,
        error: Option<oclaws_protocol::frames::ErrorDetails>,
    ) -> ResponseFrame {
        ResponseFrame {
            frame_type: oclaws_protocol::frames::ResponseFrameType::Res,
            id: id.to_string(),
            ok,
            payload,
            error,
        }
    }

    pub fn new_event(event: &str, payload: Option<serde_json::Value>) -> EventFrame {
        EventFrame {
            frame_type: oclaws_protocol::frames::EventFrameType::Event,
            event: event.to_string(),
            payload,
            seq: None,
            state_version: None,
        }
    }

    pub fn parse_frame(data: &[u8]) -> GatewayResult<GatewayFrame> {
        serde_json::from_slice(data).map_err(|e| GatewayError::InvalidFrame(e.to_string()))
    }

    pub fn serialize_frame(frame: &GatewayFrame) -> GatewayResult<Vec<u8>> {
        serde_json::to_vec(frame).map_err(|e| GatewayError::InvalidFrame(e.to_string()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub key: String,
    pub agent_id: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: i64,
}

pub struct SessionManager {
    sessions: HashMap<String, SessionInfo>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
        }
    }

    pub fn create_session(&mut self, key: &str, agent_id: &str) -> SessionInfo {
        let now = chrono::Utc::now().timestamp_millis();
        let session = SessionInfo {
            key: key.to_string(),
            agent_id: agent_id.to_string(),
            created_at: now,
            updated_at: now,
            message_count: 0,
        };
        self.sessions.insert(key.to_string(), session.clone());
        session
    }

    pub fn get_session(&self, key: &str) -> Option<&SessionInfo> {
        self.sessions.get(key)
    }

    pub fn list_sessions(&self) -> Vec<&SessionInfo> {
        self.sessions.values().collect()
    }

    pub fn remove_session(&mut self, key: &str) -> Option<SessionInfo> {
        self.sessions.remove(key)
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}
