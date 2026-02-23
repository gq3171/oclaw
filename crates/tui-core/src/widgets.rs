//! TUI Widgets - Simplified

/// Widget identifier
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct WidgetId(pub String);

/// Widget renderer container
pub struct WidgetRenderer {
    // Placeholder
}

impl WidgetRenderer {
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for WidgetRenderer {
    fn default() -> Self {
        Self::new()
    }
}

/// Base widget trait - Placeholder
pub trait Widget {}

/// List widget - Placeholder
pub struct ListWidget;

impl ListWidget {
    pub fn new(_items: Vec<String>) -> Self {
        Self
    }
}

impl Widget for ListWidget {}

/// Input widget - Placeholder
pub struct InputWidget;

impl InputWidget {
    pub fn new() -> Self {
        Self
    }
}

impl Widget for InputWidget {}

impl Default for InputWidget {
    fn default() -> Self { Self::new() }
}

/// Status bar widget - Placeholder
pub struct StatusBar;

impl StatusBar {
    pub fn new() -> Self {
        Self
    }
}

impl Widget for StatusBar {}

impl Default for StatusBar {
    fn default() -> Self { Self::new() }
}

/// Streaming text display — accumulates chunks and exposes the buffer for rendering.
pub struct StreamingDisplay {
    buffer: String,
    complete: bool,
    model: String,
}

impl StreamingDisplay {
    pub fn new(model: &str) -> Self {
        Self { buffer: String::new(), complete: false, model: model.to_string() }
    }

    pub fn push_chunk(&mut self, chunk: &str) {
        self.buffer.push_str(chunk);
    }

    pub fn mark_complete(&mut self) {
        self.complete = true;
    }

    pub fn text(&self) -> &str { &self.buffer }
    pub fn is_complete(&self) -> bool { self.complete }
    pub fn model(&self) -> &str { &self.model }
    pub fn clear(&mut self) { self.buffer.clear(); self.complete = false; }
}

impl Widget for StreamingDisplay {}

/// Status of a single tool call for visualization.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCallStatus {
    Pending,
    Running,
    Success,
    Error(String),
}

/// Represents one tool invocation in the UI.
#[derive(Debug, Clone)]
pub struct ToolCallEntry {
    pub id: String,
    pub name: String,
    pub arguments: String,
    pub status: ToolCallStatus,
    pub result: Option<String>,
}

/// Widget that tracks and displays tool calls during an agent turn.
pub struct ToolCallPanel {
    calls: Vec<ToolCallEntry>,
}

impl ToolCallPanel {
    pub fn new() -> Self { Self { calls: Vec::new() } }

    pub fn add_call(&mut self, id: &str, name: &str, arguments: &str) {
        self.calls.push(ToolCallEntry {
            id: id.to_string(),
            name: name.to_string(),
            arguments: arguments.to_string(),
            status: ToolCallStatus::Pending,
            result: None,
        });
    }

    pub fn set_running(&mut self, id: &str) {
        if let Some(c) = self.calls.iter_mut().find(|c| c.id == id) {
            c.status = ToolCallStatus::Running;
        }
    }

    pub fn set_result(&mut self, id: &str, result: &str) {
        if let Some(c) = self.calls.iter_mut().find(|c| c.id == id) {
            c.status = ToolCallStatus::Success;
            c.result = Some(result.to_string());
        }
    }

    pub fn set_error(&mut self, id: &str, err: &str) {
        if let Some(c) = self.calls.iter_mut().find(|c| c.id == id) {
            c.status = ToolCallStatus::Error(err.to_string());
            c.result = None;
        }
    }

    pub fn calls(&self) -> &[ToolCallEntry] { &self.calls }
    pub fn clear(&mut self) { self.calls.clear(); }
}

impl Default for ToolCallPanel {
    fn default() -> Self { Self::new() }
}

impl Widget for ToolCallPanel {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streaming_display() {
        let mut sd = StreamingDisplay::new("gpt-4");
        sd.push_chunk("Hello ");
        sd.push_chunk("world");
        assert_eq!(sd.text(), "Hello world");
        assert!(!sd.is_complete());
        sd.mark_complete();
        assert!(sd.is_complete());
        assert_eq!(sd.model(), "gpt-4");
    }

    #[test]
    fn test_streaming_display_clear() {
        let mut sd = StreamingDisplay::new("m");
        sd.push_chunk("data");
        sd.mark_complete();
        sd.clear();
        assert_eq!(sd.text(), "");
        assert!(!sd.is_complete());
    }

    #[test]
    fn test_tool_call_panel_lifecycle() {
        let mut panel = ToolCallPanel::new();
        panel.add_call("t1", "bash", "{\"cmd\":\"ls\"}");
        assert_eq!(panel.calls().len(), 1);
        assert_eq!(panel.calls()[0].status, ToolCallStatus::Pending);

        panel.set_running("t1");
        assert_eq!(panel.calls()[0].status, ToolCallStatus::Running);

        panel.set_result("t1", "file.txt");
        assert_eq!(panel.calls()[0].status, ToolCallStatus::Success);
        assert_eq!(panel.calls()[0].result.as_deref(), Some("file.txt"));
    }

    #[test]
    fn test_tool_call_panel_error() {
        let mut panel = ToolCallPanel::new();
        panel.add_call("t1", "bash", "{}");
        panel.set_error("t1", "timeout");
        assert_eq!(panel.calls()[0].status, ToolCallStatus::Error("timeout".into()));
        assert!(panel.calls()[0].result.is_none());
    }
}
