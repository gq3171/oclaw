//! Hook execution strategies — parallel, sequential, first-wins, merge.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum HookStrategy {
    #[default]
    Sequential,
    Parallel,
    FirstWins,
    Merge(MergeStrategy),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MergeStrategy {
    Concat,
    JsonMerge,
    Last,
}

/// Configuration for hook execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookExecutorConfig {
    pub strategy: HookStrategy,
    /// Per-hook timeout in milliseconds.
    pub timeout_ms: u64,
}

impl Default for HookExecutorConfig {
    fn default() -> Self {
        Self {
            strategy: HookStrategy::Sequential,
            timeout_ms: 5000,
        }
    }
}

/// Merge two JSON values (shallow object merge, second wins on conflict).
pub fn json_merge(base: serde_json::Value, overlay: serde_json::Value) -> serde_json::Value {
    match (base, overlay) {
        (serde_json::Value::Object(mut a), serde_json::Value::Object(b)) => {
            for (k, v) in b {
                a.insert(k, v);
            }
            serde_json::Value::Object(a)
        }
        (_, overlay) => overlay,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn json_merge_objects() {
        let a = json!({"x": 1, "y": 2});
        let b = json!({"y": 3, "z": 4});
        let merged = json_merge(a, b);
        assert_eq!(merged, json!({"x": 1, "y": 3, "z": 4}));
    }

    #[test]
    fn json_merge_non_object_overlay_wins() {
        let a = json!({"x": 1});
        let b = json!("override");
        assert_eq!(json_merge(a, b), json!("override"));
    }

    #[test]
    fn default_config() {
        let cfg = HookExecutorConfig::default();
        assert_eq!(cfg.timeout_ms, 5000);
        assert!(matches!(cfg.strategy, HookStrategy::Sequential));
    }
}
