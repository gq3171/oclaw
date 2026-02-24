use crate::embeddings::EmbeddingProvider;
use crate::store::MemoryStore;
use crate::types::MemorySearchResult;
use anyhow::Result;
use std::collections::HashMap;
use tracing::debug;

#[derive(Debug, Clone)]
pub struct HybridSearchConfig {
    pub vector_weight: f64,
    pub text_weight: f64,
    pub limit: usize,
    pub threshold: f64,
}

impl Default for HybridSearchConfig {
    fn default() -> Self {
        Self {
            vector_weight: 0.7,
            text_weight: 0.3,
            limit: 10,
            threshold: 0.3,
        }
    }
}

pub async fn hybrid_search(
    store: &MemoryStore,
    provider: &dyn EmbeddingProvider,
    query: &str,
    config: &HybridSearchConfig,
) -> Result<Vec<MemorySearchResult>> {
    let fetch_limit = config.limit * 3;

    // 1. Vector search
    let query_embedding = {
        let texts = vec![query.to_string()];
        let mut embeddings = provider.embed(&texts).await?;
        if embeddings.is_empty() {
            return Ok(Vec::new());
        }
        embeddings.remove(0)
    };

    let all_stored = store.all_with_embeddings()?;
    let mut vector_scores: Vec<(String, f64)> = all_stored
        .iter()
        .map(|(id, emb)| {
            let score = cosine_similarity(&query_embedding, emb);
            (id.clone(), score)
        })
        .collect();
    vector_scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    vector_scores.truncate(fetch_limit);

    debug!("Vector search returned {} candidates", vector_scores.len());

    // 2. FTS search
    let fts_results = store.fts_search(query, fetch_limit)?;
    debug!("FTS search returned {} candidates", fts_results.len());

    // 3. Normalize scores
    let vec_max = vector_scores.first().map(|(_, s)| *s).unwrap_or(1.0).max(0.001);
    let fts_max = fts_results.first().map(|r| r.score).unwrap_or(1.0).max(0.001);

    // 4. Merge by ID with weighted combination
    let mut merged: HashMap<String, f64> = HashMap::new();

    for (id, score) in &vector_scores {
        let normalized = score / vec_max;
        *merged.entry(id.clone()).or_default() += normalized * config.vector_weight;
    }

    // FTS search returning chunk IDs for score merging
    let fts_ids = store.fts_search_ids(query, fetch_limit)?;
    for (id, score) in &fts_ids {
        let normalized = score / fts_max;
        *merged.entry(id.clone()).or_default() += normalized * config.text_weight;
    }

    // 5. Sort by combined score, filter by threshold
    let mut ranked: Vec<(String, f64)> = merged
        .into_iter()
        .filter(|(_, score)| *score >= config.threshold)
        .collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.truncate(config.limit);

    // 6. Fetch full chunks for top results
    let ids: Vec<String> = ranked.iter().map(|(id, _)| id.clone()).collect();
    let chunks = store.get_by_ids(&ids)?;

    let score_map: HashMap<&str, f64> = ranked.iter().map(|(id, s)| (id.as_str(), *s)).collect();
    let mut results: Vec<MemorySearchResult> = chunks
        .into_iter()
        .map(|c| MemorySearchResult {
            path: c.path,
            start_line: c.start_line,
            end_line: c.end_line,
            score: score_map.get(c.id.as_str()).copied().unwrap_or(0.0),
            snippet: truncate_snippet(&c.content, 500),
            source: c.source,
        })
        .collect();
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    Ok(results)
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut norm_a = 0.0f64;
    let mut norm_b = 0.0f64;
    for (x, y) in a.iter().zip(b.iter()) {
        let x = *x as f64;
        let y = *y as f64;
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom < 1e-10 {
        0.0
    } else {
        dot / denom
    }
}

fn truncate_snippet(content: &str, max_chars: usize) -> String {
    if content.len() <= max_chars {
        content.to_string()
    } else {
        let mut end = max_chars;
        while end > 0 && !content.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &content[..end])
    }
}
