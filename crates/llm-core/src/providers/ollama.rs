use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::chat::{
    ChatCompletion, ChatMessage, ChatRequest, MessageRole, StreamChoice, StreamChunk, Usage,
};
use crate::embedding::{EmbeddingRequest, EmbeddingResponse};
use crate::error::{LlmError, LlmResult};
use crate::providers::{LlmProvider, ProviderType};

#[derive(Debug, Serialize)]
struct OllamaChatRequest {
    model: String,
    messages: Vec<OllamaRequestMessage>,
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<OllamaTool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    options: Option<OllamaRequestOptions>,
}

#[derive(Debug, Serialize)]
struct OllamaRequestOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    num_predict: Option<i32>,
}

#[derive(Debug, Clone, Serialize)]
struct OllamaRequestMessage {
    role: String,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OllamaToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OllamaToolCall {
    function: OllamaToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OllamaToolCallFunction {
    name: String,
    #[serde(default)]
    arguments: Value,
}

#[derive(Debug, Serialize)]
struct OllamaTool {
    #[serde(rename = "type")]
    type_: String,
    function: OllamaToolFunction,
}

#[derive(Debug, Serialize)]
struct OllamaToolFunction {
    name: String,
    description: String,
    parameters: Value,
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    #[serde(default)]
    message: Option<OllamaResponseMessage>,
    #[serde(default)]
    done: bool,
    #[serde(default)]
    done_reason: Option<String>,
    #[serde(default)]
    prompt_eval_count: Option<i32>,
    #[serde(default)]
    eval_count: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct OllamaResponseMessage {
    #[serde(default)]
    content: String,
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OllamaToolCall>>,
}

pub struct OllamaProvider {
    client: Client,
    base_url: String,
    default_model_name: String,
}

impl OllamaProvider {
    pub fn new(base_url: &str) -> LlmResult<Self> {
        Ok(Self {
            client: Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            default_model_name: "llama3.2".to_string(),
        })
    }
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

fn parse_tool_arguments(arguments: &str) -> Value {
    serde_json::from_str(arguments).unwrap_or_else(|_| Value::String(arguments.to_string()))
}

fn value_to_argument_string(value: &Value) -> String {
    if let Some(s) = value.as_str() {
        s.to_string()
    } else if value.is_null() {
        "{}".to_string()
    } else {
        value.to_string()
    }
}

fn chat_messages_to_ollama(messages: &[ChatMessage]) -> Vec<OllamaRequestMessage> {
    messages
        .iter()
        .map(|m| {
            let role = match m.role {
                MessageRole::System => "system",
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::Tool => "tool",
            };

            let tool_calls = if m.role == MessageRole::Assistant {
                m.tool_calls
                    .as_ref()
                    .map(|calls| {
                        calls
                            .iter()
                            .filter_map(|tc| {
                                let name = normalize_tool_name_alias(&tc.function.name);
                                if name.is_empty() {
                                    return None;
                                }
                                Some(OllamaToolCall {
                                    function: OllamaToolCallFunction {
                                        name,
                                        arguments: parse_tool_arguments(&tc.function.arguments),
                                    },
                                })
                            })
                            .collect::<Vec<_>>()
                    })
                    .filter(|calls| !calls.is_empty())
            } else {
                None
            };

            OllamaRequestMessage {
                role: role.to_string(),
                content: m.content.clone(),
                tool_calls,
                tool_name: if m.role == MessageRole::Tool {
                    m.name.clone().filter(|name| !name.trim().is_empty())
                } else {
                    None
                },
            }
        })
        .collect()
}

fn chat_tools_to_ollama(tools: Option<&[crate::chat::Tool]>) -> Option<Vec<OllamaTool>> {
    let tools = tools?;
    let converted = tools
        .iter()
        .map(|t| OllamaTool {
            type_: "function".to_string(),
            function: OllamaToolFunction {
                name: normalize_tool_name_alias(&t.function.name),
                description: t.function.description.clone(),
                parameters: t.function.parameters.clone(),
            },
        })
        .collect::<Vec<_>>();
    if converted.is_empty() {
        None
    } else {
        Some(converted)
    }
}

fn ollama_message_content(message: Option<&OllamaResponseMessage>) -> String {
    let Some(message) = message else {
        return String::new();
    };
    if !message.content.is_empty() {
        return message.content.clone();
    }
    message.reasoning.clone().unwrap_or_default()
}

fn ollama_tool_calls_to_chat(
    tool_calls: Option<&[OllamaToolCall]>,
    id_prefix: &str,
    seq_start: usize,
) -> Vec<crate::chat::ToolCall> {
    let Some(tool_calls) = tool_calls else {
        return Vec::new();
    };

    tool_calls
        .iter()
        .enumerate()
        .filter_map(|(idx, call)| {
            let name = normalize_tool_name_alias(&call.function.name);
            if name.is_empty() {
                return None;
            }
            Some(crate::chat::ToolCall {
                id: format!("{}_{}", id_prefix, seq_start + idx),
                type_: "function".to_string(),
                function: crate::chat::ToolCallFunction {
                    name,
                    arguments: value_to_argument_string(&call.function.arguments),
                },
            })
        })
        .collect()
}

fn finish_reason_from_ollama(
    done: bool,
    done_reason: Option<&str>,
    has_tool_calls: bool,
) -> Option<String> {
    if !done {
        return None;
    }
    if has_tool_calls {
        return Some("tool_calls".to_string());
    }
    if let Some(reason) = done_reason.map(str::trim).filter(|r| !r.is_empty()) {
        return Some(reason.to_string());
    }
    Some("stop".to_string())
}

fn usage_from_ollama(resp: &OllamaChatResponse) -> Option<Usage> {
    let prompt = resp.prompt_eval_count.unwrap_or(0);
    let completion = resp.eval_count.unwrap_or(0);
    if resp.prompt_eval_count.is_none() && resp.eval_count.is_none() {
        None
    } else {
        Some(Usage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
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
        let req = OllamaChatRequest {
            model: request.model.clone(),
            messages: chat_messages_to_ollama(&request.messages),
            stream: false,
            tools: chat_tools_to_ollama(request.tools.as_deref()),
            options: Some(OllamaRequestOptions {
                temperature: request.temperature,
                num_predict: request.max_tokens,
            }),
        };

        let response = self
            .client
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

        let resp: OllamaChatResponse = response
            .json()
            .await
            .map_err(|e| LlmError::ParseError(e.to_string()))?;

        let tool_calls = ollama_tool_calls_to_chat(
            resp.message.as_ref().and_then(|m| m.tool_calls.as_deref()),
            "ollama_call",
            0,
        );
        let finish_reason = finish_reason_from_ollama(
            resp.done,
            resp.done_reason.as_deref(),
            !tool_calls.is_empty(),
        );

        Ok(ChatCompletion {
            id: format!("ollama-{}", uuid::Uuid::new_v4()),
            object: "chat.completion".to_string(),
            created: chrono::Utc::now().timestamp(),
            model: request.model,
            choices: vec![crate::chat::ChatChoice {
                index: 0,
                message: ChatMessage {
                    role: MessageRole::Assistant,
                    content: ollama_message_content(resp.message.as_ref()),
                    name: None,
                    tool_calls: if tool_calls.is_empty() {
                        None
                    } else {
                        Some(tool_calls)
                    },
                    tool_call_id: None,
                },
                finish_reason,
            }],
            usage: usage_from_ollama(&resp),
            system_fingerprint: None,
        })
    }

    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> LlmResult<tokio::sync::mpsc::Receiver<LlmResult<StreamChunk>>> {
        let url = format!("{}/api/chat", self.base_url);
        let req = OllamaChatRequest {
            model: request.model.clone(),
            messages: chat_messages_to_ollama(&request.messages),
            stream: true,
            tools: chat_tools_to_ollama(request.tools.as_deref()),
            options: Some(OllamaRequestOptions {
                temperature: request.temperature,
                num_predict: request.max_tokens,
            }),
        };

        let response = self
            .client
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
            let mut tool_call_seq = 0usize;
            let mut seen_tool_calls = false;

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
                    if line.is_empty() {
                        continue;
                    }

                    let parsed: OllamaChatResponse = match serde_json::from_str(&line) {
                        Ok(v) => v,
                        Err(err) => {
                            tracing::debug!("Ollama stream parse skip malformed line: {}", err);
                            continue;
                        }
                    };

                    let chunk_tool_calls = ollama_tool_calls_to_chat(
                        parsed
                            .message
                            .as_ref()
                            .and_then(|m| m.tool_calls.as_deref()),
                        "ollama_call_stream",
                        tool_call_seq,
                    );
                    if !chunk_tool_calls.is_empty() {
                        seen_tool_calls = true;
                        tool_call_seq += chunk_tool_calls.len();
                    }
                    let content_delta = ollama_message_content(parsed.message.as_ref());
                    let finish_reason = finish_reason_from_ollama(
                        parsed.done,
                        parsed.done_reason.as_deref(),
                        seen_tool_calls,
                    );

                    if content_delta.is_empty() && chunk_tool_calls.is_empty() && !parsed.done {
                        continue;
                    }

                    let stream_chunk = StreamChunk {
                        id: "ollama-stream".to_string(),
                        object: "chat.completion.chunk".to_string(),
                        created: chrono::Utc::now().timestamp(),
                        model: model.clone(),
                        choices: vec![StreamChoice {
                            index: 0,
                            delta: Some(ChatMessage {
                                role: MessageRole::Assistant,
                                content: content_delta,
                                name: None,
                                tool_calls: if chunk_tool_calls.is_empty() {
                                    None
                                } else {
                                    Some(chunk_tool_calls)
                                },
                                tool_call_id: None,
                            }),
                            finish_reason,
                        }],
                    };

                    if tx.send(Ok(stream_chunk)).await.is_err() {
                        return;
                    }

                    if parsed.done {
                        return;
                    }
                }
            }

            let tail = buffer.trim();
            if tail.is_empty() {
                return;
            }
            let parsed: OllamaChatResponse = match serde_json::from_str(tail) {
                Ok(v) => v,
                Err(err) => {
                    tracing::debug!("Ollama stream parse skip malformed tail: {}", err);
                    return;
                }
            };

            let chunk_tool_calls = ollama_tool_calls_to_chat(
                parsed
                    .message
                    .as_ref()
                    .and_then(|m| m.tool_calls.as_deref()),
                "ollama_call_stream",
                tool_call_seq,
            );
            if !chunk_tool_calls.is_empty() {
                seen_tool_calls = true;
            }
            let stream_chunk = StreamChunk {
                id: "ollama-stream".to_string(),
                object: "chat.completion.chunk".to_string(),
                created: chrono::Utc::now().timestamp(),
                model,
                choices: vec![StreamChoice {
                    index: 0,
                    delta: Some(ChatMessage {
                        role: MessageRole::Assistant,
                        content: ollama_message_content(parsed.message.as_ref()),
                        name: None,
                        tool_calls: if chunk_tool_calls.is_empty() {
                            None
                        } else {
                            Some(chunk_tool_calls)
                        },
                        tool_call_id: None,
                    }),
                    finish_reason: finish_reason_from_ollama(
                        parsed.done,
                        parsed.done_reason.as_deref(),
                        seen_tool_calls,
                    ),
                }],
            };
            let _ = tx.send(Ok(stream_chunk)).await;
        });

        Ok(rx)
    }

    async fn embeddings(&self, _request: EmbeddingRequest) -> LlmResult<EmbeddingResponse> {
        Err(LlmError::UnsupportedModel(
            "Use /api/embeddings endpoint directly".to_string(),
        ))
    }

    fn supported_models(&self) -> Vec<String> {
        vec![
            "llama3.2".to_string(),
            "llama3.1".to_string(),
            "mistral".to_string(),
        ]
    }

    fn default_model(&self) -> &str {
        &self.default_model_name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_assistant_tool_calls_and_tool_results_to_ollama_messages() {
        let messages = vec![
            ChatMessage {
                role: MessageRole::Assistant,
                content: String::new(),
                name: None,
                tool_calls: Some(vec![crate::chat::ToolCall {
                    id: "call_1".to_string(),
                    type_: "function".to_string(),
                    function: crate::chat::ToolCallFunction {
                        name: "exec".to_string(),
                        arguments: r#"{"command":"pwd"}"#.to_string(),
                    },
                }]),
                tool_call_id: None,
            },
            ChatMessage {
                role: MessageRole::Tool,
                content: "ok".to_string(),
                name: Some("bash".to_string()),
                tool_calls: None,
                tool_call_id: Some("call_1".to_string()),
            },
        ];

        let converted = chat_messages_to_ollama(&messages);
        assert_eq!(converted.len(), 2);
        assert_eq!(
            converted[0].tool_calls.as_ref().expect("tool calls").len(),
            1
        );
        assert_eq!(
            converted[0].tool_calls.as_ref().expect("tool calls")[0]
                .function
                .name,
            "bash"
        );
        assert_eq!(converted[1].role, "tool");
        assert_eq!(converted[1].tool_name.as_deref(), Some("bash"));
    }

    #[test]
    fn parses_ollama_tool_calls_and_sets_finish_reason() {
        let resp = OllamaChatResponse {
            message: Some(OllamaResponseMessage {
                content: String::new(),
                reasoning: Some("thinking result".to_string()),
                tool_calls: Some(vec![OllamaToolCall {
                    function: OllamaToolCallFunction {
                        name: "google".to_string(),
                        arguments: serde_json::json!({"query":"金价"}),
                    },
                }]),
            }),
            done: true,
            done_reason: Some("stop".to_string()),
            prompt_eval_count: Some(10),
            eval_count: Some(2),
        };

        let calls = ollama_tool_calls_to_chat(
            resp.message.as_ref().and_then(|m| m.tool_calls.as_deref()),
            "ollama_call",
            0,
        );
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].function.name, "web_search");
        assert_eq!(
            finish_reason_from_ollama(resp.done, resp.done_reason.as_deref(), !calls.is_empty())
                .as_deref(),
            Some("tool_calls")
        );
        assert_eq!(usage_from_ollama(&resp).expect("usage").total_tokens, 12);
        assert_eq!(
            ollama_message_content(resp.message.as_ref()),
            "thinking result".to_string()
        );
    }

    #[test]
    fn converts_chat_tools_for_ollama_request() {
        let tools = vec![crate::chat::Tool {
            type_: "function".to_string(),
            function: crate::chat::ToolFunction {
                name: "read".to_string(),
                description: "read file".to_string(),
                parameters: serde_json::json!({
                    "type":"object",
                    "properties": { "path": { "type":"string" } }
                }),
            },
        }];

        let converted = chat_tools_to_ollama(Some(&tools)).expect("converted");
        assert_eq!(converted.len(), 1);
        assert_eq!(converted[0].type_, "function");
        assert_eq!(converted[0].function.name, "read_file");
    }
}
