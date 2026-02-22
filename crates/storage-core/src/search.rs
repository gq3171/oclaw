use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VectorEntry {
    pub id: String,
    pub vector: Vec<f32>,
    pub metadata: HashMap<String, String>,
    pub score: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    pub id: String,
    pub score: f32,
    pub content: String,
    pub metadata: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SearchType {
    Vector,
    FullText,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchOptions {
    pub search_type: SearchType,
    pub limit: usize,
    pub offset: usize,
    pub threshold: Option<f32>,
    pub vector_weight: Option<f32>,
    pub text_weight: Option<f32>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            search_type: SearchType::Hybrid,
            limit: 10,
            offset: 0,
            threshold: None,
            vector_weight: Some(0.5),
            text_weight: Some(0.5),
        }
    }
}

pub struct VectorStore {
    entries: Arc<RwLock<HashMap<String, VectorEntry>>>,
    dimension: usize,
}

impl VectorStore {
    pub fn new(dimension: usize) -> Self {
        Self {
            entries: Arc::new(RwLock::new(HashMap::new())),
            dimension,
        }
    }

    pub fn dimension(&self) -> usize {
        self.dimension
    }

    pub async fn insert(&self, entry: VectorEntry) {
        self.entries.write().await.insert(entry.id.clone(), entry);
    }

    pub async fn upsert(&self, entries: Vec<VectorEntry>) {
        let mut store = self.entries.write().await;
        for entry in entries {
            store.insert(entry.id.clone(), entry);
        }
    }

    pub async fn search(&self, query: &[f32], limit: usize) -> Vec<SearchResult> {
        let entries = self.entries.read().await;
        let mut results: Vec<SearchResult> = entries
            .values()
            .map(|entry| {
                let score = cosine_similarity(query, &entry.vector);
                SearchResult {
                    id: entry.id.clone(),
                    score,
                    content: entry.metadata.get("content").cloned().unwrap_or_default(),
                    metadata: entry.metadata.clone(),
                }
            })
            .filter(|r| r.score > 0.0)
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        results
    }

    pub async fn get(&self, id: &str) -> Option<VectorEntry> {
        self.entries.read().await.get(id).cloned()
    }

    pub async fn delete(&self, id: &str) {
        self.entries.write().await.remove(id);
    }

    pub async fn count(&self) -> usize {
        self.entries.read().await.len()
    }

    pub async fn clear(&self) {
        self.entries.write().await.clear();
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }

    let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let magnitude_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let magnitude_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

    if magnitude_a == 0.0 || magnitude_b == 0.0 {
        return 0.0;
    }

    dot_product / (magnitude_a * magnitude_b)
}

impl Default for VectorStore {
    fn default() -> Self {
        Self::new(1536)
    }
}

pub struct FullTextStore {
    documents: Arc<RwLock<HashMap<String, FullTextDoc>>>,
    inverted_index: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

#[derive(Debug, Clone)]
struct FullTextDoc {
    id: String,
    content: String,
    metadata: HashMap<String, String>,
}

impl FullTextStore {
    pub fn new() -> Self {
        Self {
            documents: Arc::new(RwLock::new(HashMap::new())),
            inverted_index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn insert(&self, id: String, content: String, metadata: HashMap<String, String>) {
        let tokens = self.tokenize(&content);
        
        let mut index = self.inverted_index.write().await;
        for token in &tokens {
            index.entry(token.clone()).or_insert_with(Vec::new).push(id.clone());
        }

        let mut docs = self.documents.write().await;
        docs.insert(id.clone(), FullTextDoc { id, content, metadata });
    }

    pub async fn search(&self, query: &str, limit: usize) -> Vec<SearchResult> {
        let query_tokens = self.tokenize(query);
        
        let index = self.inverted_index.read().await;
        let docs = self.documents.read().await;

        let mut scores: HashMap<String, f32> = HashMap::new();
        
        for token in &query_tokens {
            if let Some(doc_ids) = index.get(token) {
                for doc_id in doc_ids {
                    *scores.entry(doc_id.clone()).or_insert(0.0) += 1.0;
                }
            }
        }

        let mut results: Vec<SearchResult> = scores
            .into_iter()
            .filter_map(|(id, score)| {
                docs.get(&id).map(|doc| {
                    let max_score = query_tokens.len() as f32;
                    SearchResult {
                        id: doc.id.clone(),
                        score: score / max_score,
                        content: doc.content.clone(),
                        metadata: doc.metadata.clone(),
                    }
                })
            })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);
        results
    }

    pub async fn delete(&self, id: &str) {
        let mut docs = self.documents.write().await;
        if let Some(doc) = docs.remove(id) {
            let tokens = self.tokenize(&doc.content);
            let mut index = self.inverted_index.write().await;
            for token in tokens {
                if let Some(ids) = index.get_mut(&token) {
                    ids.retain(|i| i != id);
                    if ids.is_empty() {
                        index.remove(&token);
                    }
                }
            }
        }
    }

    pub async fn count(&self) -> usize {
        self.documents.read().await.len()
    }

    fn tokenize(&self, text: &str) -> Vec<String> {
        text.to_lowercase()
            .split(|c: char| !c.is_alphanumeric() && c != '_')
            .filter(|s| s.len() > 2)
            .map(|s| s.to_string())
            .collect()
    }
}

impl Default for FullTextStore {
    fn default() -> Self {
        Self::new()
    }
}

pub struct HybridSearchStore {
    vector_store: VectorStore,
    fulltext_store: FullTextStore,
}

impl HybridSearchStore {
    pub fn new(vector_dimension: usize) -> Self {
        Self {
            vector_store: VectorStore::new(vector_dimension),
            fulltext_store: FullTextStore::new(),
        }
    }

    pub async fn insert_vector(&self, entry: VectorEntry) {
        self.vector_store.insert(entry).await;
    }

    pub async fn insert_text(&self, id: String, content: String, metadata: HashMap<String, String>) {
        self.fulltext_store.insert(id, content, metadata).await;
    }

    pub async fn insert(&self, id: String, content: String, vector: Vec<f32>, metadata: HashMap<String, String>) {
        let mut meta = metadata;
        meta.insert("content".to_string(), content.clone());
        
        self.vector_store.insert(VectorEntry {
            id: id.clone(),
            vector,
            metadata: meta.clone(),
            score: None,
        }).await;

        self.fulltext_store.insert(id, content, meta).await;
    }

    pub async fn search(&self, query: &str, query_vector: &[f32], options: &SearchOptions) -> Vec<SearchResult> {
        match options.search_type {
            SearchType::Vector => {
                self.vector_store.search(query_vector, options.limit).await
            }
            SearchType::FullText => {
                self.fulltext_store.search(query, options.limit).await
            }
            SearchType::Hybrid => {
                self.hybrid_search(query, query_vector, options).await
            }
        }
    }

    async fn hybrid_search(&self, query: &str, query_vector: &[f32], options: &SearchOptions) -> Vec<SearchResult> {
        let vector_results = self.vector_store.search(query_vector, options.limit * 2).await;
        let text_results = self.fulltext_store.search(query, options.limit * 2).await;

        let vector_weight = options.vector_weight.unwrap_or(0.5);
        let text_weight = options.text_weight.unwrap_or(0.5);

        let mut combined: HashMap<String, (f32, String, HashMap<String, String>)> = HashMap::new();

        for r in vector_results {
            combined.insert(r.id.clone(), (r.score * vector_weight, r.content, r.metadata));
        }

        for r in text_results {
            if let Some((score, _content, _meta)) = combined.get_mut(&r.id) {
                *score += r.score * text_weight;
            } else {
                combined.insert(r.id.clone(), (r.score * text_weight, r.content, r.metadata));
            }
        }

        let mut results: Vec<SearchResult> = combined
            .into_iter()
            .map(|(id, (score, content, metadata))| SearchResult {
                id,
                score,
                content,
                metadata,
            })
            .collect();

        results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

        if let Some(threshold) = options.threshold {
            results.retain(|r| r.score >= threshold);
        }

        results.truncate(options.limit);
        results
    }

    pub async fn delete(&self, id: &str) {
        self.vector_store.delete(id).await;
        self.fulltext_store.delete(id).await;
    }

    pub async fn count(&self) -> usize {
        self.vector_store.count().await
    }

    pub fn vector_store(&self) -> &VectorStore {
        &self.vector_store
    }

    pub fn fulltext_store(&self) -> &FullTextStore {
        &self.fulltext_store
    }
}

impl Default for HybridSearchStore {
    fn default() -> Self {
        Self::new(1536)
    }
}
