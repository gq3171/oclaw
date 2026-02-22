use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::skill::{Skill, SkillDefinition, SkillInput, SkillOutput};

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
        by_cat.entry(def.category.clone())
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
        skills.values().find(|s| s.definition().name == name).cloned()
    }

    pub async fn list(&self) -> Vec<SkillDefinition> {
        self.skills.read()
            .await
            .values()
            .map(|s| s.definition().clone())
            .collect()
    }

    pub async fn list_by_category(&self, category: &str) -> Vec<SkillDefinition> {
        let by_cat = self.by_category.read().await;
        let skills = self.skills.read().await;
        
        by_cat.get(category)
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
        
        by_tag.get(tag)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| skills.get(id).map(|s| s.definition().clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub async fn search(&self, query: &str) -> Vec<SkillDefinition> {
        let query_lower = query.to_lowercase();
        
        self.skills.read()
            .await
            .values()
            .filter(|s| {
                let def = s.definition();
                def.name.to_lowercase().contains(&query_lower)
                    || def.description.to_lowercase().contains(&query_lower)
                    || def.tags.iter().any(|t| t.to_lowercase().contains(&query_lower))
            })
            .map(|s| s.definition().clone())
            .collect()
    }

    pub async fn execute(&self, id: &str, input: SkillInput) -> Option<SkillOutput> {
        self.get(id).await.map(|skill| {
            let runner = crate::skill::SkillRunner::new();
            futures::executor::block_on(runner.run(skill.as_ref(), input)).ok()
        }).flatten()
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
