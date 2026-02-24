//! GitHub Copilot provider - token exchange + OpenAI-compatible API

use async_trait::async_trait;
use reqwest::Client;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::chat::{ChatRequest, ChatCompletion, StreamChunk};
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::{LlmError, LlmResult};
use super::{LlmProvider, ProviderType, ProviderDefaults, openai::OpenAiProvider};

const TOKEN_EXCHANGE_URL: &str = "https://api.github.com/copilot_internal/v2/token";

struct CopilotToken {
    token: String,
    expires_at: i64,
    endpoints: CopilotEndpoints,
}

#[derive(Clone)]
struct CopilotEndpoints {
    api: String,
}

pub struct CopilotProvider {
    github_token: String,
    client: Client,
    cached: Arc<RwLock<Option<CopilotToken>>>,
    defaults: ProviderDefaults,
}

impl CopilotProvider {
    pub fn new(github_token: &str, defaults: ProviderDefaults) -> LlmResult<Self> {
        Ok(Self {
            github_token: github_token.to_string(),
            client: Client::new(),
            cached: Arc::new(RwLock::new(None)),
            defaults,
        })
    }

    async fn get_copilot_token(&self) -> LlmResult<(String, String)> {
        // Check cache
        {
            let guard = self.cached.read().await;
            if let Some(ct) = guard.as_ref() {
                let now = chrono::Utc::now().timestamp();
                if ct.expires_at > now + 60 {
                    return Ok((ct.token.clone(), ct.endpoints.api.clone()));
                }
            }
        }

        // Exchange GitHub token for Copilot token
        let resp = self.client
            .get(TOKEN_EXCHANGE_URL)
            .header("Authorization", format!("token {}", self.github_token))
            .header("User-Agent", "oclaw/1.0")
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(LlmError::AuthError(
                format!("Copilot token exchange failed ({}): {}", status, body)
            ));
        }

        let json: serde_json::Value = resp.json().await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        let token = json["token"].as_str()
            .ok_or_else(|| LlmError::AuthError("No token in Copilot response".into()))?
            .to_string();
        let expires_at = json["expires_at"].as_i64().unwrap_or(0);
        let api_url = json["endpoints"]["api"].as_str()
            .unwrap_or("https://api.githubcopilot.com")
            .to_string();

        let ct = CopilotToken {
            token: token.clone(),
            expires_at,
            endpoints: CopilotEndpoints { api: api_url.clone() },
        };
        *self.cached.write().await = Some(ct);

        Ok((token, api_url))
    }

    async fn make_inner(&self) -> LlmResult<OpenAiProvider> {
        let (token, api_url) = self.get_copilot_token().await?;
        OpenAiProvider::new(&token, Some(&api_url), self.defaults.clone())
    }
}

#[async_trait]
impl LlmProvider for CopilotProvider {
    fn provider_type(&self) -> ProviderType { ProviderType::Copilot }

    async fn chat(&self, request: ChatRequest) -> LlmResult<ChatCompletion> {
        let inner = self.make_inner().await?;
        inner.chat(request).await
    }

    async fn chat_stream(&self, request: ChatRequest) -> LlmResult<tokio::sync::mpsc::Receiver<LlmResult<StreamChunk>>> {
        let inner = self.make_inner().await?;
        inner.chat_stream(request).await
    }

    async fn embeddings(&self, request: EmbeddingRequest) -> LlmResult<EmbeddingResponse> {
        let inner = self.make_inner().await?;
        inner.embeddings(request).await
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            "gpt-4o".into(),
            "gpt-4".into(),
            "gpt-3.5-turbo".into(),
        ]
    }

    fn default_model(&self) -> &str { "gpt-4o" }
}
