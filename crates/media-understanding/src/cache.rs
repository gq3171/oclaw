//! Media attachment cache using content hash.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Mutex;

/// Caches media processing results keyed by content hash.
pub struct MediaAttachmentCache {
    entries: Mutex<HashMap<String, CacheEntry>>,
    max_entries: usize,
}

struct CacheEntry {
    result: String,
    created_at: u64,
}

impl MediaAttachmentCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            max_entries,
        }
    }

    /// Compute a SHA-256 content hash for cache key.
    pub fn content_hash(data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    pub fn get(&self, hash: &str) -> Option<String> {
        let entries = self.entries.lock().ok()?;
        entries.get(hash).map(|e| e.result.clone())
    }

    pub fn put(&self, hash: String, result: String) {
        let Ok(mut entries) = self.entries.lock() else {
            return;
        };
        if entries.len() >= self.max_entries {
            // Evict oldest entry
            if let Some(oldest_key) = entries
                .iter()
                .min_by_key(|(_, v)| v.created_at)
                .map(|(k, _)| k.clone())
            {
                entries.remove(&oldest_key);
            }
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        entries.insert(
            hash,
            CacheEntry {
                result,
                created_at: now,
            },
        );
    }

    pub fn clear(&self) {
        if let Ok(mut entries) = self.entries.lock() {
            entries.clear();
        }
    }

    pub fn len(&self) -> usize {
        self.entries.lock().map(|e| e.len()).unwrap_or(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for MediaAttachmentCache {
    fn default() -> Self {
        Self::new(256)
    }
}
