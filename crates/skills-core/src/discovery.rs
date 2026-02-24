/// Multi-directory skill discovery — workspace > user > bundled precedence.
use crate::manifest::{load_skill_md, SkillManifest};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Discovered skill with its source tier.
#[derive(Debug, Clone)]
pub struct DiscoveredSkill {
    pub manifest: SkillManifest,
    pub tier: SkillTier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillTier {
    Workspace, // ./skills (highest priority)
    User,      // ~/.oclaw/skills
    Bundled,   // shipped with binary
}

/// Resolve the three skill directories.
pub fn skill_dirs(workspace: Option<&Path>) -> Vec<(PathBuf, SkillTier)> {
    let mut dirs = Vec::new();
    if let Some(ws) = workspace {
        dirs.push((ws.join("skills"), SkillTier::Workspace));
    }
    if let Some(home) = dirs::home_dir() {
        dirs.push((home.join(".oclaw").join("skills"), SkillTier::User));
    }
    // Bundled: next to executable
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push((parent.join("skills"), SkillTier::Bundled));
        }
    }
    dirs
}

/// Scan a single directory for SKILL.md files (one level of subdirs).
async fn scan_dir(dir: &Path) -> Vec<SkillManifest> {
    let mut results = Vec::new();
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else { return results };
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.is_dir() {
            let skill_md = path.join("SKILL.md");
            if let Some(m) = load_skill_md(&skill_md).await {
                results.push(m);
            }
        }
    }
    results
}

/// Discover all skills with precedence: workspace > user > bundled.
/// Later tiers don't override earlier ones (by name).
pub async fn discover_skills(workspace: Option<&Path>) -> Vec<DiscoveredSkill> {
    let mut seen: HashMap<String, DiscoveredSkill> = HashMap::new();
    for (dir, tier) in skill_dirs(workspace) {
        for manifest in scan_dir(&dir).await {
            let name = manifest.name.clone();
            // First seen wins (higher priority tier)
            seen.entry(name).or_insert(DiscoveredSkill { manifest, tier });
        }
    }
    seen.into_values().collect()
}
