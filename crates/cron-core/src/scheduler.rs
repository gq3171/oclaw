use crate::backoff::backoff_delay_ms;
use crate::events::{CronEvent, CronEventSender};
use crate::run_log::{RunLog, RunLogEntry};
use crate::runner::{CronExecutor, CronRunResult, DeliveryResult};
use crate::service::CronService;
use crate::types::{CronJob, CronPayloadKind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::task::JoinHandle;

pub struct CronScheduler {
    service: Arc<CronService>,
    executor: Arc<dyn CronExecutor>,
    run_log: Arc<RunLog>,
    events: CronEventSender,
    max_concurrent: usize,
    stuck_threshold_ms: u64,
    running: Arc<AtomicBool>,
}

impl CronScheduler {
    pub fn new(
        service: Arc<CronService>,
        executor: Arc<dyn CronExecutor>,
        run_log: Arc<RunLog>,
        events: CronEventSender,
    ) -> Self {
        Self {
            service,
            executor,
            run_log,
            events,
            max_concurrent: 3,
            stuck_threshold_ms: 2 * 60 * 60 * 1000, // 2 hours
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Execute a single cron job immediately (independent of due state).
    pub async fn run_once(&self, job_id: &str) -> anyhow::Result<()> {
        let job = self
            .service
            .get(job_id)
            .await
            .ok_or_else(|| anyhow::anyhow!("Job not found: {}", job_id))?;
        if !job.enabled {
            anyhow::bail!("Job is disabled: {}", job_id);
        }
        execute_job(
            &self.service,
            &*self.executor,
            &self.run_log,
            &self.events,
            &job,
        )
        .await;
        Ok(())
    }

    /// Spawn the background scheduling loop. Returns a JoinHandle.
    pub fn start(self: Arc<Self>) -> JoinHandle<()> {
        let this = self.clone();
        tokio::spawn(async move { this.run_loop().await })
    }

    async fn run_loop(&self) {
        self.events.send(CronEvent::SchedulerStarted).ok();
        let semaphore = Arc::new(Semaphore::new(self.max_concurrent));

        loop {
            if !self.running.load(Ordering::Relaxed) {
                break;
            }

            let now_ms = chrono::Utc::now().timestamp_millis() as u64;
            let jobs = self.service.list().await;

            self.detect_stuck_jobs(&jobs, now_ms).await;

            for job in jobs.iter().filter(|j| j.enabled && is_due(j, now_ms)) {
                let permit = match semaphore.clone().acquire_owned().await {
                    Ok(p) => p,
                    Err(_) => break,
                };
                let svc = self.service.clone();
                let executor = self.executor.clone();
                let run_log = self.run_log.clone();
                let events = self.events.clone();
                let job = job.clone();
                tokio::spawn(async move {
                    execute_job(&svc, &*executor, &run_log, &events, &job).await;
                    drop(permit);
                });
            }

            tokio::time::sleep(Duration::from_secs(60)).await;
        }

        self.events.send(CronEvent::SchedulerStopped).ok();
    }

    async fn detect_stuck_jobs(&self, jobs: &[CronJob], now_ms: u64) {
        for job in jobs {
            if let Some(since) = job.state.running_since_ms
                && now_ms.saturating_sub(since) > self.stuck_threshold_ms
            {
                tracing::warn!(
                    "Cron job '{}' ({}) appears stuck (running since {}ms ago), clearing running state",
                    job.name,
                    job.id,
                    now_ms - since
                );
                // Clear the stuck state so it can be rescheduled
                let result = CronRunResult {
                    job_id: job.id.clone(),
                    output: "Job timed out (stuck detection)".to_string(),
                    success: false,
                    duration_ms: now_ms - since,
                    deliveries: vec![],
                };
                if let Err(e) = self.service.update_after_run(&job.id, &result).await {
                    tracing::error!("Failed to clear stuck job {}: {}", job.id, e);
                }
                self.events
                    .send(CronEvent::JobFailed {
                        job_id: job.id.clone(),
                        error: "stuck detection timeout".to_string(),
                        consecutive: job.state.consecutive_errors + 1,
                    })
                    .ok();
            }
        }
    }
}

fn is_due(job: &CronJob, now_ms: u64) -> bool {
    // Skip jobs already running
    if job.state.running_since_ms.is_some() {
        return false;
    }
    // Check backoff: if consecutive errors, delay the next run
    if job.state.consecutive_errors > 0 {
        let max_retries = job.max_retries.unwrap_or(5);
        if job.state.consecutive_errors >= max_retries {
            return false; // exhausted retries
        }
        if let Some(last_run) = job.state.last_run_at_ms {
            let delay = backoff_delay_ms(job.state.consecutive_errors);
            if now_ms < last_run + delay {
                return false; // still in backoff window
            }
        }
    }
    match job.state.next_run_at_ms {
        Some(next) => now_ms >= next,
        None => false,
    }
}

async fn execute_job(
    service: &CronService,
    executor: &dyn CronExecutor,
    run_log: &RunLog,
    events: &CronEventSender,
    job: &CronJob,
) {
    let now_ms = chrono::Utc::now().timestamp_millis() as u64;
    let start = std::time::Instant::now();

    // 1. Mark running
    if let Err(e) = service.mark_running(&job.id, now_ms).await {
        tracing::error!("Failed to mark job {} as running: {}", job.id, e);
        return;
    }
    events
        .send(CronEvent::JobStarted {
            job_id: job.id.clone(),
            timestamp_ms: now_ms,
        })
        .ok();

    // 2. Execute payload
    let timeout = Duration::from_secs(job.timeout_secs.unwrap_or(300));
    let exec_result = match &job.payload {
        CronPayloadKind::SystemEvent { text } => {
            tracing::info!("Cron system event [{}]: {}", job.id, text);
            Ok(text.clone())
        }
        CronPayloadKind::AgentTurn {
            message,
            model,
            timeout_secs,
            ..
        } => {
            let fut = executor.run_agent_turn(job, message, model.as_deref(), *timeout_secs);
            match tokio::time::timeout(timeout, fut).await {
                Ok(r) => r,
                Err(_) => Err("execution timed out".to_string()),
            }
        }
    };

    let duration_ms = start.elapsed().as_millis() as u64;
    let (output, success) = match exec_result {
        Ok(text) => (text, true),
        Err(e) => (e, false),
    };

    // 3. Deliver to all targets
    let mut deliveries = Vec::new();
    if success {
        for d in &job.delivery {
            let dr = match executor.deliver(d, &output).await {
                Ok(()) => DeliveryResult {
                    channel: d.channel.clone(),
                    target: d.target.clone(),
                    success: true,
                    error: None,
                },
                Err(e) => DeliveryResult {
                    channel: d.channel.clone(),
                    target: d.target.clone(),
                    success: false,
                    error: Some(e),
                },
            };
            deliveries.push(dr);
        }
    }

    let result = CronRunResult {
        job_id: job.id.clone(),
        output: output.clone(),
        success,
        duration_ms,
        deliveries: deliveries.clone(),
    };

    // 4. Write run log
    let log_entry = RunLogEntry {
        timestamp_ms: chrono::Utc::now().timestamp_millis() as u64,
        status: if success { "ok" } else { "error" }.to_string(),
        duration_ms,
        output_preview: RunLog::truncate_preview(&output),
        error: if success { None } else { Some(output.clone()) },
        deliveries,
    };
    if let Err(e) = run_log.append(&job.id, &log_entry).await {
        tracing::error!("Failed to write run log for {}: {}", job.id, e);
    }

    // 5. Update job state
    if let Err(e) = service.update_after_run(&job.id, &result).await {
        tracing::error!("Failed to update job {} after run: {}", job.id, e);
    }

    // 6. Delete if one-shot
    if job.delete_after_run
        && let Err(e) = service.remove(&job.id).await
    {
        tracing::error!("Failed to delete one-shot job {}: {}", job.id, e);
    }

    // 7. Broadcast event
    if success {
        events
            .send(CronEvent::JobCompleted {
                job_id: job.id.clone(),
                duration_ms,
                success: true,
            })
            .ok();
    } else {
        // Re-read to get updated consecutive_errors
        let consecutive = service
            .get(&job.id)
            .await
            .map(|j| j.state.consecutive_errors)
            .unwrap_or(0);
        events
            .send(CronEvent::JobFailed {
                job_id: job.id.clone(),
                error: result.output,
                consecutive,
            })
            .ok();
    }
}
