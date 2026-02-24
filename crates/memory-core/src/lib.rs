pub mod types;
pub mod embeddings;
pub mod store;
pub mod search;
pub mod manager;
pub mod watcher;

pub use types::*;
pub use embeddings::{EmbeddingProvider, create_embedding_provider};
pub use store::MemoryStore;
pub use search::{HybridSearchConfig, hybrid_search};
pub use manager::MemoryManager;
