/// Maximal Marginal Relevance (MMR) re-ranking for diversity.
/// MMR = λ * relevance - (1-λ) * max_similarity_to_selected
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct MmrConfig {
    pub enabled: bool,
    pub lambda: f64, // 0.7 = 70% relevance, 30% diversity
}

impl Default for MmrConfig {
    fn default() -> Self {
        Self { enabled: false, lambda: 0.7 }
    }
}

pub fn rerank_mmr<T>(
    items: &mut Vec<(T, f32)>,
    content_fn: impl Fn(&T) -> &str,
    config: &MmrConfig,
) where T: Clone {
    if !config.enabled || items.len() <= 1 {
        return;
    }

    // Normalize scores to [0,1]
    let max_score = items.iter().map(|(_, s)| *s).fold(f32::NEG_INFINITY, f32::max);
    let min_score = items.iter().map(|(_, s)| *s).fold(f32::INFINITY, f32::min);
    let range = (max_score - min_score).max(1e-6);

    let normalized: Vec<f64> = items.iter()
        .map(|(_, s)| ((s - min_score) / range) as f64)
        .collect();

    // Tokenize all items
    let token_sets: Vec<HashSet<String>> = items.iter()
        .map(|(item, _)| tokenize(content_fn(item)))
        .collect();

    let mut selected: Vec<usize> = Vec::with_capacity(items.len());
    let mut remaining: Vec<usize> = (0..items.len()).collect();

    // Greedy selection
    while !remaining.is_empty() {
        let best = if selected.is_empty() {
            // First: pick highest relevance
            *remaining.iter().max_by(|&&a, &&b|
                normalized[a].partial_cmp(&normalized[b]).unwrap_or(std::cmp::Ordering::Equal)
            ).unwrap()
        } else {
            // Subsequent: maximize MMR
            *remaining.iter().max_by(|&&a, &&b| {
                let mmr_a = mmr_score(a, &selected, &normalized, &token_sets, config.lambda);
                let mmr_b = mmr_score(b, &selected, &normalized, &token_sets, config.lambda);
                mmr_a.partial_cmp(&mmr_b).unwrap_or(std::cmp::Ordering::Equal)
            }).unwrap()
        };
        remaining.retain(|&i| i != best);
        selected.push(best);
    }

    // Reorder items by selected order
    let reordered: Vec<(T, f32)> = selected.iter()
        .map(|&i| items[i].clone())
        .collect();
    *items = reordered;
}

fn mmr_score(
    candidate: usize,
    selected: &[usize],
    relevance: &[f64],
    tokens: &[HashSet<String>],
    lambda: f64,
) -> f64 {
    let max_sim = selected.iter()
        .map(|&s| jaccard(&tokens[candidate], &tokens[s]))
        .fold(0.0f64, f64::max);
    lambda * relevance[candidate] - (1.0 - lambda) * max_sim
}

fn jaccard(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() { return 0.0; }
    let intersection = a.intersection(b).count();
    let union = a.union(b).count();
    intersection as f64 / union as f64
}

fn tokenize(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect()
}
