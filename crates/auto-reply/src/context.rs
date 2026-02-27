//! Unified message context for the auto-reply pipeline.

use serde::{Deserialize, Serialize};

/// Chat type classification.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatType {
    #[default]
    Direct,
    Group,
    Channel,
    Thread,
}

/// Raw inbound message context from a channel webhook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MsgContext {
    pub body: String,
    #[serde(default)]
    pub raw_body: Option<String>,
    pub from: String,
    #[serde(default)]
    pub from_name: Option<String>,
    pub to: String,
    pub provider: String,
    #[serde(default)]
    pub surface: Option<String>,
    #[serde(default)]
    pub chat_type: ChatType,
    pub session_key: String,
    #[serde(default)]
    pub message_id: Option<String>,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub was_mentioned: bool,
    #[serde(default)]
    pub media_paths: Vec<String>,
    #[serde(default)]
    pub timestamp_ms: u64,
    #[serde(default)]
    pub raw: serde_json::Value,
}

/// Finalized context after normalization — ready for agent processing.
#[derive(Debug, Clone)]
pub struct FinalizedMsgContext {
    pub ctx: MsgContext,
    /// Normalized body sent to the agent.
    pub body_for_agent: String,
    /// Body used for command parsing (before normalization).
    pub body_for_commands: String,
    /// Human-readable conversation label.
    pub conversation_label: String,
    /// Whether the sender is authorized to run commands.
    pub command_authorized: bool,
    /// Display label for the sender.
    pub sender_label: String,
}

/// Normalize an inbound MsgContext into a FinalizedMsgContext.
pub fn finalize_inbound_context(ctx: MsgContext) -> FinalizedMsgContext {
    // Normalize CRLF → LF
    let body_normalized = ctx.body.replace("\r\n", "\n");

    let body_for_commands = ctx
        .raw_body
        .clone()
        .unwrap_or_else(|| body_normalized.clone());

    let sender_label = ctx.from_name.clone().unwrap_or_else(|| ctx.from.clone());

    let conversation_label = match ctx.chat_type {
        ChatType::Direct => format!("DM with {}", sender_label),
        ChatType::Group | ChatType::Channel => {
            format!("Group {} ({})", ctx.to, sender_label)
        }
        ChatType::Thread => {
            format!(
                "Thread {} ({})",
                ctx.thread_id.as_deref().unwrap_or(&ctx.to),
                sender_label
            )
        }
    };

    FinalizedMsgContext {
        body_for_agent: body_normalized,
        body_for_commands,
        conversation_label,
        command_authorized: ctx.chat_type == ChatType::Direct,
        sender_label,
        ctx,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_ctx(body: &str, chat_type: ChatType) -> MsgContext {
        MsgContext {
            body: body.to_string(),
            raw_body: None,
            from: "user1".into(),
            from_name: Some("Alice".into()),
            to: "bot".into(),
            provider: "telegram".into(),
            surface: None,
            chat_type,
            session_key: "telegram:direct:user1".into(),
            message_id: None,
            thread_id: None,
            was_mentioned: false,
            media_paths: vec![],
            timestamp_ms: 0,
            raw: serde_json::Value::Null,
        }
    }

    #[test]
    fn finalize_normalizes_crlf() {
        let ctx = make_ctx("hello\r\nworld", ChatType::Direct);
        let fin = finalize_inbound_context(ctx);
        assert_eq!(fin.body_for_agent, "hello\nworld");
    }

    #[test]
    fn finalize_dm_authorized() {
        let ctx = make_ctx("hi", ChatType::Direct);
        let fin = finalize_inbound_context(ctx);
        assert!(fin.command_authorized);
        assert!(fin.conversation_label.contains("DM"));
    }

    #[test]
    fn finalize_group_not_authorized() {
        let ctx = make_ctx("hi", ChatType::Group);
        let fin = finalize_inbound_context(ctx);
        assert!(!fin.command_authorized);
    }
}
