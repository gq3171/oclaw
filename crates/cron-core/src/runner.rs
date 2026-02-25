use crate::types::{CronDelivery, CronJob};
use serde::{Deserialize, Serialize};

/// Result of delivering cron output to a single channel target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeliveryResult {
    pub channel: String,
    pub target: String,
    pub success: bool,
    pub error: Option<String>,
}

/// Result of a complete cron job execution (agent turn + deliveries).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronRunResult {
    pub job_id: String,
    pub output: String,
    pub success: bool,
    pub duration_ms: u64,
    pub deliveries: Vec<DeliveryResult>,
}

/// Trait for executing cron job payloads. Implemented in gateway-core
/// to bridge agent-core and channel-core without cron-core depending on them.
#[async_trait::async_trait]
pub trait CronExecutor: Send + Sync {
    /// Execute an AgentTurn payload, returning the agent's reply text.
    async fn run_agent_turn(
        &self,
        job: &CronJob,
        message: &str,
        model: Option<&str>,
        timeout_secs: Option<u64>,
    ) -> Result<String, String>;

    /// Deliver content to a specific channel target.
    async fn deliver(
        &self,
        delivery: &CronDelivery,
        content: &str,
    ) -> Result<(), String>;
}
