use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::chat::{
    ChatCompletion, ChatMessage, ChatRequest, MessageRole, StreamChoice, StreamChunk,
};
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::{LlmError, LlmResult};
use crate::providers::{LlmProvider, ProviderType};

pub struct CohereProvider {
    client: Client,
    api_key: String,
    default_model_name: String,
}

impl CohereProvider {
    pub fn new(api_key: &str) -> LlmResult<Self> {
        Ok(Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            default_model_name: "command-r-plus".to_string(),
        })
    }
}

#[async_trait]
impl LlmProvider for CohereProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Cohere
    }

    async fn chat(&self, request: ChatRequest) -> LlmResult<ChatCompletion> {
        let url = "https://api.cohere.ai/v1/chat";

        #[derive(Serialize)]
        struct CohereRequest {
            model: String,
            messages: Vec<CohereMessage>,
        }

        #[derive(Serialize)]
        struct CohereMessage {
            role: String,
            message: String,
        }

        let messages: Vec<CohereMessage> = request
            .messages
            .iter()
            .filter(|m| m.role != MessageRole::System)
            .map(|m| {
                let role = match m.role {
                    MessageRole::User => "USER",
                    MessageRole::Assistant => "CHATBOT",
                    _ => "USER",
                };
                CohereMessage {
                    role: role.to_string(),
                    message: m.content.clone(),
                }
            })
            .collect();

        let req = CohereRequest {
            model: request.model.clone(),
            messages,
        };

        let response = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if response.status().is_success() {
            #[derive(Deserialize)]
            struct CohereResponse {
                id: String,
                message: CohereMessage2,
            }

            #[derive(Deserialize)]
            struct CohereMessage2 {
                content: String,
            }

            let resp: CohereResponse = response
                .json()
                .await
                .map_err(|e| LlmError::ParseError(e.to_string()))?;

            Ok(ChatCompletion {
                id: resp.id,
                object: "chat.completion".to_string(),
                created: chrono::Utc::now().timestamp(),
                model: request.model,
                choices: vec![crate::chat::ChatChoice {
                    index: 0,
                    message: ChatMessage {
                        role: MessageRole::Assistant,
                        content: resp.message.content,
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

    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> LlmResult<tokio::sync::mpsc::Receiver<LlmResult<StreamChunk>>> {
        let url = "https://api.cohere.ai/v2/chat";

        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter(|m| m.role != MessageRole::System)
            .map(|m| {
                let role = match m.role {
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                    _ => "user",
                };
                serde_json::json!({ "role": role, "content": m.content })
            })
            .collect();

        let req_body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "stream": true,
        });

        let response = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&req_body)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(LlmError::ApiError(format!("HTTP {}: {}", status, body)));
        }

        let (tx, rx) = tokio::sync::mpsc::channel(32);
        let model = request.model.clone();

        tokio::spawn(async move {
            use futures_util::StreamExt;
            let mut stream = response.bytes_stream();
            let mut buffer = String::new();

            while let Some(chunk) = stream.next().await {
                let bytes = match chunk {
                    Ok(b) => b,
                    Err(e) => {
                        let _ = tx.send(Err(LlmError::NetworkError(e.to_string()))).await;
                        break;
                    }
                };
                buffer.push_str(&String::from_utf8_lossy(&bytes));

                while let Some(line_end) = buffer.find('\n') {
                    let line = buffer[..line_end].trim().to_string();
                    buffer = buffer[line_end + 1..].to_string();

                    // Cohere v2 streams SSE: "data: {...}" or event lines
                    let data_str = if let Some(d) = line.strip_prefix("data: ") {
                        d
                    } else {
                        continue;
                    };
                    if data_str.is_empty() {
                        continue;
                    }

                    let json: serde_json::Value = match serde_json::from_str(data_str) {
                        Ok(v) => v,
                        Err(_) => continue,
                    };

                    // Cohere v2 stream events: content-delta, stream-end
                    let event_type = json["type"].as_str().unwrap_or("");
                    let text = match event_type {
                        "content-delta" => json["delta"]["message"]["content"]["text"]
                            .as_str()
                            .unwrap_or("")
                            .to_string(),
                        _ => continue,
                    };

                    let chunk = StreamChunk {
                        id: "cohere-stream".to_string(),
                        object: "chat.completion.chunk".to_string(),
                        created: 0,
                        model: model.clone(),
                        choices: vec![StreamChoice {
                            index: 0,
                            delta: Some(ChatMessage {
                                role: MessageRole::Assistant,
                                content: text,
                                name: None,
                                tool_calls: None,
                                tool_call_id: None,
                            }),
                            finish_reason: None,
                        }],
                    };

                    if tx.send(Ok(chunk)).await.is_err() {
                        return;
                    }
                }
            }
        });

        Ok(rx)
    }

    async fn embeddings(&self, request: EmbeddingRequest) -> LlmResult<EmbeddingResponse> {
        let url = "https://api.cohere.ai/v1/embed";

        let texts = match request.input {
            crate::embedding::EmbeddingInput::String(s) => vec![s],
            crate::embedding::EmbeddingInput::Strings(v) => v,
        };

        #[derive(Serialize)]
        struct EmbedRequest {
            model: String,
            texts: Vec<String>,
        }

        let req = EmbedRequest {
            model: request.model.clone(),
            texts,
        };

        let response = self
            .client
            .post(url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if response.status().is_success() {
            #[derive(Deserialize)]
            struct EmbedResponse {
                embeddings: Vec<Vec<f32>>,
            }

            let resp: EmbedResponse = response
                .json()
                .await
                .map_err(|e| LlmError::ParseError(e.to_string()))?;

            let data: Vec<crate::embedding::Embedding> = resp
                .embeddings
                .into_iter()
                .enumerate()
                .map(|(index, embedding)| crate::embedding::Embedding {
                    object: "embedding".to_string(),
                    embedding,
                    index: index as i32,
                })
                .collect();

            Ok(EmbeddingResponse {
                object: "list".to_string(),
                data,
                model: request.model,
                usage: crate::embedding::EmbeddingUsage {
                    prompt_tokens: 0,
                    total_tokens: 0,
                },
            })
        } else {
            Err(LlmError::ApiError(format!("HTTP {}", response.status())))
        }
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["command-r-plus".to_string(), "command-r".to_string()]
    }

    fn default_model(&self) -> &str {
        &self.default_model_name
    }
}
