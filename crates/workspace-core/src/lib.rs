//! Workspace Core — agent workspace file management
//!
//! Manages the on-disk workspace directory where agents store their
//! personality (SOUL.md), identity (IDENTITY.md), heartbeat checklist
//! (HEARTBEAT.md), and memory files (MEMORY.md + memory/*.md).

pub mod files;
pub mod identity;
pub mod soul;
pub mod heartbeat;
pub mod bootstrap;
pub mod system_prompt;
pub mod memory_flush;

pub use files::Workspace;
pub use identity::AgentIdentity;
pub use soul::Soul;
pub use heartbeat::HeartbeatFile;
pub use bootstrap::{BootstrapRunner, BootstrapStatus, HatchingPhase};
pub use system_prompt::SystemPromptBuilder;
pub use memory_flush::MemoryFlushConfig;
