use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

/// In-memory LRU cache for embedding vectors, keyed by text hash.
pub struct EmbeddingCache {
    cache: HashMap<u64, CacheEntry>,
    max_entries: usize,
    hits: AtomicU64,
    misses: AtomicU64,
    access_counter: u64,
}

struct CacheEntry {
    embedding: Vec<f32>,
    last_access: u64,
}

impl EmbeddingCache {
    pub fn new(max_entries: usize) -> Self {
        Self {
            cache: HashMap::new(),
            max_entries,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            access_counter: 0,
        }
    }

    pub fn get(&mut self, text: &str) -> Option<&Vec<f32>> {
        let key = hash_text(text);
        if let Some(entry) = self.cache.get_mut(&key) {
            self.hits.fetch_add(1, Ordering::Relaxed);
            self.access_counter += 1;
            entry.last_access = self.access_counter;
            Some(&entry.embedding)
        } else {
            self.misses.fetch_add(1, Ordering::Relaxed);
            None
        }
    }

    pub fn insert(&mut self, text: &str, embedding: Vec<f32>) {
        if self.cache.len() >= self.max_entries {
            self.evict_lru();
        }
        self.access_counter += 1;
        let key = hash_text(text);
        self.cache.insert(
            key,
            CacheEntry {
                embedding,
                last_access: self.access_counter,
            },
        );
    }

    pub fn hit_rate(&self) -> f64 {
        let h = self.hits.load(Ordering::Relaxed) as f64;
        let m = self.misses.load(Ordering::Relaxed) as f64;
        let total = h + m;
        if total == 0.0 { 0.0 } else { h / total }
    }

    pub fn len(&self) -> usize {
        self.cache.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    fn evict_lru(&mut self) {
        if let Some((&oldest_key, _)) = self.cache.iter().min_by_key(|(_, e)| e.last_access) {
            self.cache.remove(&oldest_key);
        }
    }
}

impl Default for EmbeddingCache {
    fn default() -> Self {
        Self::new(10_000)
    }
}

fn hash_text(text: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_and_get() {
        let mut c = EmbeddingCache::new(100);
        c.insert("hello", vec![1.0, 2.0]);
        assert!(c.get("hello").is_some());
        assert!(c.get("world").is_none());
    }

    #[test]
    fn eviction() {
        let mut c = EmbeddingCache::new(2);
        c.insert("a", vec![1.0]);
        c.insert("b", vec![2.0]);
        c.insert("c", vec![3.0]);
        assert_eq!(c.len(), 2);
        // "a" should have been evicted
        assert!(c.get("a").is_none());
    }

    #[test]
    fn hit_rate() {
        let mut c = EmbeddingCache::new(10);
        c.insert("x", vec![1.0]);
        c.get("x"); // hit
        c.get("y"); // miss
        assert!((c.hit_rate() - 0.5).abs() < 0.01);
    }
}
