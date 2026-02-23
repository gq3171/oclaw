//! Mock LLM provider for testing

use async_trait::async_trait;
use std::sync::{Arc, Mutex};
use crate::chat::*;
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::{LlmError, LlmResult};
use super::{LlmProvider, ProviderType};

/// Mock provider that returns configurable responses.
pub struct MockLlmProvider {
    responses: Arc<Mutex<Vec<ChatCompletion>>>,
    calls: Arc<Mutex<Vec<ChatRequest>>>,
}

impl Default for MockLlmProvider {
    fn default() -> Self { Self::new() }
}

impl MockLlmProvider {
    pub fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(Vec::new())),
            calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Queue a response to be returned on the next chat() call.
    pub fn queue_response(&self, response: ChatCompletion) {
        self.responses.lock().unwrap().push(response);
    }

    /// Queue a simple text response.
    pub fn queue_text(&self, text: &str) {
        self.queue_response(Self::make_completion(text, None));
    }

    /// Queue a response with tool calls.
    pub fn queue_tool_call(&self, tool_name: &str, args: &str) {
        let tc = vec![ToolCall {
            id: format!("call_{}", uuid::Uuid::new_v4()),
            type_: "function".into(),
            function: ToolCallFunction {
                name: tool_name.into(),
                arguments: args.into(),
            },
        }];
        self.queue_response(Self::make_completion("", Some(tc)));
    }

    /// Get all recorded calls.
    pub fn recorded_calls(&self) -> Vec<ChatRequest> {
        self.calls.lock().unwrap().clone()
    }

    fn pop_or_default(&self) -> ChatCompletion {
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            Self::make_completion("Mock response", None)
        } else {
            responses.remove(0)
        }
    }

    fn make_completion(text: &str, tool_calls: Option<Vec<ToolCall>>) -> ChatCompletion {
        ChatCompletion {
            id: "mock-id".into(),
            object: "chat.completion".into(),
            created: 0,
            model: "mock-model".into(),
            choices: vec![ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: MessageRole::Assistant,
                    content: text.into(),
                    name: None,
                    tool_calls,
                    tool_call_id: None,
                },
                finish_reason: Some("stop".into()),
            }],
            usage: Some(Usage { prompt_tokens: 10, completion_tokens: 5, total_tokens: 15 }),
            system_fingerprint: None,
        }
    }
}

#[async_trait]
impl LlmProvider for MockLlmProvider {
    fn provider_type(&self) -> ProviderType { ProviderType::OpenAi }

    async fn chat(&self, request: ChatRequest) -> LlmResult<ChatCompletion> {
        self.calls.lock().unwrap().push(request);
        Ok(self.pop_or_default())
    }

    async fn chat_stream(&self, request: ChatRequest) -> LlmResult<tokio::sync::mpsc::Receiver<LlmResult<StreamChunk>>> {
        self.calls.lock().unwrap().push(request);
        let completion = self.pop_or_default();
        let text = completion.choices.first().map(|c| c.message.content.clone()).unwrap_or_default();
        let (tx, rx) = tokio::sync::mpsc::channel(32);
        tokio::spawn(async move {
            // Send content as word-level chunks
            for word in text.split_inclusive(' ') {
                let chunk = StreamChunk {
                    id: "mock-stream".into(),
                    object: "chat.completion.chunk".into(),
                    created: 0,
                    model: "mock-model".into(),
                    choices: vec![StreamChoice {
                        index: 0,
                        delta: Some(ChatMessage {
                            role: MessageRole::Assistant,
                            content: word.into(),
                            name: None, tool_calls: None, tool_call_id: None,
                        }),
                        finish_reason: None,
                    }],
                };
                if tx.send(Ok(chunk)).await.is_err() { return; }
            }
            // Final chunk with finish_reason
            let done = StreamChunk {
                id: "mock-stream".into(),
                object: "chat.completion.chunk".into(),
                created: 0,
                model: "mock-model".into(),
                choices: vec![StreamChoice {
                    index: 0, delta: None, finish_reason: Some("stop".into()),
                }],
            };
            let _ = tx.send(Ok(done)).await;
        });
        Ok(rx)
    }

    async fn embeddings(&self, _request: EmbeddingRequest) -> LlmResult<EmbeddingResponse> {
        Err(LlmError::UnsupportedModel("Mock does not support embeddings".into()))
    }

    fn supported_models(&self) -> Vec<String> { vec!["mock-model".into()] }
    fn default_model(&self) -> &str { "mock-model" }
}
