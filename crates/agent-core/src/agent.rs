use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use oclaws_llm_core::chat::{ChatMessage, ChatRequest, MessageRole};
use oclaws_llm_core::providers::LlmProvider;

use crate::{AgentError, AgentResult};

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
}

impl Agent {
    pub fn new(config: AgentConfig, provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            config,
            state: AgentState::Idle,
            provider,
            history: Vec::new(),
            context: HashMap::new(),
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

    pub async fn run_with_tools(&mut self, input: &str, tools: Vec<oclaws_llm_core::chat::Tool>) -> AgentResult<String> {
        self.state = AgentState::Running;

        self.history.push(ChatMessage {
            role: MessageRole::User,
            content: input.to_string(),
            name: None,
            tool_calls: None,
            tool_call_id: None,
        });

        self.state = AgentState::WaitingForResponse;

        let max_retries = self.config.max_retries.unwrap_or(3);
        let mut last_error = None;

        for attempt in 0..max_retries {
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

            match self.provider.chat(request).await {
                Ok(response) => {
                    if let Some(choice) = response.choices.first() {
                        let response_content = choice.message.content.clone();

                        self.history.push(ChatMessage {
                            role: MessageRole::Assistant,
                            content: response_content.clone(),
                            name: None,
                            tool_calls: choice.message.tool_calls.clone(),
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

    pub fn clone_with_history(&self) -> Self {
        Self {
            config: self.config.clone(),
            state: self.state,
            provider: self.provider.clone(),
            history: self.history.clone(),
            context: self.context.clone(),
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
        }
    }
}
