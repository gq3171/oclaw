//! Automatic memory capture — extracts key information from conversations
//! and stores them for future recall.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoCaptureConfig {
    pub enabled: bool,
    /// Minimum message count before triggering capture.
    pub min_message_count: usize,
    pub capture_facts: bool,
    pub capture_preferences: bool,
    pub capture_decisions: bool,
    /// Max memories to store per session.
    pub max_captures_per_session: usize,
}

impl Default for AutoCaptureConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_message_count: 3,
            capture_facts: true,
            capture_preferences: true,
            capture_decisions: true,
            max_captures_per_session: 5,
        }
    }
}

/// A single captured memory item before storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedMemory {
    pub content: String,
    pub category: CaptureCategory,
    pub confidence: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CaptureCategory {
    Fact,
    Preference,
    Decision,
    Instruction,
}

impl std::fmt::Display for CaptureCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fact => write!(f, "fact"),
            Self::Preference => write!(f, "preference"),
            Self::Decision => write!(f, "decision"),
            Self::Instruction => write!(f, "instruction"),
        }
    }
}

/// Check whether a conversation is long enough to warrant auto-capture.
pub fn should_capture(message_count: usize, config: &AutoCaptureConfig) -> bool {
    config.enabled && message_count >= config.min_message_count
}

/// Filter captured memories by the config's category flags.
pub fn filter_by_config(
    items: &[CapturedMemory],
    config: &AutoCaptureConfig,
) -> Vec<CapturedMemory> {
    items
        .iter()
        .filter(|m| match m.category {
            CaptureCategory::Fact => config.capture_facts,
            CaptureCategory::Preference => config.capture_preferences,
            CaptureCategory::Decision => config.capture_decisions,
            CaptureCategory::Instruction => true,
        })
        .take(config.max_captures_per_session)
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_capture_threshold() {
        let cfg = AutoCaptureConfig::default();
        assert!(!should_capture(1, &cfg));
        assert!(!should_capture(2, &cfg));
        assert!(should_capture(3, &cfg));
        assert!(should_capture(10, &cfg));
    }

    #[test]
    fn should_capture_disabled() {
        let cfg = AutoCaptureConfig { enabled: false, ..Default::default() };
        assert!(!should_capture(100, &cfg));
    }

    #[test]
    fn filter_respects_config() {
        let items = vec![
            CapturedMemory { content: "fact1".into(), category: CaptureCategory::Fact, confidence: 0.9 },
            CapturedMemory { content: "pref1".into(), category: CaptureCategory::Preference, confidence: 0.8 },
            CapturedMemory { content: "dec1".into(), category: CaptureCategory::Decision, confidence: 0.7 },
        ];
        let cfg = AutoCaptureConfig {
            capture_facts: true,
            capture_preferences: false,
            capture_decisions: true,
            ..Default::default()
        };
        let filtered = filter_by_config(&items, &cfg);
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].category, CaptureCategory::Fact);
        assert_eq!(filtered[1].category, CaptureCategory::Decision);
    }

    #[test]
    fn filter_respects_max() {
        let items: Vec<CapturedMemory> = (0..10)
            .map(|i| CapturedMemory {
                content: format!("item{i}"),
                category: CaptureCategory::Fact,
                confidence: 0.9,
            })
            .collect();
        let cfg = AutoCaptureConfig { max_captures_per_session: 3, ..Default::default() };
        let filtered = filter_by_config(&items, &cfg);
        assert_eq!(filtered.len(), 3);
    }
}
