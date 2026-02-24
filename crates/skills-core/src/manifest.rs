/// SKILL.md frontmatter parsing — follows Node openclaw's frontmatter.ts pattern.
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SkillManifest {
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub user_invocable: Option<bool>,
    #[serde(default)]
    pub disable_model_invocation: Option<bool>,
    #[serde(default)]
    pub command_dispatch: Option<String>,
    #[serde(default)]
    pub command_tool: Option<String>,
    #[serde(default)]
    pub homepage: Option<String>,
    #[serde(default)]
    pub metadata: Option<ManifestMetadata>,
    /// The body (instructions) after the frontmatter
    #[serde(skip)]
    pub instructions: String,
    /// Source directory
    #[serde(skip)]
    pub source_dir: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ManifestMetadata {
    pub openclaw: Option<OpenClawMeta>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawMeta {
    pub requires: Option<RequiresSpec>,
    #[serde(default)]
    pub install: Vec<InstallSpec>,
    pub primary_env: Option<String>,
    #[serde(default)]
    pub os: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RequiresSpec {
    #[serde(default)]
    pub bins: Vec<String>,
    #[serde(default)]
    pub env: Vec<String>,
    #[serde(default)]
    pub config: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallSpec {
    pub kind: String, // brew, node, go, uv, download
    pub formula: Option<String>,
    pub package: Option<String>,
    pub module: Option<String>,
    pub url: Option<String>,
    pub archive: Option<String>,
    pub target_dir: Option<String>,
}

/// Parse a SKILL.md file: YAML frontmatter between `---` delimiters + body.
pub fn parse_skill_md(content: &str, source_dir: &str) -> Option<SkillManifest> {
    let trimmed = content.trim_start();
    if !trimmed.starts_with("---") {
        return None;
    }
    let after_first = &trimmed[3..];
    let end = after_first.find("---")?;
    let yaml_str = &after_first[..end];
    let body = after_first[end + 3..].trim().to_string();

    let mut manifest: SkillManifest = serde_yaml::from_str(yaml_str).ok()?;
    manifest.instructions = body;
    manifest.source_dir = source_dir.to_string();
    Some(manifest)
}

/// Load a SKILL.md from a file path.
pub async fn load_skill_md(path: &Path) -> Option<SkillManifest> {
    let content = tokio::fs::read_to_string(path).await.ok()?;
    let dir = path.parent()?.to_string_lossy().to_string();
    parse_skill_md(&content, &dir)
}
