//! Cloudflare AI Gateway provider - Anthropic-compatible proxy

use async_trait::async_trait;
use crate::chat::{ChatRequest, ChatCompletion, StreamChunk};
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::{LlmError, LlmResult};
use super::{LlmProvider, ProviderType, anthropic::AnthropicProvider};

pub struct CloudflareProvider {
    inner: AnthropicProvider,
}

impl CloudflareProvider {
    /// Create a Cloudflare AI Gateway provider.
    /// `api_key` is the upstream provider's API key (e.g. Anthropic key).
    /// `base_url` must be the full gateway URL, e.g.
    /// `https://gateway.ai.cloudflare.com/v1/{account_id}/{gateway_id}/anthropic`
    pub fn new(api_key: &str, base_url: Option<&str>) -> LlmResult<Self> {
        let url = base_url.ok_or_else(|| {
            LlmError::InvalidRequest(
                "Cloudflare AI Gateway requires base_url (e.g. https://gateway.ai.cloudflare.com/v1/{account}/{gw}/anthropic)".into()
            )
        })?;
        let inner = AnthropicProvider::new(api_key, Some(url), Default::default())?;
        Ok(Self { inner })
    }
}

#[async_trait]
impl LlmProvider for CloudflareProvider {
    fn provider_type(&self) -> ProviderType { ProviderType::Cloudflare }

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
        self.inner.supported_models()
    }

    fn default_model(&self) -> &str { self.inner.default_model() }
}
