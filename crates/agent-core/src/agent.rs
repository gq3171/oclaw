use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use oclaws_llm_core::chat::{ChatMessage, ChatRequest, MessageRole, Tool};
use oclaws_llm_core::providers::LlmProvider;

use crate::{AgentError, AgentResult};
use crate::loop_detect::{LoopDetector, LoopLevel};
use crate::transcript::Transcript;
use crate::compaction::{CompactionConfig, needs_compaction, compact_history};
use crate::pruning::{PruningConfig, prune_tool_results};
use crate::history::limit_history_turns;
use crate::transcript_repair::repair_tool_use_result_pairing;

const MAX_OVERFLOW_COMPACTION_ATTEMPTS: usize = 3;
const TRUNCATION_SUFFIX: &str = "\n\n[Content truncated — original was too large for the model's \
context window. If you need more, request specific sections or use offset/limit parameters.]";
use oclaws_tools_core::{ApprovalGate, ApprovalDecision};

/// Trait for executing tool calls. Implement this to provide tool execution to agents.
#[async_trait::async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(&self, name: &str, arguments: &str) -> Result<String, String>;
    fn available_tools(&self) -> Vec<Tool>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentConfig {
    pub name: String,
    pub model: String,
    pub provider: String,
    pub system_prompt: Option<String>,
    pub max_tokens: Option<i32>,
    pub temperature: Option<f64>,
    pub tools: Option<Vec<String>>,
    pub max_retries: Option<i32>,
    pub timeout_ms: Option<i64>,
    pub max_iterations: Option<usize>,
}

impl AgentConfig {
    pub fn new(name: &str, model: &str, provider: &str) -> Self {
        Self {
            name: name.to_string(),
            model: model.to_string(),
            provider: provider.to_string(),
            system_prompt: None,
            max_tokens: None,
            temperature: None,
            tools: None,
            max_retries: Some(3),
            timeout_ms: Some(60000),
            max_iterations: Some(32),
        }
    }

    pub fn with_system_prompt(mut self, prompt: &str) -> Self {
        self.system_prompt = Some(prompt.to_string());
        self
    }

    pub fn with_temperature(mut self, temp: f64) -> Self {
        self.temperature = Some(temp);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentState {
    Idle,
    Initializing,
    Running,
    WaitingForResponse,
    Error,
    Stopped,
}

pub struct Agent {
    config: AgentConfig,
    state: AgentState,
    provider: Arc<dyn LlmProvider>,
    history: Vec<ChatMessage>,
    context: HashMap<String, String>,
    approval_gate: Option<Arc<ApprovalGate>>,
    transcript: Option<Transcript>,
    history_limit: Option<usize>,
    compaction_config: Option<CompactionConfig>,
    pruning_config: Option<PruningConfig>,
    recall_config: Option<crate::auto_recall::AutoRecallConfig>,
    memory_recaller: Option<Arc<dyn crate::auto_recall::MemoryRecaller>>,
}

impl Agent {
    pub fn new(config: AgentConfig, provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            config,
            state: AgentState::Idle,
            provider,
            history: Vec::new(),
            context: HashMap::new(),
            approval_gate: None,
            transcript: None,
            history_limit: None,
            compaction_config: None,
            pruning_config: None,
            recall_config: None,
            memory_recaller: None,
        }
    }

    pub fn config(&self) -> &AgentConfig {
        &self.config
    }

    pub fn state(&self) -> AgentState {
        self.state
    }

    pub fn provider(&self) -> &Arc<dyn LlmProvider> {
        &self.provider
    }

    pub fn history(&self) -> &[ChatMessage] {
        &self.history
    }

    pub fn set_context(&mut self, key: &str, value: &str) {
        self.context.insert(key.to_string(), value.to_string());
    }

    pub fn get_context(&self, key: &str) -> Option<&String> {
        self.context.get(key)
    }

    pub fn with_approval_gate(mut self, gate: Arc<ApprovalGate>) -> Self {
        self.approval_gate = Some(gate);
        self
    }

    pub fn with_transcript(mut self, session_id: &str) -> Self {
        self.transcript = Some(Transcript::new(session_id));
        self
    }

    pub fn with_history_limit(mut self, limit: usize) -> Self {
        self.history_limit = Some(limit);
        self
    }

    pub fn with_compaction(mut self, config: CompactionConfig) -> Self {
        self.compaction_config = Some(config);
        self
    }

    pub fn with_pruning(mut self, config: PruningConfig) -> Self {
        self.pruning_config = Some(config);
        self
    }

    pub fn with_auto_recall(
        mut self,
        config: crate::auto_recall::AutoRecallConfig,
        recaller: Arc<dyn crate::auto_recall::MemoryRecaller>,
    ) -> Self {
        self.recall_config = Some(config);
        self.memory_recaller = Some(recaller);
        self
    }

    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    pub async fn initialize(&mut self) -> AgentResult<()> {
        self.state = AgentState::Initializing;

        // Load persisted history from transcript if available
        if let Some(transcript) = &self.transcript
            && transcript.exists().await
        {
            let loaded = transcript.load().await;
            if !loaded.is_empty() {
                let (repaired, report) = repair_tool_use_result_pairing(loaded);
                if report.added_synthetic > 0 || report.dropped_duplicates > 0 || report.dropped_orphans > 0 {
                    tracing::info!(
                        "Transcript repair: +{} synthetic, -{} duplicates, -{} orphans",
                        report.added_synthetic, report.dropped_duplicates, report.dropped_orphans
                    );
                }
                tracing::info!("Loaded {} messages from transcript", repaired.len());
                self.history = repaired;
                self.state = AgentState::Idle;
                return Ok(());
            }
        }

        if let Some(prompt) = &self.config.system_prompt {
            self.history.push(ChatMessage {
                role: MessageRole::System,
                content: prompt.clone(),
                name: None,
                tool_calls: None,
                tool_call_id: None,
            });
        }

        self.state = AgentState::Idle;
        Ok(())
    }

    /// Helper: persist a message to transcript if enabled.
    async fn transcript_append(&self, msg: &ChatMessage) {
        if let Some(t) = &self.transcript
            && let Err(e) = t.append(msg).await
        {
            tracing::warn!("Failed to write transcript: {}", e);
        }
    }

    /// Build the messages array to send to LLM, applying pruning + history limiting.
    fn prepare_messages(&self) -> Vec<ChatMessage> {
        let mut msgs = self.history.clone();
        if let Some(cfg) = &self.pruning_config {
            prune_tool_results(&mut msgs, cfg);
        }
        if let Some(limit) = self.history_limit {
            msgs = limit_history_turns(&msgs, limit);
        }
        msgs
    }

    pub async fn run(&mut self, input: &str) -> AgentResult<String> {
        self.state = AgentState::Running;

        let user_msg = ChatMessage {
            role: MessageRole::User,
            content: input.to_string(),
            name: None, tool_calls: None, tool_call_id: None,
        };
        self.transcript_append(&user_msg).await;
        self.history.push(user_msg);

        self.state = AgentState::WaitingForResponse;

        let request = ChatRequest {
            model: self.config.model.clone(),
            messages: self.prepare_messages(),
            temperature: self.config.temperature,
            top_p: None,
            max_tokens: self.config.max_tokens,
            stop: None,
            tools: None,
            tool_choice: None,
            stream: None,
            response_format: None,
        };

        let max_retries = self.config.max_retries.unwrap_or(3);
        let mut last_error = None;

        for attempt in 0..max_retries {
            if attempt > 0 {
                let delay = std::time::Duration::from_millis(500 * 2u64.pow(attempt as u32 - 1));
                tokio::time::sleep(delay).await;
            }
            match self.provider.chat(request.clone()).await {
                Ok(response) => {
                    if let Some(choice) = response.choices.first() {
                        let response_content = choice.message.content.clone();

                        let asst_msg = ChatMessage {
                            role: MessageRole::Assistant,
                            content: response_content.clone(),
                            name: None, tool_calls: None, tool_call_id: None,
                        };
                        self.transcript_append(&asst_msg).await;
                        self.history.push(asst_msg);

                        self.state = AgentState::Idle;
                        return Ok(response_content);
                    }
                }
                Err(e) => {
                    let error_msg = e.to_string();
                    last_error = Some(e);
                    tracing::warn!("Attempt {} failed: {}", attempt + 1, error_msg);
                }
            }
        }

        self.state = AgentState::Error;
        Err(AgentError::ModelError(format!(
            "Failed after {} attempts: {:?}",
            max_retries, last_error
        )))
    }

    fn truncate_tool_result(s: &str, max_chars: usize) -> String {
        if s.len() <= max_chars {
            return s.to_string();
        }
        const MIN_KEEP: usize = 2_000;
        let keep = max_chars.saturating_sub(TRUNCATION_SUFFIX.len()).max(MIN_KEEP);
        let cut = s[..keep].rfind('\n')
            .filter(|&i| i > keep * 4 / 5)
            .unwrap_or(keep);
        format!("{}{}", &s[..cut], TRUNCATION_SUFFIX)
    }

    fn is_context_overflow(err: &str) -> bool {
        let e = err.to_lowercase();
        e.contains("context length exceeded")
            || e.contains("maximum context")
            || e.contains("too many tokens")
            || e.contains("content_too_large")
            || e.contains("request too large")
    }

    /// Chat with exponential backoff retry within the tool loop.
    async fn chat_with_retry(
        &self,
        request: ChatRequest,
    ) -> Result<oclaws_llm_core::chat::ChatCompletion, AgentError> {
        let max_retries = self.config.max_retries.unwrap_or(3);
        let mut last_err = String::new();
        for attempt in 0..max_retries {
            if attempt > 0 {
                let delay = std::time::Duration::from_millis(1000 * 2u64.pow(attempt as u32 - 1));
                tokio::time::sleep(delay).await;
            }
            match self.provider.chat(request.clone()).await {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    last_err = e.to_string();
                    if Self::is_context_overflow(&last_err) {
                        return Err(AgentError::ContextOverflow(last_err));
                    }
                    tracing::warn!("Chat attempt {} failed: {}", attempt + 1, last_err);
                }
            }
        }
        Err(AgentError::ModelError(format!(
            "Failed after {} retries: {}",
            max_retries, last_err
        )))
    }

    /// Halve the largest tool result in history to recover from context overflow.
    fn truncate_history_tool_results(&mut self) {
        let Some((idx, _)) = self
            .history
            .iter()
            .enumerate()
            .filter(|(_, m)| m.role == MessageRole::Tool)
            .max_by_key(|(_, m)| m.content.len())
        else {
            return;
        };
        let content = &self.history[idx].content;
        let half = content.len() / 2;
        let cut = content[..half].rfind('\n').unwrap_or(half);
        let truncated = format!(
            "{}...\n[truncated for context limit, {} chars total]",
            &content[..cut],
            content.len()
        );
        self.history[idx].content = truncated;
    }

    /// Run agent with tool execution loop.
    pub async fn run_with_tools(
        &mut self,
        input: &str,
        executor: &dyn ToolExecutor,
    ) -> AgentResult<String> {
        self.state = AgentState::Running;
        let user_msg = ChatMessage {
            role: MessageRole::User,
            content: input.to_string(),
            name: None, tool_calls: None, tool_call_id: None,
        };
        self.transcript_append(&user_msg).await;
        self.history.push(user_msg);

        // Auto-recall: search memory and inject context
        if let Some(cfg) = &self.recall_config
            && cfg.enabled
            && let Some(recaller) = &self.memory_recaller
        {
            let results = recaller.recall(input, cfg.max_results, cfg.min_score).await;
            if let Some(ctx_msg) = crate::auto_recall::format_recall_context(&results) {
                self.history.push(ctx_msg);
            }
        }

        let tools = executor.available_tools();
        let max_iterations = self.config.max_iterations.unwrap_or(32);
        let mut loop_detector = LoopDetector::default();
        let mut overflow_attempts = 0usize;
        let max_tool_chars = 400_000usize;

        let mut iteration = 0;
        while iteration < max_iterations {
            iteration += 1;
            self.state = AgentState::WaitingForResponse;

            // Compaction check before building request
            self.try_compact().await;

            let request = ChatRequest {
                model: self.config.model.clone(),
                messages: self.prepare_messages(),
                temperature: self.config.temperature,
                top_p: None,
                max_tokens: self.config.max_tokens,
                stop: None,
                tools: Some(tools.clone()),
                tool_choice: None,
                stream: None,
                response_format: None,
            };

            let response = match self.chat_with_retry(request).await {
                Ok(r) => r,
                Err(AgentError::ContextOverflow(_)) if overflow_attempts < MAX_OVERFLOW_COMPACTION_ATTEMPTS => {
                    overflow_attempts += 1;
                    tracing::warn!(
                        "Context overflow (attempt {}/{}), truncating largest tool result",
                        overflow_attempts, MAX_OVERFLOW_COMPACTION_ATTEMPTS
                    );
                    self.truncate_history_tool_results();
                    continue;
                }
                Err(e) => {
                    self.state = AgentState::Error;
                    return Err(e);
                }
            };

            let choice = response.choices.first().ok_or_else(|| {
                AgentError::ModelError("No choices in response".into())
            })?;

            let asst_msg = ChatMessage {
                role: MessageRole::Assistant,
                content: choice.message.content.clone(),
                name: None,
                tool_calls: choice.message.tool_calls.clone(),
                tool_call_id: None,
            };
            self.transcript_append(&asst_msg).await;
            self.history.push(asst_msg);

            let tool_calls = match &choice.message.tool_calls {
                Some(tc) if !tc.is_empty() => tc.clone(),
                _ => {
                    self.state = AgentState::Idle;
                    return Ok(choice.message.content.clone());
                }
            };

            tracing::info!("Iteration {}: executing {} tool call(s)", iteration, tool_calls.len());
            for tc in &tool_calls {
                // Record + detect loop
                loop_detector.record(&tc.function.name, &tc.function.arguments);
                let detection = loop_detector.detect(&tc.function.name, &tc.function.arguments);
                match detection.level {
                    LoopLevel::Critical => {
                        self.state = AgentState::Error;
                        return Err(AgentError::ExecutionError(
                            detection.message.unwrap_or_else(|| "Tool loop detected".into()),
                        ));
                    }
                    LoopLevel::Warning => {
                        // Inject warning into conversation so LLM can self-correct
                        if let Some(msg) = &detection.message {
                            tracing::warn!("{}", msg);
                            self.history.push(ChatMessage {
                                role: MessageRole::User,
                                content: msg.clone(),
                                name: Some("system".into()),
                                tool_calls: None,
                                tool_call_id: None,
                            });
                        }
                    }
                    LoopLevel::None => {}
                }

                // Approval gate
                if let Some(gate) = &self.approval_gate
                    && let ApprovalDecision::Denied = gate.check(&tc.function.name)
                {
                    self.history.push(ChatMessage {
                        role: MessageRole::Tool,
                        content: format!("Tool '{}' denied by approval policy", tc.function.name),
                        name: Some(tc.function.name.clone()),
                        tool_calls: None,
                        tool_call_id: Some(tc.id.clone()),
                    });
                    continue;
                }

                // Execute tool
                let result = executor.execute(&tc.function.name, &tc.function.arguments).await;
                let content = match &result {
                    Ok(output) => Self::truncate_tool_result(output, max_tool_chars),
                    Err(err) => format!("Error: {}", err),
                };

                // Record outcome for no-progress detection
                loop_detector.record_outcome(&content);

                let tool_msg = ChatMessage {
                    role: MessageRole::Tool,
                    content,
                    name: Some(tc.function.name.clone()),
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                };
                self.transcript_append(&tool_msg).await;
                self.history.push(tool_msg);
            }
        }

        self.state = AgentState::Error;
        Err(AgentError::ExecutionError(
            format!("Max tool iterations ({}) reached without final response", max_iterations),
        ))
    }

    /// Run compaction if configured and token threshold exceeded.
    async fn try_compact(&mut self) {
        let Some(cfg) = &self.compaction_config else { return };
        if !needs_compaction(&self.history, &self.config.model, cfg) {
            return;
        }
        tracing::info!("Running context compaction");
        match compact_history(
            self.provider.as_ref(),
            &self.config.model,
            &self.history,
            cfg,
        ).await {
            Ok(result) => {
                let summary_text = result.summary.content.clone();
                let mut new_history = vec![result.summary];
                new_history.extend(result.kept_messages);
                self.history = new_history;
                if let Some(t) = &self.transcript {
                    let _ = t.append_compaction(&summary_text, None).await;
                }
            }
            Err(e) => tracing::warn!("Compaction failed: {}", e),
        }
    }

    pub fn clone_with_history(&self) -> Self {
        Self {
            config: self.config.clone(),
            state: self.state,
            provider: self.provider.clone(),
            history: self.history.clone(),
            context: self.context.clone(),
            approval_gate: self.approval_gate.clone(),
            transcript: None,
            history_limit: self.history_limit,
            compaction_config: None,
            pruning_config: None,
            recall_config: None,
            memory_recaller: None,
        }
    }
}

impl Clone for Agent {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            state: self.state,
            provider: self.provider.clone(),
            history: self.history.clone(),
            context: self.context.clone(),
            approval_gate: self.approval_gate.clone(),
            transcript: None,
            history_limit: self.history_limit,
            compaction_config: None,
            pruning_config: None,
            recall_config: None,
            memory_recaller: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oclaws_llm_core::providers::MockLlmProvider;
    use oclaws_llm_core::chat::{Tool, ToolFunction};

    struct MockExecutor;

    #[async_trait::async_trait]
    impl ToolExecutor for MockExecutor {
        async fn execute(&self, name: &str, _arguments: &str) -> Result<String, String> {
            Ok(format!("{} result", name))
        }
        fn available_tools(&self) -> Vec<Tool> {
            vec![Tool {
                type_: "function".into(),
                function: ToolFunction {
                    name: "test_tool".into(),
                    description: "A test tool".into(),
                    parameters: serde_json::json!({}),
                },
            }]
        }
    }

    fn make_agent(provider: Arc<dyn oclaws_llm_core::providers::LlmProvider>) -> Agent {
        Agent::new(AgentConfig::new("test", "mock-model", "mock"), provider)
    }

    #[tokio::test]
    async fn test_run_simple() {
        let mock = MockLlmProvider::new();
        mock.queue_text("Hello!");
        let provider: Arc<dyn oclaws_llm_core::providers::LlmProvider> = Arc::new(mock);
        let mut agent = make_agent(provider);
        let result = agent.run("hi").await.unwrap();
        assert_eq!(result, "Hello!");
        assert_eq!(agent.state(), AgentState::Idle);
    }

    #[tokio::test]
    async fn test_run_with_tools_no_tool_calls() {
        let mock = MockLlmProvider::new();
        mock.queue_text("Direct answer");
        let provider: Arc<dyn oclaws_llm_core::providers::LlmProvider> = Arc::new(mock);
        let mut agent = make_agent(provider);
        let result = agent.run_with_tools("question", &MockExecutor).await.unwrap();
        assert_eq!(result, "Direct answer");
    }

    #[tokio::test]
    async fn test_run_with_tools_executes_tool() {
        let mock = MockLlmProvider::new();
        // First response: tool call
        mock.queue_tool_call("test_tool", r#"{"input":"x"}"#);
        // Second response: final text after tool result
        mock.queue_text("Final answer after tool");
        let provider: Arc<dyn oclaws_llm_core::providers::LlmProvider> = Arc::new(mock);
        let mut agent = make_agent(provider);
        let result = agent.run_with_tools("do something", &MockExecutor).await.unwrap();
        assert_eq!(result, "Final answer after tool");
        // History should contain: user, assistant(tool_call), tool(result), assistant(final)
        assert_eq!(agent.history().len(), 4);
    }

    #[tokio::test]
    async fn test_approval_gate_denies_tool() {
        let mock = MockLlmProvider::new();
        mock.queue_tool_call("blocked_tool", "{}");
        mock.queue_text("Denied fallback");
        let provider: Arc<dyn oclaws_llm_core::providers::LlmProvider> = Arc::new(mock);
        let mut policy = oclaws_tools_core::ApprovalPolicy::default();
        policy.deny.insert("blocked_tool".into());
        let gate = Arc::new(ApprovalGate::new(policy));
        let mut agent = make_agent(provider).with_approval_gate(gate);
        let result = agent.run_with_tools("try blocked", &MockExecutor).await.unwrap();
        assert_eq!(result, "Denied fallback");
    }
}
