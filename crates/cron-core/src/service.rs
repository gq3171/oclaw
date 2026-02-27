use crate::backoff::backoff_delay_ms;
use crate::runner::CronRunResult;
use crate::schedule::compute_next_run;
use crate::store::CronStore;
use crate::types::{CronJob, CronScheduleKind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::sync::Mutex;

// TODO: migrate to incremental updates for high-frequency scenarios.
// Currently every mutation does a full load→modify→save cycle, which is fine for
// small job counts but will become a bottleneck with hundreds of jobs.
pub struct CronService {
    store: Mutex<CronStore>,
    running: Arc<AtomicBool>,
}

impl CronService {
    pub fn new(store: CronStore) -> Self {
        Self {
            store: Mutex::new(store),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub async fn list(&self) -> Vec<CronJob> {
        self.store.lock().await.load().await
    }

    pub async fn add(&self, mut job: CronJob) -> anyhow::Result<CronJob> {
        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        job.created_at_ms = now_ms;
        job.updated_at_ms = now_ms;
        job.state.next_run_at_ms = compute_next_run(&job.schedule, now_ms);

        let store = self.store.lock().await;
        let mut jobs = store.load().await;
        jobs.push(job.clone());
        store.save(&jobs).await?;
        Ok(job)
    }

    pub async fn remove(&self, id: &str) -> anyhow::Result<()> {
        let store = self.store.lock().await;
        let mut jobs = store.load().await;
        let before = jobs.len();
        jobs.retain(|j| j.id != id);
        if jobs.len() == before {
            anyhow::bail!("Job not found: {}", id);
        }
        store.save(&jobs).await
    }

    pub async fn update(&self, id: &str, patch: CronJobPatch) -> anyhow::Result<CronJob> {
        let store = self.store.lock().await;
        let mut jobs = store.load().await;
        let job = jobs
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or_else(|| anyhow::anyhow!("Job not found: {}", id))?;

        if let Some(name) = patch.name {
            job.name = name;
        }
        if let Some(enabled) = patch.enabled {
            job.enabled = enabled;
        }
        if let Some(schedule) = patch.schedule {
            job.schedule = schedule;
            let now_ms = chrono::Utc::now().timestamp_millis() as u64;
            job.state.next_run_at_ms = compute_next_run(&job.schedule, now_ms);
        }
        job.updated_at_ms = chrono::Utc::now().timestamp_millis() as u64;

        let updated = job.clone();
        store.save(&jobs).await?;
        Ok(updated)
    }

    /// Get a single job by ID.
    pub async fn get(&self, id: &str) -> Option<CronJob> {
        let store = self.store.lock().await;
        let jobs = store.load().await;
        jobs.into_iter().find(|j| j.id == id)
    }

    /// Mark a job as currently running (for stuck detection).
    pub async fn mark_running(&self, id: &str, now_ms: u64) -> anyhow::Result<()> {
        let store = self.store.lock().await;
        let mut jobs = store.load().await;
        let job = jobs
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or_else(|| anyhow::anyhow!("Job not found: {}", id))?;
        job.state.running_since_ms = Some(now_ms);
        store.save(&jobs).await
    }

    /// Update job state after a run completes.
    pub async fn update_after_run(&self, id: &str, result: &CronRunResult) -> anyhow::Result<()> {
        let store = self.store.lock().await;
        let mut jobs = store.load().await;
        let job = jobs
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or_else(|| anyhow::anyhow!("Job not found: {}", id))?;

        let now_ms = chrono::Utc::now().timestamp_millis() as u64;
        job.state.last_run_at_ms = Some(now_ms);
        job.state.running_since_ms = None;
        job.state.total_runs += 1;

        let scheduled_next = compute_next_run(&job.schedule, now_ms);
        if result.success {
            job.state.last_status = Some("ok".to_string());
            job.state.last_error = None;
            job.state.consecutive_errors = 0;
            job.state.next_run_at_ms = scheduled_next;
        } else {
            job.state.last_status = Some("error".to_string());
            job.state.last_error = Some(result.output.clone());
            job.state.consecutive_errors += 1;
            job.state.total_errors += 1;
            let max_retries = job.max_retries.unwrap_or(5);
            if job.state.consecutive_errors >= max_retries {
                // Retry budget exhausted; keep disabled until manual trigger/reset.
                job.state.next_run_at_ms = None;
            } else {
                let backoff_due =
                    now_ms.saturating_add(backoff_delay_ms(job.state.consecutive_errors));
                job.state.next_run_at_ms = Some(match scheduled_next {
                    Some(next) => next.max(backoff_due),
                    None => backoff_due,
                });
            }
        }
        job.updated_at_ms = now_ms;

        store.save(&jobs).await
    }

    /// Manually trigger a job by resetting its next_run to now.
    pub async fn trigger(&self, id: &str) -> anyhow::Result<()> {
        let store = self.store.lock().await;
        let mut jobs = store.load().await;
        let job = jobs
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or_else(|| anyhow::anyhow!("Job not found: {}", id))?;
        job.state.next_run_at_ms = Some(0); // due immediately
        store.save(&jobs).await
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }
}

pub struct CronJobPatch {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub schedule: Option<CronScheduleKind>,
}
