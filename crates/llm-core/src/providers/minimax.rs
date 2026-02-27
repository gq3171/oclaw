//! MiniMax provider - Anthropic-compatible endpoint wrapper.
//!
//! MiniMax chat endpoints are exposed with Anthropic-compatible message format.
//! We route through `AnthropicProvider` to keep tool-calling behavior aligned.

use super::{LlmProvider, ProviderDefaults, ProviderType, anthropic::AnthropicProvider};
use crate::chat::{ChatCompletion, ChatRequest, StreamChunk};
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::LlmResult;
use async_trait::async_trait;

pub struct MinimaxProvider {
    inner: AnthropicProvider,
}

impl MinimaxProvider {
    pub fn new(
        api_key: &str,
        base_url: Option<&str>,
        defaults: ProviderDefaults,
    ) -> LlmResult<Self> {
        let base = base_url.unwrap_or("https://api.minimax.io/anthropic");
        let inner = AnthropicProvider::new(api_key, Some(base), defaults)?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl LlmProvider for MinimaxProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Minimax
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
        self.inner.supported_models()
    }

    fn default_model(&self) -> &str {
        self.inner.default_model()
    }

    async fn list_models(&self) -> LlmResult<Vec<String>> {
        self.inner.list_models().await
    }
}
