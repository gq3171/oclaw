/// Query expansion: extract keywords and build expanded FTS queries.
const EN_STOP_WORDS: &[&str] = &[
    "a", "an", "the", "is", "are", "was", "were", "be", "been", "being",
    "have", "has", "had", "do", "does", "did", "will", "would", "could",
    "should", "may", "might", "shall", "can", "need", "dare", "ought",
    "i", "me", "my", "we", "us", "our", "you", "your", "he", "him",
    "his", "she", "her", "it", "its", "they", "them", "their",
    "what", "which", "who", "whom", "this", "that", "these", "those",
    "am", "at", "by", "for", "with", "about", "against", "between",
    "through", "during", "before", "after", "above", "below", "to",
    "from", "up", "down", "in", "out", "on", "off", "over", "under",
    "again", "further", "then", "once", "here", "there", "when", "where",
    "why", "how", "all", "both", "each", "few", "more", "most", "other",
    "some", "such", "no", "nor", "not", "only", "own", "same", "so",
    "than", "too", "very", "just", "because", "as", "until", "while",
    "of", "if", "or", "and", "but",
];

pub struct ExpandedQuery {
    pub original: String,
    pub keywords: Vec<String>,
    pub expanded: String,
}

pub fn expand_query(query: &str) -> ExpandedQuery {
    let keywords = extract_keywords(query);
    let expanded = if keywords.is_empty() {
        query.to_string()
    } else {
        let parts: Vec<String> = std::iter::once(format!("\"{}\"", query))
            .chain(keywords.iter().cloned())
            .collect();
        parts.join(" OR ")
    };
    ExpandedQuery { original: query.to_string(), keywords, expanded }
}

pub fn extract_keywords(query: &str) -> Vec<String> {
    let stop: std::collections::HashSet<&str> = EN_STOP_WORDS.iter().copied().collect();
    let mut seen = std::collections::HashSet::new();

    query.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| w.len() > 1 && !stop.contains(w))
        .filter(|w| seen.insert(w.to_string()))
        .map(|w| w.to_string())
        .collect()
}
