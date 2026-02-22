use async_trait::async_trait;
use reqwest::Client;
use serde::Serialize;

use crate::chat::{ChatRequest, ChatCompletion, MessageRole};
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::{LlmError, LlmResult};
use crate::providers::{LlmProvider, ProviderType};

pub struct OpenAiProvider {
    client: Client,
    api_key: String,
    base_url: String,
    default_model_name: String,
}

impl OpenAiProvider {
    pub fn new(api_key: &str, base_url: Option<&str>) -> LlmResult<Self> {
        let base = base_url.unwrap_or("https://api.openai.com/v1");
        
        Ok(Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            base_url: base.to_string(),
            default_model_name: "gpt-4o".to_string(),
        })
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::OpenAi
    }

    async fn chat(&self, request: ChatRequest) -> LlmResult<ChatCompletion> {
        let url = format!("{}/chat/completions", self.base_url);
        
        #[derive(Serialize)]
        struct OpenAiRequest {
            model: String,
            messages: Vec<OpenAiMessage>,
            temperature: Option<f64>,
            max_tokens: Option<i32>,
        }

        #[derive(Serialize)]
        struct OpenAiMessage {
            role: String,
            content: String,
        }

        let messages: Vec<OpenAiMessage> = request.messages.iter().map(|m| {
            let role = match m.role {
                MessageRole::System => "system",
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::Tool => "tool",
            };
            OpenAiMessage {
                role: role.to_string(),
                content: m.content.clone(),
            }
        }).collect();

        let req = OpenAiRequest {
            model: request.model.clone(),
            messages,
            temperature: request.temperature,
            max_tokens: request.max_tokens,
        };

        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if response.status().is_success() {
            response.json::<ChatCompletion>()
                .await
                .map_err(|e| LlmError::ParseError(e.to_string()))
        } else {
            Err(LlmError::ApiError(format!("HTTP {}", response.status())))
        }
    }

    async fn embeddings(&self, request: EmbeddingRequest) -> LlmResult<EmbeddingResponse> {
        let url = format!("{}/embeddings", self.base_url);
        
        let input = match request.input {
            crate::embedding::EmbeddingInput::String(s) => vec![s],
            crate::embedding::EmbeddingInput::Strings(v) => v,
        };

        #[derive(Serialize)]
        struct EmbedRequest {
            input: Vec<String>,
            model: String,
        }

        let req = EmbedRequest {
            input,
            model: request.model.clone(),
        };

        let response = self.client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if response.status().is_success() {
            response.json::<EmbeddingResponse>()
                .await
                .map_err(|e| LlmError::ParseError(e.to_string()))
        } else {
            Err(LlmError::ApiError(format!("HTTP {}", response.status())))
        }
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gpt-4o".to_string(), "gpt-4".to_string(), "gpt-3.5-turbo".to_string()]
    }

    fn default_model(&self) -> &str {
        &self.default_model_name
    }
}
