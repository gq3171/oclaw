//! Volcengine / Doubao (豆包/火山引擎) provider - OpenAI-compatible API

use super::{LlmProvider, ProviderType, openai::OpenAiProvider};
use crate::chat::{ChatCompletion, ChatRequest, StreamChunk};
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::LlmResult;
use async_trait::async_trait;

pub struct VolcengineProvider {
    inner: OpenAiProvider,
}

impl VolcengineProvider {
    pub fn new(api_key: &str) -> LlmResult<Self> {
        let inner = OpenAiProvider::new(
            api_key,
            Some("https://ark.cn-beijing.volces.com/api/v3"),
            Default::default(),
        )?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl LlmProvider for VolcengineProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Volcengine
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
            "doubao-seed-1.6".into(),
            "doubao-1.5-pro".into(),
            "deepseek-v3".into(),
        ]
    }

    fn default_model(&self) -> &str {
        "doubao-1.5-pro"
    }
}
