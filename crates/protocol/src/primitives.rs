use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum GatewayClientId {
    WebchatUi,
    ControlUi,
    Webchat,
    #[default]
    Cli,
    GatewayClient,
    MacosApp,
    IosApp,
    AndroidApp,
    NodeHost,
    Test,
    Fingerprint,
    Probe,
}

impl std::fmt::Display for GatewayClientId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WebchatUi => write!(f, "webchat-ui"),
            Self::ControlUi => write!(f, "openclaw-control-ui"),
            Self::Webchat => write!(f, "webchat"),
            Self::Cli => write!(f, "cli"),
            Self::GatewayClient => write!(f, "gateway-client"),
            Self::MacosApp => write!(f, "openclaw-macos"),
            Self::IosApp => write!(f, "openclaw-ios"),
            Self::AndroidApp => write!(f, "openclaw-android"),
            Self::NodeHost => write!(f, "node-host"),
            Self::Test => write!(f, "test"),
            Self::Fingerprint => write!(f, "fingerprint"),
            Self::Probe => write!(f, "openclaw-probe"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum GatewayClientMode {
    Webchat,
    #[default]
    Cli,
    Ui,
    Backend,
    Node,
    Probe,
    Test,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GatewayClientCap {
    ToolEvents,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayClientInfo {
    pub id: GatewayClientId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub version: String,
    pub platform: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub device_family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_identifier: Option<String>,
    pub mode: GatewayClientMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
}

impl Default for GatewayClientInfo {
    fn default() -> Self {
        Self {
            id: GatewayClientId::default(),
            display_name: None,
            version: "0.1.0".to_string(),
            platform: "linux".to_string(),
            device_family: None,
            model_identifier: None,
            mode: GatewayClientMode::default(),
            instance_id: None,
        }
    }
}
