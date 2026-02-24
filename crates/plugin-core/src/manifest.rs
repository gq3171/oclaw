use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub license: Option<String>,
    pub entry_point: String,
    pub dependencies: HashMap<String, String>,
    #[serde(default)]
    pub optional_dependencies: HashMap<String, String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub hooks: Vec<HookDefinition>,
    pub platform: Option<PlatformRequirements>,
    #[serde(default)]
    pub builtin: bool,
    /// Plugin kind — e.g. "memory" for memory-slot plugins.
    pub kind: Option<String>,
    /// JSON Schema for plugin config validation.
    pub config_schema: Option<serde_json::Value>,
    /// UI hints for config fields.
    #[serde(default)]
    pub ui_hints: HashMap<String, UiHint>,
    /// Channel IDs this plugin provides.
    #[serde(default)]
    pub channels: Vec<String>,
    /// Provider IDs this plugin provides.
    #[serde(default)]
    pub providers: Vec<String>,
    /// Skill directories this plugin provides.
    #[serde(default)]
    pub skills: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UiHint {
    pub label: Option<String>,
    pub help: Option<String>,
    #[serde(default)]
    pub advanced: bool,
    #[serde(default)]
    pub sensitive: bool,
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
