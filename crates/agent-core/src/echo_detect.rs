use std::collections::{HashSet, VecDeque};

/// Normalize text: trim, collapse whitespace, lowercase.
fn normalize(text: &str) -> String {
    text.split_whitespace()
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Build the normalized key with `~:` prefix to avoid collisions with exact keys.
fn norm_key(text: &str) -> String {
    format!("~:{}", normalize(text))
}

/// LRU-based echo detection to prevent responding to own messages.
///
/// Each `remember()` stores two entries: the exact string and a normalized
/// (`~:<lowercase-collapsed>`) key. `has()` tries exact match first, then
/// falls back to the normalized key, enabling case- and whitespace-insensitive
/// echo detection.
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

    /// Insert a single key into the LRU structure, evicting the oldest entry if
    /// the capacity is reached. No-ops if the key is already present.
    fn insert_entry(&mut self, key: String) {
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

    /// Record a message sent by us. Stores both the exact key and a normalized
    /// key (`~:<lowercase-collapsed>`) so that later `has()` calls can match
    /// on either form. Both keys share the same capacity budget.
    pub fn remember(&mut self, text: &str) {
        let exact = text.to_string();
        if self.set.contains(&exact) {
            return;
        }
        self.insert_entry(exact);
        // Also store normalized key for fuzzy (case/whitespace) matching.
        self.insert_entry(norm_key(text));
    }

    /// Check if text was recently sent by us (echo). Consumes the matching
    /// entry on hit so that future messages with the same content are not
    /// falsely blocked.
    ///
    /// Match order:
    /// 1. Exact string — fastest path.  Also removes the paired normalized key
    ///    to prevent a secondary normalized match on the same original message.
    /// 2. Normalized (`~:...`) — handles different casing / whitespace.
    pub fn has(&mut self, text: &str) -> bool {
        // 1. Exact match — consume both exact and normalized entries.
        if self.set.remove(text) {
            self.recent.retain(|s| s != text);
            // Also consume the normalized counterpart so it can't fire again.
            let nk = norm_key(text);
            if self.set.remove(&nk) {
                self.recent.retain(|s| s != &nk);
            }
            return true;
        }
        // 2. Normalized fallback.
        let nk = norm_key(text);
        if self.set.remove(&nk) {
            self.recent.retain(|s| s != &nk);
            return true;
        }
        false
    }

    /// Explicitly forget a message (exact + normalized key).
    pub fn forget(&mut self, text: &str) {
        self.set.remove(text);
        self.set.remove(&norm_key(text));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_detected() {
        let mut t = EchoTracker::new(100);
        t.remember("hello world");
        assert!(t.has("hello world"));
    }

    #[test]
    fn normalized_match_case_insensitive() {
        let mut t = EchoTracker::new(100);
        t.remember("hello world");
        // Different case should still be detected via normalized key.
        assert!(t.has("Hello World"));
    }

    #[test]
    fn normalized_match_collapsed_whitespace() {
        let mut t = EchoTracker::new(100);
        t.remember("hello world");
        // Extra spaces should be collapsed during normalization.
        assert!(t.has("hello  world"));
    }

    #[test]
    fn consume_semantics_exact() {
        let mut t = EchoTracker::new(100);
        t.remember("msg");
        assert!(t.has("msg"));
        // Second call should not find it (consumed).
        assert!(!t.has("msg"));
    }

    #[test]
    fn consume_semantics_normalized() {
        let mut t = EchoTracker::new(100);
        t.remember("Hello World");
        assert!(t.has("hello  world")); // normalized match
        // Normalized key consumed; exact key still present.
        // A subsequent normalized call should not match.
        assert!(!t.has("hello  world"));
    }

    #[test]
    fn no_false_positive() {
        let mut t = EchoTracker::new(100);
        t.remember("foo");
        assert!(!t.has("bar"));
    }

    #[test]
    fn forget_removes_entry() {
        let mut t = EchoTracker::new(100);
        t.remember("bye");
        t.forget("bye");
        assert!(!t.has("bye"));
    }
}
