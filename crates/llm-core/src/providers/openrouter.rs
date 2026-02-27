//! OpenRouter provider - OpenAI-compatible API

use super::{LlmProvider, ProviderType, openai::OpenAiProvider};
use crate::chat::{ChatCompletion, ChatRequest, StreamChunk};
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::LlmResult;
use async_trait::async_trait;

pub struct OpenRouterProvider {
    inner: OpenAiProvider,
}

impl OpenRouterProvider {
    pub fn new(api_key: &str) -> LlmResult<Self> {
        let inner = OpenAiProvider::new(
            api_key,
            Some("https://openrouter.ai/api/v1"),
            Default::default(),
        )?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl LlmProvider for OpenRouterProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::OpenRouter
    }

    async fn chat(&self, request: ChatRequest) -> LlmResult<ChatCompletion> {
        self.inner.chat(request).await
    }

    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> LlmResult<tokio::sync::mpsc::Receiver<LlmResult<StreamChunk>>> {
        self.inner.chat_stream(request).await
    }

    async fn embeddings(&self, request: EmbeddingRequest) -> LlmResult<EmbeddingResponse> {
        self.inner.embeddings(request).await
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            "openai/gpt-4o".into(),
            "anthropic/claude-3.5-sonnet".into(),
            "google/gemini-pro-1.5".into(),
            "meta-llama/llama-3.1-405b-instruct".into(),
        ]
    }

    fn default_model(&self) -> &str {
        "openai/gpt-4o"
    }
}
