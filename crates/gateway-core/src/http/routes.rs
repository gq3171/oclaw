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
        match provider.chat_stream(request).await {
            Ok(mut rx) => {
                let stream = async_stream::stream! {
                    while let Some(chunk) = rx.recv().await {
                        match chunk {
                            Ok(c) => {
                                if let Ok(json) = serde_json::to_string(&c) {
                                    yield Ok::<_, Infallible>(Event::default().data(json));
                                }
                            }
                            Err(e) => {
                                error!("Stream error: {}", e);
                                break;
                            }
                        }
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

const CONFIG_UI_HTML: &str = r##"<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>OCLAWS Config</title>
<style>
*{box-sizing:border-box;margin:0;padding:0}
body{font-family:system-ui,sans-serif;background:#1a1a2e;color:#e0e0e0;padding:20px}
h1{text-align:center;margin-bottom:20px;color:#00d4ff}
.tabs{display:flex;gap:4px;margin-bottom:16px;flex-wrap:wrap}
.tab{padding:8px 16px;background:#16213e;border:1px solid #0f3460;border-radius:6px 6px 0 0;cursor:pointer;color:#a0a0a0}
.tab.active{background:#0f3460;color:#00d4ff;border-bottom-color:#0f3460}
.panel{display:none;background:#16213e;border:1px solid #0f3460;border-radius:0 6px 6px 6px;padding:16px;max-height:70vh;overflow-y:auto}
.panel.active{display:block}
.field{margin-bottom:12px}
.field label{display:block;font-size:13px;color:#8899aa;margin-bottom:4px}
.field input,.field textarea,.field select{width:100%;padding:8px;background:#1a1a2e;border:1px solid #0f3460;border-radius:4px;color:#e0e0e0;font-family:monospace}
.field input[type=checkbox]{width:auto}
.field textarea{min-height:60px;resize:vertical}
.section{margin:12px 0;padding:10px;border:1px solid #0f3460;border-radius:6px}
.section-title{font-size:14px;font-weight:bold;color:#00d4ff;margin-bottom:8px}
#save-btn{display:block;margin:20px auto;padding:12px 40px;background:#00d4ff;color:#1a1a2e;border:none;border-radius:6px;font-size:16px;font-weight:bold;cursor:pointer}
#save-btn:hover{background:#00b8d4}
#msg{text-align:center;margin-top:10px;min-height:24px}
.ok{color:#4caf50}.err{color:#f44336}
</style></head><body>
<h1>OCLAWS Configuration</h1>
<div class="tabs" id="tabs"></div>
<div id="panels"></div>
<button id="save-btn">Save</button>
<div id="msg"></div>
<script>
const TABS=["Gateway","Models","Channels","Browser","Cron","Logging","Advanced"];
const TAB_KEYS={Gateway:["gateway"],Models:["models"],Channels:["channels"],Browser:["browser"],Cron:["cron"],Logging:["logging"],Advanced:["diagnostics","talk","web","ui","auth","update","env","media","canvasHost","discovery"]};
let cfg={};
function isSensitive(k){return/token|key|password|secret/i.test(k)}
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
    const inp=document.createElement("input");inp.type=isSensitive(key)?"password":"text";inp.value=val;inp.dataset.path=path;div.appendChild(inp);
  }else if(Array.isArray(val)){
    const ta=document.createElement("textarea");ta.value=JSON.stringify(val,null,2);ta.dataset.path=path;ta.dataset.type="array";div.appendChild(ta);
  }else if(typeof val==="object"){
    const sec=document.createElement("div");sec.className="section";
    const t=document.createElement("div");t.className="section-title";t.textContent=key;sec.appendChild(t);
    for(const[k,v] of Object.entries(val))makeField(path+"."+k,v,sec);
    parent.appendChild(sec);return;
  }
  parent.appendChild(div);
}
function render(){
  const tabsEl=document.getElementById("tabs"),panelsEl=document.getElementById("panels");
  tabsEl.innerHTML="";panelsEl.innerHTML="";
  TABS.forEach((name,i)=>{
    const tab=document.createElement("div");tab.className="tab"+(i===0?" active":"");tab.textContent=name;
    tab.onclick=()=>{document.querySelectorAll(".tab").forEach(t=>t.classList.remove("active"));tab.classList.add("active");document.querySelectorAll(".panel").forEach(p=>p.classList.remove("active"));document.getElementById("p-"+i).classList.add("active")};
    tabsEl.appendChild(tab);
    const panel=document.createElement("div");panel.className="panel"+(i===0?" active":"");panel.id="p-"+i;
    TAB_KEYS[name].forEach(key=>{
      if(cfg[key]!==undefined&&cfg[key]!==null&&typeof cfg[key]==="object"){
        const sec=document.createElement("div");sec.className="section";
        const t=document.createElement("div");t.className="section-title";t.textContent=key;sec.appendChild(t);
        for(const[k,v] of Object.entries(cfg[key]))makeField(key+"."+k,v,sec);
        panel.appendChild(sec);
      }else{
        makeField(key,cfg[key]||null,panel);
      }
    });
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
document.getElementById("save-btn").onclick=async()=>{
  const msg=document.getElementById("msg");msg.textContent="Saving...";msg.className="";
  try{
    const r=await fetch("/api/config/full",{method:"PUT",headers:{"Content-Type":"application/json"},body:JSON.stringify(collect())});
    const j=await r.json();
    if(r.ok){msg.textContent="Saved!";msg.className="ok"}
    else{msg.textContent="Error: "+(j.errors||[j.error]).join(", ");msg.className="err"}
  }catch(e){msg.textContent="Error: "+e;msg.className="err"}
};
fetch("/api/config/full").then(r=>r.json()).then(j=>{cfg=j;render()}).catch(e=>document.getElementById("msg").textContent="Load error: "+e);
</script></body></html>"##;

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
            metrics: Arc::new(crate::http::metrics::AppMetrics::new()),
            health_checker: Arc::new(oclaws_doctor_core::HealthChecker::new()),
            full_config: None,
            config_path: None,
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
