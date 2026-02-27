//! Workspace Core — agent workspace file management
//!
//! Manages the on-disk workspace directory where agents store their
//! personality (SOUL.md), identity (IDENTITY.md), heartbeat checklist
//! (HEARTBEAT.md), and memory files (MEMORY.md + memory/*.md).

pub mod bootstrap;
pub mod evolution;
pub mod files;
pub mod heartbeat;
pub mod identity;
pub mod memory_flush;
pub mod soul;
pub mod system_prompt;

pub use bootstrap::{BootstrapRunner, BootstrapStatus, HatchingPhase};
pub use evolution::{EVOLUTION_OK_TOKEN, EvolutionConfig, EvolutionState, should_run_evolution};
pub use files::Workspace;
pub use heartbeat::HeartbeatFile;
pub use identity::AgentIdentity;
pub use memory_flush::MemoryFlushConfig;
pub use soul::Soul;
pub use system_prompt::SystemPromptBuilder;
