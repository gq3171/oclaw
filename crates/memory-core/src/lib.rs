pub mod auto_capture;
pub mod embedding_cache;
pub mod embeddings;
pub mod manager;
pub mod mmr;
pub mod query_expand;
pub mod search;
pub mod store;
pub mod types;
pub mod watcher;

pub use auto_capture::{
    AutoCaptureConfig, CaptureCategory, CapturedMemory, filter_by_config, should_capture,
};
pub use embedding_cache::EmbeddingCache;
pub use embeddings::{EmbeddingProvider, create_embedding_provider};
pub use manager::MemoryManager;
pub use mmr::{MmrConfig, mmr_rerank};
pub use query_expand::{bm25_rank_to_score, build_fts5_query, extract_keywords, has_cjk};
pub use search::{HybridSearchConfig, hybrid_search};
pub use store::MemoryStore;
pub use types::*;
