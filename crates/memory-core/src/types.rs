use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryChunk {
    pub id: String,
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub content: String,
    pub source: String,
    pub embedding: Option<Vec<f32>>,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MemorySearchResult {
    pub id: String,
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub score: f64,
    pub snippet: String,
    pub source: String,
    /// Timestamp of the chunk's last update, in milliseconds since Unix epoch.
    /// Used for temporal decay scoring.
    #[serde(default)]
    pub updated_at_ms: u64,
}
