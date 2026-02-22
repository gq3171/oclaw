use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod openai;
pub mod anthropic;
pub mod ollama;
pub mod google;
pub mod cohere;

pub use openai::OpenAiProvider;
pub use anthropic::AnthropicProvider;
pub use ollama::OllamaProvider;
pub use google::GoogleProvider;
pub use cohere::CohereProvider;

use crate::chat::{ChatRequest, ChatCompletion};
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::{LlmError, LlmResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderType {
    OpenAi,
    Anthropic,
    Ollama,
    Google,
    Cohere,
    Voyage,
}

impl ProviderType {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "openai" | "openai-compatible" => Some(Self::OpenAi),
            "anthropic" => Some(Self::Anthropic),
            "ollama" => Some(Self::Ollama),
            "google" | "gemini" => Some(Self::Google),
            "cohere" => Some(Self::Cohere),
            "voyage" | "voyageai" => Some(Self::Voyage),
            _ => None,
        }
    }
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    fn provider_type(&self) -> ProviderType;
    
    async fn chat(&self, request: ChatRequest) -> LlmResult<ChatCompletion>;
    
    async fn embeddings(&self, request: EmbeddingRequest) -> LlmResult<EmbeddingResponse>;
    
    fn supported_models(&self) -> Vec<String>;
    
    fn default_model(&self) -> &str;
}

pub struct LlmFactory;

impl LlmFactory {
    pub fn create(provider_type: ProviderType, api_key: &str, base_url: Option<&str>) -> LlmResult<Box<dyn LlmProvider>> {
        match provider_type {
            ProviderType::OpenAi => Ok(Box::new(OpenAiProvider::new(api_key, base_url)?)),
            ProviderType::Anthropic => Ok(Box::new(AnthropicProvider::new(api_key)?)),
            ProviderType::Ollama => Ok(Box::new(OllamaProvider::new(base_url.unwrap_or("http://localhost:11434"))?)),
            ProviderType::Google => Ok(Box::new(GoogleProvider::new(api_key)?)),
            ProviderType::Cohere => Ok(Box::new(CohereProvider::new(api_key)?)),
            ProviderType::Voyage => Err(LlmError::UnsupportedModel("Voyage not implemented".to_string())),
        }
    }
}
