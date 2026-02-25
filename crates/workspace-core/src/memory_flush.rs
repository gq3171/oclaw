//! Memory flush — pre-compaction memory preservation to workspace files.
//!
//! Before a session is compacted (old turns removed), the agent gets a chance
//! to write durable memories to `memory/YYYY-MM-DD.md` files. This ensures
//! key facts survive across session boundaries.

use serde::{Deserialize, Serialize};

/// Token the agent replies with when there's nothing to store.
pub const SILENT_REPLY_TOKEN: &str = "HEARTBEAT_OK";

/// Default soft threshold in tokens before triggering a memory flush.
pub const DEFAULT_SOFT_THRESHOLD_TOKENS: u64 = 4000;

/// Default reserve floor — minimum tokens to keep available.
pub const DEFAULT_RESERVE_TOKENS_FLOOR: u64 = 2000;

/// Configuration for the memory flush mechanism.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFlushConfig {
    /// Whether memory flush is enabled (default: true).
    pub enabled: bool,
    /// Token threshold before triggering flush.
    pub soft_threshold_tokens: u64,
    /// Minimum reserve tokens.
    pub reserve_tokens_floor: u64,
    /// Custom flush prompt (None = use default).
    pub prompt: Option<String>,
}

impl Default for MemoryFlushConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            soft_threshold_tokens: DEFAULT_SOFT_THRESHOLD_TOKENS,
            reserve_tokens_floor: DEFAULT_RESERVE_TOKENS_FLOOR,
            prompt: None,
        }
    }
}
