use crate::manifest::{PluginManifest, PluginMetadata};
use crate::{PluginError, PluginResult};
use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

pub struct PluginLoader {
    plugin_dirs: Vec<PathBuf>,
    builtins: Vec<PathBuf>,
}

impl PluginLoader {
    pub fn new() -> Self {
        let mut plugin_dirs = Vec::new();

        if let Some(data_dir) = dirs::data_local_dir() {
            plugin_dirs.push(data_dir.join("oclaw").join("plugins"));
        }

        if let Ok(cwd) = std::env::current_dir() {
            plugin_dirs.push(cwd.join("plugins"));
        }

        Self {
            plugin_dirs,
            builtins: Vec::new(),
        }
    }

    pub fn add_plugin_dir(&mut self, path: PathBuf) {
        self.plugin_dirs.push(path);
    }

    pub fn add_builtin(&mut self, path: PathBuf) {
        self.builtins.push(path);
    }

    pub fn discover_plugins(&self) -> Vec<PluginMetadata> {
        let mut plugins = Vec::new();

        for dir in &self.plugin_dirs {
            if dir.exists() {
                for entry in WalkDir::new(dir)
                    .max_depth(2)
                    .into_iter()
                    .filter_map(|e| e.ok())
                {
                    if entry.file_type().is_file()
                        && let Some(ext) = entry.path().extension()
                        && ext == "json"
                        && entry.file_name().to_string_lossy().contains("manifest")
                        && let Ok(manifest) = self.load_manifest(entry.path())
                    {
                        let metadata = PluginMetadata::new(manifest)
                            .with_path(entry.path().to_str().unwrap_or(""));
                        plugins.push(metadata);
                    }
                }
            }
        }

        for path in &self.builtins {
            if let Ok(manifest) = self.load_builtin_manifest(path) {
                let metadata = PluginMetadata::new(manifest).with_path(path.to_str().unwrap_or(""));
                plugins.push(metadata);
            }
        }

        plugins
    }

    pub fn load_manifest(&self, path: &Path) -> Result<PluginManifest, String> {
        let content =
            fs::read_to_string(path).map_err(|e| format!("Failed to read manifest: {}", e))?;

        let manifest: PluginManifest = serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse manifest: {}", e))?;

        manifest.validate()?;

        Ok(manifest)
    }

    fn load_builtin_manifest(&self, path: &Path) -> Result<PluginManifest, String> {
        let manifest_path = path.join("manifest.json");

        if manifest_path.exists() {
            self.load_manifest(&manifest_path)
        } else {
            Err("No manifest.json found".to_string())
        }
    }

    pub fn get_plugin_path(&self, plugin_id: &str) -> Option<PathBuf> {
        for dir in &self.plugin_dirs {
            let plugin_path = dir.join(plugin_id);
            if plugin_path.exists() {
                return Some(plugin_path);
            }
        }
        None
    }

    pub fn get_plugin_dirs(&self) -> &[PathBuf] {
        &self.plugin_dirs
    }

    pub fn ensure_plugin_dir(&self, plugin_id: &str) -> PluginResult<PathBuf> {
        for dir in &self.plugin_dirs {
            let plugin_dir = dir.join(plugin_id);
            if !plugin_dir.exists() {
                fs::create_dir_all(&plugin_dir)
                    .map_err(|e| PluginError::LoadError(e.to_string()))?;
            }
            if plugin_dir.exists() {
                return Ok(plugin_dir);
            }
        }

        if let Some(first_dir) = self.plugin_dirs.first() {
            let plugin_dir = first_dir.join(plugin_id);
            fs::create_dir_all(&plugin_dir).map_err(|e| PluginError::LoadError(e.to_string()))?;
            return Ok(plugin_dir);
        }

        Err(PluginError::LoadError(
            "No plugin directories configured".to_string(),
        ))
    }
}

impl Default for PluginLoader {
    fn default() -> Self {
        Self::new()
    }
}

pub fn load_manifest_from_file(path: &str) -> PluginResult<PluginManifest> {
    let content = fs::read_to_string(path)
        .map_err(|e| PluginError::LoadError(format!("Failed to read {}: {}", path, e)))?;

    let manifest: PluginManifest = serde_json::from_str(&content)
        .map_err(|e| PluginError::LoadError(format!("Failed to parse manifest: {}", e)))?;

    manifest.validate().map_err(PluginError::LoadError)?;

    Ok(manifest)
}

pub fn create_default_manifest(plugin_id: &str, name: &str, version: &str) -> PluginManifest {
    PluginManifest {
        id: plugin_id.to_string(),
        name: name.to_string(),
        version: version.to_string(),
        description: None,
        author: None,
        homepage: None,
        repository: None,
        license: None,
        entry_point: "mod.rs".to_string(),
        dependencies: HashMap::new(),
        optional_dependencies: HashMap::new(),
        tags: Vec::new(),
        capabilities: Vec::new(),
        hooks: Vec::new(),
        platform: None,
        builtin: false,
        kind: None,
        config_schema: None,
        ui_hints: None,
        channels: Vec::new(),
        providers: Vec::new(),
        skills: Vec::new(),
    }
}

/// Validate plugin config against the manifest's JSON Schema (required-fields check).
/// Does not require a jsonschema crate — performs basic required-field presence check.
pub fn validate_plugin_config(
    manifest: &crate::manifest::PluginManifest,
    config: &serde_json::Value,
) -> Result<(), String> {
    if let Some(schema) = &manifest.config_schema {
        if let Some(required) = schema.get("required").and_then(|r| r.as_array()) {
            for field in required {
                if let Some(field_name) = field.as_str() {
                    if config.get(field_name).is_none() {
                        return Err(format!(
                            "Plugin '{}' config missing required field: {}",
                            manifest.id, field_name
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}
