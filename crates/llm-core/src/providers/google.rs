use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::chat::{ChatMessage, ChatRequest, ChatCompletion, MessageRole};
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::{LlmError, LlmResult};
use crate::providers::{LlmProvider, ProviderType};

pub struct GoogleProvider {
    client: Client,
    api_key: String,
    default_model_name: String,
}

impl GoogleProvider {
    pub fn new(api_key: &str) -> LlmResult<Self> {
        Ok(Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            default_model_name: "gemini-2.0-flash".to_string(),
        })
    }
}

#[async_trait]
impl LlmProvider for GoogleProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Google
    }

    async fn chat(&self, request: ChatRequest) -> LlmResult<ChatCompletion> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:generateContent?key={}",
            request.model,
            self.api_key
        );

        #[derive(Serialize)]
        struct GoogleRequest {
            contents: Vec<GoogleContent>,
        }

        #[derive(Serialize)]
        struct GoogleContent {
            role: String,
            parts: Vec<GooglePart>,
        }

        #[derive(Serialize)]
        struct GooglePart {
            text: String,
        }

        let contents: Vec<GoogleContent> = request.messages.iter().map(|m| {
            let role = match m.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "model",
                _ => "user",
            };
            GoogleContent {
                role: role.to_string(),
                parts: vec![GooglePart { text: m.content.clone() }],
            }
        }).collect();

        let req = GoogleRequest { contents };

        let response = self.client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if response.status().is_success() {
            #[derive(Deserialize)]
            struct GoogleResponse {
                candidates: Vec<GoogleCandidate>,
            }

            #[derive(Deserialize)]
            struct GoogleCandidate {
                content: GoogleContent2,
            }

            #[derive(Deserialize)]
            struct GoogleContent2 {
                parts: Vec<GooglePart2>,
            }

            #[derive(Deserialize)]
            struct GooglePart2 {
                text: String,
            }

            let resp: GoogleResponse = response.json()
                .await
                .map_err(|e| LlmError::ParseError(e.to_string()))?;

            let content = resp.candidates.first()
                .and_then(|c| c.content.parts.first())
                .map(|p| p.text.clone())
                .unwrap_or_default();

            Ok(ChatCompletion {
                id: format!("gemini-{}", uuid::Uuid::new_v4()),
                object: "chat.completion".to_string(),
                created: chrono::Utc::now().timestamp(),
                model: request.model,
                choices: vec![crate::chat::ChatChoice {
                    index: 0,
                    message: ChatMessage {
                        role: MessageRole::Assistant,
                        content,
                        name: None,
                        tool_calls: None,
                        tool_call_id: None,
                    },
                    finish_reason: Some("stop".to_string()),
                }],
                usage: None,
                system_fingerprint: None,
            })
        } else {
            Err(LlmError::ApiError(format!("HTTP {}", response.status())))
        }
    }

    async fn embeddings(&self, _request: EmbeddingRequest) -> LlmResult<EmbeddingResponse> {
        Err(LlmError::UnsupportedModel("Use Google Embeddings API directly".to_string()))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gemini-2.0-flash".to_string(), "gemini-1.5-pro".to_string()]
    }

    fn default_model(&self) -> &str {
        &self.default_model_name
    }
}
