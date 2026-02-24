use crate::types::{CronJob, CronPayloadKind};

pub struct CronRunner;

pub struct CronRunResult {
    pub job_id: String,
    pub output: String,
    pub success: bool,
    pub duration_ms: u64,
}

impl CronRunner {
    /// Execute a cron job's payload and return the result.
    pub async fn run(job: &CronJob) -> CronRunResult {
        let start = std::time::Instant::now();

        let (output, success) = match &job.payload {
            CronPayloadKind::SystemEvent { text } => {
                tracing::info!("Cron system event [{}]: {}", job.id, text);
                (text.clone(), true)
            }
            CronPayloadKind::AgentTurn { message, .. } => {
                // In a full integration, this would create an isolated
                // Agent, run a turn, and return the response.
                // For now, log and return a placeholder.
                tracing::info!(
                    "Cron agent turn [{}]: {}",
                    job.id,
                    message
                );
                (format!("Agent turn scheduled: {}", message), true)
            }
        };

        CronRunResult {
            job_id: job.id.clone(),
            output,
            success,
            duration_ms: start.elapsed().as_millis() as u64,
        }
    }
}
