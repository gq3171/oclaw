//! SOUL.md — agent personality, values, and behavioral boundaries.

use std::path::PathBuf;
use crate::files::Workspace;

/// Parsed soul file content.
#[derive(Debug, Clone, Default)]
pub struct Soul {
    pub raw: String,
    pub core_truths: Vec<String>,
    pub boundaries: Vec<String>,
    pub vibe: Option<String>,
    pub continuity: Option<String>,
}

impl Soul {
    /// Load and parse SOUL.md from workspace.
    pub async fn load(ws: &Workspace) -> anyhow::Result<Option<Self>> {
        let Some(content) = ws.read_file(&ws.soul_path()).await? else {
            return Ok(None);
        };
        Ok(Some(Self::parse(&content)))
    }

    /// Parse raw markdown into structured sections.
    pub fn parse(raw: &str) -> Self {
        let mut soul = Soul {
            raw: raw.to_string(),
            ..Default::default()
        };

        let mut current_section = "";
        for line in raw.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("## ") {
                current_section = trimmed.trim_start_matches("## ").trim();
                continue;
            }
            let bullet = trimmed.strip_prefix("- ").or_else(|| trimmed.strip_prefix("* "));
            match current_section.to_lowercase().as_str() {
                s if s.contains("core") || s.contains("truth") => {
                    if let Some(b) = bullet {
                        soul.core_truths.push(b.to_string());
                    }
                }
                s if s.contains("boundar") => {
                    if let Some(b) = bullet {
                        soul.boundaries.push(b.to_string());
                    }
                }
                s if s.contains("vibe") => {
                    if !trimmed.is_empty() {
                        let prev = soul.vibe.get_or_insert_with(String::new);
                        if !prev.is_empty() {
                            prev.push(' ');
                        }
                        prev.push_str(trimmed);
                    }
                }
                s if s.contains("continuity") => {
                    if !trimmed.is_empty() {
                        let prev = soul.continuity.get_or_insert_with(String::new);
                        if !prev.is_empty() {
                            prev.push(' ');
                        }
                        prev.push_str(trimmed);
                    }
                }
                _ => {}
            }
        }
        soul
    }

    /// Save a dated snapshot of SOUL.md to `soul_versions/YYYY-MM-DD.md`.
    ///
    /// Idempotent: if a snapshot already exists for today it is **not**
    /// overwritten and `Ok(None)` is returned.  Returns the path written on
    /// success.
    pub async fn backup(ws: &Workspace) -> anyhow::Result<Option<PathBuf>> {
        let Some(content) = ws.read_file(&ws.soul_path()).await? else {
            return Ok(None);
        };
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let versions_dir = ws.soul_versions_dir();
        tokio::fs::create_dir_all(&versions_dir).await?;
        let path = versions_dir.join(format!("{}.md", today));
        // Already backed up today — skip to preserve the morning snapshot.
        if tokio::fs::metadata(&path).await.is_ok() {
            return Ok(None);
        }
        tokio::fs::write(&path, content).await?;
        Ok(Some(path))
    }

    /// Inject soul guidance into a system prompt fragment.
    pub fn to_prompt_section(&self) -> String {
        if self.raw.trim().is_empty() {
            return String::new();
        }
        format!(
            "## Soul\n\
             Embody the persona and tone defined in SOUL.md. \
             Avoid stiff, generic replies; follow its guidance \
             unless higher-priority instructions override it.\n\n\
             <soul>\n{}\n</soul>\n",
            self.raw.trim()
        )
    }
}
