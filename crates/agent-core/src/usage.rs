//! Usage accumulation across LLM calls within an agent session.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageSummary {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub total_calls: u32,
    pub total_cost_usd: f64,
}

#[derive(Debug, Default)]
pub struct UsageAccumulator {
    inner: UsageSummary,
}

impl UsageAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record usage from a single LLM call.
    pub fn record(&mut self, usage: &oclaws_llm_core::chat::Usage) {
        self.inner.input_tokens += usage.prompt_tokens as i64;
        self.inner.output_tokens += usage.completion_tokens as i64;
        self.inner.total_calls += 1;
    }

    /// Record usage with optional cache fields.
    pub fn record_with_cache(
        &mut self,
        input: i64,
        output: i64,
        cache_read: i64,
        cache_write: i64,
    ) {
        self.inner.input_tokens += input;
        self.inner.output_tokens += output;
        self.inner.cache_read_tokens += cache_read;
        self.inner.cache_write_tokens += cache_write;
        self.inner.total_calls += 1;
    }

    /// Add estimated cost (caller computes based on model pricing).
    pub fn add_cost(&mut self, cost_usd: f64) {
        self.inner.total_cost_usd += cost_usd;
    }

    /// Get current summary snapshot.
    pub fn summary(&self) -> &UsageSummary {
        &self.inner
    }

    /// Reset all counters.
    pub fn reset(&mut self) {
        self.inner = UsageSummary::default();
    }

    /// Total tokens (input + output).
    pub fn total_tokens(&self) -> i64 {
        self.inner.input_tokens + self.inner.output_tokens
    }
}
