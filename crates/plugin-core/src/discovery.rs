//! Plugin dynamic discovery — scan directories for plugin manifests.

use crate::manifest::PluginManifest;
use std::path::{Path, PathBuf};

pub struct PluginDiscovery {
    search_paths: Vec<PathBuf>,
}

impl PluginDiscovery {
    pub fn new(search_paths: Vec<PathBuf>) -> Self {
        Self { search_paths }
    }

    /// Scan all search paths for plugin manifest files (`plugin.json`).
    pub fn scan(&self) -> Vec<DiscoveredPlugin> {
        let mut found = Vec::new();
        for base in &self.search_paths {
            if !base.is_dir() {
                continue;
            }
            for entry in walkdir::WalkDir::new(base)
                .max_depth(2)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if entry.file_name() == "plugin.json"
                    && let Some(dp) = Self::try_load(entry.path())
                {
                    found.push(dp);
                }
            }
        }
        found
    }

    fn try_load(path: &Path) -> Option<DiscoveredPlugin> {
        let data = std::fs::read_to_string(path).ok()?;
        let manifest: PluginManifest = serde_json::from_str(&data).ok()?;
        Some(DiscoveredPlugin {
            manifest,
            path: path.to_path_buf(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    pub manifest: PluginManifest,
    pub path: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_paths() {
        let d = PluginDiscovery::new(vec![]);
        assert!(d.scan().is_empty());
    }

    #[test]
    fn nonexistent_path() {
        let d = PluginDiscovery::new(vec![PathBuf::from("/nonexistent/path/xyz")]);
        assert!(d.scan().is_empty());
    }
}
