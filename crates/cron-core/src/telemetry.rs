//! Runtime metrics for the cron scheduler.

use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Default)]
pub struct CronMetrics {
    pub total_runs: AtomicU64,
    pub successful_runs: AtomicU64,
    pub failed_runs: AtomicU64,
    pub total_duration_ms: AtomicU64,
    pub active_jobs: AtomicU64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CronMetricsSnapshot {
    pub total_runs: u64,
    pub successful_runs: u64,
    pub failed_runs: u64,
    pub total_duration_ms: u64,
    pub active_jobs: u64,
    pub avg_duration_ms: u64,
    pub success_rate: f64,
}

impl CronMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_run(&self, success: bool, duration_ms: u64) {
        self.total_runs.fetch_add(1, Ordering::Relaxed);
        self.total_duration_ms
            .fetch_add(duration_ms, Ordering::Relaxed);
        if success {
            self.successful_runs.fetch_add(1, Ordering::Relaxed);
        } else {
            self.failed_runs.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn set_active_jobs(&self, count: u64) {
        self.active_jobs.store(count, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> CronMetricsSnapshot {
        let total = self.total_runs.load(Ordering::Relaxed);
        let successful = self.successful_runs.load(Ordering::Relaxed);
        let failed = self.failed_runs.load(Ordering::Relaxed);
        let duration = self.total_duration_ms.load(Ordering::Relaxed);
        let active = self.active_jobs.load(Ordering::Relaxed);

        let avg = if total > 0 { duration / total } else { 0 };
        let rate = if total > 0 {
            successful as f64 / total as f64
        } else {
            0.0
        };

        CronMetricsSnapshot {
            total_runs: total,
            successful_runs: successful,
            failed_runs: failed,
            total_duration_ms: duration,
            active_jobs: active,
            avg_duration_ms: avg,
            success_rate: rate,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_and_snapshot() {
        let m = CronMetrics::new();
        m.record_run(true, 100);
        m.record_run(true, 200);
        m.record_run(false, 50);
        m.set_active_jobs(5);

        let s = m.snapshot();
        assert_eq!(s.total_runs, 3);
        assert_eq!(s.successful_runs, 2);
        assert_eq!(s.failed_runs, 1);
        assert_eq!(s.total_duration_ms, 350);
        assert_eq!(s.active_jobs, 5);
        assert_eq!(s.avg_duration_ms, 116); // 350/3
        assert!((s.success_rate - 0.6667).abs() < 0.01);
    }

    #[test]
    fn empty_snapshot() {
        let m = CronMetrics::new();
        let s = m.snapshot();
        assert_eq!(s.total_runs, 0);
        assert_eq!(s.avg_duration_ms, 0);
        assert_eq!(s.success_rate, 0.0);
    }
}
