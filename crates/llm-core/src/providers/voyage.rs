use async_trait::async_trait;
use reqwest::Client;

use crate::chat::{ChatCompletion, ChatRequest};
use crate::embedding::{
    Embedding, EmbeddingInput, EmbeddingRequest, EmbeddingResponse, EmbeddingUsage,
};
use crate::error::{LlmError, LlmResult};
use crate::providers::{LlmProvider, ProviderDefaults, ProviderType};

pub struct VoyageProvider {
    client: Client,
    api_key: String,
    base_url: String,
    default_model_name: String,
}

impl VoyageProvider {
    pub fn new(
        api_key: &str,
        base_url: Option<&str>,
        defaults: ProviderDefaults,
    ) -> LlmResult<Self> {
        Ok(Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            base_url: base_url
                .unwrap_or("https://api.voyageai.com/v1")
                .trim_end_matches('/')
                .to_string(),
            default_model_name: defaults
                .model
                .filter(|m| !m.trim().is_empty())
                .unwrap_or_else(|| "voyage-3-lite".to_string()),
        })
    }
}

#[async_trait]
impl LlmProvider for VoyageProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Voyage
    }

    async fn chat(&self, _request: ChatRequest) -> LlmResult<ChatCompletion> {
        Err(LlmError::UnsupportedModel(
            "Voyage provider supports embeddings only; chat completion is not available"
                .to_string(),
        ))
    }

    async fn embeddings(&self, request: EmbeddingRequest) -> LlmResult<EmbeddingResponse> {
        let input = match request.input {
            EmbeddingInput::String(s) => vec![s],
            EmbeddingInput::Strings(v) => v,
        };

        let model = if request.model.trim().is_empty() {
            self.default_model_name.clone()
        } else {
            request.model.clone()
        };

        let body = serde_json::json!({
            "model": model,
            "input": input,
            "input_type": "document",
            "truncation": true,
        });

        let response = self
            .client
            .post(format!("{}/embeddings", self.base_url))
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(LlmError::ApiError(format!("HTTP {}: {}", status, text)));
        }

        let payload: serde_json::Value = response
            .json()
            .await
            .map_err(|e| LlmError::ParseError(e.to_string()))?;

        let data = payload
            .get("data")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                LlmError::ParseError("missing data[] in voyage embedding response".to_string())
            })?
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                let embedding = item
                    .get("embedding")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| {
                        LlmError::ParseError("missing embedding vector in data[]".to_string())
                    })?
                    .iter()
                    .map(|n| {
                        n.as_f64().map(|f| f as f32).ok_or_else(|| {
                            LlmError::ParseError("embedding element is not numeric".to_string())
                        })
                    })
                    .collect::<Result<Vec<f32>, LlmError>>()?;

                Ok(Embedding {
                    object: "embedding".to_string(),
                    embedding,
                    index: idx as i32,
                })
            })
            .collect::<Result<Vec<Embedding>, LlmError>>()?;

        let usage = payload.get("usage");
        let total_tokens = usage
            .and_then(|u| u.get("total_tokens"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0) as i32;
        let prompt_tokens = usage
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(|v| v.as_i64())
            .unwrap_or(total_tokens as i64) as i32;

        Ok(EmbeddingResponse {
            object: "list".to_string(),
            data,
            model,
            usage: EmbeddingUsage {
                prompt_tokens,
                total_tokens,
            },
        })
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            "voyage-3-lite".to_string(),
            "voyage-3".to_string(),
            "voyage-code-3".to_string(),
            "voyage-4-lite".to_string(),
            "voyage-4-large".to_string(),
        ]
    }

    fn default_model(&self) -> &str {
        &self.default_model_name
    }
}
