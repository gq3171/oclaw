//! Protocol translator — converts between ACP messages and internal formats.

use crate::types::{AcpMessage, AcpRole, AcpToolCall, AcpToolResult};

/// Translate an external chat completion message into ACP format.
pub fn from_chat_completion(role: &str, content: &str) -> AcpMessage {
    let role = match role {
        "assistant" => AcpRole::Assistant,
        "system" => AcpRole::System,
        "tool" => AcpRole::Tool,
        _ => AcpRole::User,
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    AcpMessage {
        role,
        content: content.to_string(),
        timestamp: now,
    }
}

/// Build a tool call request in ACP format.
pub fn build_tool_call(name: &str, arguments: serde_json::Value) -> AcpToolCall {
    AcpToolCall {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.to_string(),
        arguments,
    }
}

/// Build a tool result in ACP format.
pub fn build_tool_result(call_id: &str, output: &str, is_error: bool) -> AcpToolResult {
    AcpToolResult {
        call_id: call_id.to_string(),
        output: output.to_string(),
        is_error,
    }
}
