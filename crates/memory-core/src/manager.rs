use crate::embeddings::EmbeddingProvider;
use crate::search::{hybrid_search, HybridSearchConfig};
use crate::store::MemoryStore;
use crate::types::{MemoryChunk, MemorySearchResult};
use anyhow::Result;
use std::path::Path;
use tracing::{info, warn};

pub struct MemoryManager {
    store: MemoryStore,
    provider: Option<Box<dyn EmbeddingProvider>>,
    search_config: HybridSearchConfig,
    chunk_size: usize,
    chunk_overlap: usize,
}

impl MemoryManager {
    pub fn new(store: MemoryStore, provider: Option<Box<dyn EmbeddingProvider>>) -> Self {
        Self {
            store,
            provider,
            search_config: HybridSearchConfig::default(),
            chunk_size: 60,
            chunk_overlap: 10,
        }
    }

    pub fn with_search_config(mut self, config: HybridSearchConfig) -> Self {
        self.search_config = config;
        self
    }

    pub fn init(&self) -> Result<()> {
        self.store.init()
    }

    pub async fn index_file(&self, path: &Path) -> Result<usize> {
        let content = tokio::fs::read_to_string(path).await?;
        let path_str = path.display().to_string();
        let lines: Vec<&str> = content.lines().collect();

        // Remove old chunks for this path
        self.store.delete_by_path(&path_str)?;

        let chunks = self.chunk_lines(&lines, &path_str, "file");
        let count = chunks.len();

        // Embed if provider available
        let chunks = if let Some(ref provider) = self.provider {
            self.embed_chunks(chunks, provider.as_ref()).await?
        } else {
            chunks
        };

        for chunk in &chunks {
            self.store.upsert(chunk)?;
        }

        info!("Indexed {} chunks from {}", count, path_str);
        Ok(count)
    }

    pub async fn index_directory(&self, dir: &Path) -> Result<usize> {
        let mut total = 0;
        let mut entries = tokio::fs::read_dir(dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.is_file() && is_indexable(&path) {
                match self.index_file(&path).await {
                    Ok(n) => total += n,
                    Err(e) => warn!("Failed to index {:?}: {}", path, e),
                }
            } else if path.is_dir() {
                match Box::pin(self.index_directory(&path)).await {
                    Ok(n) => total += n,
                    Err(e) => warn!("Failed to index dir {:?}: {}", path, e),
                }
            }
        }
        Ok(total)
    }

    pub async fn search(&self, query: &str) -> Result<Vec<MemorySearchResult>> {
        if let Some(ref provider) = self.provider {
            hybrid_search(&self.store, provider.as_ref(), query, &self.search_config).await
        } else {
            // FTS-only fallback
            self.store.fts_search(query, self.search_config.limit)
        }
    }

    pub async fn add_memory(&self, content: &str, source: &str) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let embedding = if let Some(ref provider) = self.provider {
            let texts = vec![content.to_string()];
            let mut embs = provider.embed(&texts).await?;
            embs.pop()
        } else {
            None
        };

        let chunk = MemoryChunk {
            id: id.clone(),
            path: String::new(),
            start_line: 0,
            end_line: 0,
            content: content.to_string(),
            source: source.to_string(),
            embedding,
            updated_at_ms: now,
        };
        self.store.upsert(&chunk)?;
        Ok(id)
    }

    pub fn get_memory(&self, id: &str) -> Result<Option<MemoryChunk>> {
        self.store.get(id)
    }

    pub fn count(&self) -> Result<usize> {
        self.store.count()
    }

    fn chunk_lines(&self, lines: &[&str], path: &str, source: &str) -> Vec<MemoryChunk> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut chunks = Vec::new();
        let total = lines.len();
        if total == 0 {
            return chunks;
        }

        // Clamp overlap to at most chunk_size - 1 to guarantee forward progress
        let safe_overlap = self.chunk_overlap.min(self.chunk_size.saturating_sub(1));

        let mut start = 0;
        while start < total {
            let end = (start + self.chunk_size).min(total);
            let content: String = lines[start..end].join("\n");
            let id = format!("{}:{}:{}", path, start + 1, end);

            chunks.push(MemoryChunk {
                id,
                path: path.to_string(),
                start_line: start + 1,
                end_line: end,
                content,
                source: source.to_string(),
                embedding: None,
                updated_at_ms: now,
            });

            if end >= total {
                break;
            }
            start = end.saturating_sub(safe_overlap);
        }
        chunks
    }

    async fn embed_chunks(
        &self,
        mut chunks: Vec<MemoryChunk>,
        provider: &dyn EmbeddingProvider,
    ) -> Result<Vec<MemoryChunk>> {
        let texts: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
        if texts.is_empty() {
            return Ok(chunks);
        }

        // Batch in groups of 64 to avoid API limits
        let batch_size = 64;
        let mut all_embeddings: Vec<Vec<f32>> = Vec::with_capacity(texts.len());

        for batch in texts.chunks(batch_size) {
            let embs = provider.embed(batch).await?;
            if embs.len() != batch.len() {
                anyhow::bail!(
                    "Embedding provider returned {} vectors for {} texts",
                    embs.len(),
                    batch.len()
                );
            }
            all_embeddings.extend(embs);
        }

        for (chunk, emb) in chunks.iter_mut().zip(all_embeddings) {
            chunk.embedding = Some(emb);
        }
        Ok(chunks)
    }
}

fn is_indexable(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    matches!(
        ext,
        "rs" | "py" | "js" | "ts" | "tsx" | "jsx" | "go" | "java"
            | "c" | "cpp" | "h" | "hpp" | "md" | "txt" | "toml"
            | "yaml" | "yml" | "json" | "html" | "css" | "sql"
            | "sh" | "rb" | "php" | "swift" | "kt" | "scala"
    )
}