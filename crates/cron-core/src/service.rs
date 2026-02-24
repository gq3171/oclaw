use crate::store::CronStore;
use crate::schedule::compute_next_run;
use crate::types::{CronJob, CronJobState, CronPayloadKind, CronScheduleKind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

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
        let job = jobs.iter_mut().find(|j| j.id == id)
            .ok_or_else(|| anyhow::anyhow!("Job not found: {}", id))?;

        if let Some(name) = patch.name { job.name = name; }
        if let Some(enabled) = patch.enabled { job.enabled = enabled; }
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
