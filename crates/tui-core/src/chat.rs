//! Chat message types for the TUI.

/// A single chat message.
#[derive(Debug, Clone)]
pub enum ChatMessage {
    User {
        text: String,
        timestamp: u64,
    },
    Assistant {
        text: String,
        model: String,
        streaming: bool,
        timestamp: u64,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: String,
        result: Option<String>,
        status: ToolStatus,
    },
    System {
        text: String,
        level: SystemLevel,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolStatus {
    Pending,
    Running,
    Success,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemLevel {
    Info,
    Warning,
    Error,
}
