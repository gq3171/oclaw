//! TUI Core - Terminal User Interface for OpenClaw
//! 
//! Provides interactive terminal UI components using ratatui.

pub mod app;
pub mod widgets;
pub mod screens;
pub mod input;
pub mod state;

pub use app::{TuiApp, TuiConfig};
pub use widgets::{Widget, WidgetId, WidgetRenderer, StreamingDisplay, ToolCallPanel, ToolCallEntry, ToolCallStatus};
pub use screens::{Screen, ScreenId, ScreenManager};
pub use input::{InputHandler, InputMode, KeyBinding};
pub use state::AppStateManager;

pub type TuiResult<T> = Result<T, TuiError>;

/// Application events
#[derive(Debug, Clone)]
pub enum AppEvent {
    // Navigation
    NextScreen,
    PreviousScreen,
    GotoScreen(ScreenId),
    NextItem,
    PreviousItem,
    
    // Actions
    Select,
    Cancel,
    Submit,
    Quit,
    Refresh,
    
    // Input
    InsertChar(char),
    Command(String),
    Search,
    
    // UI
    ToggleHelp,
    
    // State updates
    UpdateState(String, StateValue),

    // Streaming
    StreamChunk(String),
    StreamComplete,

    // Tool calls
    ToolCallStart { id: String, name: String, arguments: String },
    ToolCallResult { id: String, result: String },
    ToolCallError { id: String, error: String },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum StateValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Object(std::collections::HashMap<String, StateValue>),
    Array(Vec<StateValue>),
    Null,
}

#[derive(Debug, thiserror::Error)]
pub enum TuiError {
    #[error("Initialization error: {0}")]
    InitError(String),
    
    #[error("Render error: {0}")]
    RenderError(String),
    
    #[error("Input error: {0}")]
    InputError(String),
    
    #[error("Screen error: {0}")]
    ScreenError(String),
    
    #[error("Widget error: {0}")]
    WidgetError(String),
    
    #[error("State error: {0}")]
    StateError(String),
}

impl serde::Serialize for TuiError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
