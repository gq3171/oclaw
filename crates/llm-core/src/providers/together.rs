//! Together AI provider - OpenAI-compatible API

use super::{LlmProvider, ProviderType, openai::OpenAiProvider};
use crate::chat::{ChatCompletion, ChatRequest, StreamChunk};
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::LlmResult;
use async_trait::async_trait;

pub struct TogetherProvider {
    inner: OpenAiProvider,
}

impl TogetherProvider {
    pub fn new(api_key: &str) -> LlmResult<Self> {
        let inner = OpenAiProvider::new(
            api_key,
            Some("https://api.together.xyz/v1"),
            Default::default(),
        )?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl LlmProvider for TogetherProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Together
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
            "meta-llama/Llama-3.1-405B-Instruct-Turbo".into(),
            "meta-llama/Llama-3.1-70B-Instruct-Turbo".into(),
            "mistralai/Mixtral-8x22B-Instruct-v0.1".into(),
        ]
    }

    fn default_model(&self) -> &str {
        "meta-llama/Llama-3.1-70B-Instruct-Turbo"
    }
}
