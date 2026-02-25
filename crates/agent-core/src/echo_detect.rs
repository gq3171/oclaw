use std::collections::{HashSet, VecDeque};

/// LRU-based echo detection to prevent responding to own messages.
pub struct EchoTracker {
    recent: VecDeque<String>,
    set: HashSet<String>,
    capacity: usize,
}

impl EchoTracker {
    pub fn new(capacity: usize) -> Self {
        Self {
            recent: VecDeque::with_capacity(capacity),
            set: HashSet::new(),
            capacity,
        }
    }

    pub fn remember(&mut self, text: &str) {
        let key = text.to_string();
        if self.set.contains(&key) {
            return;
        }
        if self.recent.len() >= self.capacity
            && let Some(old) = self.recent.pop_front()
        {
            self.set.remove(&old);
        }
        self.set.insert(key.clone());
        self.recent.push_back(key);
    }

    /// Check if text was recently sent by us (echo). Consumes the entry on match
    /// so that future messages with the same content are not falsely blocked.
    pub fn has(&mut self, text: &str) -> bool {
        if self.set.remove(text) {
            self.recent.retain(|s| s != text);
            true
        } else {
            false
        }
    }

    pub fn forget(&mut self, text: &str) {
        self.set.remove(text);
    }

    /// Build a composite key for group echo detection.
    pub fn combined_key(session_key: &str, body: &str) -> String {
        format!("{}:{}", session_key, body)
    }
}

impl Default for EchoTracker {
    fn default() -> Self {
        Self::new(100)
    }
}
