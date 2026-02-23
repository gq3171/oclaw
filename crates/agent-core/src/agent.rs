use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use oclaws_llm_core::chat::{ChatMessage, ChatRequest, MessageRole, Tool};
use oclaws_llm_core::providers::LlmProvider;

use crate::{AgentError, AgentResult};
use crate::loop_detect::LoopDetector;
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

    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    pub async fn initialize(&mut self) -> AgentResult<()> {
        self.state = AgentState::Initializing;

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

    pub async fn run(&mut self, input: &str) -> AgentResult<String> {
        self.state = AgentState::Running;

        self.history.push(ChatMessage {
            role: MessageRole::User,
            content: input.to_string(),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        });

        self.state = AgentState::WaitingForResponse;

        let request = ChatRequest {
            model: self.config.model.clone(),
            messages: self.history.clone(),
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

                        self.history.push(ChatMessage {
                            role: MessageRole::Assistant,
                            content: response_content.clone(),
                            name: None,
                            tool_calls: None,
                            tool_call_id: None,
                        });

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

    /// Run agent with tool execution loop. The agent will call the LLM, execute any
    /// tool calls via the executor, feed results back, and repeat until the LLM
    /// produces a final text response (no more tool calls).
    pub async fn run_with_tools(
        &mut self,
        input: &str,
        executor: &dyn ToolExecutor,
    ) -> AgentResult<String> {
        self.state = AgentState::Running;

        self.history.push(ChatMessage {
            role: MessageRole::User,
            content: input.to_string(),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        });

        let tools = executor.available_tools();
        let max_iterations = 20;
        let mut loop_detector = LoopDetector::default();

        for iteration in 0..max_iterations {
            self.state = AgentState::WaitingForResponse;

            let request = ChatRequest {
                model: self.config.model.clone(),
                messages: self.history.clone(),
                temperature: self.config.temperature,
                top_p: None,
                max_tokens: self.config.max_tokens,
                stop: None,
                tools: Some(tools.clone()),
                tool_choice: None,
                stream: None,
                response_format: None,
            };

            let response = self.provider.chat(request).await.map_err(|e| {
                AgentError::ModelError(e.to_string())
            })?;

            let choice = response.choices.first().ok_or_else(|| {
                AgentError::ModelError("No choices in response".to_string())
            })?;

            // Push assistant message to history
            self.history.push(ChatMessage {
                role: MessageRole::Assistant,
                content: choice.message.content.clone(),
                name: None,
                tool_calls: choice.message.tool_calls.clone(),
                tool_call_id: None,
            });

            // If no tool calls, we're done — return the text response
            let tool_calls = match &choice.message.tool_calls {
                Some(tc) if !tc.is_empty() => tc.clone(),
                _ => {
                    self.state = AgentState::Idle;
                    return Ok(choice.message.content.clone());
                }
            };

            // Execute each tool call and feed results back
            tracing::info!("Iteration {}: executing {} tool call(s)", iteration, tool_calls.len());
            for tc in &tool_calls {
                if loop_detector.record(&tc.function.name, &tc.function.arguments) {
                    self.state = AgentState::Error;
                    return Err(AgentError::ExecutionError(
                        format!("Tool loop detected: {} called repeatedly with same arguments", tc.function.name),
                    ));
                }
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
                let result = executor.execute(&tc.function.name, &tc.function.arguments).await;
                let (content, _is_err) = match result {
                    Ok(output) => (output, false),
                    Err(err) => (format!("Error: {}", err), true),
                };
                self.history.push(ChatMessage {
                    role: MessageRole::Tool,
                    content,
                    name: Some(tc.function.name.clone()),
                    tool_calls: None,
                    tool_call_id: Some(tc.id.clone()),
                });
            }
        }

        self.state = AgentState::Error;
        Err(AgentError::ExecutionError(
            "Max tool iterations reached without final response".to_string(),
        ))
    }

    pub fn clone_with_history(&self) -> Self {
        Self {
            config: self.config.clone(),
            state: self.state,
            provider: self.provider.clone(),
            history: self.history.clone(),
            context: self.context.clone(),
            approval_gate: self.approval_gate.clone(),
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
