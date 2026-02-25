use std::collections::VecDeque;

/// A single chat message in the log.
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
        expanded: bool,
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

/// Scrollable chat message log with capacity limit.
pub struct ChatLog {
    messages: VecDeque<ChatMessage>,
    max_messages: usize,
    scroll_offset: usize,
}

impl ChatLog {
    pub fn new(max_messages: usize) -> Self {
        Self {
            messages: VecDeque::new(),
            max_messages,
            scroll_offset: 0,
        }
    }

    pub fn push(&mut self, msg: ChatMessage) {
        self.messages.push_back(msg);
        while self.messages.len() > self.max_messages {
            self.messages.pop_front();
        }
        // Auto-scroll to bottom on new message
        self.scroll_to_bottom();
    }

    pub fn add_user(&mut self, text: &str) {
        let ts = now_ms();
        self.push(ChatMessage::User {
            text: text.to_string(),
            timestamp: ts,
        });
    }

    pub fn start_assistant(&mut self, model: &str) {
        let ts = now_ms();
        self.push(ChatMessage::Assistant {
            text: String::new(),
            model: model.to_string(),
            streaming: true,
            timestamp: ts,
        });
    }

    pub fn append_assistant_chunk(&mut self, chunk: &str) {
        if let Some(ChatMessage::Assistant { text, streaming, .. }) = self.messages.back_mut()
            && *streaming
        {
            text.push_str(chunk);
        }
    }

    pub fn finish_assistant(&mut self) {
        if let Some(ChatMessage::Assistant { streaming, .. }) = self.messages.back_mut() {
            *streaming = false;
        }
    }

    pub fn start_tool(&mut self, id: &str, name: &str, arguments: &str) {
        self.push(ChatMessage::ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: arguments.to_string(),
            result: None,
            status: ToolStatus::Running,
            expanded: false,
        });
    }

    pub fn finish_tool(&mut self, id: &str, result: &str) {
        for msg in self.messages.iter_mut().rev() {
            if let ChatMessage::ToolCall { id: tid, result: r, status, .. } = msg
                && tid == id
            {
                *r = Some(result.to_string());
                *status = ToolStatus::Success;
                break;
            }
        }
    }

    pub fn fail_tool(&mut self, id: &str, error: &str) {
        for msg in self.messages.iter_mut().rev() {
            if let ChatMessage::ToolCall { id: tid, status, .. } = msg
                && tid == id
            {
                *status = ToolStatus::Error(error.to_string());
                break;
            }
        }
    }

    pub fn toggle_tool_expand(&mut self, id: &str) {
        for msg in self.messages.iter_mut().rev() {
            if let ChatMessage::ToolCall { id: tid, expanded, .. } = msg
                && tid == id
            {
                *expanded = !*expanded;
                break;
            }
        }
    }

    pub fn add_system(&mut self, text: &str, level: SystemLevel) {
        self.push(ChatMessage::System {
            text: text.to_string(),
            level,
        });
    }

    pub fn clear(&mut self) {
        self.messages.clear();
        self.scroll_offset = 0;
    }

    pub fn messages(&self) -> &VecDeque<ChatMessage> {
        &self.messages
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(lines);
    }

    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = 0;
    }
}

impl Default for ChatLog {
    fn default() -> Self {
        Self::new(200)
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
