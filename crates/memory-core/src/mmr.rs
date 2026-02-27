/// Maximal Marginal Relevance — reranks candidates to balance
/// relevance against diversity.
pub struct MmrConfig {
    /// 0.0 = pure diversity, 1.0 = pure relevance. Default 0.7.
    pub lambda: f64,
    /// Candidate pool size. Default 20.
    pub top_k: usize,
    /// Final result count. Default 5.
    pub final_k: usize,
}

impl Default for MmrConfig {
    fn default() -> Self {
        Self {
            lambda: 0.7,
            top_k: 20,
            final_k: 5,
        }
    }
}

/// Rerank `candidates` using MMR, returning at most `config.final_k` items.
///
/// Each candidate is `(score, embedding)` where `score` is the original
/// relevance score and `embedding` is the vector representation.
pub fn mmr_rerank(
    _query_embedding: &[f32],
    candidates: &[(f64, Vec<f32>)],
    config: &MmrConfig,
) -> Vec<usize> {
    let n = candidates.len().min(config.top_k);
    if n == 0 {
        return vec![];
    }

    let mut selected: Vec<usize> = Vec::with_capacity(config.final_k);
    let mut remaining: Vec<usize> = (0..n).collect();

    while selected.len() < config.final_k && !remaining.is_empty() {
        let mut best_idx = 0;
        let mut best_score = f64::NEG_INFINITY;

        for (ri, &ci) in remaining.iter().enumerate() {
            let relevance = candidates[ci].0;

            let max_sim = selected
                .iter()
                .map(|&si| cosine_similarity(&candidates[ci].1, &candidates[si].1) as f64)
                .fold(f64::NEG_INFINITY, f64::max);
            let max_sim = if max_sim == f64::NEG_INFINITY {
                0.0
            } else {
                max_sim
            };

            let mmr = config.lambda * relevance - (1.0 - config.lambda) * max_sim;
            if mmr > best_score {
                best_score = mmr;
                best_idx = ri;
            }
        }

        let chosen = remaining.swap_remove(best_idx);
        selected.push(chosen);
    }

    selected
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom == 0.0 { 0.0 } else { dot / denom }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_mmr() {
        let query = vec![1.0, 0.0];
        let candidates = vec![
            (0.9, vec![1.0, 0.0]),
            (0.85, vec![0.99, 0.1]),
            (0.5, vec![0.0, 1.0]),
        ];
        let config = MmrConfig {
            lambda: 0.5,
            top_k: 10,
            final_k: 2,
        };
        let selected = mmr_rerank(&query, &candidates, &config);
        assert_eq!(selected.len(), 2);
        // First pick should be highest relevance
        assert_eq!(selected[0], 0);
        // Second should prefer diversity → index 2
        assert_eq!(selected[1], 2);
    }

    #[test]
    fn empty_candidates() {
        let selected = mmr_rerank(&[1.0], &[], &MmrConfig::default());
        assert!(selected.is_empty());
    }
}
