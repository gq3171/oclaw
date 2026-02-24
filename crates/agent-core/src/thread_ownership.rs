/// Thread-ownership: in-memory mention tracking with TTL + ownership claims.
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub struct ThreadOwnership {
    /// thread_key → agent_id that owns it
    owners: HashMap<String, (String, Instant)>,
    /// thread_key → timestamp of last @-mention
    mentions: HashMap<String, Instant>,
    ttl: Duration,
}

impl ThreadOwnership {
    pub fn new(ttl_secs: u64) -> Self {
        Self {
            owners: HashMap::new(),
            mentions: HashMap::new(),
            ttl: Duration::from_secs(ttl_secs),
        }
    }

    fn clean_expired(&mut self) {
        let now = Instant::now();
        self.mentions.retain(|_, ts| now.duration_since(*ts) < self.ttl);
        self.owners.retain(|_, (_, ts)| now.duration_since(*ts) < self.ttl);
    }

    /// Record that this agent was @-mentioned in a thread.
    pub fn record_mention(&mut self, thread_key: &str) {
        self.clean_expired();
        self.mentions.insert(thread_key.to_string(), Instant::now());
    }

    /// Was this agent recently mentioned in the thread?
    pub fn was_mentioned(&self, thread_key: &str) -> bool {
        self.mentions.get(thread_key)
            .is_some_and(|ts| Instant::now().duration_since(*ts) < self.ttl)
    }

    /// Try to claim ownership of a thread. Returns true if claimed or already owned.
    /// Returns false if another agent owns it.
    pub fn try_claim(&mut self, thread_key: &str, agent_id: &str) -> bool {
        self.clean_expired();
        if let Some((owner, _)) = self.owners.get(thread_key) {
            return owner == agent_id;
        }
        self.owners.insert(thread_key.to_string(), (agent_id.to_string(), Instant::now()));
        true
    }

    /// Check if sending is allowed: either owns thread, was mentioned, or no owner yet.
    pub fn can_send(&mut self, thread_key: &str, agent_id: &str) -> bool {
        if self.was_mentioned(thread_key) {
            return true;
        }
        self.try_claim(thread_key, agent_id)
    }
}

impl Default for ThreadOwnership {
    fn default() -> Self {
        Self::new(300) // 5 min TTL
    }
}
