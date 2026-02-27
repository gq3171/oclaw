use anyhow::Result;
use oclaw_llm_core::chat::{ChatMessage, ChatRequest, MessageRole};
use oclaw_llm_core::providers::LlmProvider;
use oclaw_llm_core::tokenizer::TokenCounter;

pub struct RetryConfig {
    pub attempts: usize,
    pub min_delay_ms: u64,
    pub max_delay_ms: u64,
    pub jitter: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            attempts: 3,
            min_delay_ms: 500,
            max_delay_ms: 5000,
            jitter: 0.2,
        }
    }
}

pub struct CompactionConfig {
    pub reserve_tokens: usize,
    pub keep_recent_tokens: usize,
    pub retry: RetryConfig,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            reserve_tokens: 16384,
            keep_recent_tokens: 20000,
            retry: RetryConfig::default(),
        }
    }
}

pub struct CompactionResult {
    pub summary: ChatMessage,
    pub kept_messages: Vec<ChatMessage>,
}

const COMPACTION_PROMPT: &str = "Summarize this conversation concisely, preserving key facts, \
decisions, code context, and any important details the assistant needs to continue helping.";

pub fn needs_compaction(messages: &[ChatMessage], model: &str, config: &CompactionConfig) -> bool {
    let Some(max) = TokenCounter::max_tokens(model) else {
        return false;
    };
    let used = TokenCounter::estimate_messages(messages, model);
    used.total_tokens > max.saturating_sub(config.reserve_tokens)
}

/// Compute retry delay with exponential backoff + jitter.
fn retry_delay(attempt: usize, cfg: &RetryConfig) -> std::time::Duration {
    let base = cfg.min_delay_ms as f64 * 2.0f64.powi(attempt as i32);
    let clamped = base.min(cfg.max_delay_ms as f64);
    let jitter_range = clamped * cfg.jitter;
    let jittered = clamped + rand::random::<f64>() * jitter_range * 2.0 - jitter_range;
    std::time::Duration::from_millis(jittered.max(0.0) as u64)
}

/// Generate the summary via LLM with retry.
async fn generate_summary(
    provider: &dyn LlmProvider,
    request: ChatRequest,
    retry: &RetryConfig,
) -> Result<String> {
    let mut last_err = String::new();
    for attempt in 0..retry.attempts {
        if attempt > 0 {
            tokio::time::sleep(retry_delay(attempt - 1, retry)).await;
            tracing::info!(
                "Compaction retry attempt {}/{}",
                attempt + 1,
                retry.attempts
            );
        }
        match provider.chat(request.clone()).await {
            Ok(resp) => {
                return Ok(resp
                    .choices
                    .first()
                    .map(|c| c.message.content.clone())
                    .unwrap_or_default());
            }
            Err(e) => {
                last_err = e.to_string();
                tracing::warn!("Compaction attempt {} failed: {}", attempt + 1, last_err);
            }
        }
    }
    Err(anyhow::anyhow!(
        "Compaction failed after {} attempts: {}",
        retry.attempts,
        last_err
    ))
}

pub async fn compact_history(
    provider: &dyn LlmProvider,
    model: &str,
    messages: &[ChatMessage],
    config: &CompactionConfig,
) -> Result<CompactionResult> {
    // Preserve the system prompt so it survives compaction
    let system_prompt = messages
        .iter()
        .find(|m| m.role == MessageRole::System)
        .cloned();

    // Split: old (to summarize) vs recent (to keep)
    let mut recent_tokens = 0usize;
    let mut split_idx = messages.len();
    for (i, msg) in messages.iter().enumerate().rev() {
        let t = TokenCounter::estimate(&msg.content, model).total_tokens + 4;
        if recent_tokens + t > config.keep_recent_tokens {
            split_idx = i + 1;
            break;
        }
        recent_tokens += t;
    }
    // Never summarize away the system prompt (index 0)
    if split_idx <= 1 {
        split_idx = 1;
    }

    let old = &messages[..split_idx];
    let kept = messages[split_idx..].to_vec();

    let mut summary_messages = old.to_vec();
    summary_messages.push(ChatMessage {
        role: MessageRole::User,
        content: COMPACTION_PROMPT.to_string(),
        name: None,
        tool_calls: None,
        tool_call_id: None,
    });

    let request = ChatRequest {
        model: model.to_string(),
        messages: summary_messages,
        temperature: Some(0.0),
        top_p: None,
        max_tokens: Some(2048),
        stop: None,
        tools: None,
        tool_choice: None,
        stream: None,
        response_format: None,
    };

    let summary_text = generate_summary(provider, request, &config.retry).await?;

    // Build result: preserve original system prompt, then add summary, then kept messages
    let summary_msg = ChatMessage {
        role: MessageRole::System,
        content: format!("[Conversation summary]\n{}", summary_text),
        name: None,
        tool_calls: None,
        tool_call_id: None,
    };

    if let Some(sys) = system_prompt {
        // Return the original system prompt as the "summary" (it goes first),
        // and prepend the actual summary to kept_messages.
        let mut kept_with_summary = vec![summary_msg];
        kept_with_summary.extend(kept);
        Ok(CompactionResult {
            summary: sys,
            kept_messages: kept_with_summary,
        })
    } else {
        Ok(CompactionResult {
            summary: summary_msg,
            kept_messages: kept,
        })
    }
}
