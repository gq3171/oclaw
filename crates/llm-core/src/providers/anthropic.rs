use async_trait::async_trait;
use regex::Regex;
use reqwest::Client;
use std::sync::OnceLock;

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

fn minimax_marker_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)minimax:tool_call").expect("valid minimax marker regex"))
}

fn xml_invoke_block_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?is)<invoke\b([^>]*)>(.*?)</invoke>").expect("valid invoke block regex")
    })
}

fn xml_parameter_block_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?is)<parameter\b([^>]*)>(.*?)</parameter>")
            .expect("valid parameter block regex")
    })
}

fn xml_attr_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?is)\b([A-Za-z_:][A-Za-z0-9_.:-]*)\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+))"#,
        )
        .expect("valid xml attr regex")
    })
}

fn minimax_tool_tag_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?is)</?minimax:tool_call\b[^>]*>").expect("valid minimax tag regex")
    })
}

fn downgraded_marker_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)\[Tool (?:Call|Result)|\[Historical context:")
            .expect("valid downgraded marker regex")
    })
}

fn downgraded_tool_call_with_args_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r#"(?is)\[Tool Call:[^\]]*\]\s*(?:\r?\n)?\s*Arguments:\s*(?:\{[\s\S]*?\}|\[[\s\S]*?\]|"(?:\\.|[^"])*"|[^\r\n]*)"#,
        )
        .expect("valid downgraded tool call+args regex")
    })
}

fn downgraded_tool_call_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?is)\[Tool Call:[^\]]*\]").expect("valid downgraded tool call regex")
    })
}

fn downgraded_tool_result_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?is)\[Tool Result for ID[^\]]*\]")
            .expect("valid downgraded tool result regex")
    })
}

fn downgraded_historical_context_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?is)\[Historical context:[^\]]*\]\s*")
            .expect("valid downgraded historical context regex")
    })
}

fn parse_xml_attr_value(attrs: &str, key: &str) -> Option<String> {
    xml_attr_re().captures_iter(attrs).find_map(|cap| {
        let name = cap.get(1)?.as_str();
        if !name.eq_ignore_ascii_case(key) {
            return None;
        }
        let value = cap
            .get(2)
            .or_else(|| cap.get(3))
            .or_else(|| cap.get(4))
            .map(|m| m.as_str())
            .unwrap_or("");
        Some(value.to_string())
    })
}

fn normalize_tool_name_alias(name: &str) -> String {
    let n = name.trim();
    if n.eq_ignore_ascii_case("google")
        || n.eq_ignore_ascii_case("google_search")
        || n.eq_ignore_ascii_case("search")
    {
        return "web_search".to_string();
    }
    if n.eq_ignore_ascii_case("exec")
        || n.eq_ignore_ascii_case("shell")
        || n.eq_ignore_ascii_case("terminal")
        || n.eq_ignore_ascii_case("bash")
    {
        return "bash".to_string();
    }
    if n.eq_ignore_ascii_case("apply-patch") {
        return "apply_patch".to_string();
    }
    if n.eq_ignore_ascii_case("browser") {
        return "browse".to_string();
    }
    if n.eq_ignore_ascii_case("read") {
        return "read_file".to_string();
    }
    if n.eq_ignore_ascii_case("write") {
        return "write_file".to_string();
    }
    if n.eq_ignore_ascii_case("list")
        || n.eq_ignore_ascii_case("ls")
        || n.eq_ignore_ascii_case("list_files")
    {
        return "list_dir".to_string();
    }
    n.to_string()
}

fn parse_parameter_value(raw: &str) -> serde_json::Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return serde_json::Value::String(String::new());
    }
    serde_json::from_str(trimmed).unwrap_or_else(|_| serde_json::Value::String(trimmed.to_string()))
}

fn parse_minimax_xml_tool_calls(content: &str, choice_index: i32) -> Vec<crate::chat::ToolCall> {
    let mut out = Vec::new();
    if !minimax_marker_re().is_match(content) {
        return out;
    }

    for (invoke_index, cap) in xml_invoke_block_re().captures_iter(content).enumerate() {
        let attrs = cap.get(1).map(|m| m.as_str()).unwrap_or("");
        let body = cap.get(2).map(|m| m.as_str()).unwrap_or("");
        let Some(name_raw) = parse_xml_attr_value(attrs, "name") else {
            continue;
        };
        let name = normalize_tool_name_alias(&name_raw);
        if name.is_empty() {
            continue;
        }

        let mut args = serde_json::Map::new();
        for pc in xml_parameter_block_re().captures_iter(body) {
            let p_attrs = pc.get(1).map(|m| m.as_str()).unwrap_or("");
            let p_value_raw = pc.get(2).map(|m| m.as_str()).unwrap_or("");
            let Some(param_name) = parse_xml_attr_value(p_attrs, "name") else {
                continue;
            };
            if param_name.trim().is_empty() {
                continue;
            }
            args.insert(param_name, parse_parameter_value(p_value_raw));
        }

        out.push(crate::chat::ToolCall {
            id: format!("call_minimax_{}_{}", choice_index, invoke_index),
            type_: "function".to_string(),
            function: crate::chat::ToolCallFunction {
                name,
                arguments: serde_json::Value::Object(args).to_string(),
            },
        });
    }
    out
}

fn strip_minimax_tool_call_xml(text: &str) -> String {
    if text.is_empty() || !minimax_marker_re().is_match(text) {
        return text.to_string();
    }
    let cleaned = xml_invoke_block_re().replace_all(text, "");
    minimax_tool_tag_re()
        .replace_all(cleaned.as_ref(), "")
        .to_string()
}

fn strip_downgraded_tool_call_text(text: &str) -> String {
    if text.is_empty() || !downgraded_marker_re().is_match(text) {
        return text.to_string();
    }
    let cleaned = downgraded_tool_call_with_args_re().replace_all(text, "");
    let cleaned = downgraded_tool_call_re().replace_all(cleaned.as_ref(), "");
    let cleaned = downgraded_tool_result_re().replace_all(cleaned.as_ref(), "");
    downgraded_historical_context_re()
        .replace_all(cleaned.as_ref(), "")
        .to_string()
}

fn sanitize_assistant_content(text: &str) -> String {
    let cleaned = strip_minimax_tool_call_xml(text);
    let cleaned = strip_downgraded_tool_call_text(&cleaned);
    cleaned.trim().to_string()
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
                            name: normalize_tool_name_alias(name),
                            arguments: args,
                        },
                    });
                }
            }
            _ => {}
        }
    }

    let raw_text = text_parts.join("");
    if tool_calls.is_empty() {
        tool_calls = parse_minimax_xml_tool_calls(&raw_text, 0);
    }
    let finish = if stop_reason == "tool_use" || !tool_calls.is_empty() {
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
                content: sanitize_assistant_content(&raw_text),
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

#[cfg(test)]
mod tests {
    use super::parse_anthropic_response;

    #[test]
    fn parses_minimax_xml_fallback_in_text_block() {
        let resp = serde_json::json!({
            "id": "msg_1",
            "model": "test",
            "stop_reason": "end_turn",
            "content": [{
                "type": "text",
                "text": "先查一下。\n<minimax:tool_call><invoke name=\"google\"><parameter name=\"query\">黄金价格 今日</parameter></invoke></minimax:tool_call>"
            }]
        });

        let parsed = parse_anthropic_response(resp).expect("should parse");
        let message = &parsed.choices[0].message;
        let calls = message.tool_calls.as_ref().expect("tool calls expected");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "web_search");
        assert_eq!(message.content, "先查一下。");
    }

    #[test]
    fn normalizes_tool_use_alias_name() {
        let resp = serde_json::json!({
            "id": "msg_2",
            "model": "test",
            "stop_reason": "tool_use",
            "content": [{
                "type": "tool_use",
                "id": "toolu_1",
                "name": "exec",
                "input": {"command":"pwd"}
            }]
        });

        let parsed = parse_anthropic_response(resp).expect("should parse");
        let calls = parsed.choices[0]
            .message
            .tool_calls
            .as_ref()
            .expect("tool calls expected");
        assert_eq!(calls[0].function.name, "bash");
    }

    #[test]
    fn strips_downgraded_tool_call_text_from_anthropic_content() {
        let resp = serde_json::json!({
            "id": "msg_3",
            "model": "test",
            "stop_reason": "end_turn",
            "content": [{
                "type": "text",
                "text": "[Tool Call: exec (ID: toolu_1)]\nArguments: {\"command\":\"pwd\"}\nDone"
            }]
        });

        let parsed = parse_anthropic_response(resp).expect("should parse");
        assert_eq!(parsed.choices[0].message.content, "Done");
    }
}
