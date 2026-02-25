use std::collections::VecDeque;
use unicode_width::UnicodeWidthStr;

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

/// Scrollable chat message log with capacity limit and wrap-aware scrolling.
pub struct ChatLog {
    messages: VecDeque<ChatMessage>,
    max_messages: usize,
    /// Scroll position from top (in visual lines after wrapping).
    scroll_top: usize,
    /// Last known viewport height (visual lines).
    viewport_height: usize,
    /// Last known viewport width (columns) for wrap calculation.
    viewport_width: usize,
    /// Cached total visual line count.
    total_visual_lines: usize,
    /// Whether we're pinned to the bottom (auto-scroll on new messages).
    pinned_to_bottom: bool,
}

impl ChatLog {
    pub fn new(max_messages: usize) -> Self {
        Self {
            messages: VecDeque::new(),
            max_messages,
            scroll_top: 0,
            viewport_height: 20,
            viewport_width: 80,
            total_visual_lines: 0,
            pinned_to_bottom: true,
        }
    }

    pub fn push(&mut self, msg: ChatMessage) {
        self.messages.push_back(msg);
        while self.messages.len() > self.max_messages {
            self.messages.pop_front();
        }
        self.recompute_visual_lines();
        // Auto-scroll to bottom on new message if pinned
        if self.pinned_to_bottom {
            self.scroll_to_bottom();
        }
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
        self.scroll_top = 0;
        self.total_visual_lines = 0;
        self.pinned_to_bottom = true;
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

    /// Current scroll position from top (in visual lines).
    pub fn scroll_top(&self) -> usize {
        self.scroll_top
    }

    /// Whether the view is pinned to the bottom.
    pub fn is_pinned(&self) -> bool {
        self.pinned_to_bottom
    }

    /// Total visual lines (after wrapping).
    pub fn total_visual_lines(&self) -> usize {
        self.total_visual_lines
    }

    pub fn viewport_height(&self) -> usize {
        self.viewport_height
    }

    /// Update viewport dimensions; recompute visual lines if width changed.
    pub fn set_viewport(&mut self, width: usize, height: usize) {
        let width_changed = self.viewport_width != width;
        self.viewport_width = width.max(1);
        self.viewport_height = height.max(1);
        if width_changed {
            self.recompute_visual_lines();
        }
        // Re-clamp scroll after viewport resize
        if self.pinned_to_bottom {
            self.scroll_to_bottom();
        } else {
            self.clamp_scroll();
        }
    }

    /// Scroll up by N visual lines.
    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_top = self.scroll_top.saturating_sub(lines);
        self.pinned_to_bottom = false;
    }

    /// Scroll down by N visual lines.
    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_top = self.scroll_top.saturating_add(lines);
        self.clamp_scroll();
        // Re-pin if we've scrolled to the very bottom
        let max_scroll = self.max_scroll_top();
        if self.scroll_top >= max_scroll {
            self.pinned_to_bottom = true;
        }
    }

    /// Jump to the very bottom.
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_top = self.max_scroll_top();
        self.pinned_to_bottom = true;
    }

    /// Jump to the very top.
    pub fn scroll_to_top(&mut self) {
        self.scroll_top = 0;
        self.pinned_to_bottom = false;
    }

    /// Maximum valid scroll_top value.
    fn max_scroll_top(&self) -> usize {
        self.total_visual_lines.saturating_sub(self.viewport_height)
    }

    fn clamp_scroll(&mut self) {
        let max = self.max_scroll_top();
        if self.scroll_top > max {
            self.scroll_top = max;
        }
    }

    /// Recompute total visual lines from all messages.
    fn recompute_visual_lines(&mut self) {
        self.total_visual_lines = 0;
        for msg in &self.messages {
            self.total_visual_lines += visual_lines_for_message(msg, self.viewport_width);
        }
    }
}

impl Default for ChatLog {
    fn default() -> Self {
        Self::new(500)
    }
}

/// Calculate how many visual lines a message occupies given a viewport width.
/// Accounts for the prefix (e.g. "You: ", "AI: ") and text wrapping.
pub fn visual_lines_for_message(msg: &ChatMessage, width: usize) -> usize {
    let width = width.max(1);
    match msg {
        ChatMessage::User { text, .. } => {
            // "HH:MM You: " prefix ~ 12 chars + text
            wrapped_line_count(text, width, 12)
        }
        ChatMessage::Assistant { text, streaming, .. } => {
            // "HH:MM AI: " prefix ~ 10 chars + text + possible cursor
            let extra = if *streaming { 2 } else { 0 };
            wrapped_line_count(text, width, 10 + extra)
        }
        ChatMessage::ToolCall { name: _, expanded, arguments, result, .. } => {
            // Tool header line: "  ✓ tool_name"
            let mut lines = 1;
            if *expanded {
                // Arguments + result lines
                lines += wrapped_line_count(arguments, width, 4);
                if let Some(r) = result {
                    lines += wrapped_line_count(r, width, 4);
                }
            }
            lines
        }
        ChatMessage::System { text, .. } => {
            wrapped_line_count(text, width, 2)
        }
    }
}

/// Count how many visual lines a text string occupies, given a viewport width
/// and a prefix width (characters consumed by the label before the text starts).
fn wrapped_line_count(text: &str, viewport_width: usize, prefix_width: usize) -> usize {
    if text.is_empty() {
        return 1;
    }
    let mut total = 0;
    for line in text.split('\n') {
        let display_width = UnicodeWidthStr::width(line);
        let first_line_avail = viewport_width.saturating_sub(prefix_width);
        if display_width <= first_line_avail {
            total += 1;
        } else {
            // First line uses remaining space after prefix
            let remaining = display_width.saturating_sub(first_line_avail);
            // Subsequent lines use full width
            let continuation_lines = remaining.div_ceil(viewport_width.max(1));
            total += 1 + continuation_lines;
        }
    }
    total.max(1)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
