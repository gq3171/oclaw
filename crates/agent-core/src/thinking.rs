//! Thinking Mode — extended reasoning support for Claude and OpenAI o-series models.

use oclaw_llm_core::chat::ChatMessage;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningLevel {
    #[default]
    Off,
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Default)]
pub struct ThinkingConfig {
    pub level: ReasoningLevel,
}

/// A thinking block produced by extended reasoning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingBlock {
    pub content: String,
}

/// Strip thinking blocks from message content for downstream consumers.
pub fn drop_thinking_blocks(content: &str) -> String {
    let mut result = String::with_capacity(content.len());
    let mut in_thinking = false;

    for line in content.lines() {
        if line.trim() == "<thinking>" {
            in_thinking = true;
            continue;
        }
        if line.trim() == "</thinking>" {
            in_thinking = false;
            continue;
        }
        if !in_thinking {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(line);
        }
    }
    result
}

/// Extract thinking content from a message, returning (thinking, visible_content).
pub fn extract_thinking(content: &str) -> (Option<String>, String) {
    let mut thinking = String::new();
    let mut visible = String::new();
    let mut in_thinking = false;

    for line in content.lines() {
        if line.trim() == "<thinking>" {
            in_thinking = true;
            continue;
        }
        if line.trim() == "</thinking>" {
            in_thinking = false;
            continue;
        }
        if in_thinking {
            if !thinking.is_empty() {
                thinking.push('\n');
            }
            thinking.push_str(line);
        } else {
            if !visible.is_empty() {
                visible.push('\n');
            }
            visible.push_str(line);
        }
    }

    let thinking_opt = if thinking.is_empty() {
        None
    } else {
        Some(thinking)
    };
    (thinking_opt, visible)
}

/// Check if a model supports extended thinking.
///
/// Only models known to support extended thinking are matched.
/// Older models like claude-3-haiku, claude-3-sonnet (non-3.5) are excluded.
pub fn supports_thinking(model: &str) -> bool {
    let m = model.to_lowercase();

    // Claude models with extended thinking support
    let claude_thinking = [
        "claude-3-5-sonnet",
        "claude-3.5-sonnet",
        "claude-3-5-opus",
        "claude-3.5-opus",
        "claude-3-7-sonnet",
        "claude-3.7-sonnet",
        "claude-4",
        "claude-sonnet-4",
        "claude-opus-4",
    ];
    if claude_thinking.iter().any(|pat| m.contains(pat)) {
        return true;
    }

    // OpenAI o-series reasoning models (exclude mini variants)
    if (m.starts_with("o1") || m.starts_with("o3") || m.starts_with("o4"))
        && !m.starts_with("o1-mini")
        && !m.starts_with("o3-mini")
    {
        return true;
    }

    // DeepSeek reasoning models
    if m.contains("deepseek-r1") || m.contains("deepseek-reasoner") {
        return true;
    }

    false
}

/// Build the thinking parameter value for Anthropic API requests.
pub fn anthropic_thinking_param(level: ReasoningLevel) -> Option<serde_json::Value> {
    match level {
        ReasoningLevel::Off => None,
        _ => {
            let budget = match level {
                ReasoningLevel::Low => 1024,
                ReasoningLevel::Medium => 4096,
                ReasoningLevel::High => 16384,
                ReasoningLevel::Off => unreachable!(),
            };
            Some(serde_json::json!({
                "type": "enabled",
                "budget_tokens": budget
            }))
        }
    }
}

/// Strip thinking blocks from all assistant messages in a history.
pub fn strip_thinking_from_history(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    messages
        .iter()
        .map(|msg| {
            if msg.role == oclaw_llm_core::chat::MessageRole::Assistant {
                let cleaned = drop_thinking_blocks(&msg.content);
                ChatMessage {
                    content: cleaned,
                    ..msg.clone()
                }
            } else {
                msg.clone()
            }
        })
        .collect()
}

/// Apply thinking configuration to a request, with graceful fallback.
///
/// If the model does not support thinking, silently downgrades to Off and logs a warning.
/// Returns the effective reasoning level actually applied.
pub fn apply_thinking(config: &ThinkingConfig, model: &str) -> ReasoningLevel {
    if config.level == ReasoningLevel::Off {
        return ReasoningLevel::Off;
    }

    if !supports_thinking(model) {
        tracing::warn!(
            "Model '{}' does not support extended thinking; falling back to normal mode",
            model
        );
        return ReasoningLevel::Off;
    }

    config.level
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_drop_thinking_blocks() {
        let input = "Hello\n<thinking>\nI need to think\n</thinking>\nWorld";
        assert_eq!(drop_thinking_blocks(input), "Hello\nWorld");
    }

    #[test]
    fn test_extract_thinking() {
        let input = "<thinking>\nStep 1\nStep 2\n</thinking>\nThe answer is 42.";
        let (thinking, visible) = extract_thinking(input);
        assert_eq!(thinking.unwrap(), "Step 1\nStep 2");
        assert_eq!(visible, "The answer is 42.");
    }

    #[test]
    fn test_no_thinking() {
        let input = "Just a normal response";
        let (thinking, visible) = extract_thinking(input);
        assert!(thinking.is_none());
        assert_eq!(visible, "Just a normal response");
    }

    #[test]
    fn test_supports_thinking() {
        // Supported models
        assert!(supports_thinking("claude-3-5-sonnet-20241022"));
        assert!(supports_thinking("claude-3.5-sonnet-latest"));
        assert!(supports_thinking("claude-3-7-sonnet-20250219"));
        assert!(supports_thinking("claude-4-opus-20260101"));
        assert!(supports_thinking("claude-sonnet-4-20260101"));
        assert!(supports_thinking("claude-opus-4-20260101"));
        assert!(supports_thinking("o1-preview"));
        assert!(!supports_thinking("o3-mini"));
        assert!(supports_thinking("deepseek-r1"));

        // NOT supported — old Claude 3 models
        assert!(!supports_thinking("claude-3-haiku-20240307"));
        assert!(!supports_thinking("claude-3-sonnet-20240229"));
        assert!(!supports_thinking("claude-3-opus-20240229"));
        assert!(!supports_thinking("gpt-4o"));
        assert!(!supports_thinking("llama3.2"));
        assert!(!supports_thinking("o1-mini"));
    }
}
