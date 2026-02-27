use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CronScheduleKind {
    At {
        at: String,
    },
    Every {
        every_ms: u64,
        anchor_ms: Option<u64>,
    },
    Cron {
        expr: String,
        tz: Option<String>,
    },
}

/// Delivery target for cron job output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronDelivery {
    pub channel: String,
    pub target: String,
    pub target_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CronPayloadKind {
    SystemEvent {
        text: String,
    },
    AgentTurn {
        message: String,
        model: Option<String>,
        timeout_secs: Option<u64>,
        channel: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CronJobState {
    pub next_run_at_ms: Option<u64>,
    pub last_run_at_ms: Option<u64>,
    pub last_status: Option<String>,
    pub last_error: Option<String>,
    pub consecutive_errors: u32,
    #[serde(default)]
    pub running_since_ms: Option<u64>,
    #[serde(default)]
    pub total_runs: u64,
    #[serde(default)]
    pub total_errors: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    pub schedule: CronScheduleKind,
    pub payload: CronPayloadKind,
    pub session_target: String,
    pub state: CronJobState,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub delete_after_run: bool,
    #[serde(default)]
    pub delivery: Vec<CronDelivery>,
    #[serde(default)]
    pub max_retries: Option<u32>,
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}
