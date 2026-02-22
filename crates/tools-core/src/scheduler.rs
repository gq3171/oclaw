use chrono::{DateTime, Utc};
use cron::Schedule;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::error::{ToolError, ToolResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub cron_expression: String,
    pub command: String,
    pub enabled: bool,
    pub last_run: Option<DateTime<Utc>>,
    pub next_run: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerStats {
    pub total_jobs: usize,
    pub enabled_jobs: usize,
    pub running_jobs: usize,
}

#[derive(Debug, Clone)]
struct JobSchedule {
    job: CronJob,
    schedule: Schedule,
}

pub struct Scheduler {
    jobs: Arc<RwLock<HashMap<String, JobSchedule>>>,
    stop_tx: Arc<RwLock<Option<tokio::sync::oneshot::Sender<()>>>>,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            jobs: Arc::new(RwLock::new(HashMap::new())),
            stop_tx: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn add_job(&self, name: String, cron_expression: String, command: String) -> ToolResult<String> {
        let schedule = Schedule::from_str(&cron_expression)
            .map_err(|e| ToolError::SchedulerError(format!("Invalid cron expression: {}", e)))?;

        let next_run = schedule.upcoming(Utc).next().ok_or_else(|| ToolError::SchedulerError("Failed to calculate next run".to_string()))?;

        let id = Uuid::new_v4().to_string();
        
        let job = CronJob {
            id: id.clone(),
            name: name.clone(),
            cron_expression: cron_expression.clone(),
            command,
            enabled: true,
            last_run: None,
            next_run: Some(next_run),
            created_at: Utc::now(),
        };

        let job_schedule = JobSchedule { job, schedule };
        
        let mut jobs = self.jobs.write().await;
        jobs.insert(id.clone(), job_schedule);

        Ok(id)
    }

    pub async fn remove_job(&self, id: &str) -> ToolResult<()> {
        let mut jobs = self.jobs.write().await;
        jobs.remove(id);
        Ok(())
    }

    pub async fn list_jobs(&self) -> Vec<CronJob> {
        let jobs = self.jobs.read().await;
        jobs.values().map(|js| js.job.clone()).collect()
    }

    pub async fn get_job(&self, id: &str) -> Option<CronJob> {
        let jobs = self.jobs.read().await;
        jobs.get(id).map(|js| js.job.clone())
    }

    pub async fn enable_job(&self, id: &str) -> ToolResult<()> {
        let mut jobs = self.jobs.write().await;
        if let Some(js) = jobs.get_mut(id) {
            js.job.enabled = true;
            js.job.next_run = js.schedule.upcoming(Utc).next();
            Ok(())
        } else {
            Err(ToolError::SchedulerError(format!("Job not found: {}", id)))
        }
    }

    pub async fn disable_job(&self, id: &str) -> ToolResult<()> {
        let mut jobs = self.jobs.write().await;
        if let Some(js) = jobs.get_mut(id) {
            js.job.enabled = false;
            js.job.next_run = None;
            Ok(())
        } else {
            Err(ToolError::SchedulerError(format!("Job not found: {}", id)))
        }
    }

    pub async fn stats(&self) -> SchedulerStats {
        let jobs = self.jobs.read().await;
        
        let total = jobs.len();
        let enabled = jobs.values().filter(|js| js.job.enabled).count();
        
        SchedulerStats {
            total_jobs: total,
            enabled_jobs: enabled,
            running_jobs: 0,
        }
    }

    pub async fn run_job(&self, id: &str) -> ToolResult<serde_json::Value> {
        let job = {
            let jobs = self.jobs.read().await;
            jobs.get(id).map(|js| js.job.clone())
        };

        match job {
            Some(j) => {
                let output = self.execute_command(&j.command).await?;

                {
                    let mut jobs = self.jobs.write().await;
                    if let Some(js) = jobs.get_mut(id) {
                        js.job.last_run = Some(Utc::now());
                        js.job.next_run = js.schedule.upcoming(Utc).next();
                    }
                }

                Ok(output)
            }
            None => Err(ToolError::SchedulerError(format!("Job not found: {}", id))),
        }
    }

    async fn execute_command(&self, command: &str) -> ToolResult<serde_json::Value> {
        let output = if cfg!(windows) {
            tokio::process::Command::new("cmd")
                .args(["/C", command])
                .output()
                .await
        } else {
            tokio::process::Command::new("sh")
                .args(["-c", command])
                .output()
                .await
        }.map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(serde_json::json!({
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
            "exit_code": output.status.code(),
        }))
    }

    pub async fn start(&self) {
        let jobs = Arc::clone(&self.jobs);
        let stop_tx = Arc::clone(&self.stop_tx);
        
        let (tx, mut rx) = tokio::sync::oneshot::channel();
        {
            let mut tx_guard = stop_tx.write().await;
            *tx_guard = Some(tx);
        }

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));
            
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let now = Utc::now();
                        
                        let jobs_to_run: Vec<(String, String)> = {
                            let jobs_guard = jobs.read().await;
                            jobs_guard.iter()
                                .filter(|(_, js)| {
                                    js.job.enabled && js.job.next_run.is_some_and(|nr| now >= nr)
                                })
                                .map(|(id, js)| (id.clone(), js.job.command.clone()))
                                .collect()
                        };
                        
                        for (id, command) in jobs_to_run {
                            let id_clone = id.clone();
                            let jobs_ref = Arc::clone(&jobs);
                            
                            tokio::spawn(async move {
                                let output = if cfg!(windows) {
                                    tokio::process::Command::new("cmd")
                                        .args(["/C", &command])
                                        .output()
                                        .await
                                } else {
                                    tokio::process::Command::new("sh")
                                        .args(["-c", &command])
                                        .output()
                                        .await
                                };
                                
                                if let Ok(output) = output {
                                    tracing::info!("Job {} output: {}", id_clone, String::from_utf8_lossy(&output.stdout));
                                }
                                
                                let mut jobs_write = jobs_ref.write().await;
                                if let Some(js) = jobs_write.get_mut(&id_clone) {
                                    js.job.last_run = Some(Utc::now());
                                    js.job.next_run = js.schedule.upcoming(Utc).next();
                                }
                            });
                        }
                    }
                    _ = &mut rx => {
                        tracing::info!("Scheduler stopped");
                        break;
                    }
                }
            }
        });
    }

    pub async fn stop(&self) {
        let mut tx_guard = self.stop_tx.write().await;
        if let Some(tx) = tx_guard.take() {
            let _ = tx.send(());
        }
    }
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}
