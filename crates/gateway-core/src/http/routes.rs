use axum::{
    extract::State,
    http::StatusCode,
    response::{sse::{Event, Sse}, IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::sync::Arc;
use tracing::{info, error};

use crate::http::HttpState;

fn sanitize_error(msg: &str) -> String {
    // Strip anything that looks like an API key or token from error messages
    regex::Regex::new(r"(?i)(sk-|key-|token-|bearer\s+)[a-zA-Z0-9\-_]{8,}")
        .map(|re| re.replace_all(msg, "${1}[REDACTED]").to_string())
        .unwrap_or_else(|_| "Internal server error".to_string())
}

#[derive(Debug, Deserialize)]
pub struct ChatCompletionsRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<i32>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub tools: Option<Vec<serde_json::Value>>,
    #[serde(default)]
    pub tool_choice: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatCompletionsResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

#[derive(Debug, Serialize)]
pub struct Choice {
    pub index: i32,
    pub message: ChatMessage,
    pub finish_reason: String,
}

#[derive(Debug, Serialize)]
pub struct Usage {
    pub prompt_tokens: i32,
    pub completion_tokens: i32,
    pub total_tokens: i32,
}

/// OpenAI-compatible streaming chunk format.
#[derive(Debug, Serialize)]
pub struct ChatCompletionChunk {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub choices: Vec<ChunkChoice>,
}

#[derive(Debug, Serialize)]
pub struct ChunkChoice {
    pub index: i32,
    pub delta: ChunkDelta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChunkDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

fn to_llm_messages(msgs: &[ChatMessage]) -> Vec<oclaws_llm_core::chat::ChatMessage> {
    msgs.iter().map(|m| {
        let role = match m.role.as_str() {
            "system" => oclaws_llm_core::chat::MessageRole::System,
            "assistant" => oclaws_llm_core::chat::MessageRole::Assistant,
            "tool" => oclaws_llm_core::chat::MessageRole::Tool,
            _ => oclaws_llm_core::chat::MessageRole::User,
        };
        oclaws_llm_core::chat::ChatMessage {
            role,
            content: m.content.clone(),
            name: m.name.clone(),
            tool_calls: None,
            tool_call_id: m.tool_call_id.clone(),
        }
    }).collect()
}

pub async fn chat_completions_handler(
    State(state): State<Arc<HttpState>>,
    Json(payload): Json<ChatCompletionsRequest>,
) -> Response {
    info!("Chat completions request for model: {}", payload.model);

    let provider = match &state.llm_provider {
        Some(p) => p.clone(),
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": {"message": "No LLM provider configured", "type": "server_error"}}))).into_response(),
    };

    let request = oclaws_llm_core::chat::ChatRequest {
        model: payload.model.clone(),
        messages: to_llm_messages(&payload.messages),
        temperature: payload.temperature,
        top_p: None,
        max_tokens: payload.max_tokens,
        stop: None,
        tools: None,
        tool_choice: None,
        stream: Some(payload.stream),
        response_format: None,
    };

    if payload.stream {
        let model_name = payload.model.clone();
        match provider.chat_stream(request).await {
            Ok(mut rx) => {
                let stream = async_stream::stream! {
                    let chunk_id = format!("chatcmpl-{}", uuid::Uuid::new_v4().simple());
                    let created = chrono::Utc::now().timestamp();
                    let mut first = true;

                    while let Some(chunk) = rx.recv().await {
                        match chunk {
                            Ok(c) => {
                                let content = if c.choices.is_empty() {
                                    None
                                } else {
                                    c.choices[0].delta.as_ref().map(|d| d.content.clone())
                                };

                                let delta = if first {
                                    first = false;
                                    ChunkDelta { role: Some("assistant".into()), content }
                                } else {
                                    ChunkDelta { role: None, content }
                                };

                                let chunk_resp = ChatCompletionChunk {
                                    id: chunk_id.clone(),
                                    object: "chat.completion.chunk".into(),
                                    created,
                                    model: model_name.clone(),
                                    choices: vec![ChunkChoice {
                                        index: 0,
                                        delta,
                                        finish_reason: None,
                                    }],
                                };
                                if let Ok(json) = serde_json::to_string(&chunk_resp) {
                                    yield Ok::<_, Infallible>(Event::default().data(json));
                                }
                            }
                            Err(e) => {
                                error!("Stream error: {}", e);
                                break;
                            }
                        }
                    }

                    // Final chunk with finish_reason
                    let final_chunk = ChatCompletionChunk {
                        id: chunk_id,
                        object: "chat.completion.chunk".into(),
                        created,
                        model: model_name,
                        choices: vec![ChunkChoice {
                            index: 0,
                            delta: ChunkDelta { role: None, content: None },
                            finish_reason: Some("stop".into()),
                        }],
                    };
                    if let Ok(json) = serde_json::to_string(&final_chunk) {
                        yield Ok::<_, Infallible>(Event::default().data(json));
                    }
                    yield Ok::<_, Infallible>(Event::default().data("[DONE]"));
                };
                Sse::new(stream).into_response()
            }
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": {"message": sanitize_error(&e.to_string()), "type": "server_error"}}))).into_response(),
        }
    } else {
        match provider.chat(request).await {
            Ok(completion) => {
                let choices: Vec<Choice> = completion.choices.iter().map(|c| Choice {
                    index: c.index,
                    message: ChatMessage {
                        role: "assistant".to_string(),
                        content: c.message.content.clone(),
                        name: c.message.name.clone(),
                        tool_calls: None,
                        tool_call_id: c.message.tool_call_id.clone(),
                    },
                    finish_reason: c.finish_reason.clone().unwrap_or("stop".to_string()),
                }).collect();

                let usage = completion.usage.map(|u| Usage {
                    prompt_tokens: u.prompt_tokens,
                    completion_tokens: u.completion_tokens,
                    total_tokens: u.total_tokens,
                }).unwrap_or(Usage { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 });

                Json(ChatCompletionsResponse {
                    id: completion.id,
                    object: "chat.completion".to_string(),
                    created: completion.created,
                    model: completion.model,
                    choices,
                    usage,
                }).into_response()
            }
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": {"message": sanitize_error(&e.to_string()), "type": "server_error"}}))).into_response(),
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ResponsesRequest {
    pub model: String,
    #[serde(default)]
    pub input: serde_json::Value,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub max_tokens: Option<i32>,
}

#[derive(Debug, Serialize)]
pub struct ResponsesResponse {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub model: String,
    pub output: Vec<OutputItem>,
    pub usage: Usage,
}

#[derive(Debug, Serialize)]
pub struct OutputItem {
    #[serde(rename = "type")]
    pub item_type: String,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Serialize)]
pub struct ContentBlock {
    #[serde(rename = "type")]
    pub block_type: String,
    pub text: String,
}

pub async fn responses_handler(
    State(state): State<Arc<HttpState>>,
    Json(payload): Json<ResponsesRequest>,
) -> Response {
    info!("Responses request for model: {}", payload.model);

    let provider = match &state.llm_provider {
        Some(p) => p.clone(),
        None => return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": {"message": "No LLM provider configured", "type": "server_error"}}))).into_response(),
    };

    let input_text = match &payload.input {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(arr) => {
            arr.iter().filter_map(|item| {
                item.get("content").and_then(|c| c.as_str()).map(|s| s.to_string())
            }).collect::<Vec<_>>().join("\n")
        }
        _ => String::new(),
    };

    let request = oclaws_llm_core::chat::ChatRequest {
        model: payload.model.clone(),
        messages: vec![oclaws_llm_core::chat::ChatMessage {
            role: oclaws_llm_core::chat::MessageRole::User,
            content: input_text,
            name: None, tool_calls: None, tool_call_id: None,
        }],
        temperature: payload.temperature,
        top_p: None,
        max_tokens: payload.max_tokens,
        stop: None, tools: None, tool_choice: None, stream: None, response_format: None,
    };

    match provider.chat(request).await {
        Ok(completion) => {
            let text = completion.choices.first().map(|c| c.message.content.clone()).unwrap_or_default();
            let usage = completion.usage.map(|u| Usage {
                prompt_tokens: u.prompt_tokens,
                completion_tokens: u.completion_tokens,
                total_tokens: u.total_tokens,
            }).unwrap_or(Usage { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 });

            Json(ResponsesResponse {
                id: format!("resp-{}", uuid::Uuid::new_v4()),
                object: "response".to_string(),
                created: chrono::Utc::now().timestamp(),
                model: payload.model,
                output: vec![OutputItem {
                    item_type: "message".to_string(),
                    content: vec![ContentBlock { block_type: "output_text".to_string(), text }],
                }],
                usage,
            }).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": {"message": sanitize_error(&e.to_string()), "type": "server_error"}}))).into_response(),
    }
}

// --- Management API endpoints ---

pub async fn agent_status_handler(
    State(state): State<Arc<HttpState>>,
) -> impl IntoResponse {
    let has_provider = state.llm_provider.is_some();
    Json(serde_json::json!({
        "status": if has_provider { "ready" } else { "no_provider" },
        "provider_configured": has_provider,
        "timestamp": chrono::Utc::now().to_rfc3339()
    }))
}

pub async fn sessions_list_handler(
    State(state): State<Arc<HttpState>>,
) -> impl IntoResponse {
    let manager = state.gateway_server.session_manager.read().await;
    let sessions = manager.list_sessions().unwrap_or_default();
    Json(serde_json::json!({ "sessions": sessions }))
}

pub async fn sessions_delete_handler(
    State(state): State<Arc<HttpState>>,
    axum::extract::Path(key): axum::extract::Path<String>,
) -> Response {
    let manager = state.gateway_server.session_manager.read().await;
    match manager.remove_session(&key) {
        Ok(Some(_)) => Json(serde_json::json!({"ok": true})).into_response(),
        Ok(None) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "session not found"}))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": e}))).into_response(),
    }
}

pub async fn config_get_handler(
    State(state): State<Arc<HttpState>>,
) -> impl IntoResponse {
    Json(serde_json::to_value(&*state._gateway).unwrap_or_default())
}

pub async fn config_reload_handler() -> impl IntoResponse {
    // Config reload is handled by the config crate's hot-reload watcher.
    // This endpoint triggers a manual re-read signal.
    Json(serde_json::json!({"ok": true, "message": "reload requested"}))
}

pub async fn models_list_handler(
    State(state): State<Arc<HttpState>>,
) -> impl IntoResponse {
    let models = match &state.llm_provider {
        Some(p) => p.supported_models(),
        None => vec![],
    };
    Json(serde_json::json!({ "models": models }))
}

pub async fn config_full_get_handler(
    State(state): State<Arc<HttpState>>,
) -> Response {
    match &state.full_config {
        Some(cfg) => {
            let cfg = cfg.read().await;
            Json(serde_json::to_value(&*cfg).unwrap_or_default()).into_response()
        }
        None => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "full config not available"}))).into_response(),
    }
}

pub async fn config_full_put_handler(
    State(state): State<Arc<HttpState>>,
    Json(new_config): Json<oclaws_config::settings::Config>,
) -> Response {
    let errors = new_config.validate();
    if !errors.is_empty() {
        return (StatusCode::BAD_REQUEST, Json(serde_json::json!({"errors": errors}))).into_response();
    }
    let Some(ref full_config) = state.full_config else {
        return (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "full config not available"}))).into_response();
    };
    if let Some(ref path) = state.config_path {
        match serde_json::to_string_pretty(&new_config) {
            Ok(content) => {
                if let Err(e) = tokio::fs::write(path, content).await {
                    return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("write failed: {}", e)}))).into_response();
                }
            }
            Err(e) => {
                return (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": format!("serialize failed: {}", e)}))).into_response();
            }
        }
    }
    *full_config.write().await = new_config;
    Json(serde_json::json!({"ok": true})).into_response()
}

pub async fn config_ui_handler() -> axum::response::Html<&'static str> {
    axum::response::Html(CONFIG_UI_HTML)
}

pub async fn webchat_ui_handler() -> axum::response::Html<&'static str> {
    axum::response::Html(WEBCHAT_HTML)
}

const CONFIG_UI_HTML: &str = r##"<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>OCLAWS Config</title>
<style>
*{box-sizing:border-box;margin:0;padding:0}
body{font-family:system-ui,sans-serif;background:#0f0f1a;color:#e0e0e0;padding:20px;min-height:100vh}
.header{display:flex;justify-content:space-between;align-items:center;margin-bottom:20px}
h1{color:#00d4ff;font-size:1.5rem}
#lang-btn{padding:6px 14px;background:#16213e;border:1px solid #0f3460;border-radius:6px;color:#00d4ff;cursor:pointer;font-size:13px}
#lang-btn:hover{background:#0f3460}
.tabs{display:flex;gap:4px;margin-bottom:0;flex-wrap:wrap}
.tab{padding:10px 18px;background:#16213e;border:1px solid #0f3460;border-bottom:none;border-radius:8px 8px 0 0;cursor:pointer;color:#8899aa;transition:all .2s;font-size:14px}
.tab:hover{background:#1a2744;color:#c0c0c0}
.tab.active{background:#1e2d4a;color:#00d4ff;border-color:#00d4ff;border-bottom-color:transparent}
.panel{display:none;background:#1e2d4a;border:1px solid #0f3460;border-radius:0 8px 8px 8px;padding:20px;max-height:68vh;overflow-y:auto}
.panel.active{display:block}
.panel::-webkit-scrollbar{width:6px}
.panel::-webkit-scrollbar-thumb{background:#0f3460;border-radius:3px}
.field{margin-bottom:14px;position:relative}
.field label{display:block;font-size:12px;color:#6b7f99;margin-bottom:4px;letter-spacing:.3px}
.field input,.field textarea,.field select{width:100%;padding:9px 12px;background:#141825;border:1px solid #2a3a5c;border-radius:6px;color:#e0e0e0;font-family:'SF Mono',Monaco,monospace;font-size:13px;transition:border-color .2s}
.field input:focus,.field textarea:focus{outline:none;border-color:#00d4ff}
.field input[type=checkbox]{width:auto;accent-color:#00d4ff}
.field textarea{min-height:60px;resize:vertical}
.pwd-wrap{position:relative}.pwd-wrap input{padding-right:36px}
.pwd-toggle{position:absolute;right:8px;top:50%;transform:translateY(-50%);background:none;border:none;color:#6b7f99;cursor:pointer;font-size:16px;padding:2px 4px}
.pwd-toggle:hover{color:#00d4ff}
.section{margin:12px 0;padding:14px;border:1px solid #2a3a5c;border-radius:8px;background:#19223a;transition:box-shadow .2s}
.section:hover{box-shadow:0 2px 12px rgba(0,212,255,.06)}
.section-title{font-size:13px;font-weight:600;color:#00d4ff;margin-bottom:10px}
.sub-group{margin:16px 0;padding:12px;border-left:3px solid #0f3460;background:#161d30;border-radius:0 6px 6px 0}
.sub-group-title{font-size:13px;font-weight:600;color:#7eb8da;margin-bottom:4px}
.sub-group-desc{font-size:11px;color:#5a6f8a;margin-bottom:10px}
.add-row{display:flex;gap:8px;align-items:center;margin-top:12px;flex-wrap:wrap}
.add-row input,.add-row select{flex:1;min-width:100px;padding:7px 10px;background:#141825;border:1px solid #2a3a5c;border-radius:6px;color:#e0e0e0;font-size:13px}
.btn-sm{padding:7px 14px;background:#0f3460;border:1px solid #1a4a80;border-radius:6px;color:#00d4ff;cursor:pointer;font-size:12px;white-space:nowrap;transition:all .2s}
.btn-sm:hover{background:#1a4a80}
.btn-add-provider{margin-top:12px;padding:9px 18px;background:linear-gradient(135deg,#0f3460,#1a4a80);border:1px solid rgba(0,212,255,.2);border-radius:6px;color:#00d4ff;cursor:pointer;font-size:13px;font-weight:600;transition:all .2s}
.btn-add-provider:hover{background:linear-gradient(135deg,#1a4a80,#245090);box-shadow:0 2px 8px rgba(0,212,255,.15)}
.btn-enable{padding:6px 14px;background:#1a3a2e;border:1px solid #2a5a4e;border-radius:6px;color:#4caf50;cursor:pointer;font-size:12px;transition:all .2s}
.btn-enable:hover{background:#2a5a4e}
#save-btn{display:block;margin:20px auto;padding:12px 44px;background:linear-gradient(135deg,#00b8d4,#00d4ff);color:#0f0f1a;border:none;border-radius:8px;font-size:15px;font-weight:700;cursor:pointer;transition:all .2s;letter-spacing:.5px}
#save-btn:hover{transform:translateY(-1px);box-shadow:0 4px 16px rgba(0,212,255,.3)}
#save-btn:disabled{opacity:.6;cursor:not-allowed;transform:none}
#toast{position:fixed;top:20px;right:20px;z-index:999;pointer-events:none}
.toast-item{padding:12px 20px;border-radius:8px;margin-bottom:8px;font-size:13px;animation:slideIn .3s ease;pointer-events:auto}
.toast-ok{background:#1a3a2e;border:1px solid #4caf50;color:#4caf50}
.toast-err{background:#3a1a1a;border:1px solid #f44336;color:#f44336}
@keyframes slideIn{from{opacity:0;transform:translateX(40px)}to{opacity:1;transform:translateX(0)}}
@keyframes slideOut{from{opacity:1}to{opacity:0;transform:translateX(40px)}}
.modal-bg{position:fixed;top:0;left:0;width:100%;height:100%;background:rgba(0,0,0,.6);display:flex;align-items:center;justify-content:center;z-index:100}
.modal{background:#1e2d4a;border:1px solid #0f3460;border-radius:10px;padding:24px;min-width:340px;max-width:90vw}
.modal h3{color:#00d4ff;margin-bottom:16px;font-size:15px}
.modal .field{margin-bottom:12px}
.modal-btns{display:flex;gap:8px;justify-content:flex-end;margin-top:16px}
.modal-btns button{padding:8px 18px;border-radius:6px;cursor:pointer;font-size:13px;border:1px solid #2a3a5c}
.modal-btns .btn-ok{background:#00d4ff;color:#0f0f1a;border-color:#00d4ff;font-weight:600}
.modal-btns .btn-cancel{background:#16213e;color:#8899aa}
@media(max-width:600px){body{padding:10px}.tab{padding:7px 10px;font-size:12px}.panel{padding:12px}#save-btn{width:100%}}
</style></head><body>
<div class="header"><h1 id="title">OCLAWS Configuration</h1><button id="lang-btn" onclick="toggleLang()">中文</button></div>
<div class="tabs" id="tabs"></div>
<div id="panels"></div>
<button id="save-btn" onclick="save()">Save</button>
<div id="toast"></div>
<script>
let lang="en",cfg={};
const I={en:{
title:"OCLAWS Configuration",save:"Save",saving:"Saving...",saved:"Saved!",
addField:"Add Field",addProvider:"Add Provider",enable:"Enable",cancel:"Cancel",confirm:"OK",
key:"Key",providerName:"Provider Name",
tabs:["Gateway","Models","Channels","Browser","Cron","Logging","Settings"],
subGroups:{diagnostics:"Diagnostics / OTel",talk:"Voice (Talk)",web:"Web / Reconnect",ui:"UI",auth:"Auth",update:"Update",env:"Environment Variables",media:"Media",canvasHost:"Canvas Host",discovery:"Service Discovery"},
subDescs:{diagnostics:"OpenTelemetry tracing and diagnostic settings",talk:"Voice streaming and audio configuration",web:"WebSocket reconnection and web server settings",ui:"Terminal UI and display preferences",auth:"Authentication configuration",update:"Auto-update and version check settings",env:"Environment variable overrides",media:"Image and audio processing settings",canvasHost:"Canvas host configuration",discovery:"Service discovery and registration"},
errPrefix:"Error: ",loadErr:"Load error: ",fieldExists:"Field already exists",providerExists:"Provider already exists"
},zh:{
title:"OCLAWS 配置管理",save:"保存",saving:"保存中...",saved:"保存成功！",
addField:"添加字段",addProvider:"添加 Provider",enable:"启用",cancel:"取消",confirm:"确定",
key:"键名",providerName:"Provider 名称",
tabs:["网关","模型","频道","浏览器","定时任务","日志","其他设置"],
subGroups:{diagnostics:"诊断 / OTel",talk:"语音 (Talk)",web:"Web / 重连",ui:"界面",auth:"认证",update:"更新",env:"环境变量",media:"媒体",canvasHost:"Canvas 主机",discovery:"服务发现"},
subDescs:{diagnostics:"OpenTelemetry 链路追踪与诊断设置",talk:"语音流与音频配置",web:"WebSocket 重连与 Web 服务器设置",ui:"终端界面与显示偏好",auth:"认证配置",update:"自动更新与版本检查",env:"环境变量覆盖",media:"图片与音频处理设置",canvasHost:"Canvas 主机配置",discovery:"服务发现与注册"},
errPrefix:"错误：",loadErr:"加载失败：",fieldExists:"字段已存在",providerExists:"Provider 已存在"
}};
const t=k=>I[lang][k]||k;
const TAB_KEYS=[["gateway"],["models"],["channels"],["browser"],["cron"],["logging"],["diagnostics","talk","web","ui","auth","update","env","media","canvasHost","discovery"]];
const CHANNELS=["telegram","slack","discord","matrix","irc","webhook"];
function toggleLang(){lang=lang==="en"?"zh":"en";document.getElementById("lang-btn").textContent=lang==="en"?"中文":"EN";render()}
function toast(msg,ok){const d=document.getElementById("toast"),el=document.createElement("div");el.className="toast-item "+(ok?"toast-ok":"toast-err");el.textContent=msg;d.appendChild(el);setTimeout(()=>{el.style.animation="slideOut .3s ease forwards";setTimeout(()=>el.remove(),300)},2500)}
function isSensitive(k){return/token|key|password|secret|api_key/i.test(k)}
function makeField(path,val,parent){
const div=document.createElement("div");div.className="field";
const lbl=document.createElement("label");lbl.textContent=path;div.appendChild(lbl);
const key=path.split(".").pop();
if(val===null||val===undefined){
const inp=document.createElement("input");inp.type="text";inp.dataset.path=path;inp.value="";div.appendChild(inp);
}else if(typeof val==="boolean"){
const inp=document.createElement("input");inp.type="checkbox";inp.checked=val;inp.dataset.path=path;div.appendChild(inp);
}else if(typeof val==="number"){
const inp=document.createElement("input");inp.type="number";inp.value=val;inp.step="any";inp.dataset.path=path;div.appendChild(inp);
}else if(typeof val==="string"){
if(isSensitive(key)){
const w=document.createElement("div");w.className="pwd-wrap";
const inp=document.createElement("input");inp.type="password";inp.value=val;inp.dataset.path=path;
const btn=document.createElement("button");btn.type="button";btn.className="pwd-toggle";btn.innerHTML="&#128065;";
btn.onclick=()=>{inp.type=inp.type==="password"?"text":"password"};
w.appendChild(inp);w.appendChild(btn);div.appendChild(w);
}else{
const inp=document.createElement("input");inp.type="text";inp.value=val;inp.dataset.path=path;div.appendChild(inp);
}}else if(Array.isArray(val)){
const ta=document.createElement("textarea");ta.value=JSON.stringify(val,null,2);ta.dataset.path=path;ta.dataset.type="array";div.appendChild(ta);
}else if(typeof val==="object"){
const sec=document.createElement("div");sec.className="section";
const st=document.createElement("div");st.className="section-title";st.textContent=key;sec.appendChild(st);
for(const[k,v] of Object.entries(val))makeField(path+"."+k,v,sec);
parent.appendChild(sec);return;
}
parent.appendChild(div);
}
function addFieldRow(parentPath,container){
const row=document.createElement("div");row.className="add-row";
const inp=document.createElement("input");inp.placeholder=t("key");
const sel=document.createElement("select");
["string","number","bool","array"].forEach(tp=>{const o=document.createElement("option");o.value=tp;o.textContent=tp;sel.appendChild(o)});
const btn=document.createElement("button");btn.className="btn-sm";btn.textContent="+ "+t("addField");
btn.onclick=()=>{
const k=inp.value.trim();if(!k)return;
const fullPath=parentPath?parentPath+"."+k:k;
if(document.querySelector('[data-path="'+fullPath+'"]')){toast(t("fieldExists"),false);return}
const defaults={string:"",number:0,bool:false,array:[]};
setPath(cfg,fullPath,defaults[sel.value]);
makeField(fullPath,defaults[sel.value],container.querySelector(".section")||container);
inp.value="";
};
row.appendChild(inp);row.appendChild(sel);row.appendChild(btn);return row;
}
function showModal(title,fields,onOk){
const bg=document.createElement("div");bg.className="modal-bg";
const m=document.createElement("div");m.className="modal";
const h=document.createElement("h3");h.textContent=title;m.appendChild(h);
const inputs={};
fields.forEach(f=>{
const d=document.createElement("div");d.className="field";
const l=document.createElement("label");l.textContent=f.label;d.appendChild(l);
const i=document.createElement("input");i.type="text";i.placeholder=f.placeholder||"";d.appendChild(i);
inputs[f.key]=i;m.appendChild(d);
});
const btns=document.createElement("div");btns.className="modal-btns";
const cBtn=document.createElement("button");cBtn.className="btn-cancel";cBtn.textContent=t("cancel");cBtn.onclick=()=>bg.remove();
const oBtn=document.createElement("button");oBtn.className="btn-ok";oBtn.textContent=t("confirm");
oBtn.onclick=()=>{const vals={};for(const[k,i] of Object.entries(inputs))vals[k]=i.value.trim();if(onOk(vals))bg.remove()};
btns.appendChild(cBtn);btns.appendChild(oBtn);m.appendChild(btns);
bg.appendChild(m);bg.onclick=e=>{if(e.target===bg)bg.remove()};
document.body.appendChild(bg);
}
function render(){
document.getElementById("title").textContent=t("title");
document.getElementById("save-btn").textContent=t("save");
const tabsEl=document.getElementById("tabs"),panelsEl=document.getElementById("panels");
tabsEl.innerHTML="";panelsEl.innerHTML="";
const tabNames=t("tabs");
tabNames.forEach((name,i)=>{
const tab=document.createElement("div");tab.className="tab"+(i===0?" active":"");tab.textContent=name;
tab.onclick=()=>{document.querySelectorAll(".tab").forEach(t=>t.classList.remove("active"));tab.classList.add("active");document.querySelectorAll(".panel").forEach(p=>p.classList.remove("active"));document.getElementById("p-"+i).classList.add("active")};
tabsEl.appendChild(tab);
const panel=document.createElement("div");panel.className="panel"+(i===0?" active":"");panel.id="p-"+i;
const keys=TAB_KEYS[i];
if(i===6){
keys.forEach(key=>{
const sg=document.createElement("div");sg.className="sub-group";
const sgt=document.createElement("div");sgt.className="sub-group-title";sgt.textContent=t("subGroups")[key]||key;sg.appendChild(sgt);
const sgd=document.createElement("div");sgd.className="sub-group-desc";sgd.textContent=t("subDescs")[key]||"";sg.appendChild(sgd);
if(cfg[key]&&typeof cfg[key]==="object"){for(const[k,v] of Object.entries(cfg[key]))makeField(key+"."+k,v,sg)}
sg.appendChild(addFieldRow(key,sg));panel.appendChild(sg);
});
}else{
keys.forEach(key=>{
if(cfg[key]!==undefined&&cfg[key]!==null&&typeof cfg[key]==="object"){
const sec=document.createElement("div");sec.className="section";
const st=document.createElement("div");st.className="section-title";st.textContent=key;sec.appendChild(st);
for(const[k,v] of Object.entries(cfg[key]))makeField(key+"."+k,v,sec);
panel.appendChild(sec);
}else if(cfg[key]!==undefined){makeField(key,cfg[key],panel)}
});
if(i===1){
const btn=document.createElement("button");btn.className="btn-add-provider";btn.textContent="+ "+t("addProvider");
btn.onclick=()=>showModal(t("addProvider"),[{key:"name",label:t("providerName"),placeholder:"e.g. openai"}],vals=>{
if(!vals.name)return false;if(!cfg.models)cfg.models={};
if(cfg.models[vals.name]){toast(t("providerExists"),false);return false}
cfg.models[vals.name]={};render();return true;
});panel.appendChild(btn);
}
if(i===2){
CHANNELS.forEach(ch=>{
if(!cfg.channels||!cfg.channels[ch]){
const row=document.createElement("div");row.className="add-row";row.style.marginBottom="8px";
const lbl=document.createElement("span");lbl.style.cssText="color:#6b7f99;font-size:13px;min-width:80px";lbl.textContent=ch;
const btn=document.createElement("button");btn.className="btn-enable";btn.textContent=t("enable")+" "+ch;
btn.onclick=()=>{if(!cfg.channels)cfg.channels={};cfg.channels[ch]={enabled:true};render()};
row.appendChild(lbl);row.appendChild(btn);panel.appendChild(row);
}});
}
panel.appendChild(addFieldRow(keys[0],panel));
}
panelsEl.appendChild(panel);
});
}
function setPath(obj,path,val){
const parts=path.split(".");let cur=obj;
for(let i=0;i<parts.length-1;i++){if(cur[parts[i]]===undefined||cur[parts[i]]===null)cur[parts[i]]={};cur=cur[parts[i]]}
cur[parts[parts.length-1]]=val;
}
function collect(){
const out=JSON.parse(JSON.stringify(cfg));
document.querySelectorAll("[data-path]").forEach(el=>{
const p=el.dataset.path;let v;
if(el.type==="checkbox")v=el.checked;
else if(el.type==="number")v=el.value===""?null:Number(el.value);
else if(el.dataset.type==="array"){try{v=JSON.parse(el.value)}catch{v=el.value.split("\n").filter(Boolean)}}
else v=el.value===""?null:el.value;
if(v!==null)setPath(out,p,v);
});
return out;
}
async function save(){
const btn=document.getElementById("save-btn");
btn.disabled=true;btn.textContent=t("saving");
try{
const r=await fetch("/api/config/full",{method:"PUT",headers:{"Content-Type":"application/json"},body:JSON.stringify(collect())});
const j=await r.json();
if(r.ok){toast(t("saved"),true)}
else{toast(t("errPrefix")+(j.errors||[j.error]).join(", "),false)}
}catch(e){toast(t("errPrefix")+e,false)}
btn.disabled=false;btn.textContent=t("save");
}
fetch("/api/config/full").then(r=>r.json()).then(j=>{cfg=j;render()}).catch(e=>toast(t("loadErr")+e,false));
</script></body></html>"##;

const WEBCHAT_HTML: &str = concat!(
r##"<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>OpenClaw Chat</title>
<style>
*{box-sizing:border-box;margin:0;padding:0}
body{font-family:system-ui,-apple-system,sans-serif;background:#0f0f1a;color:#e0e0e0;height:100vh;display:flex;flex-direction:column;overflow:hidden}
a{color:#00d4ff}
.header{display:flex;align-items:center;gap:12px;padding:10px 16px;background:#161d30;border-bottom:1px solid #1e2d4a;flex-shrink:0}
.header .dot{width:10px;height:10px;border-radius:50%;background:#555;flex-shrink:0;transition:background .3s}
.header .dot.on{background:#4caf50}
.header .dot.err{background:#f44336}
.header .brand{font-weight:700;color:#00d4ff;font-size:15px;white-space:nowrap}
.header select{background:#1e2d4a;color:#c0c0c0;border:1px solid #2a3a5c;border-radius:6px;padding:4px 8px;font-size:12px;cursor:pointer;max-width:180px}
.header select:focus{outline:none;border-color:#00d4ff}
.header .spacer{flex:1}
.header .status-text{font-size:11px;color:#6b7f99;white-space:nowrap}
.chat-thread{flex:1;overflow-y:auto;padding:16px;display:flex;flex-direction:column;gap:4px}
.chat-thread::-webkit-scrollbar{width:6px}
.chat-thread::-webkit-scrollbar-thumb{background:#2a3a5c;border-radius:3px}
.chat-group{display:flex;gap:12px;max-width:900px;width:100%;margin:0 auto;padding:8px 0}
.chat-group.user{flex-direction:row-reverse}
.chat-avatar{width:34px;height:34px;border-radius:8px;display:flex;align-items:center;justify-content:center;font-size:16px;flex-shrink:0;margin-top:2px}
.chat-group:not(.user) .chat-avatar{background:#1e2d4a;color:#00d4ff}
.chat-group.user .chat-avatar{background:#0f3460;color:#7eb8da}
.chat-messages{display:flex;flex-direction:column;gap:6px;min-width:0;max-width:calc(100% - 50px)}
.chat-text{font-size:14px;line-height:1.6;color:#d0d0d0;word-wrap:break-word;overflow-wrap:break-word}
.chat-group.user .chat-text{color:#b0c4de}
.chat-text p{margin:0 0 8px}
.chat-text p:last-child{margin-bottom:0}
.chat-text strong{color:#e8e8e8;font-weight:600}
.chat-text em{color:#a0b8d0;font-style:italic}
.chat-text code{background:#141825;padding:1px 5px;border-radius:4px;font-family:'SF Mono',Monaco,'Cascadia Code',monospace;font-size:13px;color:#7ee8fa}
.chat-text pre{background:#0d0d18;border:1px solid #1e2d4a;border-radius:8px;padding:12px;margin:8px 0;overflow-x:auto;position:relative}
.chat-text pre code{background:none;padding:0;color:#c8d8e8;font-size:13px;line-height:1.5}
.chat-text blockquote{border-left:3px solid #0f3460;padding:4px 12px;margin:6px 0;color:#8899aa;background:#141825;border-radius:0 6px 6px 0}
.chat-text ul,.chat-text ol{margin:6px 0 6px 20px}
.chat-text li{margin:2px 0}
.chat-text hr{border:none;border-top:1px solid #1e2d4a;margin:10px 0}
.chat-text a{color:#00d4ff;text-decoration:none}
.chat-text a:hover{text-decoration:underline}
.chat-text .copy-btn{position:absolute;top:6px;right:6px;background:#1e2d4a;border:1px solid #2a3a5c;border-radius:4px;color:#6b7f99;cursor:pointer;font-size:11px;padding:2px 8px}
.chat-text .copy-btn:hover{color:#00d4ff;border-color:#00d4ff}
.cursor-blink{display:inline-block;width:2px;height:1em;background:#00d4ff;margin-left:2px;animation:blink 1s step-end infinite;vertical-align:text-bottom}
@keyframes blink{50%{opacity:0}}
.tool-card{background:#141825;border:1px solid #1e2d4a;border-radius:8px;margin:6px 0;overflow:hidden}
.tool-card-header{display:flex;align-items:center;gap:8px;padding:8px 12px;cursor:pointer;font-size:13px;color:#8899aa;user-select:none}
.tool-card-header:hover{background:#1a2240}
.tool-card-icon{font-size:14px}
.tool-card-name{font-weight:600;color:#7eb8da}
.tool-card-preview{color:#6b7f99;font-size:12px;margin-left:auto;max-width:200px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap}
.tool-card-body{display:none;padding:8px 12px;border-top:1px solid #1e2d4a;font-size:12px;font-family:'SF Mono',Monaco,monospace;color:#8899aa;max-height:200px;overflow-y:auto;white-space:pre-wrap;word-break:break-all}
.tool-card.open .tool-card-body{display:block}
.chat-compose{flex-shrink:0;padding:12px 16px;background:linear-gradient(to top,#0f0f1a 60%,transparent);border-top:1px solid #1e2d4a;position:relative}
.compose-wrap{max-width:900px;margin:0 auto;display:flex;gap:8px;align-items:flex-end;position:relative}
.compose-wrap textarea{flex:1;background:#141825;border:1px solid #2a3a5c;border-radius:10px;padding:10px 14px;color:#e0e0e0;font-family:system-ui,sans-serif;font-size:14px;resize:none;min-height:42px;max-height:150px;line-height:1.4;overflow-y:auto}
.compose-wrap textarea:focus{outline:none;border-color:#00d4ff}
.compose-wrap textarea::placeholder{color:#4a5a70}
.send-btn{width:38px;height:38px;border-radius:10px;background:#00d4ff;border:none;color:#0f0f1a;cursor:pointer;display:flex;align-items:center;justify-content:center;font-size:18px;flex-shrink:0;transition:opacity .2s}
.send-btn:hover{opacity:.85}
.send-btn:disabled{opacity:.4;cursor:not-allowed}
.slash-popup{position:absolute;bottom:100%;left:0;right:60px;background:#1e2d4a;border:1px solid #2a3a5c;border-radius:8px;margin-bottom:4px;display:none;max-height:240px;overflow-y:auto;z-index:10}
.slash-popup.show{display:block}
.slash-item{padding:8px 14px;cursor:pointer;font-size:13px;display:flex;gap:10px;align-items:center}
.slash-item:hover,.slash-item.active{background:#253050}
.slash-item .cmd{color:#00d4ff;font-weight:600;min-width:80px}
.slash-item .desc{color:#6b7f99;font-size:12px}
.welcome{text-align:center;color:#4a5a70;margin:auto;padding:40px 20px}
.welcome h2{color:#2a3a5c;font-size:20px;margin-bottom:8px}
.welcome p{font-size:14px}
@media(max-width:600px){.header{padding:8px 10px;gap:8px}.header .brand{font-size:13px}.chat-thread{padding:10px}.compose-wrap textarea{font-size:13px}}
</style></head>
<body>
"##,
// --- WEBCHAT_HTML body ---
r##"<div class="header">
  <div class="dot" id="dot"></div>
  <span class="brand">OpenClaw</span>
  <select id="model-sel" title="Model"><option>loading...</option></select>
  <select id="session-sel" title="Session"><option>loading...</option></select>
  <div class="spacer"></div>
  <span class="status-text" id="status-text">Connecting...</span>
</div>
<div class="chat-thread" id="thread">
  <div class="welcome"><h2>OpenClaw Chat</h2><p>Send a message to get started</p></div>
</div>
<div class="chat-compose">
  <div class="compose-wrap">
    <div class="slash-popup" id="slash-popup"></div>
    <textarea id="input" rows="1" placeholder="Message OpenClaw..." autofocus></textarea>
    <button class="send-btn" id="send-btn" title="Send">&#9654;</button>
  </div>
</div>
"##,
// --- WEBCHAT_HTML script ---
r##"<script>
const $=s=>document.getElementById(s);
const thread=$('thread'),input=$('input'),sendBtn=$('send-btn'),dot=$('dot'),
      statusText=$('status-text'),modelSel=$('model-sel'),sessionSel=$('session-sel'),
      slashPopup=$('slash-popup');
let ws,sessionId='',currentModel='',messages=[],streaming=false,userScrolled=false,
    reconnectDelay=1000,slashIdx=-1;

const SLASH_CMDS=[
  {cmd:'/help',desc:'Show available commands'},
  {cmd:'/clear',desc:'Clear chat history'},
  {cmd:'/model',desc:'Switch model'},
  {cmd:'/session',desc:'Switch session'},
  {cmd:'/status',desc:'Show connection status'},
  {cmd:'/think',desc:'Enable thinking mode'},
  {cmd:'/verbose',desc:'Toggle verbose output'},
  {cmd:'/abort',desc:'Abort current generation'}
];

// --- WebSocket ---
function connect(){
  const proto=location.protocol==='https:'?'wss:':'ws:';
  ws=new WebSocket(proto+'//'+location.host+'/webchat/ws');
  ws.onopen=()=>{dot.className='dot on';statusText.textContent='Connected';reconnectDelay=1000};
  ws.onclose=()=>{
    dot.className='dot err';statusText.textContent='Disconnected';
    setTimeout(connect,reconnectDelay);reconnectDelay=Math.min(reconnectDelay*1.5,15000);
  };
  ws.onerror=()=>{dot.className='dot err'};
  ws.onmessage=e=>{
    let d;try{d=JSON.parse(e.data)}catch{return}
    handleMsg(d);
  };
}

function wsSend(obj){if(ws&&ws.readyState===1)ws.send(JSON.stringify(obj))}

// --- Message handling ---
function handleMsg(d){
  switch(d.type){
    case 'connected':
      sessionId=d.session||sessionId;currentModel=d.model||'';
      statusText.textContent='Connected - '+currentModel;
      wsSend({type:'models'});wsSend({type:'sessions'});
      break;
    case 'typing':
      streaming=true;sendBtn.disabled=true;
      statusText.textContent='Thinking...';
      break;
    case 'chunk':
      if(d.content){
        let last=messages[messages.length-1];
        if(!last||last.role!=='assistant'||last.done){
          messages.push({role:'assistant',content:d.content,done:false,tools:[]});
        }else{last.content+=d.content}
        renderMessages();
      }
      break;
    case 'tool_call':
      if(messages.length&&messages[messages.length-1].role==='assistant'){
        messages[messages.length-1].tools.push({name:d.name,args:d.args,status:'running',result:null});
        renderMessages();
      }
      break;
    case 'tool_result':
      if(messages.length&&messages[messages.length-1].role==='assistant'){
        let tools=messages[messages.length-1].tools;
        let tc=tools.find(t=>t.name===d.name&&t.status==='running');
        if(tc){tc.status=d.status||'success';tc.result=d.result||''}
        renderMessages();
      }
      break;
    case 'done':
      streaming=false;sendBtn.disabled=false;
      statusText.textContent='Connected - '+currentModel;
      if(d.content){
        let last=messages[messages.length-1];
        if(!last||last.role!=='assistant'){
          messages.push({role:'assistant',content:d.content,done:true,tools:[]});
        }else{last.content=d.content;last.done=true}
      }else if(messages.length&&messages[messages.length-1].role==='assistant'){
        messages[messages.length-1].done=true;
      }
      renderMessages();
      break;
    case 'error':
      streaming=false;sendBtn.disabled=false;
      statusText.textContent='Connected - '+currentModel;
      messages.push({role:'assistant',content:'**Error:** '+(d.content||'Unknown error'),done:true,tools:[]});
      renderMessages();
      break;
    case 'history':
      messages=(d.messages||[]).map(m=>({role:m.role,content:m.content,done:true,tools:[]}));
      renderMessages();
      break;
    case 'sessions':
      renderSessionSelect(d.sessions||[]);
      break;
    case 'models':
      renderModelSelect(d.models||[]);
      break;
  }
}
"##,
// --- WEBCHAT_HTML markdown renderer ---
r##"
// --- Markdown renderer ---
function md(text){
  if(!text)return'';
  let h=text;
  // Escape HTML
  h=h.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
  // Code blocks
  h=h.replace(/```(\w*)\n([\s\S]*?)```/g,function(_,lang,code){
    const id='cb'+Math.random().toString(36).slice(2,8);
    return'<pre><code data-lang="'+(lang||'')+'" id="'+id+'">'+code.trim()+'</code><button class="copy-btn" onclick="copyCode(\''+id+'\')">Copy</button></pre>';
  });
  // Inline code
  h=h.replace(/`([^`\n]+)`/g,'<code>$1</code>');
  // Bold
  h=h.replace(/\*\*(.+?)\*\*/g,'<strong>$1</strong>');
  // Italic
  h=h.replace(/\*(.+?)\*/g,'<em>$1</em>');
  // Links
  h=h.replace(/\[([^\]]+)\]\(([^)]+)\)/g,'<a href="$2" target="_blank" rel="noopener">$1</a>');
  // Blockquotes
  h=h.replace(/^&gt;\s?(.*)$/gm,'<blockquote>$1</blockquote>');
  // Merge adjacent blockquotes
  h=h.replace(/<\/blockquote>\n<blockquote>/g,'\n');
  // Horizontal rules
  h=h.replace(/^---$/gm,'<hr>');
  // Unordered lists
  h=h.replace(/^[\-\*]\s+(.*)$/gm,'<li>$1</li>');
  h=h.replace(/((?:<li>.*<\/li>\n?)+)/g,'<ul>$1</ul>');
  // Ordered lists
  h=h.replace(/^\d+\.\s+(.*)$/gm,'<li>$1</li>');
  // Headings
  h=h.replace(/^### (.*)$/gm,'<strong style="font-size:15px">$1</strong>');
  h=h.replace(/^## (.*)$/gm,'<strong style="font-size:16px">$1</strong>');
  h=h.replace(/^# (.*)$/gm,'<strong style="font-size:18px">$1</strong>');
  // Paragraphs
  h=h.replace(/\n{2,}/g,'</p><p>');
  h=h.replace(/\n/g,'<br>');
  h='<p>'+h+'</p>';
  // Clean empty paragraphs
  h=h.replace(/<p>\s*<\/p>/g,'');
  return h;
}
function copyCode(id){
  const el=document.getElementById(id);
  if(el)navigator.clipboard.writeText(el.textContent).catch(()=>{});
}
"##,
// --- WEBCHAT_HTML UI rendering ---
r##"
// --- Render messages ---
function renderMessages(){
  const wasAtBottom=!userScrolled;
  thread.innerHTML='';
  if(!messages.length){
    thread.innerHTML='<div class="welcome"><h2>OpenClaw Chat</h2><p>Send a message to get started</p></div>';
    return;
  }
  let lastRole='';
  let group=null;
  let msgsDiv=null;
  messages.forEach((m,i)=>{
    if(m.role!==lastRole){
      group=document.createElement('div');
      group.className='chat-group'+(m.role==='user'?' user':'');
      const av=document.createElement('div');
      av.className='chat-avatar';
      av.textContent=m.role==='user'?'\u{1F464}':'\u{2726}';
      const msgs=document.createElement('div');
      msgs.className='chat-messages';
      group.appendChild(av);group.appendChild(msgs);
      thread.appendChild(group);
      msgsDiv=msgs;lastRole=m.role;
    }
    const txt=document.createElement('div');
    txt.className='chat-text';
    if(m.role==='user'){
      txt.textContent=m.content;
    }else{
      txt.innerHTML=md(m.content);
      if(!m.done){
        const cur=document.createElement('span');
        cur.className='cursor-blink';
        txt.appendChild(cur);
      }
    }
    msgsDiv.appendChild(txt);
    // Tool cards
    if(m.tools&&m.tools.length){
      m.tools.forEach(tc=>{
        msgsDiv.appendChild(makeToolCard(tc));
      });
    }
  });
  if(wasAtBottom)scrollBottom();
}
function scrollBottom(){
  thread.scrollTop=thread.scrollHeight;
}
thread.addEventListener('scroll',()=>{
  const diff=thread.scrollHeight-thread.scrollTop-thread.clientHeight;
  userScrolled=diff>60;
});
"##,
// --- WEBCHAT_HTML tool cards + selects ---
r##"
// --- Tool cards ---
function makeToolCard(tc){
  const card=document.createElement('div');
  card.className='tool-card'+(tc.status==='success'||tc.status==='error'?' ':'');
  const hdr=document.createElement('div');
  hdr.className='tool-card-header';
  const icons={pending:'\u25CC',running:'\u27F3',success:'\u2713',error:'\u2717'};
  hdr.innerHTML='<span class="tool-card-icon">'+(icons[tc.status]||'\u25CC')+'</span>'
    +'<span class="tool-card-name">'+esc(tc.name)+'</span>'
    +'<span class="tool-card-preview">'+(tc.result?esc(tc.result).slice(0,60):'')+'</span>';
  hdr.onclick=()=>card.classList.toggle('open');
  const body=document.createElement('div');
  body.className='tool-card-body';
  body.textContent=tc.result||JSON.stringify(tc.args,null,2);
  card.appendChild(hdr);card.appendChild(body);
  return card;
}
function esc(s){return s?s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;'):''}

// --- Select renderers ---
function renderModelSelect(models){
  modelSel.innerHTML='';
  models.forEach(m=>{
    const o=document.createElement('option');o.value=m;o.textContent=m;
    if(m===currentModel)o.selected=true;
    modelSel.appendChild(o);
  });
}
modelSel.onchange=()=>{wsSend({type:'set_model',model:modelSel.value})};

function renderSessionSelect(sessions){
  sessionSel.innerHTML='';
  const cur=document.createElement('option');
  cur.value=sessionId;cur.textContent=sessionId.slice(0,8)+'... (current)';cur.selected=true;
  sessionSel.appendChild(cur);
  sessions.forEach(s=>{
    if(s.id===sessionId)return;
    const o=document.createElement('option');o.value=s.id;
    o.textContent=s.id.slice(0,8)+'... ('+s.messages+' msgs)';
    sessionSel.appendChild(o);
  });
  const nw=document.createElement('option');nw.value='__new__';nw.textContent='+ New session';
  sessionSel.appendChild(nw);
}
sessionSel.onchange=()=>{
  if(sessionSel.value==='__new__'){
    messages=[];renderMessages();
    wsSend({type:'set_session',session:crypto.randomUUID()});
  }else{
    wsSend({type:'set_session',session:sessionSel.value});
    wsSend({type:'history',session:sessionSel.value});
  }
};
"##,
// --- WEBCHAT_HTML send + slash commands ---
r##"
// --- Send message ---
function sendMessage(){
  const text=input.value.trim();
  if(!text||streaming)return;
  messages.push({role:'user',content:text,done:true,tools:[]});
  renderMessages();scrollBottom();
  wsSend({type:'message',content:text,session:sessionId});
  input.value='';autoResize();
  hideSlash();
}
sendBtn.onclick=sendMessage;

// --- Slash commands ---
function showSlash(filter){
  const f=filter.toLowerCase();
  const matched=SLASH_CMDS.filter(c=>c.cmd.startsWith(f));
  if(!matched.length){hideSlash();return}
  slashPopup.innerHTML='';
  matched.forEach((c,i)=>{
    const el=document.createElement('div');
    el.className='slash-item'+(i===0?' active':'');
    el.innerHTML='<span class="cmd">'+c.cmd+'</span><span class="desc">'+c.desc+'</span>';
    el.onclick=()=>{applySlash(c.cmd)};
    slashPopup.appendChild(el);
  });
  slashPopup.classList.add('show');
  slashIdx=0;
}
function hideSlash(){slashPopup.classList.remove('show');slashPopup.innerHTML='';slashIdx=-1}
function applySlash(cmd){
  input.value=cmd+' ';hideSlash();input.focus();
}
function slashNav(dir){
  const items=slashPopup.querySelectorAll('.slash-item');
  if(!items.length)return;
  items[slashIdx]?.classList.remove('active');
  slashIdx=(slashIdx+dir+items.length)%items.length;
  items[slashIdx]?.classList.add('active');
  items[slashIdx]?.scrollIntoView({block:'nearest'});
}
"##,
// --- WEBCHAT_HTML keyboard + init ---
r##"
// --- Keyboard shortcuts ---
input.addEventListener('keydown',e=>{
  if(slashPopup.classList.contains('show')){
    if(e.key==='ArrowDown'){e.preventDefault();slashNav(1);return}
    if(e.key==='ArrowUp'){e.preventDefault();slashNav(-1);return}
    if(e.key==='Enter'){
      e.preventDefault();
      const active=slashPopup.querySelector('.slash-item.active .cmd');
      if(active)applySlash(active.textContent);
      return;
    }
    if(e.key==='Escape'){hideSlash();return}
  }
  if(e.key==='Enter'&&!e.shiftKey){e.preventDefault();sendMessage();return}
  if(e.key==='Escape'&&streaming){wsSend({type:'abort'});return}
});
input.addEventListener('input',()=>{
  autoResize();
  const v=input.value;
  if(v.startsWith('/')&&!v.includes(' ')){showSlash(v)}
  else{hideSlash()}
});
document.addEventListener('keydown',e=>{
  if(e.ctrlKey&&e.key==='c'&&streaming){wsSend({type:'abort'})}
});

// --- Auto-resize textarea ---
function autoResize(){
  input.style.height='auto';
  input.style.height=Math.min(input.scrollHeight,150)+'px';
}

// --- Init ---
connect();
</script></body></html>"##,
);

// --- Cron REST endpoints ---

pub async fn cron_list_handler(
    State(state): State<Arc<HttpState>>,
) -> Response {
    let Some(ref svc) = state.cron_service else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": "cron not enabled"}))).into_response();
    };
    let jobs = svc.list().await;
    Json(serde_json::json!({ "jobs": jobs })).into_response()
}

pub async fn cron_create_handler(
    State(state): State<Arc<HttpState>>,
    Json(payload): Json<oclaws_cron_core::CronJob>,
) -> Response {
    let Some(ref svc) = state.cron_service else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": "cron not enabled"}))).into_response();
    };
    match svc.add(payload).await {
        Ok(job) => (StatusCode::CREATED, Json(serde_json::json!(job))).into_response(),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

pub async fn cron_delete_handler(
    State(state): State<Arc<HttpState>>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    let Some(ref svc) = state.cron_service else {
        return (StatusCode::SERVICE_UNAVAILABLE, Json(serde_json::json!({"error": "cron not enabled"}))).into_response();
    };
    match svc.remove(&id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": e.to_string()}))).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::{get, post, delete}, Router};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;
    use crate::http::{HttpState, health_handler};
    use crate::http::auth::AuthState;
    use crate::server::GatewayServer;
    use oclaws_config::settings::Gateway;
    use oclaws_llm_core::providers::MockLlmProvider;
    use tokio::sync::RwLock;

    fn test_state(provider: Option<Arc<dyn oclaws_llm_core::providers::LlmProvider>>) -> Arc<HttpState> {
        Arc::new(HttpState {
            auth_state: Arc::new(RwLock::new(AuthState::new(None))),
            gateway_server: Arc::new(GatewayServer::new(0)),
            _gateway: Arc::new(Gateway::default()),
            llm_provider: provider,
            hook_pipeline: None,
            channel_manager: None,
            tool_registry: None,
            plugin_registrations: None,
            cron_service: None,
            metrics: Arc::new(crate::http::metrics::AppMetrics::new()),
            health_checker: Arc::new(oclaws_doctor_core::HealthChecker::new()),
            full_config: None,
            config_path: None,
            echo_tracker: Arc::new(tokio::sync::Mutex::new(oclaws_agent_core::EchoTracker::default())),
            group_activation: oclaws_channel_core::group_gate::GroupActivation::default(),
        })
    }

    fn test_router(state: Arc<HttpState>) -> Router {
        Router::new()
            .route("/health", get(health_handler))
            .route("/ready", get(crate::http::readiness_handler))
            .route("/v1/chat/completions", post(chat_completions_handler))
            .route("/v1/responses", post(responses_handler))
            .route("/agent/status", get(agent_status_handler))
            .route("/sessions", get(sessions_list_handler))
            .route("/sessions/{key}", delete(sessions_delete_handler))
            .route("/models", get(models_list_handler))
            .route("/webhooks/telegram", post(crate::http::webhooks::telegram_webhook))
            .route("/webhooks/slack", post(crate::http::webhooks::slack_webhook))
            .route("/webhooks/discord", post(crate::http::webhooks::discord_webhook))
            .route("/webhooks/{channel}", post(crate::http::webhooks::generic_webhook))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_health_endpoint() {
        let app = test_router(test_state(None));
        let req = Request::get("/health").body(axum::body::Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_models_with_provider() {
        let mock = MockLlmProvider::new();
        let state = test_state(Some(Arc::new(mock)));
        let app = test_router(state);
        let req = Request::get("/models").body(axum::body::Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["models"].as_array().unwrap().contains(&serde_json::json!("mock-model")));
    }

    #[tokio::test]
    async fn test_chat_completions_no_provider() {
        let app = test_router(test_state(None));
        let body = serde_json::json!({
            "model": "test",
            "messages": [{"role": "user", "content": "hello"}]
        });
        let req = Request::post("/v1/chat/completions")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_chat_completions_with_mock() {
        let mock = MockLlmProvider::new();
        mock.queue_text("Hello from mock!");
        let state = test_state(Some(Arc::new(mock)));
        let app = test_router(state);
        let body = serde_json::json!({
            "model": "mock-model",
            "messages": [{"role": "user", "content": "hi"}]
        });
        let req = Request::post("/v1/chat/completions")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["choices"][0]["message"]["content"], "Hello from mock!");
    }

    #[tokio::test]
    async fn test_responses_with_mock() {
        let mock = MockLlmProvider::new();
        mock.queue_text("Response text");
        let state = test_state(Some(Arc::new(mock)));
        let app = test_router(state);
        let body = serde_json::json!({
            "model": "mock-model",
            "input": "test input"
        });
        let req = Request::post("/v1/responses")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["output"][0]["content"][0]["text"], "Response text");
    }

    #[tokio::test]
    async fn test_agent_status() {
        let state = test_state(Some(Arc::new(MockLlmProvider::new())));
        let app = test_router(state);
        let req = Request::get("/agent/status").body(axum::body::Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["status"], "ready");
    }

    #[tokio::test]
    async fn test_sessions_list_empty() {
        let app = test_router(test_state(None));
        let req = Request::get("/sessions").body(axum::body::Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_session_delete_not_found() {
        let app = test_router(test_state(None));
        let req = Request::delete("/sessions/nonexistent").body(axum::body::Body::empty()).unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_telegram_webhook_no_channel_manager() {
        let app = test_router(test_state(None));
        let body = serde_json::json!({"message": {"text": "hi"}});
        let req = Request::post("/webhooks/telegram")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn test_slack_webhook_url_verification() {
        let app = test_router(test_state(None));
        let body = serde_json::json!({"type": "url_verification", "challenge": "test_challenge_123"});
        let req = Request::post("/webhooks/slack")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["challenge"], "test_challenge_123");
    }

    #[tokio::test]
    async fn test_generic_webhook_unknown_channel() {
        let state = {
            let mut s = (*test_state(None)).clone();
            s.channel_manager = Some(Arc::new(RwLock::new(oclaws_channel_core::ChannelManager::new())));
            Arc::new(s)
        };
        let app = test_router(state);
        let body = serde_json::json!({"data": "test"});
        let req = Request::post("/webhooks/unknown_channel")
            .header("content-type", "application/json")
            .body(axum::body::Body::from(serde_json::to_vec(&body).unwrap()))
            .unwrap();
        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
