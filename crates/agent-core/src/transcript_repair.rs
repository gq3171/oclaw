use oclaw_llm_core::chat::{ChatMessage, MessageRole, ToolCall};
use std::collections::HashSet;

pub struct RepairReport {
    pub added_synthetic: usize,
    pub dropped_duplicates: usize,
    pub dropped_orphans: usize,
    pub moved_results: bool,
}

const SYNTHETIC_ERROR: &str = "[oclaw] missing tool result in session history; \
inserted synthetic error result for transcript repair.";

/// Repair tool use/result pairing in a message history.
/// Ensures every assistant tool call is followed by a matching tool result,
/// drops duplicates and orphans, inserts synthetic errors for missing results.
pub fn repair_tool_use_result_pairing(
    messages: Vec<ChatMessage>,
) -> (Vec<ChatMessage>, RepairReport) {
    let mut result: Vec<ChatMessage> = Vec::new();
    let mut report = RepairReport {
        added_synthetic: 0,
        dropped_duplicates: 0,
        dropped_orphans: 0,
        moved_results: false,
    };
    let mut seen_result_ids: HashSet<String> = HashSet::new();

    // Collect all tool results indexed by tool_call_id for relocation
    let mut result_pool: std::collections::HashMap<String, Vec<ChatMessage>> =
        std::collections::HashMap::new();
    for msg in &messages {
        if msg.role == MessageRole::Tool
            && let Some(id) = &msg.tool_call_id
        {
            result_pool.entry(id.clone()).or_default().push(msg.clone());
        }
    }

    for msg in &messages {
        // Skip standalone tool results — they'll be placed after their tool calls
        if msg.role == MessageRole::Tool {
            continue;
        }

        result.push(msg.clone());

        // After each assistant message with tool calls, pair results
        if msg.role == MessageRole::Assistant
            && let Some(tool_calls) = &msg.tool_calls
        {
            for tc in tool_calls {
                if let Some(results) = result_pool.get(&tc.id) {
                    let mut placed = false;
                    for tr in results {
                        if seen_result_ids.contains(&tc.id) {
                            report.dropped_duplicates += 1;
                            continue;
                        }
                        seen_result_ids.insert(tc.id.clone());
                        result.push(tr.clone());
                        placed = true;
                        break;
                    }
                    if !placed && !seen_result_ids.contains(&tc.id) {
                        result.push(make_synthetic_result(&tc.id, &tc.function.name));
                        report.added_synthetic += 1;
                        seen_result_ids.insert(tc.id.clone());
                    }
                } else if !seen_result_ids.contains(&tc.id) {
                    result.push(make_synthetic_result(&tc.id, &tc.function.name));
                    report.added_synthetic += 1;
                    seen_result_ids.insert(tc.id.clone());
                }
            }
        }
    }

    // Count orphans: tool results whose IDs were never matched
    for msg in &messages {
        if msg.role == MessageRole::Tool
            && let Some(id) = &msg.tool_call_id
            && !seen_result_ids.contains(id)
        {
            report.dropped_orphans += 1;
        }
    }

    // Detect if any results were moved
    report.moved_results = check_results_moved(&messages, &result);

    (result, report)
}

fn make_synthetic_result(tool_call_id: &str, tool_name: &str) -> ChatMessage {
    ChatMessage {
        role: MessageRole::Tool,
        content: SYNTHETIC_ERROR.to_string(),
        name: Some(tool_name.to_string()),
        tool_calls: None,
        tool_call_id: Some(tool_call_id.to_string()),
    }
}

fn check_results_moved(original: &[ChatMessage], repaired: &[ChatMessage]) -> bool {
    let orig_order: Vec<&str> = original
        .iter()
        .filter_map(|m| {
            if m.role == MessageRole::Tool {
                m.tool_call_id.as_deref()
            } else {
                None
            }
        })
        .collect();
    let new_order: Vec<&str> = repaired
        .iter()
        .filter_map(|m| {
            if m.role == MessageRole::Tool {
                m.tool_call_id.as_deref()
            } else {
                None
            }
        })
        .collect();
    orig_order != new_order
}

/// Validate tool call inputs: ensure id, name, and arguments are valid.
/// Returns cleaned messages with invalid tool calls removed.
pub fn sanitize_tool_call_inputs(
    messages: Vec<ChatMessage>,
    allowed_tools: Option<&HashSet<String>>,
) -> Vec<ChatMessage> {
    messages
        .into_iter()
        .filter_map(|mut msg| {
            if msg.role != MessageRole::Assistant || msg.tool_calls.is_none() {
                return Some(msg);
            }
            let tool_calls: Vec<ToolCall> = msg
                .tool_calls
                .take()
                .unwrap()
                .into_iter()
                .filter(|tc| {
                    if tc.id.is_empty() || tc.function.name.is_empty() {
                        return false;
                    }
                    if tc.function.name.len() > 64 {
                        return false;
                    }
                    if let Some(allowed) = allowed_tools
                        && !allowed.contains(&tc.function.name)
                    {
                        return false;
                    }
                    true
                })
                .collect();

            if tool_calls.is_empty() && msg.content.is_empty() {
                return None; // drop empty assistant message
            }
            msg.tool_calls = if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            };
            Some(msg)
        })
        .collect()
}

/// Repair a JSONL session file: drop unparseable lines, return cleaned messages.
/// Returns (valid_messages, dropped_count).
pub fn repair_jsonl_lines(raw: &str) -> (Vec<ChatMessage>, usize) {
    let mut valid = Vec::new();
    let mut dropped = 0;
    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<ChatMessage>(trimmed) {
            Ok(msg) => valid.push(msg),
            Err(_) => dropped += 1,
        }
    }
    (valid, dropped)
}
