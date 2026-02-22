use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdpCommand {
    pub id: i32,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdpResponse {
    pub id: i32,
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<CdpError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdpError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CdpEvent {
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CdpMessage {
    Response(CdpResponse),
    Event(CdpEvent),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CdpDomain {
    Browser,
    Target,
    Page,
    Network,
    Runtime,
    Console,
    DOM,
    DOMSnapshot,
    CSS,
    Input,
    Security,
    ServiceWorker,
    Storage,
    Performance,
    Log,
    Audits,
    Accessibility,
}

impl CdpDomain {
    pub fn as_str(&self) -> &'static str {
        match self {
            CdpDomain::Browser => "Browser",
            CdpDomain::Target => "Target",
            CdpDomain::Page => "Page",
            CdpDomain::Network => "Network",
            CdpDomain::Runtime => "Runtime",
            CdpDomain::Console => "Console",
            CdpDomain::DOM => "DOM",
            CdpDomain::DOMSnapshot => "DOMSnapshot",
            CdpDomain::CSS => "CSS",
            CdpDomain::Input => "Input",
            CdpDomain::Security => "Security",
            CdpDomain::ServiceWorker => "ServiceWorker",
            CdpDomain::Storage => "Storage",
            CdpDomain::Performance => "Performance",
            CdpDomain::Log => "Log",
            CdpDomain::Audits => "Audits",
            CdpDomain::Accessibility => "Accessibility",
        }
    }
}

pub fn build_method(domain: CdpDomain, method: &str) -> String {
    format!("{}.{}", domain.as_str(), method)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TargetInfo {
    pub target_id: String,
    #[serde(rename = "type")]
    pub target_type: String,
    pub title: String,
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub browser_context_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageNavigateParams {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageNavigateResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub frame_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_text: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeEvaluateParams {
    pub expression: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_group: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_command_line_api: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub silent: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_by_value: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generate_preview: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteObject {
    #[serde(rename = "type")]
    pub object_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unserializable_value: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub object_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeEvaluateResult {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<RemoteObject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exception_details: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkRequestWillBeSentParams {
    pub request_id: String,
    #[serde(rename = "type")]
    pub request_type: String,
    pub timestamp: f64,
    pub wall_time: f64,
    pub initiator: Option<serde_json::Value>,
    pub redirect_response: Option<serde_json::Value>,
    pub request: NetworkRequest,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkRequest {
    pub url: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub post_data: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageLoadEventParams {
    pub id: String,
    pub timestamp: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub type_: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsoleAPICalledParams {
    #[serde(rename = "type")]
    pub console_type: String,
    pub args: Vec<RemoteObject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_context_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<f64>,
}
