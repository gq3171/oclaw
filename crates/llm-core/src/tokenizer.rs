use serde::{Deserialize, Serialize};
use tiktoken_rs::CoreBPE;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenCount {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

impl TokenCount {
    pub fn new(prompt: usize, completion: usize) -> Self {
        Self {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
        }
    }
}

pub struct TokenCounter;

impl TokenCounter {
    fn bpe_for_model(model: &str) -> Option<CoreBPE> {
        tiktoken_rs::get_bpe_from_model(model).ok()
    }

    /// Fallback estimation when no BPE tokenizer is available.
    fn estimate_fallback(text: &str) -> usize {
        // ~4 chars per token is a reasonable cross-model heuristic
        text.len().div_ceil(4)
    }

    pub fn count(text: &str, model: &str) -> TokenCount {
        let tokens = match Self::bpe_for_model(model) {
            Some(bpe) => bpe.encode_with_special_tokens(text).len(),
            None => Self::estimate_fallback(text),
        };
        TokenCount::new(tokens, 0)
    }

    /// Kept for backward compat — delegates to `count`.
    pub fn estimate(text: &str, model: &str) -> TokenCount {
        Self::count(text, model)
    }

    pub fn estimate_messages(messages: &[super::chat::ChatMessage], model: &str) -> TokenCount {
        let bpe = Self::bpe_for_model(model);
        let mut total = 0;

        for message in messages {
            total += match &bpe {
                Some(b) => b.encode_with_special_tokens(&message.content).len(),
                None => Self::estimate_fallback(&message.content),
            };
            // per-message overhead (role, separators)
            total += 4;
            if let Some(name) = &message.name {
                total += match &bpe {
                    Some(b) => b.encode_with_special_tokens(name).len(),
                    None => Self::estimate_fallback(name),
                };
            }
        }
        // reply priming
        total += 3;
        TokenCount::new(total, 0)
    }

    pub fn max_tokens(model: &str) -> Option<usize> {
        let m = model.to_lowercase();
        if m.contains("gpt-4o") {
            return Some(128000);
        }
        if m.contains("gpt-4-turbo") {
            return Some(128000);
        }
        if m.contains("gpt-4-32k") {
            return Some(32768);
        }
        if m.contains("gpt-4") {
            return Some(8192);
        }
        if m.contains("gpt-3.5-turbo-16k") {
            return Some(16385);
        }
        if m.contains("gpt-3.5") {
            return Some(4096);
        }
        if m.contains("claude-3") || m.contains("claude-4") {
            return Some(200000);
        }
        if m.contains("gemini-pro") {
            return Some(32768);
        }
        if m.contains("gemini-1.5") {
            return Some(1048576);
        }
        None
    }

    pub fn remaining_tokens(model: &str, used: &TokenCount) -> Option<usize> {
        Self::max_tokens(model).map(|max| max.saturating_sub(used.total_tokens))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_estimation() {
        let text = "Hello world this is a test";
        let count = TokenCounter::estimate(text, "gpt-4");

        assert!(count.total_tokens > 0);
    }

    #[test]
    fn test_max_tokens() {
        assert_eq!(TokenCounter::max_tokens("gpt-4"), Some(8192));
        assert_eq!(TokenCounter::max_tokens("gpt-3.5-turbo"), Some(4096));
        assert_eq!(TokenCounter::max_tokens("unknown-model"), None);
    }

    #[test]
    fn test_remaining_tokens() {
        let used = TokenCount::new(1000, 500);
        let remaining = TokenCounter::remaining_tokens("gpt-4", &used);

        assert_eq!(remaining, Some(8192 - 1500));
    }
}
