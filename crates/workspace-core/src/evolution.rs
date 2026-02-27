//! Autonomous evolution — periodic self-reflection and growth for agents.
//!
//! After every N substantial messages, the agent enters a brief reflection turn
//! where it can read and update SOUL.md, USER.md, and the daily memory log.
//! This mirrors the Node OpenClaw "evolution" pipeline step.

use crate::files::Workspace;
use serde::{Deserialize, Serialize};

/// Token returned when the agent decides no evolution occurred.
pub const EVOLUTION_OK_TOKEN: &str = "EVOLUTION_OK";

/// Configuration for the autonomous evolution mechanism.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvolutionConfig {
    /// Whether the evolution step is enabled (default: true).
    pub enabled: bool,

    /// Trigger a reflection session every N counted messages (default: 20).
    pub trigger_every_n_messages: u64,

    /// Only count a message toward the trigger if the session used at least
    /// this many tokens. Set to 0 to count every message regardless of size.
    pub min_tokens_to_count: u64,

    /// Custom evolution prompt (None = use the built-in `evolution_system_prompt()`).
    pub prompt: Option<String>,
}

impl Default for EvolutionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            trigger_every_n_messages: 20,
            // Default to 0 so every message is counted while real token
            // counts are not yet wired through the pipeline.  When the
            // agent exposes usage, this can be raised to e.g. 200.
            min_tokens_to_count: 0,
            prompt: None,
        }
    }
}

/// Persistent counters that track when the next evolution should fire.
///
/// Stored as `.evolution_state.json` in the workspace root.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EvolutionState {
    /// Total messages counted toward evolution (only substantial ones).
    pub message_count: u64,

    /// The value of `message_count` at the last evolution trigger.
    pub last_evolved_at_message: u64,

    /// ISO-8601 date of the last evolution, if it has ever run.
    pub last_evolved_date: Option<String>,

    /// How many evolution sessions have been completed.
    pub evolution_count: u64,
}

impl EvolutionState {
    /// Load from `.evolution_state.json`.  Returns a zeroed default on any error.
    pub async fn load(ws: &Workspace) -> Self {
        let path = ws.evolution_state_path();
        match tokio::fs::read_to_string(&path).await {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Persist to `.evolution_state.json`.
    pub async fn save(&self, ws: &Workspace) -> anyhow::Result<()> {
        let path = ws.evolution_state_path();
        let json = serde_json::to_string_pretty(self)?;
        tokio::fs::write(&path, json).await?;
        Ok(())
    }

    /// Increment `message_count` when the session used at least
    /// `config.min_tokens_to_count` tokens.
    pub fn tick(&mut self, tokens: u64, config: &EvolutionConfig) {
        if tokens >= config.min_tokens_to_count {
            self.message_count += 1;
        }
    }
}

/// Return `true` when an evolution session should be triggered.
///
/// Conditions:
/// * Evolution is enabled.
/// * At least one message has been counted.
/// * The number of messages since the last evolution meets the configured interval.
pub fn should_run_evolution(state: &EvolutionState, config: &EvolutionConfig) -> bool {
    config.enabled
        && state.message_count > 0
        && (state.message_count - state.last_evolved_at_message) >= config.trigger_every_n_messages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tick_below_min_does_not_count() {
        let config = EvolutionConfig {
            min_tokens_to_count: 200,
            ..Default::default()
        };
        let mut state = EvolutionState::default();
        state.tick(5, &config); // 5 < 200 → no increment
        assert_eq!(state.message_count, 0);
    }

    #[test]
    fn tick_at_min_counts() {
        let config = EvolutionConfig {
            min_tokens_to_count: 200,
            ..Default::default()
        };
        let mut state = EvolutionState::default();
        state.tick(200, &config);
        assert_eq!(state.message_count, 1);
    }

    #[test]
    fn tick_above_min_counts() {
        let config = EvolutionConfig {
            min_tokens_to_count: 200,
            ..Default::default()
        };
        let mut state = EvolutionState::default();
        state.tick(999, &config);
        assert_eq!(state.message_count, 1);
    }

    #[test]
    fn tick_zero_min_always_counts() {
        let config = EvolutionConfig::default(); // min = 0
        let mut state = EvolutionState::default();
        state.tick(0, &config);
        assert_eq!(state.message_count, 1);
    }

    #[test]
    fn should_run_when_interval_met() {
        let config = EvolutionConfig {
            trigger_every_n_messages: 20,
            ..Default::default()
        };
        let state = EvolutionState {
            message_count: 20,
            last_evolved_at_message: 0,
            ..Default::default()
        };
        assert!(should_run_evolution(&state, &config));
    }

    #[test]
    fn should_not_run_before_interval() {
        let config = EvolutionConfig {
            trigger_every_n_messages: 20,
            ..Default::default()
        };
        let state = EvolutionState {
            message_count: 19,
            last_evolved_at_message: 0,
            ..Default::default()
        };
        assert!(!should_run_evolution(&state, &config));
    }

    #[test]
    fn should_not_run_at_zero_messages() {
        let config = EvolutionConfig::default();
        let state = EvolutionState::default();
        assert!(!should_run_evolution(&state, &config));
    }

    #[test]
    fn should_not_run_when_disabled() {
        let config = EvolutionConfig {
            enabled: false,
            trigger_every_n_messages: 1,
            ..Default::default()
        };
        let state = EvolutionState {
            message_count: 100,
            last_evolved_at_message: 0,
            ..Default::default()
        };
        assert!(!should_run_evolution(&state, &config));
    }

    #[test]
    fn should_run_after_previous_evolution() {
        // Evolved at 20, now at 40 → trigger again.
        let config = EvolutionConfig {
            trigger_every_n_messages: 20,
            ..Default::default()
        };
        let state = EvolutionState {
            message_count: 40,
            last_evolved_at_message: 20,
            ..Default::default()
        };
        assert!(should_run_evolution(&state, &config));
    }

    #[test]
    fn should_not_run_mid_interval_after_evolution() {
        let config = EvolutionConfig {
            trigger_every_n_messages: 20,
            ..Default::default()
        };
        let state = EvolutionState {
            message_count: 35,
            last_evolved_at_message: 20,
            ..Default::default()
        };
        assert!(!should_run_evolution(&state, &config)); // 35 - 20 = 15 < 20
    }
}
