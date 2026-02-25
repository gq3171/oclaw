//! TUI Core - Terminal User Interface for OpenClaw

pub mod app;
pub mod chat;
pub mod commands;
pub mod editor;
pub mod gateway;
pub mod input;
pub mod overlay;
pub mod theme;

pub use app::{TuiApp, TuiConfig};
pub use chat::{ChatLog, ChatMessage, ToolStatus, SystemLevel, visual_lines_for_message};
pub use commands::{SlashCommand, parse_command};
pub use editor::TextEditor;
pub use gateway::{GatewayClient, GatewayConfig, GatewayEvent};
pub use overlay::{SelectList, SelectItem};
pub use theme::Theme;
