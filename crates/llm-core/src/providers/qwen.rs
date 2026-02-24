//! Qwen (通义千问) provider - OpenAI-compatible API

use async_trait::async_trait;
use crate::chat::{ChatRequest, ChatCompletion, StreamChunk};
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::LlmResult;
use super::{LlmProvider, ProviderType, openai::OpenAiProvider};

pub struct QwenProvider {
    inner: OpenAiProvider,
}

impl QwenProvider {
    pub fn new(api_key: &str) -> LlmResult<Self> {
        let inner = OpenAiProvider::new(
            api_key,
            Some("https://dashscope.aliyuncs.com/compatible-mode/v1"),
            Default::default(),
        )?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl LlmProvider for QwenProvider {
    fn provider_type(&self) -> ProviderType { ProviderType::Qwen }

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
            "qwen-turbo".into(),
            "qwen-plus".into(),
            "qwen-max".into(),
            "qwen-long".into(),
        ]
    }

    fn default_model(&self) -> &str { "qwen-plus" }
}
