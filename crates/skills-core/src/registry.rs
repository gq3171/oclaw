use crate::skill::{Skill, SkillDefinition, SkillInput, SkillOutput};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

pub struct SkillRegistry {
    skills: Arc<RwLock<HashMap<String, Arc<dyn Skill>>>>,
    by_category: Arc<RwLock<HashMap<String, Vec<String>>>>,
    by_tag: Arc<RwLock<HashMap<String, Vec<String>>>>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        Self {
            skills: Arc::new(RwLock::new(HashMap::new())),
            by_category: Arc::new(RwLock::new(HashMap::new())),
            by_tag: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register(&self, skill: Arc<dyn Skill>) -> String {
        let def = skill.definition().clone();
        let id = def.id.clone();

        self.skills.write().await.insert(id.clone(), skill);

        let mut by_cat = self.by_category.write().await;
        by_cat
            .entry(def.category.clone())
            .or_insert_with(Vec::new)
            .push(id.clone());

        for tag in &def.tags {
            let mut by_t = self.by_tag.write().await;
            by_t.entry(tag.clone())
                .or_insert_with(Vec::new)
                .push(id.clone());
        }

        tracing::info!("Skill registered: {} ({})", def.name, id);
        id
    }

    pub async fn unregister(&self, id: &str) -> Option<Arc<dyn Skill>> {
        if let Some(skill) = self.skills.write().await.remove(id) {
            tracing::info!("Skill unregistered: {}", id);
            Some(skill)
        } else {
            None
        }
    }

    pub async fn get(&self, id: &str) -> Option<Arc<dyn Skill>> {
        self.skills.read().await.get(id).cloned()
    }

    pub async fn get_by_name(&self, name: &str) -> Option<Arc<dyn Skill>> {
        let skills = self.skills.read().await;
        skills
            .values()
            .find(|s| s.definition().name == name)
            .cloned()
    }

    pub async fn list(&self) -> Vec<SkillDefinition> {
        self.skills
            .read()
            .await
            .values()
            .map(|s| s.definition().clone())
            .collect()
    }

    pub async fn list_by_category(&self, category: &str) -> Vec<SkillDefinition> {
        let by_cat = self.by_category.read().await;
        let skills = self.skills.read().await;

        by_cat
            .get(category)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| skills.get(id).map(|s| s.definition().clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub async fn list_by_tag(&self, tag: &str) -> Vec<SkillDefinition> {
        let by_tag = self.by_tag.read().await;
        let skills = self.skills.read().await;

        by_tag
            .get(tag)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| skills.get(id).map(|s| s.definition().clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub async fn search(&self, query: &str) -> Vec<SkillDefinition> {
        let query_lower = query.to_lowercase();

        self.skills
            .read()
            .await
            .values()
            .filter(|s| {
                let def = s.definition();
                def.name.to_lowercase().contains(&query_lower)
                    || def.description.to_lowercase().contains(&query_lower)
                    || def
                        .tags
                        .iter()
                        .any(|t| t.to_lowercase().contains(&query_lower))
            })
            .map(|s| s.definition().clone())
            .collect()
    }

    pub async fn execute(&self, id: &str, input: SkillInput) -> Option<SkillOutput> {
        let skill = self.get(id).await?;
        if let Err(msg) = check_skill_permissions(skill.definition(), &input) {
            return Some(SkillOutput {
                success: false,
                result: None,
                error: Some(msg),
                metadata: HashMap::new(),
                execution_time_ms: 0,
            });
        }
        let runner = crate::skill::SkillRunner::new();
        runner.run(skill.as_ref(), input).await.ok()
    }

    pub async fn count(&self) -> usize {
        self.skills.read().await.len()
    }

    pub async fn categories(&self) -> Vec<String> {
        self.by_category.read().await.keys().cloned().collect()
    }

    pub async fn tags(&self) -> Vec<String> {
        self.by_tag.read().await.keys().cloned().collect()
    }
}

impl Default for SkillRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn check_skill_permissions(def: &SkillDefinition, input: &SkillInput) -> Result<(), String> {
    if def.permissions.is_empty() {
        return Ok(());
    }

    let granted: HashSet<String> = input
        .context
        .as_ref()
        .and_then(|ctx| ctx.metadata.get("permissions"))
        .map(|raw| {
            raw.split(',')
                .map(|p| p.trim().to_string())
                .filter(|p| !p.is_empty())
                .collect()
        })
        .unwrap_or_default();

    if granted.contains("*") {
        return Ok(());
    }

    let missing: Vec<String> = def
        .permissions
        .iter()
        .filter(|p| !granted.contains(p.as_str()))
        .cloned()
        .collect();

    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Permission denied, missing: {}",
            missing.join(", ")
        ))
    }
}
