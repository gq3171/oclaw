use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    pub entry_point: String,
    #[serde(default)]
    pub dependencies: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub optional_dependencies: HashMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub capabilities: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hooks: Vec<HookDefinition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<PlatformRequirements>,
    #[serde(default)]
    pub builtin: bool,
    /// Plugin kind: "memory" | "channel" | "provider" | "tool".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// JSON Schema for plugin configuration validation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_schema: Option<serde_json::Value>,
    /// UI hints for config fields (label, help, sensitive, placeholder).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ui_hints: Option<HashMap<String, UiHint>>,
    /// Channels this plugin supports (informational).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub channels: Vec<String>,
    /// Providers this plugin supports (informational).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub providers: Vec<String>,
    /// Skill directories this plugin provides.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
}

/// UI rendering hints for a plugin configuration field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiHint {
    /// Display label for the field.
    pub label: Option<String>,
    /// Help text shown below the field.
    pub help: Option<String>,
    /// Whether the field contains sensitive data (mask in UI).
    #[serde(default)]
    pub sensitive: bool,
    /// Placeholder text for empty fields.
    pub placeholder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookDefinition {
    pub name: String,
    pub description: Option<String>,
    pub input_type: Option<String>,
    pub output_type: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformRequirements {
    pub os: Option<Vec<String>>,
    pub arch: Option<Vec<String>>,
    pub memory_min_mb: Option<u64>,
    pub disk_min_mb: Option<u64>,
}

impl PluginManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() {
            return Err("Plugin ID cannot be empty".to_string());
        }
        if self.name.is_empty() {
            return Err("Plugin name cannot be empty".to_string());
        }
        if self.version.is_empty() {
            return Err("Plugin version cannot be empty".to_string());
        }
        if self.entry_point.is_empty() {
            return Err("Plugin entry point cannot be empty".to_string());
        }
        Ok(())
    }

    pub fn check_version_compatibility(&self, required_version: &str) -> bool {
        match_version(required_version, &self.version)
    }
}

fn match_version(required: &str, actual: &str) -> bool {
    let required_parts: Vec<&str> = required.split('.').collect();
    let actual_parts: Vec<&str> = actual.split('.').collect();

    if required_parts.is_empty() || actual_parts.is_empty() {
        return false;
    }

    let min_len = required_parts.len().min(actual_parts.len());

    for i in 0..min_len {
        let req: u32 = required_parts[i].parse().unwrap_or(0);
        let act: u32 = actual_parts[i].parse().unwrap_or(0);

        if act < req {
            return false;
        }
    }

    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginMetadata {
    pub manifest: PluginManifest,
    pub loaded_at: chrono::DateTime<chrono::Utc>,
    pub file_path: Option<String>,
}

impl PluginMetadata {
    pub fn new(manifest: PluginManifest) -> Self {
        Self {
            manifest,
            loaded_at: chrono::Utc::now(),
            file_path: None,
        }
    }

    pub fn with_path(mut self, path: &str) -> Self {
        self.file_path = Some(path.to_string());
        self
    }
}
