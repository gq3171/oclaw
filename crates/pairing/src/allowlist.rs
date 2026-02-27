//! Allowlist management for paired devices/users.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Allowlist {
    entries: HashSet<String>,
}

impl Allowlist {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, id: &str) {
        self.entries.insert(id.to_string());
    }

    pub fn remove(&mut self, id: &str) -> bool {
        self.entries.remove(id)
    }

    pub fn contains(&self, id: &str) -> bool {
        self.entries.contains(id)
    }

    pub fn list(&self) -> Vec<&str> {
        self.entries.iter().map(|s| s.as_str()).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allowlist_crud() {
        let mut al = Allowlist::new();
        assert!(al.is_empty());

        al.add("user-1");
        al.add("user-2");
        assert_eq!(al.len(), 2);
        assert!(al.contains("user-1"));

        al.remove("user-1");
        assert!(!al.contains("user-1"));
        assert_eq!(al.len(), 1);
    }
}
