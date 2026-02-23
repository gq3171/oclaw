use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use oclaws_llm_core::providers::LlmProvider;
use crate::agent::{Agent, AgentConfig, ToolExecutor};

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

    /// Spawn a subagent: register it, then run it asynchronously in a background task.
    /// Returns the subagent ID immediately. Poll `get()` to check status/result.
    pub async fn spawn(
        &self,
        config: SubagentConfig,
        provider: Arc<dyn LlmProvider>,
        input: String,
        parent_id: Option<&str>,
        tool_executor: Option<Arc<dyn ToolExecutor>>,
    ) -> String {
        let mut sub = Subagent::new(config.clone());
        if let Some(pid) = parent_id {
            sub.parent_id = Some(pid.to_string());
        }
        let id = self.register(sub).await;

        let agents = self.agents.clone();
        let agent_id = id.clone();
        let timeout_secs = config.timeout_seconds.unwrap_or(300) as u64;

        tokio::spawn(async move {
            // Mark running
            if let Some(sa) = agents.write().await.get_mut(&agent_id) {
                sa.set_running();
            }

            let agent_config = AgentConfig::new(&config.name, &config.model, &config.provider)
                .with_system_prompt(&config.system_prompt);
            let mut agent = Agent::new(agent_config, provider);
            if let Err(e) = agent.initialize().await {
                if let Some(sa) = agents.write().await.get_mut(&agent_id) {
                    sa.set_failed(&e.to_string());
                }
                return;
            }

            let run_fut = async {
                match tool_executor {
                    Some(exec) => agent.run_with_tools(&input, exec.as_ref()).await,
                    None => agent.run(&input).await,
                }
            };

            match tokio::time::timeout(
                tokio::time::Duration::from_secs(timeout_secs),
                run_fut,
            ).await {
                Ok(Ok(result)) => {
                    if let Some(sa) = agents.write().await.get_mut(&agent_id) {
                        sa.set_completed(&result);
                    }
                }
                Ok(Err(e)) => {
                    if let Some(sa) = agents.write().await.get_mut(&agent_id) {
                        sa.set_failed(&e.to_string());
                    }
                }
                Err(_) => {
                    if let Some(sa) = agents.write().await.get_mut(&agent_id) {
                        sa.set_failed("Timeout");
                    }
                }
            }
        });

        id
    }

    /// Spawn and wait for the result (blocking until completion or timeout).
    pub async fn spawn_and_wait(
        &self,
        config: SubagentConfig,
        provider: Arc<dyn LlmProvider>,
        input: String,
        parent_id: Option<&str>,
        tool_executor: Option<Arc<dyn ToolExecutor>>,
    ) -> Result<String, String> {
        let id = self.spawn(config, provider, input, parent_id, tool_executor).await;

        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            if let Some(sa) = self.get(&id).await {
                match sa.status {
                    SubagentStatus::Completed => return Ok(sa.result.unwrap_or_default()),
                    SubagentStatus::Failed => return Err(sa.error.unwrap_or_default()),
                    SubagentStatus::Terminated => return Err("Terminated".to_string()),
                    _ => continue,
                }
            } else {
                return Err("Subagent not found".to_string());
            }
        }
    }

    /// Terminate a running subagent by ID.
    pub async fn terminate(&self, id: &str) -> bool {
        if let Some(sa) = self.agents.write().await.get_mut(id)
            && !sa.is_completed()
        {
            sa.terminate();
            return true;
        }
        false
    }
}

impl Default for SubagentRegistry {
    fn default() -> Self {
        Self::new()
    }
}
