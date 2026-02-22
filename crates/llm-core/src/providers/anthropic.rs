use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::chat::{ChatMessage, ChatRequest, ChatCompletion, MessageRole};
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::{LlmError, LlmResult};
use crate::providers::{LlmProvider, ProviderType};

pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    default_model_name: String,
}

impl AnthropicProvider {
    pub fn new(api_key: &str) -> LlmResult<Self> {
        Ok(Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            default_model_name: "claude-3-5-sonnet-20241022".to_string(),
        })
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Anthropic
    }

    async fn chat(&self, request: ChatRequest) -> LlmResult<ChatCompletion> {
        let url = "https://api.anthropic.com/v1/messages";

        #[derive(Serialize)]
        struct AnthropicRequest {
            model: String,
            messages: Vec<AnthropicMessage>,
            max_tokens: i32,
        }

        #[derive(Serialize)]
        struct AnthropicMessage {
            role: String,
            content: String,
        }

        let messages: Vec<AnthropicMessage> = request.messages.iter()
            .filter(|m| m.role != MessageRole::System)
            .map(|m| {
                let role = match m.role {
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                    _ => "user",
                };
                AnthropicMessage {
                    role: role.to_string(),
                    content: m.content.clone(),
                }
            }).collect();

        let req = AnthropicRequest {
            model: request.model.clone(),
            messages,
            max_tokens: request.max_tokens.unwrap_or(4096),
        };

        let response = self.client
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if response.status().is_success() {
            #[derive(Deserialize)]
            struct AnthropicResponse {
                id: String,
                content: Vec<ContentBlock>,
                model: String,
            }

            #[derive(Deserialize)]
            struct ContentBlock {
                text: Option<String>,
            }

            let resp: AnthropicResponse = response.json()
                .await
                .map_err(|e| LlmError::ParseError(e.to_string()))?;

            let content = resp.content.first()
                .and_then(|c| c.text.clone())
                .unwrap_or_default();

            Ok(ChatCompletion {
                id: resp.id,
                object: "chat.completion".to_string(),
                created: chrono::Utc::now().timestamp(),
                model: resp.model,
                choices: vec![crate::chat::ChatChoice {
                    index: 0,
                    message: ChatMessage {
                        role: MessageRole::Assistant,
                        content,
                        name: None,
                        tool_calls: None,
                        tool_call_id: None,
                    },
                    finish_reason: Some("stop".to_string()),
                }],
                usage: None,
                system_fingerprint: None,
            })
        } else {
            Err(LlmError::ApiError(format!("HTTP {}", response.status())))
        }
    }

    async fn embeddings(&self, _request: EmbeddingRequest) -> LlmResult<EmbeddingResponse> {
        Err(LlmError::UnsupportedModel("Anthropic does not support embeddings".to_string()))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["claude-3-5-sonnet-20241022".to_string(), "claude-3-opus-20240229".to_string()]
    }

    fn default_model(&self) -> &str {
        &self.default_model_name
    }
}
