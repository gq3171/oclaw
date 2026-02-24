//! Workspace skill loader — ties discovery + gates into a single entry point.
//!
//! Skill directory precedence: ./skills (workspace) > ~/.oclaw/skills (user) > bundled.
//! Each skill directory contains subdirectories with a SKILL.md manifest.

use crate::discovery::{discover_skills, DiscoveredSkill, SkillTier};
use crate::gates::{check_gates, GateResult};
use std::path::Path;

/// A skill that passed gate checks and is ready to use.
#[derive(Debug, Clone)]
pub struct ResolvedSkill {
    pub skill: DiscoveredSkill,
    pub gate_result: GateResult,
}

/// High-level loader: discover → gate-check → return eligible skills.
pub struct WorkspaceSkillLoader {
    workspace: Option<std::path::PathBuf>,
    config_lookup: Box<dyn Fn(&str) -> bool + Send + Sync>,
}

impl WorkspaceSkillLoader {
    pub fn new(workspace: Option<&Path>) -> Self {
        Self {
            workspace: workspace.map(|p| p.to_path_buf()),
            config_lookup: Box::new(|_| false),
        }
    }

    pub fn with_config_lookup<F>(mut self, f: F) -> Self
    where
        F: Fn(&str) -> bool + Send + Sync + 'static,
    {
        self.config_lookup = Box::new(f);
        self
    }

    /// Discover all skills (no gate filtering).
    pub async fn discover_all(&self) -> Vec<DiscoveredSkill> {
        discover_skills(self.workspace.as_deref()).await
    }

    /// Discover and return only skills that pass all gates.
    pub async fn load_eligible(&self) -> Vec<ResolvedSkill> {
        let all = self.discover_all().await;
        all.into_iter()
            .filter_map(|skill| {
                let gate_result = check_gates(&skill.manifest, &self.config_lookup);
                if gate_result.passed {
                    Some(ResolvedSkill { skill, gate_result })
                } else {
                    None
                }
            })
            .collect()
    }

    /// Discover all skills with gate results (including failures).
    pub async fn load_all_with_gates(&self) -> Vec<ResolvedSkill> {
        let all = self.discover_all().await;
        all.into_iter()
            .map(|skill| {
                let gate_result = check_gates(&skill.manifest, &self.config_lookup);
                ResolvedSkill { skill, gate_result }
            })
            .collect()
    }

    /// Find a specific skill by name (highest priority tier wins).
    pub async fn find(&self, name: &str) -> Option<ResolvedSkill> {
        let all = self.load_all_with_gates().await;
        all.into_iter().find(|s| s.skill.manifest.name == name)
    }
}
