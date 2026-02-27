//! TUI Core - Lightweight terminal interface for OpenClaw

pub mod app;
pub mod chat;
pub mod commands;
pub mod gateway;
pub mod render;
pub mod theme;

pub use app::{TuiApp, TuiConfig};
pub use chat::{ChatMessage, SystemLevel, ToolStatus};
pub use commands::{SlashCommand, parse_command};
pub use gateway::{GatewayClient, GatewayConfig, GatewayEvent};
