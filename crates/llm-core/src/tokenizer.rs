use serde::{Deserialize, Serialize};

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
    pub fn estimate(text: &str, model: &str) -> TokenCount {
        let tokens_per_word = Self::tokens_per_word(model);
        let word_count = text.split_whitespace().count();
        let estimated = (word_count as f64 * tokens_per_word) as usize;

        TokenCount::new(estimated, 0)
    }

    pub fn estimate_messages(messages: &[super::chat::ChatMessage], model: &str) -> TokenCount {
        let mut total = 0;

        for message in messages {
            let tokens = Self::estimate(&message.content, model);
            total += tokens.total_tokens;

            if let Some(name) = &message.name {
                total += name.len() / 4;
            }
        }

        total += messages.len() * 4;

        TokenCount::new(total, 0)
    }

    fn tokens_per_word(model: &str) -> f64 {
        let model_lower = model.to_lowercase();

        if model_lower.contains("gpt-4") || model_lower.contains("claude") {
            0.75
        } else if model_lower.contains("gpt-3.5") {
            0.8
        } else if model_lower.contains("gemini") {
            0.7
        } else {
            0.75
        }
    }

    pub fn max_tokens(model: &str) -> Option<usize> {
        let model_lower = model.to_lowercase();

        if model_lower.contains("gpt-4-32k") {
            Some(32768)
        } else if model_lower.contains("gpt-4") {
            Some(8192)
        } else if model_lower.contains("gpt-3.5-turbo-16k") {
            Some(16385)
        } else if model_lower.contains("gpt-3.5") {
            Some(4096)
        } else if model_lower.contains("claude-3-opus") {
            Some(200000)
        } else if model_lower.contains("claude-3-sonnet") {
            Some(200000)
        } else if model_lower.contains("claude-3-haiku") {
            Some(200000)
        } else if model_lower.contains("gemini-pro") {
            Some(32768)
        } else {
            None
        }
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
