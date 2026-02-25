//! Context Window Guard — prevents context overflow by proactively managing token budgets.

use oclaws_llm_core::chat::{ChatMessage, MessageRole};
use oclaws_llm_core::tokenizer::TokenCounter;

const HARD_MIN_TOKENS: usize = 16_000;
const WARNING_THRESHOLD: usize = 32_000;
const INPUT_BUDGET_RATIO: f64 = 0.75;
const TOOL_RESULT_BUDGET_RATIO: f64 = 0.50;

#[derive(Debug, Clone)]
pub struct ContextGuardConfig {
    pub context_window: usize,
    pub input_budget_ratio: f64,
    pub tool_result_budget_ratio: f64,
}

impl Default for ContextGuardConfig {
    fn default() -> Self {
        Self {
            context_window: 128_000,
            input_budget_ratio: INPUT_BUDGET_RATIO,
            tool_result_budget_ratio: TOOL_RESULT_BUDGET_RATIO,
        }
    }
}

#[derive(Debug)]
pub enum GuardAction {
    Ok,
    Warning { used: usize, budget: usize },
    TruncateNeeded { used: usize, budget: usize },
}

pub struct ContextGuard {
    config: ContextGuardConfig,
}

impl ContextGuard {
    pub fn new(config: ContextGuardConfig) -> Self {
        Self { config }
    }

    pub fn from_context_window(context_window: usize) -> Self {
        Self::new(ContextGuardConfig {
            context_window,
            ..Default::default()
        })
    }

    fn input_budget(&self) -> usize {
        let budget = (self.config.context_window as f64 * self.config.input_budget_ratio) as usize;
        budget.max(HARD_MIN_TOKENS)
    }

    fn tool_result_limit(&self) -> usize {
        let budget = self.input_budget();
        (budget as f64 * self.config.tool_result_budget_ratio) as usize
    }

    /// Estimate total tokens used by messages (4 chars ≈ 1 token).
    fn estimate_tokens(messages: &[ChatMessage], model: &str) -> usize {
        TokenCounter::estimate_messages(messages, model).total_tokens
    }

    /// Check whether the current message list is within budget.
    pub fn check_budget(&self, messages: &[ChatMessage], model: &str) -> GuardAction {
        let used = Self::estimate_tokens(messages, model);
        let budget = self.input_budget();

        if used > budget {
            GuardAction::TruncateNeeded { used, budget }
        } else if used > WARNING_THRESHOLD && used > budget * 3 / 4 {
            GuardAction::Warning { used, budget }
        } else {
            GuardAction::Ok
        }
    }

    /// Truncate oversized tool results in-place, oldest first.
    pub fn truncate_tool_results(&self, messages: &mut [ChatMessage], model: &str) {
        let budget = self.input_budget();
        let limit_per_tool = self.tool_result_limit();

        // First pass: truncate any single tool result exceeding per-tool limit
        for msg in messages.iter_mut() {
            if msg.role == MessageRole::Tool {
                let tok = TokenCounter::estimate(&msg.content, model).total_tokens;
                if tok > limit_per_tool {
                    let max_chars = limit_per_tool * 4;
                    if msg.content.len() > max_chars {
                        let cut = msg.content[..max_chars]
                            .rfind('\n')
                            .unwrap_or(max_chars);
                        msg.content = format!(
                            "{}\n\n[Truncated by context guard — original ~{} tokens, limit {}]",
                            &msg.content[..cut],
                            tok,
                            limit_per_tool
                        );
                    }
                }
            }
        }

        // Second pass: if still over budget, progressively halve oldest tool results
        let mut iterations = 0;
        const MIN_TOOL_CHARS: usize = 200;
        while iterations < 5 {
            let used = Self::estimate_tokens(messages, model);
            if used <= budget {
                break;
            }
            iterations += 1;

            // Find the largest tool result
            let Some((idx, _)) = messages
                .iter()
                .enumerate()
                .filter(|(_, m)| m.role == MessageRole::Tool)
                .max_by_key(|(_, m)| m.content.len())
            else {
                break;
            };

            // Stop if the largest tool result is already small — no progress possible
            if messages[idx].content.len() <= MIN_TOOL_CHARS {
                break;
            }

            let content = &messages[idx].content;
            let half = content.len() / 2;
            let cut = content[..half].rfind('\n').unwrap_or(half);
            messages[idx].content = format!(
                "{}\n\n[Truncated by context guard — halved for budget]",
                &content[..cut]
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_msg(role: MessageRole, content: &str) -> ChatMessage {
        ChatMessage {
            role,
            content: content.to_string(),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn test_check_budget_ok() {
        let guard = ContextGuard::from_context_window(128_000);
        let msgs = vec![make_msg(MessageRole::User, "hello")];
        match guard.check_budget(&msgs, "gpt-4o") {
            GuardAction::Ok => {}
            other => panic!("Expected Ok, got {:?}", other),
        }
    }

    #[test]
    fn test_truncate_large_tool_result() {
        // HARD_MIN_TOKENS=16000, tool_result_budget_ratio=0.5 → limit=8000 tokens ≈ 32000 chars
        // Use "unknown-model" so TokenCounter uses fallback (4 chars/token) instead of BPE.
        let guard = ContextGuard::from_context_window(1000);
        let big = "x".repeat(50_000);
        let mut msgs = vec![
            make_msg(MessageRole::User, "hi"),
            make_msg(MessageRole::Tool, &big),
        ];
        guard.truncate_tool_results(&mut msgs, "unknown-model");
        assert!(msgs[1].content.len() < big.len());
        assert!(msgs[1].content.contains("[Truncated by context guard"));
    }
}
