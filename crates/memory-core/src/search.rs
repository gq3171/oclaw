use crate::embeddings::EmbeddingProvider;
use crate::store::MemoryStore;
use crate::types::MemorySearchResult;
use anyhow::Result;
use std::collections::HashMap;
use tracing::debug;

// ─── Citation Mode ───────────────────────────────────────────────────

/// Controls whether search results include file path + line citations.
#[derive(Debug, Clone, Copy, Default, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CitationMode {
    /// Private chats show citations; group/channel chats suppress them (default).
    #[default]
    Auto,
    /// Always include citations.
    On,
    /// Never include citations.
    Off,
}

// ─── Temporal Decay ──────────────────────────────────────────────────

/// Configuration for time-based score decay of search results.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct TemporalDecayConfig {
    pub enabled: bool,
    /// Half-life in days: results this old have 50% of their original score.
    pub half_life_days: f32,
}

impl Default for TemporalDecayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            half_life_days: 30.0,
        }
    }
}

// ─── Hybrid Search Config ─────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct HybridSearchConfig {
    pub vector_weight: f32,
    pub text_weight: f32,
    pub limit: usize,
    pub threshold: f32,
    pub candidate_multiplier: usize,
    /// Citation decoration mode.
    pub citation_mode: CitationMode,
    /// Temporal decay configuration.
    pub temporal_decay: TemporalDecayConfig,
}

impl Default for HybridSearchConfig {
    fn default() -> Self {
        Self {
            vector_weight: 0.7,
            text_weight: 0.3,
            limit: 6,        // Aligned with Node default (was 10)
            threshold: 0.35, // Aligned with Node default (was 0.3)
            candidate_multiplier: 4,
            citation_mode: CitationMode::Auto,
            temporal_decay: TemporalDecayConfig::default(),
        }
    }
}

// ─── Hybrid Search ────────────────────────────────────────────────────

pub async fn hybrid_search(
    store: &MemoryStore,
    provider: &dyn EmbeddingProvider,
    query: &str,
    config: &HybridSearchConfig,
) -> Result<Vec<MemorySearchResult>> {
    let fetch_limit = config.limit * config.candidate_multiplier;

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
    let vec_max = vector_scores
        .first()
        .map(|(_, s)| *s)
        .unwrap_or(1.0)
        .max(0.001);
    let fts_max = fts_results
        .first()
        .map(|r| r.score)
        .unwrap_or(1.0)
        .max(0.001);

    // 4. Merge by ID with weighted combination
    let mut merged: HashMap<String, f64> = HashMap::new();

    for (id, score) in &vector_scores {
        let normalized = score / vec_max;
        *merged.entry(id.clone()).or_default() += normalized * config.vector_weight as f64;
    }

    // FTS search returning chunk IDs for score merging
    let fts_ids = store.fts_search_ids(query, fetch_limit)?;
    for (id, score) in &fts_ids {
        let normalized = score / fts_max;
        *merged.entry(id.clone()).or_default() += normalized * config.text_weight as f64;
    }

    // 5. Sort by combined score, filter by threshold
    let threshold_f64 = config.threshold as f64;
    let mut ranked: Vec<(String, f64)> = merged
        .into_iter()
        .filter(|(_, score)| *score >= threshold_f64)
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
            id: c.id.clone(),
            path: c.path,
            start_line: c.start_line,
            end_line: c.end_line,
            score: score_map.get(c.id.as_str()).copied().unwrap_or(0.0),
            snippet: truncate_snippet(&c.content, 500),
            source: c.source,
            updated_at_ms: c.updated_at_ms,
        })
        .collect();
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

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
    if denom < 1e-10 { 0.0 } else { dot / denom }
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

// ─── Citation Decoration ─────────────────────────────────────────────

/// Decorate search results with source citations (path#L{start}-L{end}).
pub fn decorate_citations(
    results: Vec<crate::types::MemorySearchResult>,
    mode: CitationMode,
    is_group: bool,
) -> Vec<crate::types::MemorySearchResult> {
    let include = match mode {
        CitationMode::On => true,
        CitationMode::Off => false,
        CitationMode::Auto => !is_group,
    };
    if !include {
        return results;
    }
    results
        .into_iter()
        .map(|mut r| {
            let citation = if r.start_line == r.end_line {
                format!("{}#L{}", r.path, r.start_line)
            } else {
                format!("{}#L{}-L{}", r.path, r.start_line, r.end_line)
            };
            r.snippet = format!("{}\n\nSource: {}", r.snippet.trim(), citation);
            r
        })
        .collect()
}

// ─── Temporal Decay Application ──────────────────────────────────────

/// Apply time-based score decay to search results.
/// Results decay toward 50% of their score at `half_life_days`.
pub fn apply_temporal_decay(
    mut results: Vec<crate::types::MemorySearchResult>,
    config: &TemporalDecayConfig,
    now_ms: u64,
) -> Vec<crate::types::MemorySearchResult> {
    if !config.enabled {
        return results;
    }
    let half_life_ms = config.half_life_days as f64 * 86_400_000.0;
    for r in &mut results {
        let age_ms = now_ms.saturating_sub(r.updated_at_ms) as f64;
        let decay = 0.5f64.powf(age_ms / half_life_ms) as f32;
        // Decay toward 50% minimum (not zero) to keep old content discoverable
        r.score *= (0.5 + 0.5 * decay) as f64;
    }
    // Re-sort after score adjustment
    results.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    results
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod citation_tests {
    use super::*;
    use crate::types::MemorySearchResult;

    fn make_result(path: &str, start_line: usize, end_line: usize) -> MemorySearchResult {
        MemorySearchResult {
            id: "test".to_string(),
            path: path.to_string(),
            start_line,
            end_line,
            snippet: "some content".to_string(),
            score: 0.9,
            source: "file".to_string(),
            updated_at_ms: 0,
        }
    }

    #[test]
    fn auto_mode_includes_in_dm() {
        let results = vec![make_result("MEMORY.md", 1, 5)];
        let decorated = decorate_citations(results, CitationMode::Auto, false);
        assert!(decorated[0].snippet.contains("MEMORY.md#L1-L5"));
    }

    #[test]
    fn auto_mode_excludes_in_group() {
        let results = vec![make_result("MEMORY.md", 1, 5)];
        let decorated = decorate_citations(results, CitationMode::Auto, true);
        assert!(!decorated[0].snippet.contains("Source:"));
    }

    #[test]
    fn on_mode_always_includes() {
        let results = vec![make_result("MEMORY.md", 3, 3)];
        let decorated = decorate_citations(results, CitationMode::On, true);
        assert!(decorated[0].snippet.contains("MEMORY.md#L3"));
        assert!(!decorated[0].snippet.contains("L3-L")); // single line uses #L3 not #L3-L3
    }

    #[test]
    fn off_mode_never_includes() {
        let results = vec![make_result("MEMORY.md", 1, 10)];
        let decorated = decorate_citations(results, CitationMode::Off, false);
        assert!(!decorated[0].snippet.contains("Source:"));
    }
}

#[cfg(test)]
mod decay_tests {
    use super::*;
    use crate::types::MemorySearchResult;

    fn make_result_with_age(score: f64, age_days: f32) -> MemorySearchResult {
        let now_ms = 1_700_000_000_000u64;
        let updated_at_ms = now_ms - (age_days * 86_400_000.0) as u64;
        MemorySearchResult {
            id: "test".to_string(),
            path: "MEMORY.md".to_string(),
            start_line: 1,
            end_line: 1,
            snippet: "content".to_string(),
            score,
            source: "file".to_string(),
            updated_at_ms,
        }
    }

    #[test]
    fn disabled_no_change() {
        let config = TemporalDecayConfig {
            enabled: false,
            half_life_days: 30.0,
        };
        let results = vec![make_result_with_age(0.9, 365.0)];
        let now_ms = 1_700_000_000_000u64;
        let decayed = apply_temporal_decay(results, &config, now_ms);
        assert!((decayed[0].score - 0.9).abs() < 0.001);
    }

    #[test]
    fn fresh_memory_minimal_decay() {
        let config = TemporalDecayConfig {
            enabled: true,
            half_life_days: 30.0,
        };
        let results = vec![make_result_with_age(0.9, 0.1)]; // 0.1 days old
        let now_ms = 1_700_000_000_000u64;
        let decayed = apply_temporal_decay(results, &config, now_ms);
        // Nearly fresh: score should be close to original
        assert!(decayed[0].score > 0.88);
    }

    #[test]
    fn half_life_halves_bonus() {
        let config = TemporalDecayConfig {
            enabled: true,
            half_life_days: 30.0,
        };
        let results = vec![make_result_with_age(1.0, 30.0)]; // exactly half-life old
        let now_ms = 1_700_000_000_000u64;
        let decayed = apply_temporal_decay(results, &config, now_ms);
        // decay=0.5, score = 1.0 * (0.5 + 0.5*0.5) = 0.75
        assert!((decayed[0].score - 0.75).abs() < 0.01);
    }
}
