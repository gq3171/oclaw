use async_trait::async_trait;
use reqwest::Client;

use crate::chat::{ChatCompletion, ChatMessage, ChatRequest, MessageRole};
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::{LlmError, LlmResult};
use crate::providers::media_markdown::{
    ParsedMarkdownSegment, markdown_contains_data_url_image, parse_markdown_data_url_segments,
};
use crate::providers::{LlmProvider, ProviderType};

pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    base_url: String,
    default_model_name: String,
    default_max_tokens: i32,
    default_temperature: Option<f64>,
}

impl AnthropicProvider {
    pub fn new(
        api_key: &str,
        base_url: Option<&str>,
        defaults: crate::providers::ProviderDefaults,
    ) -> LlmResult<Self> {
        let model = defaults
            .model
            .unwrap_or_else(|| "claude-3-5-sonnet-20241022".to_string());
        Ok(Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            base_url: base_url
                .unwrap_or("https://api.anthropic.com")
                .trim_end_matches('/')
                .to_string(),
            default_max_tokens: defaults.max_tokens.unwrap_or(4096),
            default_temperature: defaults.temperature,
            default_model_name: model,
        })
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Anthropic
    }

    async fn chat(&self, request: ChatRequest) -> LlmResult<ChatCompletion> {
        let url = format!("{}/v1/messages", self.base_url);

        let system_prompt = request
            .messages
            .iter()
            .find(|m| m.role == MessageRole::System)
            .map(|m| m.content.clone());

        // Build messages with tool support
        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter(|m| m.role != MessageRole::System)
            .map(anthropic_message)
            .collect();

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "max_tokens": request.max_tokens.unwrap_or(self.default_max_tokens),
        });
        if let Some(t) = request.temperature.or(self.default_temperature) {
            body["temperature"] = serde_json::json!(t);
        }
        if let Some(sp) = system_prompt {
            body["system"] = serde_json::Value::String(sp);
        }
        // Add tools
        if let Some(ref tools) = request.tools {
            let tools_json: Vec<serde_json::Value> = tools
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "name": t.function.name,
                        "description": t.function.description,
                        "input_schema": t.function.parameters,
                    })
                })
                .collect();
            body["tools"] = serde_json::Value::Array(tools_json);
        }

        let response = self
            .client
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            let status = response.status();
            let text = response.text().await.unwrap_or_default();
            return Err(LlmError::ApiError(format!("HTTP {} : {}", status, text)));
        }

        let resp: serde_json::Value = response
            .json()
            .await
            .map_err(|e| LlmError::ParseError(e.to_string()))?;

        parse_anthropic_response(resp)
    }

    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> LlmResult<tokio::sync::mpsc::Receiver<LlmResult<crate::chat::StreamChunk>>> {
        let url = format!("{}/v1/messages", self.base_url);

        let system_prompt = request
            .messages
            .iter()
            .find(|m| m.role == MessageRole::System)
            .map(|m| m.content.clone());

        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .filter(|m| m.role != MessageRole::System)
            .map(anthropic_message)
            .collect();

        let mut body = serde_json::json!({
            "model": request.model,
            "messages": messages,
            "max_tokens": request.max_tokens.unwrap_or(self.default_max_tokens),
            "stream": true,
        });
        if let Some(t) = request.temperature.or(self.default_temperature) {
            body["temperature"] = serde_json::json!(t);
        }
        if let Some(sp) = system_prompt {
            body["system"] = serde_json::Value::String(sp);
        }

        let response = self
            .client
            .post(url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::NetworkError(e.to_string()))?;

        if !response.status().is_success() {
            return Err(LlmError::ApiError(format!("HTTP {}", response.status())));
        }

        let (tx, rx) = tokio::sync::mpsc::channel(32);
        let model = request.model.clone();

        tokio::spawn(async move {
            use futures_util::StreamExt;
            let mut stream = response.bytes_stream();
            let mut buffer = String::new();
            let id = format!("chatcmpl-{}", uuid::Uuid::new_v4());

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

                    if line.is_empty() || !line.starts_with("data: ") {
                        continue;
                    }
                    let data = &line[6..];
                    if let Ok(event) = serde_json::from_str::<serde_json::Value>(data) {
                        if event.get("type").and_then(|t| t.as_str()) == Some("content_block_delta")
                        {
                            if let Some(text) =
                                event.pointer("/delta/text").and_then(|t| t.as_str())
                            {
                                let sc = crate::chat::StreamChunk {
                                    id: id.clone(),
                                    object: "chat.completion.chunk".to_string(),
                                    created: chrono::Utc::now().timestamp(),
                                    model: model.clone(),
                                    choices: vec![crate::chat::StreamChoice {
                                        index: 0,
                                        delta: Some(crate::chat::ChatMessage {
                                            role: crate::chat::MessageRole::Assistant,
                                            content: text.to_string(),
                                            name: None,
                                            tool_calls: None,
                                            tool_call_id: None,
                                        }),
                                        finish_reason: None,
                                    }],
                                };
                                if tx.send(Ok(sc)).await.is_err() {
                                    return;
                                }
                            }
                        } else if event.get("type").and_then(|t| t.as_str()) == Some("message_stop")
                        {
                            return;
                        }
                    }
                }
            }
        });

        Ok(rx)
    }

    async fn embeddings(&self, _request: EmbeddingRequest) -> LlmResult<EmbeddingResponse> {
        Err(LlmError::UnsupportedModel(
            "Anthropic does not support embeddings".to_string(),
        ))
    }

    fn supported_models(&self) -> Vec<String> {
        let mut models = vec![
            "claude-3-5-sonnet-20241022".to_string(),
            "claude-3-opus-20240229".to_string(),
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

/// Convert a ChatMessage to Anthropic API message format.
fn anthropic_message(m: &ChatMessage) -> serde_json::Value {
    let role = match m.role {
        MessageRole::User => "user",
        MessageRole::Tool => "user",
        _ => "assistant",
    };

    // Tool result → user message with tool_result content block
    if m.role == MessageRole::Tool
        && let Some(ref id) = m.tool_call_id
    {
        return serde_json::json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": id,
                "content": m.content,
            }]
        });
    }

    // Assistant message with tool_calls → tool_use content blocks
    if let Some(ref tcs) = m.tool_calls {
        let mut content: Vec<serde_json::Value> = Vec::new();
        if !m.content.is_empty() {
            content.push(serde_json::json!({"type": "text", "text": m.content}));
        }
        for tc in tcs {
            let input: serde_json::Value =
                serde_json::from_str(&tc.function.arguments).unwrap_or(serde_json::json!({}));
            content.push(serde_json::json!({
                "type": "tool_use",
                "id": tc.id,
                "name": tc.function.name,
                "input": input,
            }));
        }
        return serde_json::json!({"role": "assistant", "content": content});
    }

    if m.role == MessageRole::User && markdown_contains_data_url_image(&m.content) {
        let mut blocks = Vec::new();
        for seg in parse_markdown_data_url_segments(&m.content) {
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
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": image.mime_type,
                            "data": image.base64_data,
                        }
                    }));
                }
            }
        }
        if !blocks.is_empty() {
            return serde_json::json!({"role": role, "content": blocks});
        }
    }

    serde_json::json!({"role": role, "content": m.content})
}

/// Parse Anthropic response JSON into ChatCompletion, extracting tool_use blocks.
fn parse_anthropic_response(resp: serde_json::Value) -> LlmResult<ChatCompletion> {
    use crate::chat::*;

    let id = resp["id"].as_str().unwrap_or("").to_string();
    let model = resp["model"].as_str().unwrap_or("").to_string();
    let stop_reason = resp["stop_reason"].as_str().unwrap_or("stop");

    let content_blocks = resp["content"]
        .as_array()
        .ok_or_else(|| LlmError::ParseError("missing content".into()))?;

    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();

    for block in content_blocks {
        match block["type"].as_str() {
            Some("text") => {
                if let Some(t) = block["text"].as_str() {
                    text_parts.push(t.to_string());
                }
            }
            Some("tool_use") => {
                if let (Some(tc_id), Some(name)) = (block["id"].as_str(), block["name"].as_str()) {
                    let args = serde_json::to_string(&block["input"]).unwrap_or_default();
                    tool_calls.push(ToolCall {
                        id: tc_id.to_string(),
                        type_: "function".to_string(),
                        function: ToolCallFunction {
                            name: name.to_string(),
                            arguments: args,
                        },
                    });
                }
            }
            _ => {}
        }
    }

    let finish = if stop_reason == "tool_use" {
        "tool_calls"
    } else {
        "stop"
    };

    Ok(ChatCompletion {
        id,
        object: "chat.completion".to_string(),
        created: chrono::Utc::now().timestamp(),
        model,
        choices: vec![ChatChoice {
            index: 0,
            message: ChatMessage {
                role: MessageRole::Assistant,
                content: text_parts.join(""),
                name: None,
                tool_calls: if tool_calls.is_empty() {
                    None
                } else {
                    Some(tool_calls)
                },
                tool_call_id: None,
            },
            finish_reason: Some(finish.to_string()),
        }],
        usage: None,
        system_fingerprint: None,
    })
}
