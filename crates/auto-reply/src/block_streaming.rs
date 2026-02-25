//! Block streaming coalescing configuration.

/// How chunks are split for streaming delivery.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ChunkMode {
    /// Split by character count.
    #[default]
    Length,
    /// Split on paragraph/newline boundaries.
    Newline,
}

/// Configuration for coalescing streamed blocks before delivery.
#[derive(Debug, Clone)]
pub struct CoalescingConfig {
    /// Minimum characters before flushing a block.
    pub min_chars: usize,
    /// Maximum characters per block.
    pub max_chars: usize,
    /// Idle timeout in ms — flush if no new data arrives.
    pub idle_ms: u64,
    /// Joiner between coalesced chunks.
    pub joiner: String,
    /// Chunk splitting mode.
    pub chunk_mode: ChunkMode,
}

impl Default for CoalescingConfig {
    fn default() -> Self {
        Self {
            min_chars: 1500,
            max_chars: 4000,
            idle_ms: 1000,
            joiner: "\n".into(),
            chunk_mode: ChunkMode::default(),
        }
    }
}
