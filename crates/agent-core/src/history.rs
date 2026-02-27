use oclaw_llm_core::chat::{ChatMessage, MessageRole};

/// Limit messages sent to LLM by keeping only the last `max_turns` user turns
/// plus their associated assistant/tool responses. System messages at index 0 are always kept.
pub fn limit_history_turns(messages: &[ChatMessage], max_turns: usize) -> Vec<ChatMessage> {
    if max_turns == 0 || messages.is_empty() {
        return messages.to_vec();
    }

    // Collect system messages from the start
    let mut prefix_end = 0;
    for msg in messages {
        if msg.role == MessageRole::System {
            prefix_end += 1;
        } else {
            break;
        }
    }

    let rest = &messages[prefix_end..];

    // Count user turns from the end, find the cut point
    let mut user_count = 0;
    let mut cut_idx = rest.len();
    for (i, msg) in rest.iter().enumerate().rev() {
        if msg.role == MessageRole::User {
            user_count += 1;
            if user_count >= max_turns {
                cut_idx = i;
                break;
            }
        }
    }

    let mut result: Vec<ChatMessage> = messages[..prefix_end].to_vec();
    result.extend_from_slice(&rest[cut_idx..]);

    // Repair orphaned tool results: remove Tool messages whose tool_call_id
    // doesn't match any tool_call in the kept assistant messages
    let kept_tc_ids: std::collections::HashSet<String> = result
        .iter()
        .filter_map(|m| m.tool_calls.as_ref())
        .flatten()
        .map(|tc| tc.id.clone())
        .collect();

    result.retain(|m| {
        if m.role == MessageRole::Tool {
            m.tool_call_id
                .as_ref()
                .is_none_or(|id| kept_tc_ids.contains(id))
        } else {
            true
        }
    });

    result
}
