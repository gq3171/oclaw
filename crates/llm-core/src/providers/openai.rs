use async_trait::async_trait;
use regex::Regex;
use reqwest::Client;
use serde::Serialize;
use std::sync::OnceLock;

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

    fn auth_header_value(&self) -> Option<String> {
        let key = self.api_key.trim();
        if key.is_empty() || key.eq_ignore_ascii_case("empty") {
            None
        } else {
            Some(format!("Bearer {}", key))
        }
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

        let mut req_builder = self
            .client
            .post(&url)
            .header("Content-Type", "application/json");
        if let Some(auth) = self.auth_header_value() {
            req_builder = req_builder.header("Authorization", auth);
        }
        let response = req_builder
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
                let mut msg = serde_json::json!({
                    "role": role,
                    "content": openai_message_content(role, &m.content),
                });
                if let Some(ref tc) = m.tool_calls {
                    let calls: Vec<serde_json::Value> = tc
                        .iter()
                        .map(|c| {
                            serde_json::json!({
                                "id": c.id,
                                "type": c.type_,
                                "function": { "name": c.function.name, "arguments": c.function.arguments }
                            })
                        })
                        .collect();
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

        let mut req_builder = self
            .client
            .post(&url)
            .header("Content-Type", "application/json");
        if let Some(auth) = self.auth_header_value() {
            req_builder = req_builder.header("Authorization", auth);
        }
        let response = req_builder
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

        let mut req_builder = self
            .client
            .post(&url)
            .header("Content-Type", "application/json");
        if let Some(auth) = self.auth_header_value() {
            req_builder = req_builder.header("Authorization", auth);
        }
        let response = req_builder
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

    async fn list_models(&self) -> LlmResult<Vec<String>> {
        let url = format!("{}/models", self.base_url.trim_end_matches('/'));
        let mut req_builder = self.client.get(&url);
        if let Some(auth) = self.auth_header_value() {
            req_builder = req_builder.header("Authorization", auth);
        }

        let response = match req_builder.send().await {
            Ok(r) => r,
            Err(err) => {
                tracing::debug!("openai list_models request failed: {}", err);
                return Ok(self.supported_models());
            }
        };

        if !response.status().is_success() {
            tracing::debug!(
                "openai list_models non-success status: {}",
                response.status()
            );
            return Ok(self.supported_models());
        }

        let body: serde_json::Value = match response.json().await {
            Ok(v) => v,
            Err(err) => {
                tracing::debug!("openai list_models parse failed: {}", err);
                return Ok(self.supported_models());
            }
        };

        let mut models: Vec<String> = Vec::new();
        if let Some(data) = body["data"].as_array() {
            for item in data {
                if let Some(id) = item["id"].as_str().map(str::trim).filter(|s| !s.is_empty()) {
                    if !models.iter().any(|m| m == id) {
                        models.push(id.to_string());
                    }
                }
            }
        }
        if models.is_empty()
            && let Some(arr) = body["models"].as_array()
        {
            for item in arr {
                let id = item
                    .as_str()
                    .or_else(|| item["id"].as_str())
                    .or_else(|| item["name"].as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                if let Some(id) = id
                    && !models.iter().any(|m| m == id)
                {
                    models.push(id.to_string());
                }
            }
        }

        if models.is_empty() {
            Ok(self.supported_models())
        } else {
            Ok(models)
        }
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

fn extract_message_content(msg: &serde_json::Value) -> String {
    if let Some(s) = msg["content"].as_str() {
        return s.to_string();
    }
    if let Some(arr) = msg["content"].as_array() {
        return arr
            .iter()
            .filter_map(|b| b["text"].as_str().or_else(|| b.as_str()))
            .collect::<Vec<_>>()
            .join("");
    }
    String::new()
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

fn argument_string_from_value(v: &serde_json::Value) -> String {
    if let Some(s) = v.as_str() {
        s.to_string()
    } else if v.is_null() {
        "{}".to_string()
    } else {
        v.to_string()
    }
}

fn parse_parameter_value(raw: &str) -> serde_json::Value {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return serde_json::Value::String(String::new());
    }
    serde_json::from_str(trimmed).unwrap_or_else(|_| serde_json::Value::String(trimmed.to_string()))
}

fn parse_structured_tool_calls(
    msg: &serde_json::Value,
    choice_index: i32,
) -> Vec<crate::chat::ToolCall> {
    let mut out = Vec::new();
    let calls = msg["tool_calls"]
        .as_array()
        .or_else(|| msg["toolCalls"].as_array());
    let Some(calls) = calls else {
        return out;
    };

    for (tc_index, tc) in calls.iter().enumerate() {
        let function = tc.get("function").unwrap_or(tc);
        let Some(name_raw) = function.get("name").and_then(|v| v.as_str()) else {
            continue;
        };
        let name = normalize_tool_name_alias(name_raw);
        if name.is_empty() {
            continue;
        }
        let arguments = function
            .get("arguments")
            .or_else(|| tc.get("arguments"))
            .map(argument_string_from_value)
            .unwrap_or_else(|| "{}".to_string());
        let id = tc
            .get("id")
            .and_then(|v| v.as_str())
            .filter(|v| !v.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| format!("call_{}_{}", choice_index, tc_index));
        let type_ = tc
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("function")
            .to_string();

        out.push(crate::chat::ToolCall {
            id,
            type_,
            function: crate::chat::ToolCallFunction { name, arguments },
        });
    }
    out
}

fn parse_legacy_function_call(
    msg: &serde_json::Value,
    choice_index: i32,
) -> Vec<crate::chat::ToolCall> {
    let mut out = Vec::new();
    let fc = msg.get("function_call").or_else(|| msg.get("functionCall"));
    let Some(fc) = fc else {
        return out;
    };

    let Some(name_raw) = fc.get("name").and_then(|v| v.as_str()) else {
        return out;
    };
    let name = normalize_tool_name_alias(name_raw);
    if name.is_empty() {
        return out;
    }

    let arguments = fc
        .get("arguments")
        .or_else(|| fc.get("args"))
        .map(argument_string_from_value)
        .unwrap_or_else(|| "{}".to_string());

    let id = fc
        .get("id")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("call_legacy_{}", choice_index));

    out.push(crate::chat::ToolCall {
        id,
        type_: "function".to_string(),
        function: crate::chat::ToolCallFunction { name, arguments },
    });
    out
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

        // Parse content (string or array text blocks), then strip leaked tool-markup.
        let raw_content = extract_message_content(msg);
        let content = sanitize_assistant_content(&raw_content);

        // Parse tool calls with fallback chain:
        // 1) OpenAI tool_calls / toolCalls
        // 2) legacy function_call / functionCall
        // 3) MiniMax XML leaked in content text
        let mut parsed_tool_calls =
            parse_structured_tool_calls(msg, c["index"].as_i64().unwrap_or(0) as i32);
        if parsed_tool_calls.is_empty() {
            parsed_tool_calls =
                parse_legacy_function_call(msg, c["index"].as_i64().unwrap_or(0) as i32);
        }
        if parsed_tool_calls.is_empty() {
            parsed_tool_calls =
                parse_minimax_xml_tool_calls(&raw_content, c["index"].as_i64().unwrap_or(0) as i32);
        }
        let tool_calls = if parsed_tool_calls.is_empty() {
            None
        } else {
            Some(parsed_tool_calls)
        };

        choices.push(ChatChoice {
            index: c["index"].as_i64().unwrap_or(0) as i32,
            message: ChatMessage {
                role,
                content,
                name: msg["name"].as_str().map(|s| s.to_string()),
                tool_calls,
                tool_call_id: msg["tool_call_id"]
                    .as_str()
                    .or_else(|| msg["toolCallId"].as_str())
                    .map(|s| s.to_string()),
            },
            finish_reason: c["finish_reason"]
                .as_str()
                .or_else(|| c["finishReason"].as_str())
                .map(|s| s.to_string()),
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

#[cfg(test)]
mod tests {
    use super::parse_chat_completion;

    #[test]
    fn parses_openai_tool_calls_with_object_arguments_and_alias() {
        let body = serde_json::json!({
            "id": "resp_1",
            "object": "chat.completion",
            "created": 1,
            "model": "test",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "",
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "Bash",
                            "arguments": {"command": "pwd"}
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }]
        });

        let parsed = parse_chat_completion(body).expect("should parse");
        let calls = parsed.choices[0]
            .message
            .tool_calls
            .as_ref()
            .expect("tool calls expected");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "bash");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&calls[0].function.arguments)
                .expect("valid arguments json"),
            serde_json::json!({"command":"pwd"})
        );
    }

    #[test]
    fn parses_legacy_function_call_fallback() {
        let body = serde_json::json!({
            "id": "resp_2",
            "object": "chat.completion",
            "created": 1,
            "model": "test",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "",
                    "function_call": {
                        "name": "exec",
                        "arguments": "{\"command\":\"ls\"}"
                    }
                },
                "finish_reason": "tool_calls"
            }]
        });

        let parsed = parse_chat_completion(body).expect("should parse");
        let calls = parsed.choices[0]
            .message
            .tool_calls
            .as_ref()
            .expect("tool calls expected");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "bash");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&calls[0].function.arguments)
                .expect("valid arguments json"),
            serde_json::json!({"command":"ls"})
        );
    }

    #[test]
    fn parses_minimax_xml_fallback_and_strips_xml_from_content() {
        let body = serde_json::json!({
            "id": "resp_3",
            "object": "chat.completion",
            "created": 1,
            "model": "test",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "让我帮你查一下最近的黄金价格！\n<minimax:tool_call>\n<invoke name=\"google\">\n<parameter name=\"query\">黄金价格 今日</parameter>\n<parameter name=\"max_results\">5</parameter>\n</invoke>\n</minimax:tool_call>"
                },
                "finish_reason": "tool_calls"
            }]
        });

        let parsed = parse_chat_completion(body).expect("should parse");
        let message = &parsed.choices[0].message;
        let calls = message.tool_calls.as_ref().expect("tool calls expected");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "web_search");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&calls[0].function.arguments)
                .expect("valid arguments json"),
            serde_json::json!({"query":"黄金价格 今日","max_results":5})
        );
        assert_eq!(message.content, "让我帮你查一下最近的黄金价格！");
    }

    #[test]
    fn keeps_invoke_snippet_without_minimax_markers() {
        let body = serde_json::json!({
            "id": "resp_4",
            "object": "chat.completion",
            "created": 1,
            "model": "test",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "Example:\n<invoke name=\"Bash\">\n<parameter name=\"command\">ls</parameter>\n</invoke>"
                },
                "finish_reason": "stop"
            }]
        });

        let parsed = parse_chat_completion(body).expect("should parse");
        let message = &parsed.choices[0].message;
        assert!(message.tool_calls.is_none());
        assert_eq!(
            message.content,
            "Example:\n<invoke name=\"Bash\">\n<parameter name=\"command\">ls</parameter>\n</invoke>"
        );
    }

    #[test]
    fn strips_downgraded_tool_call_text_from_content() {
        let body = serde_json::json!({
            "id": "resp_5",
            "object": "chat.completion",
            "created": 1,
            "model": "test",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": "[Tool Call: exec (ID: toolu_1)]\nArguments: {\"command\":\"pwd\"}\nDone"
                },
                "finish_reason": "stop"
            }]
        });

        let parsed = parse_chat_completion(body).expect("should parse");
        let message = &parsed.choices[0].message;
        assert!(message.tool_calls.is_none());
        assert_eq!(message.content, "Done");
    }
}
