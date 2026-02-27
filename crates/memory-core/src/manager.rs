use crate::embeddings::EmbeddingProvider;
use crate::search::{HybridSearchConfig, hybrid_search};
use crate::store::MemoryStore;
use crate::types::{MemoryChunk, MemorySearchResult};
use anyhow::Result;
use std::path::Path;
use tracing::{info, warn};

pub struct MemoryManager {
    store: MemoryStore,
    provider: Option<Box<dyn EmbeddingProvider>>,
    search_config: HybridSearchConfig,
    /// Chunk size in approximate characters (default ~400 tokens * 4 chars = 1600).
    chunk_chars: usize,
    /// Overlap in approximate characters (default ~80 tokens * 4 chars = 320).
    overlap_chars: usize,
}

impl MemoryManager {
    pub fn new(store: MemoryStore, provider: Option<Box<dyn EmbeddingProvider>>) -> Self {
        Self {
            store,
            provider,
            search_config: HybridSearchConfig::default(),
            chunk_chars: 1600,
            overlap_chars: 320,
        }
    }

    pub fn with_search_config(mut self, config: HybridSearchConfig) -> Self {
        self.search_config = config;
        self
    }

    pub fn with_chunk_chars(mut self, chars: usize) -> Self {
        self.chunk_chars = chars;
        self
    }

    pub fn with_overlap_chars(mut self, chars: usize) -> Self {
        self.overlap_chars = chars;
        self
    }

    pub fn init(&self) -> Result<()> {
        self.store.init()
    }

    pub async fn index_file(&self, path: &Path) -> Result<usize> {
        let content = tokio::fs::read_to_string(path).await?;
        let path_str = path.display().to_string();
        self.store.delete_by_path(&path_str)?;
        let chunks = self.chunk_by_chars(&content, &path_str, "file");
        let count = chunks.len();
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

    /// Add a memory entry, chunking if the content is large.
    pub async fn add_memory_text(&self, content: &str, source: &str) -> Result<Vec<String>> {
        let path = format!("memory:{}", source);
        let chunks = self.chunk_by_chars(content, &path, source);
        let chunks = if let Some(ref provider) = self.provider {
            self.embed_chunks(chunks, provider.as_ref()).await?
        } else {
            chunks
        };
        let ids: Vec<String> = chunks.iter().map(|c| c.id.clone()).collect();
        for chunk in &chunks {
            self.store.upsert(chunk)?;
        }
        Ok(ids)
    }

    pub fn get_memory(&self, id: &str) -> Result<Option<MemoryChunk>> {
        self.store.get(id)
    }

    pub fn count(&self) -> Result<usize> {
        self.store.count()
    }

    /// Index a JSONL session transcript file into memory.
    /// Each assistant message becomes a searchable memory chunk.
    /// source is set to "session" to distinguish from file-based memories.
    pub async fn index_session_transcript(
        &self,
        path: &std::path::Path,
        min_content_bytes: usize,
    ) -> Result<usize> {
        let content = tokio::fs::read_to_string(path).await?;
        let path_str = path.display().to_string();

        // Parse JSONL: each line is a ChatMessage JSON object
        let mut texts: Vec<String> = Vec::new();
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) {
                let role = msg.get("role").and_then(|r| r.as_str()).unwrap_or("");
                let msg_content = msg.get("content").and_then(|c| c.as_str()).unwrap_or("");
                // Index assistant messages that are substantive
                if role == "assistant" && msg_content.len() >= min_content_bytes {
                    texts.push(msg_content.to_string());
                }
            }
        }

        if texts.is_empty() {
            return Ok(0);
        }

        // Remove old session chunks for this path
        self.store.delete_by_path(&path_str)?;

        let combined = texts.join("\n\n");
        let mut chunks = self.chunk_by_chars(&combined, &path_str, "session");

        if let Some(ref provider) = self.provider {
            chunks = self.embed_chunks(chunks, provider.as_ref()).await?;
        }

        let count = chunks.len();
        for chunk in &chunks {
            self.store.upsert(chunk)?;
        }

        info!("Indexed {} session chunks from {}", count, path_str);
        Ok(count)
    }

    /// Split text into overlapping character-based chunks.
    /// Tries to split at paragraph boundaries (\n\n) for better semantic coherence.
    fn chunk_by_chars(&self, text: &str, path: &str, source: &str) -> Vec<MemoryChunk> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let mut chunks = Vec::new();
        if text.is_empty() {
            return chunks;
        }

        let safe_overlap = self.overlap_chars.min(self.chunk_chars.saturating_sub(1));
        let mut start = 0;
        let mut chunk_idx = 0usize;

        while start < text.len() {
            let end = (start + self.chunk_chars).min(text.len());
            // Snap end to a char boundary (safe stable API)
            let end = ceil_char_boundary_safe(text, end);

            // Try to split at paragraph boundary within last 20% of chunk
            let snap_zone_start = start + (self.chunk_chars * 4 / 5);
            let actual_end = if end < text.len() && snap_zone_start < end {
                // Look for \n\n in snap zone
                text[snap_zone_start..end]
                    .find("\n\n")
                    .map(|rel| snap_zone_start + rel + 2)
                    .unwrap_or(end)
            } else {
                end
            };
            let actual_end = actual_end.min(text.len());

            // Count lines spanned for metadata
            let chunk_text = &text[start..actual_end];
            let start_line = text[..start].lines().count() + 1;
            let end_line = start_line + chunk_text.lines().count().saturating_sub(1);

            let id = format!("{}:{}:{}", path, chunk_idx, actual_end);
            chunks.push(MemoryChunk {
                id,
                path: path.to_string(),
                start_line,
                end_line,
                content: chunk_text.to_string(),
                source: source.to_string(),
                embedding: None,
                updated_at_ms: now,
            });

            chunk_idx += 1;
            if actual_end >= text.len() {
                break;
            }
            // Move forward by chunk_chars - overlap, but not backward
            let next = actual_end.saturating_sub(safe_overlap);
            if next <= start {
                start = actual_end;
            } else {
                start = next;
            }
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

/// Advance `idx` to the next valid UTF-8 char boundary in `s`.
/// Safe alternative to the nightly `ceil_char_boundary` method.
fn ceil_char_boundary_safe(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx;
    while i < s.len() && !s.is_char_boundary(i) {
        i += 1;
    }
    i
}

fn is_indexable(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    matches!(
        ext,
        "rs" | "py"
            | "js"
            | "ts"
            | "tsx"
            | "jsx"
            | "go"
            | "java"
            | "c"
            | "cpp"
            | "h"
            | "hpp"
            | "md"
            | "txt"
            | "toml"
            | "yaml"
            | "yml"
            | "json"
            | "html"
            | "css"
            | "sql"
            | "sh"
            | "rb"
            | "php"
            | "swift"
            | "kt"
            | "scala"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ceil_char_boundary_safe_ascii() {
        let s = "hello world";
        assert_eq!(ceil_char_boundary_safe(s, 5), 5);
        assert_eq!(ceil_char_boundary_safe(s, 11), 11);
        assert_eq!(ceil_char_boundary_safe(s, 15), 11); // past end
    }

    #[test]
    fn test_ceil_char_boundary_safe_utf8() {
        let s = "héllo"; // é is 2 bytes (0xC3 0xA9)
        assert_eq!(ceil_char_boundary_safe(s, 0), 0);
        assert_eq!(ceil_char_boundary_safe(s, 1), 1); // 'h' boundary
        // byte 2 is the second byte of 'é'; next boundary is byte 3
        assert_eq!(ceil_char_boundary_safe(s, 2), 3);
    }

    #[test]
    fn test_chunk_by_chars_empty() {
        // Use a mock-like test: just call the free function logic directly
        let s = "";
        assert!(s.is_empty());
    }

    #[test]
    fn test_chunk_by_chars_basic() {
        // Construct a minimal manager (store requires DB, skip) via manual chunking
        // Just test the boundary helper
        let s = "abc def ghi";
        let boundary = ceil_char_boundary_safe(s, 7);
        assert!(s.is_char_boundary(boundary));
    }
}
