use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SubagentStatus {
    Pending,
    Initializing,
    Ready,
    Running,
    Completed,
    Failed,
    Terminated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubagentConfig {
    pub name: String,
    pub description: Option<String>,
    pub system_prompt: String,
    pub model: String,
    pub provider: String,
    pub max_iterations: Option<i32>,
    pub timeout_seconds: Option<i64>,
    pub capabilities: Vec<String>,
}

impl SubagentConfig {
    pub fn new(name: &str, system_prompt: &str, model: &str, provider: &str) -> Self {
        Self {
            name: name.to_string(),
            description: None,
            system_prompt: system_prompt.to_string(),
            model: model.to_string(),
            provider: provider.to_string(),
            max_iterations: Some(10),
            timeout_seconds: Some(300),
            capabilities: Vec::new(),
        }
    }

    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = Some(desc.to_string());
        self
    }

    pub fn with_capabilities(mut self, caps: Vec<&str>) -> Self {
        self.capabilities = caps.into_iter().map(|s| s.to_string()).collect();
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Subagent {
    pub id: String,
    pub config: SubagentConfig,
    pub status: SubagentStatus,
    pub parent_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub result: Option<String>,
    pub error: Option<String>,
    pub metadata: HashMap<String, String>,
}

impl Subagent {
    pub fn new(config: SubagentConfig) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            config,
            status: SubagentStatus::Pending,
            parent_id: None,
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            result: None,
            error: None,
            metadata: HashMap::new(),
        }
    }

    pub fn with_parent(mut self, parent_id: &str) -> Self {
        self.parent_id = Some(parent_id.to_string());
        self
    }

    pub fn is_completed(&self) -> bool {
        matches!(
            self.status,
            SubagentStatus::Completed | SubagentStatus::Failed | SubagentStatus::Terminated
        )
    }

    pub fn set_running(&mut self) {
        self.status = SubagentStatus::Running;
        self.started_at = Some(Utc::now());
    }

    pub fn set_completed(&mut self, result: &str) {
        self.status = SubagentStatus::Completed;
        self.completed_at = Some(Utc::now());
        self.result = Some(result.to_string());
    }

    pub fn set_failed(&mut self, error: &str) {
        self.status = SubagentStatus::Failed;
        self.completed_at = Some(Utc::now());
        self.error = Some(error.to_string());
    }

    pub fn terminate(&mut self) {
        self.status = SubagentStatus::Terminated;
        self.completed_at = Some(Utc::now());
    }
}

pub struct SubagentRegistry {
    agents: Arc<RwLock<HashMap<String, Subagent>>>,
}

impl SubagentRegistry {
    pub fn new() -> Self {
        Self {
            agents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register(&self, agent: Subagent) -> String {
        let id = agent.id.clone();
        self.agents.write().await.insert(id.clone(), agent);
        id
    }

    pub async fn get(&self, id: &str) -> Option<Subagent> {
        self.agents.read().await.get(id).cloned()
    }

    pub async fn update(&self, agent: Subagent) {
        self.agents.write().await.insert(agent.id.clone(), agent);
    }

    pub async fn remove(&self, id: &str) {
        self.agents.write().await.remove(id);
    }

    pub async fn list(&self) -> Vec<Subagent> {
        self.agents.read().await.values().cloned().collect()
    }

    pub async fn list_by_parent(&self, parent_id: &str) -> Vec<Subagent> {
        self.agents
            .read()
            .await
            .values()
            .filter(|a| a.parent_id.as_deref() == Some(parent_id))
            .cloned()
            .collect()
    }

    pub async fn list_by_status(&self, status: SubagentStatus) -> Vec<Subagent> {
        self.agents
            .read()
            .await
            .values()
            .filter(|a| a.status == status)
            .cloned()
            .collect()
    }

    pub async fn count(&self) -> usize {
        self.agents.read().await.len()
    }

    pub async fn clear_completed(&self) {
        let mut agents = self.agents.write().await;
        agents.retain(|_, a| !a.is_completed());
    }
}

impl Default for SubagentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
