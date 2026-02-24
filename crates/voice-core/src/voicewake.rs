//! Voice wake trigger configuration management.
//!
//! Stores and retrieves wake word triggers (e.g. "openclaw", "claude", "computer")
//! persisted to a JSON file in the state directory.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::sync::Mutex;

const DEFAULT_TRIGGERS: &[&str] = &["openclaw", "claude", "computer"];
const MAX_TRIGGERS: usize = 32;
const MAX_TRIGGER_LEN: usize = 64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceWakeConfig {
    pub triggers: Vec<String>,
    #[serde(default)]
    pub updated_at_ms: i64,
}

impl Default for VoiceWakeConfig {
    fn default() -> Self {
        Self {
            triggers: default_triggers(),
            updated_at_ms: 0,
        }
    }
}

pub fn default_triggers() -> Vec<String> {
    DEFAULT_TRIGGERS.iter().map(|s| s.to_string()).collect()
}

/// Sanitize and normalize a list of trigger words.
pub fn normalize_triggers(triggers: &[String]) -> Vec<String> {
    let mut cleaned: Vec<String> = triggers
        .iter()
        .map(|w| w.trim().to_string())
        .filter(|w| !w.is_empty())
        .map(|w| {
            if w.len() > MAX_TRIGGER_LEN {
                w[..MAX_TRIGGER_LEN].to_string()
            } else {
                w
            }
        })
        .collect();

    // Deduplicate (case-insensitive)
    let mut seen = std::collections::HashSet::new();
    cleaned.retain(|w| seen.insert(w.to_lowercase()));

    // Cap at max
    cleaned.truncate(MAX_TRIGGERS);

    if cleaned.is_empty() {
        default_triggers()
    } else {
        cleaned
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Persistent voice wake config store.
pub struct VoiceWakeStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl VoiceWakeStore {
    pub fn new(state_dir: &Path) -> Self {
        let path = state_dir.join("settings").join("voicewake.json");
        Self {
            path,
            lock: Mutex::new(()),
        }
    }

    pub async fn load(&self) -> VoiceWakeConfig {
        match tokio::fs::read_to_string(&self.path).await {
            Ok(content) => serde_json::from_str::<VoiceWakeConfig>(&content)
                .map(|mut cfg| {
                    cfg.triggers = normalize_triggers(&cfg.triggers);
                    cfg
                })
                .unwrap_or_default(),
            Err(_) => VoiceWakeConfig::default(),
        }
    }

    pub async fn set_triggers(&self, triggers: Vec<String>) -> anyhow::Result<VoiceWakeConfig> {
        let _guard = self.lock.lock().await;
        let sanitized = normalize_triggers(&triggers);
        let config = VoiceWakeConfig {
            triggers: sanitized,
            updated_at_ms: now_ms(),
        };

        // Ensure parent directory exists
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Atomic write: write to temp file then rename
        let tmp = self.path.with_extension("json.tmp");
        let json = serde_json::to_string_pretty(&config)?;
        tokio::fs::write(&tmp, json.as_bytes()).await?;
        tokio::fs::rename(&tmp, &self.path).await?;

        Ok(config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_empty_returns_defaults() {
        let result = normalize_triggers(&[]);
        assert_eq!(result, default_triggers());
    }

    #[test]
    fn test_normalize_trims_and_deduplicates() {
        let input = vec![
            " hello ".to_string(),
            "Hello".to_string(),
            "world".to_string(),
        ];
        let result = normalize_triggers(&input);
        assert_eq!(result, vec!["hello", "world"]);
    }

    #[test]
    fn test_normalize_truncates_long_triggers() {
        let long = "a".repeat(100);
        let result = normalize_triggers(&[long]);
        assert_eq!(result[0].len(), MAX_TRIGGER_LEN);
    }

    #[test]
    fn test_normalize_caps_at_max() {
        let input: Vec<String> = (0..50).map(|i| format!("trigger{}", i)).collect();
        let result = normalize_triggers(&input);
        assert_eq!(result.len(), MAX_TRIGGERS);
    }
}
