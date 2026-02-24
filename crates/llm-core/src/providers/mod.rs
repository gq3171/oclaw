use std::str::FromStr;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod openai;
pub mod anthropic;
pub mod ollama;
pub mod google;
pub mod cohere;
pub mod openrouter;
pub mod together;
pub mod bedrock;
pub mod mock;

pub use openai::OpenAiProvider;
pub use anthropic::AnthropicProvider;
pub use ollama::OllamaProvider;
pub use google::GoogleProvider;
pub use cohere::CohereProvider;
pub use openrouter::OpenRouterProvider;
pub use together::TogetherProvider;
pub use bedrock::BedrockProvider;
pub use mock::MockLlmProvider;

use std::collections::HashMap;
use crate::chat::{ChatRequest, ChatCompletion, StreamChunk};
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::{LlmError, LlmResult};

#[derive(Debug, Clone, Default)]
pub struct ProviderDefaults {
    pub model: Option<String>,
    pub max_tokens: Option<i32>,
    pub temperature: Option<f64>,
    pub headers: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    OpenAi,
    Anthropic,
    Ollama,
    Google,
    Cohere,
    Voyage,
    OpenRouter,
    Together,
    Bedrock,
}
impl FromStr for ProviderType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "openai" | "openai-compatible" => Ok(Self::OpenAi),
            "anthropic" => Ok(Self::Anthropic),
            "ollama" => Ok(Self::Ollama),
            "google" | "gemini" => Ok(Self::Google),
            "cohere" => Ok(Self::Cohere),
            "voyage" | "voyageai" => Ok(Self::Voyage),
            "openrouter" => Ok(Self::OpenRouter),
            "together" | "togetherai" => Ok(Self::Together),
            "bedrock" | "aws" => Ok(Self::Bedrock),
            other => Err(format!("Unknown provider: {other}")),
        }
    }
}


#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn provider_type(&self) -> ProviderType;
    async fn chat(&self, request: ChatRequest) -> LlmResult<ChatCompletion>;
    async fn chat_stream(&self, _request: ChatRequest) -> LlmResult<tokio::sync::mpsc::Receiver<LlmResult<StreamChunk>>> {
        Err(LlmError::UnsupportedModel("Streaming not supported by this provider".to_string()))
    }
    async fn embeddings(&self, request: EmbeddingRequest) -> LlmResult<EmbeddingResponse>;
    fn supported_models(&self) -> Vec<String>;
    fn default_model(&self) -> &str;
}

pub struct LlmFactory;

impl LlmFactory {
    pub fn create(provider_type: ProviderType, api_key: &str, base_url: Option<&str>, defaults: ProviderDefaults) -> LlmResult<Box<dyn LlmProvider>> {
        match provider_type {
            ProviderType::OpenAi => Ok(Box::new(OpenAiProvider::new(api_key, base_url, defaults)?)),
            ProviderType::Anthropic => Ok(Box::new(AnthropicProvider::new(api_key, base_url, defaults)?)),
            ProviderType::Ollama => Ok(Box::new(OllamaProvider::new(base_url.unwrap_or("http://localhost:11434"))?)),
            ProviderType::Google => Ok(Box::new(GoogleProvider::new(api_key)?)),
            ProviderType::Cohere => Ok(Box::new(CohereProvider::new(api_key)?)),
            ProviderType::Voyage => Err(LlmError::UnsupportedModel("Voyage not implemented".to_string())),
            ProviderType::OpenRouter => Ok(Box::new(OpenRouterProvider::new(api_key)?)),
            ProviderType::Together => Ok(Box::new(TogetherProvider::new(api_key)?)),
            ProviderType::Bedrock => {
                let (access, secret) = api_key.split_once(':')
                    .ok_or_else(|| LlmError::InvalidRequest("Bedrock api_key must be ACCESS_KEY:SECRET_KEY".into()))?;
                Ok(Box::new(BedrockProvider::new(access, secret, base_url)?))
            }
        }
    }
}
