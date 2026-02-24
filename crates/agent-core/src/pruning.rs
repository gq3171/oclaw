use oclaws_llm_core::chat::{ChatMessage, MessageRole};

pub struct PruningConfig {
    pub soft_trim_max_chars: usize,
    pub head_chars: usize,
    pub tail_chars: usize,
    pub hard_clear_max_chars: usize,
    pub keep_last_assistants: usize,
}

impl Default for PruningConfig {
    fn default() -> Self {
        Self {
            soft_trim_max_chars: 4000,
            head_chars: 1500,
            tail_chars: 1500,
            hard_clear_max_chars: 50000,
            keep_last_assistants: 3,
        }
    }
}

pub fn prune_tool_results(messages: &mut [ChatMessage], config: &PruningConfig) {
    // Find indices of the last N assistant messages to protect
    let protected_assistant_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .rev()
        .filter(|(_, m)| m.role == MessageRole::Assistant)
        .take(config.keep_last_assistants)
        .map(|(i, _)| i)
        .collect();

    // Collect tool_call_ids from protected assistants
    let protected_tc_ids: std::collections::HashSet<String> = protected_assistant_indices
        .iter()
        .filter_map(|&i| messages[i].tool_calls.as_ref())
        .flatten()
        .map(|tc| tc.id.clone())
        .collect();

    for msg in messages.iter_mut() {
        if msg.role != MessageRole::Tool {
            continue;
        }
        // Skip protected tool results
        if let Some(id) = &msg.tool_call_id
            && protected_tc_ids.contains(id)
        {
            continue;
        }

        let len = msg.content.len();
        if len > config.hard_clear_max_chars {
            msg.content = format!("[Tool result cleared — {} chars]", len);
        } else if len > config.soft_trim_max_chars {
            let head = &msg.content[..config.head_chars.min(len)];
            let tail_start = len.saturating_sub(config.tail_chars);
            let tail = &msg.content[tail_start..];
            msg.content = format!("{}...\n[trimmed — {} chars total]\n...{}", head, len, tail);
        }
    }
}
