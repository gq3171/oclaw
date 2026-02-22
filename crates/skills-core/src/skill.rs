use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{SkillError, SkillResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInput {
    pub name: String,
    pub description: String,
    pub parameters: HashMap<String, serde_json::Value>,
    pub context: Option<SkillContext>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillOutput {
    pub success: bool,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub metadata: HashMap<String, String>,
    pub execution_time_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillContext {
    pub user_id: Option<String>,
    pub session_id: Option<String>,
    pub request_id: Option<String>,
    pub metadata: HashMap<String, String>,
}

impl Default for SkillContext {
    fn default() -> Self {
        Self {
            user_id: None,
            session_id: None,
            request_id: Some(uuid::Uuid::new_v4().to_string()),
            metadata: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: String,
    pub tags: Vec<String>,
    pub parameters: Vec<SkillParameter>,
    pub returns: SkillReturnType,
    pub examples: Vec<SkillExample>,
    pub permissions: Vec<String>,
    pub rate_limit: Option<RateLimit>,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillParameter {
    pub name: String,
    pub param_type: String,
    pub description: String,
    pub required: bool,
    pub default: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillReturnType {
    pub param_type: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillExample {
    pub input: serde_json::Value,
    pub output: serde_json::Value,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RateLimit {
    pub requests_per_minute: u32,
    pub burst: u32,
}

#[async_trait]
pub trait Skill: Send + Sync {
    fn definition(&self) -> &SkillDefinition;
    
    async fn execute(&self, input: SkillInput) -> SkillResult<SkillOutput>;
    
    async fn validate(&self, input: &SkillInput) -> Result<(), String> {
        Ok(())
    }
    
    async fn on_enabled(&self) -> Result<(), String> {
        Ok(())
    }
    
    async fn on_disabled(&self) -> Result<(), String> {
        Ok(())
    }
}

pub struct SkillRunner {
    timeout_ms: u64,
}

impl SkillRunner {
    pub fn new() -> Self {
        Self {
            timeout_ms: 30000,
        }
    }

    pub fn with_timeout(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    pub async fn run(&self, skill: &dyn Skill, input: SkillInput) -> SkillResult<SkillOutput> {
        let start = std::time::Instant::now();
        
        skill.validate(&input).await
            .map_err(|e| SkillError::ValidationError(e))?;
        
        let result = skill.execute(input).await;
        
        let execution_time = start.elapsed().as_millis() as i64;
        
        match result {
            Ok(mut output) => {
                output.execution_time_ms = execution_time;
                Ok(output)
            }
            Err(e) => {
                Ok(SkillOutput {
                    success: false,
                    result: None,
                    error: Some(e.to_string()),
                    metadata: HashMap::new(),
                    execution_time_ms: execution_time,
                })
            }
        }
    }
}

impl Default for SkillRunner {
    fn default() -> Self {
        Self::new()
    }
}
