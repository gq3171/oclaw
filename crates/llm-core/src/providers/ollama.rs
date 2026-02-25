use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::chat::{ChatMessage, ChatRequest, ChatCompletion, StreamChunk, StreamChoice, MessageRole};
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::{LlmError, LlmResult};
use crate::providers::{LlmProvider, ProviderType};

pub struct OllamaProvider {
    client: Client,
    base_url: String,
    default_model_name: String,
}

impl OllamaProvider {
    pub fn new(base_url: &str) -> LlmResult<Self> {
        Ok(Self {
            client: Client::new(),
            base_url: base_url.to_string(),
            default_model_name: "llama3.2".to_string(),
        })
    }
}

#[async_trait]
impl LlmProvider for OllamaProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Ollama
    }

    async fn chat(&self, request: ChatRequest) -> LlmResult<ChatCompletion> {
        let url = format!("{}/api/chat", self.base_url);

        #[derive(Serialize)]
        struct OllamaRequest {
            model: String,
            messages: Vec<OllamaMessage>,
            stream: bool,
        }

        #[derive(Serialize)]
        struct OllamaMessage {
            role: String,
            content: String,
        }

        let messages: Vec<OllamaMessage> = request.messages.iter().map(|m| {
            let role = match m.role {
                MessageRole::System => "system",
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::Tool => "tool",
            };
            OllamaMessage {
                role: role.to_string(),
                content: m.content.clone(),
            }
        }).collect();

        let req = OllamaRequest {
            model: request.model.clone(),
            messages,
            stream: false,
        };

        let response = self.client
            .post(&url)
            .json(&req)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if response.status().is_success() {
            #[derive(Deserialize)]
            struct OllamaResponse {
                message: OllamaMessage,
                done: bool,
            }

            #[derive(Deserialize)]
            struct OllamaMessage {
                content: String,
            }

            let resp: OllamaResponse = response.json()
                .await
                .map_err(|e| LlmError::ParseError(e.to_string()))?;

            Ok(ChatCompletion {
                id: format!("ollama-{}", uuid::Uuid::new_v4()),
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
                    finish_reason: if resp.done { Some("stop".to_string()) } else { None },
                }],
                usage: None,
                system_fingerprint: None,
            })
        } else {
            Err(LlmError::ApiError(format!("HTTP {}", response.status())))
        }
    }

    async fn chat_stream(&self, request: ChatRequest) -> LlmResult<tokio::sync::mpsc::Receiver<LlmResult<StreamChunk>>> {
        let url = format!("{}/api/chat", self.base_url);

        #[derive(Serialize)]
        struct OllamaStreamRequest {
            model: String,
            messages: Vec<OllamaMsg>,
            stream: bool,
        }
        #[derive(Serialize)]
        struct OllamaMsg {
            role: String,
            content: String,
        }

        let messages: Vec<OllamaMsg> = request.messages.iter().map(|m| {
            let role = match m.role {
                MessageRole::System => "system",
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::Tool => "tool",
            };
            OllamaMsg { role: role.to_string(), content: m.content.clone() }
        }).collect();

        let req = OllamaStreamRequest {
            model: request.model.clone(),
            messages,
            stream: true,
        };

        let response = self.client
            .post(&url)
            .json(&req)
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

                // Ollama streams newline-delimited JSON
                while let Some(line_end) = buffer.find('\n') {
                    let line = buffer[..line_end].trim().to_string();
                    buffer = buffer[line_end + 1..].to_string();

                    if line.is_empty() { continue; }

                    #[derive(Deserialize)]
                    struct OllamaStreamChunk {
                        message: Option<OllamaStreamMsg>,
                        done: bool,
                    }
                    #[derive(Deserialize)]
                    struct OllamaStreamMsg {
                        content: String,
                    }

                    let parsed: OllamaStreamChunk = match serde_json::from_str(&line) {
                        Ok(c) => c,
                        Err(_) => continue,
                    };

                    let content = parsed.message.map(|m| m.content).unwrap_or_default();
                    let finish = if parsed.done { Some("stop".to_string()) } else { None };

                    let chunk = StreamChunk {
                        id: "ollama-stream".to_string(),
                        object: "chat.completion.chunk".to_string(),
                        created: 0,
                        model: model.clone(),
                        choices: vec![StreamChoice {
                            index: 0,
                            delta: Some(ChatMessage {
                                role: MessageRole::Assistant,
                                content,
                                name: None,
                                tool_calls: None,
                                tool_call_id: None,
                            }),
                            finish_reason: finish,
                        }],
                    };

                    if tx.send(Ok(chunk)).await.is_err() {
                        return;
                    }

                    if parsed.done { return; }
                }
            }
        });

        Ok(rx)
    }

    async fn embeddings(&self, _request: EmbeddingRequest) -> LlmResult<EmbeddingResponse> {
        Err(LlmError::UnsupportedModel("Use /api/embeddings endpoint directly".to_string()))
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["llama3.2".to_string(), "llama3.1".to_string(), "mistral".to_string()]
    }

    fn default_model(&self) -> &str {
        &self.default_model_name
    }
}
