use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::chat::{ChatMessage, ChatRequest, ChatCompletion, MessageRole};
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::{LlmError, LlmResult};
use crate::providers::{LlmProvider, ProviderType};

pub struct OllamaProvider {
    client: Client,
    base_url: String,
    default_model_name: String,
}

impl OllamaProvider {
    pub fn new(base_url: &str) -> LlmResult<Self> {
        Ok(Self {
            client: Client::new(),
            base_url: base_url.to_string(),
            default_model_name: "llama3.2".to_string(),
        })
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Ollama
    }

    async fn chat(&self, request: ChatRequest) -> LlmResult<ChatCompletion> {
        let url = format!("{}/api/chat", self.base_url);

        #[derive(Serialize)]
        struct OllamaRequest {
            model: String,
            messages: Vec<OllamaMessage>,
            stream: bool,
        }

        #[derive(Serialize)]
        struct OllamaMessage {
            role: String,
            content: String,
        }

        let messages: Vec<OllamaMessage> = request.messages.iter().map(|m| {
            let role = match m.role {
                MessageRole::System => "system",
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::Tool => "tool",
            };
            OllamaMessage {
                role: role.to_string(),
                content: m.content.clone(),
            }
        }).collect();

        let req = OllamaRequest {
            model: request.model.clone(),
            messages,
            stream: false,
        };

        let response = self.client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if response.status().is_success() {
            #[derive(Deserialize)]
            struct OllamaResponse {
                message: OllamaMessage,
                done: bool,
            }

            #[derive(Deserialize)]
            struct OllamaMessage {
                content: String,
            }

            let resp: OllamaResponse = response.json()
                .await
                .map_err(|e| LlmError::ParseError(e.to_string()))?;

            Ok(ChatCompletion {
                id: format!("ollama-{}", uuid::Uuid::new_v4()),
                object: "chat.completion".to_string(),
                created: chrono::Utc::now().timestamp(),
                model: request.model,
                choices: vec![crate::chat::ChatChoice {
                    index: 0,
                    message: ChatMessage {
                        role: MessageRole::Assistant,
                        content: resp.message.content,
                        name: None,
                        tool_calls: None,
                        tool_call_id: None,
                    },
                    finish_reason: if resp.done { Some("stop".to_string()) } else { None },
                }],
                usage: None,
                system_fingerprint: None,
            })
        } else {
            Err(LlmError::ApiError(format!("HTTP {}", response.status())))
        }
    }

    async fn embeddings(&self, _request: EmbeddingRequest) -> LlmResult<EmbeddingResponse> {
        Err(LlmError::UnsupportedModel("Use /api/embeddings endpoint directly".to_string()))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["llama3.2".to_string(), "llama3.1".to_string(), "mistral".to_string()]
    }

    fn default_model(&self) -> &str {
        &self.default_model_name
    }
}
