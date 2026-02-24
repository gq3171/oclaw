//! vLLM provider - OpenAI-compatible local inference server

use async_trait::async_trait;
use crate::chat::{ChatRequest, ChatCompletion, StreamChunk};
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::LlmResult;
use super::{LlmProvider, ProviderType, openai::OpenAiProvider};

pub struct VllmProvider {
    inner: OpenAiProvider,
}

impl VllmProvider {
    pub fn new(api_key: Option<&str>, base_url: Option<&str>) -> LlmResult<Self> {
        let inner = OpenAiProvider::new(
            api_key.unwrap_or("EMPTY"),
            Some(base_url.unwrap_or("http://127.0.0.1:8000/v1")),
            Default::default(),
        )?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl LlmProvider for VllmProvider {
    fn provider_type(&self) -> ProviderType { ProviderType::Vllm }

    async fn chat(&self, request: ChatRequest) -> LlmResult<ChatCompletion> {
        self.inner.chat(request).await
    }

    async fn chat_stream(&self, request: ChatRequest) -> LlmResult<tokio::sync::mpsc::Receiver<LlmResult<StreamChunk>>> {
        self.inner.chat_stream(request).await
    }

    async fn embeddings(&self, request: EmbeddingRequest) -> LlmResult<EmbeddingResponse> {
        self.inner.embeddings(request).await
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["default".into()]
    }

    fn default_model(&self) -> &str { "default" }
}
