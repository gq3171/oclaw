use async_trait::async_trait;
use reqwest::Client;
use serde::Serialize;

use crate::chat::{ChatCompletion, ChatRequest, MessageRole};
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::{LlmError, LlmResult};
use crate::providers::media_markdown::{
    ParsedMarkdownSegment, markdown_contains_data_url_image, parse_markdown_data_url_segments,
};
use crate::providers::{LlmProvider, ProviderType};

pub struct OpenAiProvider {
    client: Client,
    api_key: String,
    base_url: String,
    default_model_name: String,
    default_max_tokens: Option<i32>,
    default_temperature: Option<f64>,
}

impl OpenAiProvider {
    pub fn new(
        api_key: &str,
        base_url: Option<&str>,
        defaults: crate::providers::ProviderDefaults,
    ) -> LlmResult<Self> {
        let base = base_url.unwrap_or("https://api.openai.com/v1");
        let model = defaults.model.unwrap_or_else(|| "gpt-4o".to_string());
        Ok(Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            base_url: base.to_string(),
            default_max_tokens: defaults.max_tokens,
            default_temperature: defaults.temperature,
            default_model_name: model,
        })
    }
}

#[async_trait]
impl LlmProvider for OpenAiProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::OpenAi
    }

    async fn chat(&self, request: ChatRequest) -> LlmResult<ChatCompletion> {
        let url = format!("{}/chat/completions", self.base_url);

        // Build messages as serde_json::Value to handle all fields correctly
        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    MessageRole::System => "system",
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                    MessageRole::Tool => "tool",
                };
                let mut msg = serde_json::json!({
                    "role": role,
                    "content": openai_message_content(role, &m.content),
                });
                if let Some(ref tc) = m.tool_calls {
                    let calls: Vec<serde_json::Value> = tc.iter().map(|c| {
                    serde_json::json!({
                        "id": c.id,
                        "type": c.type_,
                        "function": { "name": c.function.name, "arguments": c.function.arguments }
                    })
                }).collect();
                    msg["tool_calls"] = serde_json::Value::Array(calls);
                }
                if let Some(ref id) = m.tool_call_id {
                    msg["tool_call_id"] = serde_json::Value::String(id.clone());
                }
                if let Some(ref name) = m.name {
                    msg["name"] = serde_json::Value::String(name.clone());
                }
                msg
            })
            .collect();

        let mut req = serde_json::json!({
            "model": request.model,
            "messages": messages,
        });
        if let Some(t) = request.temperature.or(self.default_temperature) {
            req["temperature"] = serde_json::json!(t);
        }
        if let Some(m) = request.max_tokens.or(self.default_max_tokens) {
            req["max_tokens"] = serde_json::json!(m);
        }
        if let Some(ref tools) = request.tools {
            let tools_json: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "type": t.type_,
                        "function": {
                            "name": t.function.name,
                            "description": t.function.description,
                            "parameters": t.function.parameters,
                        }
                    })
                })
                .collect();
            req["tools"] = serde_json::Value::Array(tools_json);
        }

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(LlmError::ApiError(format!("HTTP {} : {}", status, body)));
        }

        // Parse response manually to handle snake_case fields
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| LlmError::ParseError(e.to_string()))?;

        parse_chat_completion(body)
    }

    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> LlmResult<tokio::sync::mpsc::Receiver<LlmResult<crate::chat::StreamChunk>>> {
        let url = format!("{}/chat/completions", self.base_url);
        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|m| {
                let role = match m.role {
                    MessageRole::System => "system",
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                    MessageRole::Tool => "tool",
                };
                serde_json::json!({
                    "role": role,
                    "content": openai_message_content(role, &m.content),
                })
            })
            .collect();
        let mut req = serde_json::json!({
            "model": request.model.clone(),
            "messages": messages,
            "stream": true,
        });
        if let Some(t) = request.temperature.or(self.default_temperature) {
            req["temperature"] = serde_json::json!(t);
        }
        if let Some(m) = request.max_tokens.or(self.default_max_tokens) {
            req["max_tokens"] = serde_json::json!(m);
        }

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(LlmError::ApiError(format!("HTTP {}", response.status())));
        }

        let (tx, rx) = tokio::sync::mpsc::channel(32);

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

                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            return;
                        }
                        if let Ok(chunk) = serde_json::from_str::<crate::chat::StreamChunk>(data)
                            && tx.send(Ok(chunk)).await.is_err()
                        {
                            return;
                        }
                    }
                }
            }

            // Process any remaining data in buffer after stream ends
            let line = buffer.trim();
            if let Some(data) = line.strip_prefix("data: ")
                && data != "[DONE]"
                && let Ok(chunk) = serde_json::from_str::<crate::chat::StreamChunk>(data)
            {
                let _ = tx.send(Ok(chunk)).await;
            }
        });

        Ok(rx)
    }

    async fn embeddings(&self, request: EmbeddingRequest) -> LlmResult<EmbeddingResponse> {
        let url = format!("{}/embeddings", self.base_url);

        let input = match request.input {
            crate::embedding::EmbeddingInput::String(s) => vec![s],
            crate::embedding::EmbeddingInput::Strings(v) => v,
        };

        #[derive(Serialize)]
        struct EmbedRequest {
            input: Vec<String>,
            model: String,
        }

        let req = EmbedRequest {
            input,
            model: request.model.clone(),
        };

        let response = self
            .client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&req)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if response.status().is_success() {
            response
                .json::<EmbeddingResponse>()
                .await
                .map_err(|e| LlmError::ParseError(e.to_string()))
        } else {
            Err(LlmError::ApiError(format!("HTTP {}", response.status())))
        }
    }

    fn supported_models(&self) -> Vec<String> {
        let mut models = vec![
            "gpt-4o".to_string(),
            "gpt-4".to_string(),
            "gpt-3.5-turbo".to_string(),
        ];
        if !models.contains(&self.default_model_name) {
            models.insert(0, self.default_model_name.clone());
        }
        models
    }

    fn default_model(&self) -> &str {
        &self.default_model_name
    }
}

fn openai_message_content(role: &str, content: &str) -> serde_json::Value {
    if role == "user" && markdown_contains_data_url_image(content) {
        let mut blocks = Vec::new();
        for seg in parse_markdown_data_url_segments(content) {
            match seg {
                ParsedMarkdownSegment::Text(text) => {
                    if !text.is_empty() {
                        blocks.push(serde_json::json!({
                            "type": "text",
                            "text": text,
                        }));
                    }
                }
                ParsedMarkdownSegment::Image(image) => {
                    blocks.push(serde_json::json!({
                        "type": "image_url",
                        "image_url": {
                            "url": format!("data:{};base64,{}", image.mime_type, image.base64_data),
                        }
                    }));
                }
            }
        }
        if !blocks.is_empty() {
            return serde_json::Value::Array(blocks);
        }
    }
    serde_json::Value::String(content.to_string())
}

/// Parse OpenAI-compatible JSON response into ChatCompletion, handling snake_case fields.
pub fn parse_chat_completion(body: serde_json::Value) -> LlmResult<ChatCompletion> {
    use crate::chat::*;

    let id = body["id"].as_str().unwrap_or("").to_string();
    let object = body["object"]
        .as_str()
        .unwrap_or("chat.completion")
        .to_string();
    let created = body["created"].as_i64().unwrap_or(0);
    let model = body["model"].as_str().unwrap_or("").to_string();

    let choices_arr = body["choices"]
        .as_array()
        .ok_or_else(|| LlmError::ParseError("missing choices".into()))?;

    let mut choices = Vec::new();
    for c in choices_arr {
        let msg = &c["message"];
        let role = match msg["role"].as_str().unwrap_or("assistant") {
            "system" => MessageRole::System,
            "user" => MessageRole::User,
            "tool" => MessageRole::Tool,
            _ => MessageRole::Assistant,
        };

        // Parse content - handle both string and array (MiniMax style)
        let content = if let Some(s) = msg["content"].as_str() {
            s.to_string()
        } else if let Some(arr) = msg["content"].as_array() {
            arr.iter()
                .filter_map(|b| b["text"].as_str())
                .collect::<Vec<_>>()
                .join("")
        } else {
            String::new()
        };

        // Parse tool_calls
        let tool_calls = msg["tool_calls"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|tc| {
                        Some(ToolCall {
                            id: tc["id"].as_str()?.to_string(),
                            type_: tc["type"].as_str().unwrap_or("function").to_string(),
                            function: ToolCallFunction {
                                name: tc["function"]["name"].as_str()?.to_string(),
                                arguments: tc["function"]["arguments"]
                                    .as_str()
                                    .unwrap_or("{}")
                                    .to_string(),
                            },
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .filter(|v| !v.is_empty());

        choices.push(ChatChoice {
            index: c["index"].as_i64().unwrap_or(0) as i32,
            message: ChatMessage {
                role,
                content,
                name: msg["name"].as_str().map(|s| s.to_string()),
                tool_calls,
                tool_call_id: msg["tool_call_id"].as_str().map(|s| s.to_string()),
            },
            finish_reason: c["finish_reason"].as_str().map(|s| s.to_string()),
        });
    }

    Ok(ChatCompletion {
        id,
        object,
        created,
        model,
        choices,
        usage: None,
        system_fingerprint: body["system_fingerprint"].as_str().map(|s| s.to_string()),
    })
}
