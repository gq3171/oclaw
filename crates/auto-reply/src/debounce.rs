//! Inbound message deduplication.

use std::collections::HashMap;
use sha2::{Sha256, Digest};

/// Deduplicates inbound messages within a TTL window.
pub struct InboundDebounce {
    seen: HashMap<String, u64>,
    ttl_ms: u64,
}

impl InboundDebounce {
    pub fn new(ttl_ms: u64) -> Self {
        Self {
            seen: HashMap::new(),
            ttl_ms,
        }
    }

    /// Returns true if this message should be skipped (duplicate).
    pub fn should_skip(&mut self, body: &str, from: &str, now_ms: u64) -> bool {
        self.cleanup_stale(now_ms);
        let key = Self::hash_key(body, from);
        if self.seen.contains_key(&key) {
            return true;
        }
        self.seen.insert(key, now_ms);
        false
    }

    /// Remove entries older than TTL.
    pub fn cleanup_stale(&mut self, now_ms: u64) {
        self.seen.retain(|_, ts| now_ms.saturating_sub(*ts) < self.ttl_ms);
    }

    fn hash_key(body: &str, from: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(from.as_bytes());
        hasher.update(b"|");
        hasher.update(body.as_bytes());
        format!("{:x}", hasher.finalize())
    }

    pub fn len(&self) -> usize {
        self.seen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_same_message() {
        let mut d = InboundDebounce::new(5000);
        assert!(!d.should_skip("hello", "user1", 1000));
        assert!(d.should_skip("hello", "user1", 2000));
    }

    #[test]
    fn different_sender_not_deduped() {
        let mut d = InboundDebounce::new(5000);
        assert!(!d.should_skip("hello", "user1", 1000));
        assert!(!d.should_skip("hello", "user2", 1000));
    }

    #[test]
    fn expired_entries_cleaned() {
        let mut d = InboundDebounce::new(5000);
        assert!(!d.should_skip("hello", "user1", 1000));
        assert!(!d.should_skip("hello", "user1", 7000));
    }
}
