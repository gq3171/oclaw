//! Memory flush — pre-compaction memory preservation to workspace files.
//!
//! Before a session is compacted (old turns removed), the agent gets a chance
//! to write durable memories to `memory/YYYY-MM-DD.md` files. This ensures
//! key facts survive across session boundaries.
//!
//! Aligned with Node OpenClaw's shouldRunMemoryFlush logic:
//!   threshold = contextWindow - reserveTokens - softThreshold
//!   Trigger when: totalTokens >= threshold AND not yet flushed this compaction round.

use serde::{Deserialize, Serialize};

/// Token the agent replies with when there's nothing to store.
/// Distinct from HEARTBEAT_OK_TOKEN to avoid confusion.
pub const SILENT_REPLY_TOKEN: &str = "MEMORY_FLUSH_OK";

/// Default soft threshold in tokens before triggering a memory flush.
/// Aligned with Node's DEFAULT_MEMORY_FLUSH_SOFT_TOKENS = 4000.
pub const DEFAULT_SOFT_THRESHOLD_TOKENS: u64 = 4000;

/// Default reserve floor — minimum tokens to keep available.
pub const DEFAULT_RESERVE_TOKENS_FLOOR: u64 = 2000;

/// Configuration for the memory flush mechanism.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryFlushConfig {
    /// Whether memory flush is enabled (default: true).
    pub enabled: bool,
    /// Soft threshold: flush when (contextWindow - reserveFloor - softThreshold) tokens used.
    pub soft_threshold_tokens: u64,
    /// Minimum reserve tokens — never consume below this floor.
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

/// Determine whether a memory flush should run.
///
/// Mirrors Node's shouldRunMemoryFlush:
///   trigger when totalTokens >= (contextWindow - reserveFloor - softThreshold)
///   AND the current compaction round hasn't been flushed yet.
///
/// # Arguments
/// * `total_tokens` — current token usage in the session
/// * `context_window` — model context window size
/// * `config` — flush configuration
/// * `last_flush_at_compaction` — compaction_count at which flush last ran (u64::MAX = never)
/// * `current_compaction_count` — current compaction round counter
pub fn should_run_memory_flush(
    total_tokens: u64,
    context_window: u64,
    config: &MemoryFlushConfig,
    last_flush_at_compaction: u64,
    current_compaction_count: u64,
) -> bool {
    if !config.enabled {
        return false;
    }
    let threshold = context_window
        .saturating_sub(config.reserve_tokens_floor)
        .saturating_sub(config.soft_threshold_tokens);
    // Only flush once per compaction round
    total_tokens >= threshold && last_flush_at_compaction != current_compaction_count
}

/// Build the default memory flush system prompt referencing SILENT_REPLY_TOKEN.
pub fn default_flush_prompt() -> String {
    format!(
        "Pre-compaction memory flush.\n\n\
         Store durable memories now using the workspace tool. \
         Write to memory/<today's date YYYY-MM-DD>.md.\n\
         IMPORTANT: If the file already exists, use action \"append\" to add new content \
         -- do NOT overwrite existing entries.\n\n\
         What to store:\n\
         - Key facts the user shared (preferences, names, projects, decisions)\n\
         - Important context that would be useful in future sessions\n\
         - Anything you'd want to remember if you woke up fresh tomorrow\n\n\
         If there is nothing worth storing, reply with exactly: {token}",
        token = SILENT_REPLY_TOKEN
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triggers_when_over_threshold() {
        let cfg = MemoryFlushConfig::default();
        // contextWindow=128000, reserve=2000, soft=4000 → threshold=122000
        assert!(should_run_memory_flush(122_000, 128_000, &cfg, u64::MAX, 0));
    }

    #[test]
    fn does_not_trigger_when_under_threshold() {
        let cfg = MemoryFlushConfig::default();
        assert!(!should_run_memory_flush(
            100_000,
            128_000,
            &cfg,
            u64::MAX,
            0
        ));
    }

    #[test]
    fn does_not_trigger_twice_same_compaction() {
        let cfg = MemoryFlushConfig::default();
        // Already flushed at compaction round 1
        assert!(!should_run_memory_flush(122_000, 128_000, &cfg, 1, 1));
    }

    #[test]
    fn triggers_after_new_compaction() {
        let cfg = MemoryFlushConfig::default();
        // Flushed at round 0, now on round 1
        assert!(should_run_memory_flush(122_000, 128_000, &cfg, 0, 1));
    }

    #[test]
    fn disabled_never_triggers() {
        let cfg = MemoryFlushConfig {
            enabled: false,
            ..Default::default()
        };
        assert!(!should_run_memory_flush(
            200_000,
            128_000,
            &cfg,
            u64::MAX,
            0
        ));
    }

    #[test]
    fn silent_reply_token_is_distinct() {
        assert_ne!(SILENT_REPLY_TOKEN, "HEARTBEAT_OK");
        assert_eq!(SILENT_REPLY_TOKEN, "MEMORY_FLUSH_OK");
    }
}
