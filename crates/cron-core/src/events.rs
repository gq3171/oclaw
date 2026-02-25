/// Cron lifecycle events broadcast via tokio::sync::broadcast.
#[derive(Debug, Clone)]
pub enum CronEvent {
    JobStarted {
        job_id: String,
        timestamp_ms: u64,
    },
    JobCompleted {
        job_id: String,
        duration_ms: u64,
        success: bool,
    },
    JobFailed {
        job_id: String,
        error: String,
        consecutive: u32,
    },
    SchedulerStarted,
    SchedulerStopped,
}

pub type CronEventSender = tokio::sync::broadcast::Sender<CronEvent>;
pub type CronEventReceiver = tokio::sync::broadcast::Receiver<CronEvent>;

pub fn event_channel() -> (CronEventSender, CronEventReceiver) {
    tokio::sync::broadcast::channel(256)
}
