use async_trait::async_trait;
use reqwest::Client;

use crate::chat::{ChatMessage, ChatRequest, ChatCompletion, StreamChunk, StreamChoice, MessageRole};
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

    /// Convert ChatMessages to Gemini contents format, skipping system messages.
    fn build_contents(messages: &[ChatMessage]) -> Vec<serde_json::Value> {
        messages.iter()
            .filter(|m| m.role != MessageRole::System)
            .map(|m| {
                let role = match m.role {
                    MessageRole::User | MessageRole::Tool => "user",
                    MessageRole::Assistant => "model",
                    MessageRole::System => unreachable!(),
                };
                serde_json::json!({
                    "role": role,
                    "parts": [{ "text": m.content }]
                })
            })
            .collect()
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
            request.model, self.api_key
        );

        let mut req_body = serde_json::json!({
            "contents": Self::build_contents(&request.messages),
        });

        if let Some(sys) = request.messages.iter().find(|m| m.role == MessageRole::System) {
            req_body["systemInstruction"] = serde_json::json!({
                "parts": [{ "text": sys.content }]
            });
        }

        if let Some(ref tools) = request.tools {
            let decls: Vec<serde_json::Value> = tools.iter().map(|t| {
                serde_json::json!({
                    "name": t.function.name,
                    "description": t.function.description,
                    "parameters": t.function.parameters,
                })
            }).collect();
            req_body["tools"] = serde_json::json!([{ "functionDeclarations": decls }]);
        }

        let response = self.client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&req_body)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(LlmError::ApiError(format!("HTTP {}", response.status())));
        }

        let json: serde_json::Value = response.json().await
            .map_err(|e| LlmError::ParseError(e.to_string()))?;

        let candidate = &json["candidates"][0];
        let parts = candidate["content"]["parts"].as_array();

        // Extract text content
        let content = parts
            .and_then(|p| p.iter().find_map(|part| part["text"].as_str()))
            .unwrap_or("")
            .to_string();

        // Extract function calls as tool_calls
        let tool_calls: Option<Vec<crate::chat::ToolCall>> = parts.and_then(|p| {
            let calls: Vec<crate::chat::ToolCall> = p.iter().filter_map(|part| {
                let fc = part.get("functionCall")?;
                Some(crate::chat::ToolCall {
                    id: format!("call_{}", uuid::Uuid::new_v4().simple()),
                    type_: "function".to_string(),
                    function: crate::chat::ToolCallFunction {
                        name: fc["name"].as_str()?.to_string(),
                        arguments: fc["args"].to_string(),
                    },
                })
            }).collect();
            if calls.is_empty() { None } else { Some(calls) }
        });

        let finish = if tool_calls.is_some() { "tool_calls" } else { "stop" };

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
                    tool_calls,
                    tool_call_id: None,
                },
                finish_reason: Some(finish.to_string()),
            }],
            usage: None,
            system_fingerprint: None,
        })
    }

    async fn chat_stream(&self, request: ChatRequest) -> LlmResult<tokio::sync::mpsc::Receiver<LlmResult<StreamChunk>>> {
        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/{}:streamGenerateContent?alt=sse&key={}",
            request.model, self.api_key
        );

        let mut req_body = serde_json::json!({
            "contents": Self::build_contents(&request.messages),
        });

        // Add system instruction if present
        if let Some(sys) = request.messages.iter().find(|m| m.role == MessageRole::System) {
            req_body["systemInstruction"] = serde_json::json!({
                "parts": [{ "text": sys.content }]
            });
        }

        // Add tools (function declarations) if present
        if let Some(ref tools) = request.tools {
            let decls: Vec<serde_json::Value> = tools.iter().map(|t| {
                serde_json::json!({
                    "name": t.function.name,
                    "description": t.function.description,
                    "parameters": t.function.parameters,
                })
            }).collect();
            req_body["tools"] = serde_json::json!([{ "functionDeclarations": decls }]);
        }

        let response = self.client
            .post(&url)
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

                    let Some(data) = line.strip_prefix("data: ") else { continue };
                    if data.is_empty() { continue; }

                    let json: serde_json::Value = match serde_json::from_str(data) {
                        Ok(v) => v,
                        Err(e) => {
                            tracing::debug!("Google stream: failed to parse SSE data: {}", e);
                            continue;
                        }
                    };

                    let text = json["candidates"][0]["content"]["parts"][0]["text"]
                        .as_str().unwrap_or("").to_string();

                    let finish = json["candidates"][0]["finishReason"]
                        .as_str().map(|s| s.to_lowercase());

                    let chunk = StreamChunk {
                        id: "gemini-stream".to_string(),
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
                            finish_reason: finish,
                        }],
                    };

                    if tx.send(Ok(chunk)).await.is_err() { return; }
                }
            }

            // Process any remaining data in buffer after stream ends
            for line in buffer.lines() {
                let line = line.trim();
                let Some(data) = line.strip_prefix("data: ") else { continue };
                if data.is_empty() { continue; }
                let json: serde_json::Value = match serde_json::from_str(data) {
                    Ok(v) => v,
                    Err(_) => continue,
                };
                let text = json["candidates"][0]["content"]["parts"][0]["text"]
                    .as_str().unwrap_or("").to_string();
                if text.is_empty() { continue; }
                let finish = json["candidates"][0]["finishReason"]
                    .as_str().map(|s| s.to_lowercase());
                let chunk = StreamChunk {
                    id: "gemini-stream".to_string(),
                    object: "chat.completion.chunk".to_string(),
                    created: 0,
                    model: model.clone(),
                    choices: vec![StreamChoice {
                        index: 0,
                        delta: Some(ChatMessage {
                            role: MessageRole::Assistant,
                            content: text,
                            name: None, tool_calls: None, tool_call_id: None,
                        }),
                        finish_reason: finish,
                    }],
                };
                if tx.send(Ok(chunk)).await.is_err() { return; }
            }
        });

        Ok(rx)
    }

    async fn embeddings(&self, request: EmbeddingRequest) -> LlmResult<EmbeddingResponse> {
        let texts = match request.input {
            crate::embedding::EmbeddingInput::String(s) => vec![s],
            crate::embedding::EmbeddingInput::Strings(v) => v,
        };

        let embed_model = if request.model.contains("embed") {
            request.model.clone()
        } else {
            "text-embedding-004".to_string()
        };

        let mut all_embeddings = Vec::new();
        for (i, text) in texts.iter().enumerate() {
            let url = format!(
                "https://generativelanguage.googleapis.com/v1beta/models/{}:embedContent?key={}",
                embed_model, self.api_key
            );
            let body = serde_json::json!({
                "model": format!("models/{}", embed_model),
                "content": { "parts": [{ "text": text }] }
            });

            let resp = self.client.post(&url)
                .json(&body)
                .send().await
                .map_err(|e| LlmError::NetworkError(e.to_string()))?;

            if !resp.status().is_success() {
                return Err(LlmError::ApiError(format!("HTTP {}", resp.status())));
            }

            let json: serde_json::Value = resp.json().await
                .map_err(|e| LlmError::ParseError(e.to_string()))?;

            let values: Vec<f32> = json["embedding"]["values"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_f64().map(|f| f as f32)).collect())
                .unwrap_or_default();

            all_embeddings.push(crate::embedding::Embedding {
                object: "embedding".to_string(),
                embedding: values,
                index: i as i32,
            });
        }

        Ok(EmbeddingResponse {
            object: "list".to_string(),
            data: all_embeddings,
            model: embed_model,
            usage: crate::embedding::EmbeddingUsage { prompt_tokens: 0, total_tokens: 0 },
        })
    }

    fn supported_models(&self) -> Vec<String> {
        vec!["gemini-2.0-flash".to_string(), "gemini-1.5-pro".to_string()]
    }

    fn default_model(&self) -> &str {
        &self.default_model_name
    }
}
