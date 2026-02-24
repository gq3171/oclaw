//! Hugging Face provider - OpenAI-compatible API

use async_trait::async_trait;
use crate::chat::{ChatRequest, ChatCompletion, StreamChunk};
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::LlmResult;
use super::{LlmProvider, ProviderType, openai::OpenAiProvider};

pub struct HuggingFaceProvider {
    inner: OpenAiProvider,
}

impl HuggingFaceProvider {
    pub fn new(api_key: &str) -> LlmResult<Self> {
        let inner = OpenAiProvider::new(
            api_key,
            Some("https://router.huggingface.co/v1"),
            Default::default(),
        )?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl LlmProvider for HuggingFaceProvider {
    fn provider_type(&self) -> ProviderType { ProviderType::HuggingFace }

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
        vec![
            "meta-llama/Llama-3.1-70B-Instruct".into(),
            "mistralai/Mixtral-8x7B-Instruct-v0.1".into(),
            "microsoft/Phi-3-mini-4k-instruct".into(),
        ]
    }

    fn default_model(&self) -> &str { "meta-llama/Llama-3.1-70B-Instruct" }
}
