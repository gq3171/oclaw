use async_trait::async_trait;
use std::collections::HashMap;

use crate::search::{VectorEntry, VectorStore, SearchResult};

/// Trait for embedding providers — implemented by LLM provider adapters.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String>;
    fn dimension(&self) -> usize;
    fn model_name(&self) -> &str;
}

/// Semantic memory: embeds text via any provider and stores/searches in a VectorStore.
pub struct SemanticMemory {
    provider: Box<dyn EmbeddingProvider>,
    store: VectorStore,
}

impl SemanticMemory {
    pub fn new(provider: Box<dyn EmbeddingProvider>) -> Self {
        let dim = provider.dimension();
        Self { provider, store: VectorStore::new(dim) }
    }

    pub async fn add(&self, id: &str, text: &str, metadata: HashMap<String, String>) -> Result<(), String> {
        let vecs = self.provider.embed(&[text.to_string()]).await?;
        let vec = vecs.into_iter().next().ok_or("No embedding returned")?;
        let mut meta = metadata;
        meta.insert("content".to_string(), text.to_string());
        self.store.insert(VectorEntry {
            id: id.to_string(),
            vector: vec,
            metadata: meta,
            score: None,
        }).await;
        Ok(())
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchResult>, String> {
        let vecs = self.provider.embed(&[query.to_string()]).await?;
        let vec = vecs.into_iter().next().ok_or("No embedding returned")?;
        Ok(self.store.search(&vec, limit).await)
    }

    pub async fn delete(&self, id: &str) {
        self.store.delete(id).await;
    }

    pub async fn count(&self) -> usize {
        self.store.count().await
    }

    pub fn store(&self) -> &VectorStore {
        &self.store
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeEmbedder;

    #[async_trait]
    impl EmbeddingProvider for FakeEmbedder {
        async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
            Ok(texts.iter().map(|t| {
                let v = t.len() as f32;
                vec![v, 0.0, 0.0]
            }).collect())
        }
        fn dimension(&self) -> usize { 3 }
        fn model_name(&self) -> &str { "fake" }
    }

    #[tokio::test]
    async fn test_semantic_memory_add_and_search() {
        let mem = SemanticMemory::new(Box::new(FakeEmbedder));
        mem.add("1", "hello", HashMap::new()).await.unwrap();
        mem.add("2", "hello world", HashMap::new()).await.unwrap();
        assert_eq!(mem.count().await, 2);

        let results = mem.search("hello", 5).await.unwrap();
        assert!(!results.is_empty());
    }

    #[tokio::test]
    async fn test_semantic_memory_delete() {
        let mem = SemanticMemory::new(Box::new(FakeEmbedder));
        mem.add("1", "test", HashMap::new()).await.unwrap();
        mem.delete("1").await;
        assert_eq!(mem.count().await, 0);
    }
}
