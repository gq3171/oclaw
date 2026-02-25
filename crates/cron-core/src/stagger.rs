//! Deterministic stagger offsets for cron jobs.
//!
//! Uses SHA-256 of the job ID to produce a stable offset within a window,
//! preventing all jobs from firing at the exact same instant.

use sha2::{Sha256, Digest};

/// Compute a deterministic stagger offset in `0..window_ms` for a given job ID.
pub fn stagger_offset(job_id: &str, window_ms: u64) -> u64 {
    if window_ms == 0 {
        return 0;
    }
    let hash = Sha256::digest(job_id.as_bytes());
    let val = u64::from_le_bytes(hash[0..8].try_into().unwrap());
    val % window_ms
}

/// Apply stagger to a scheduled next-run timestamp.
pub fn apply_stagger(next_run_ms: u64, job_id: &str, window_ms: u64) -> u64 {
    next_run_ms + stagger_offset(job_id, window_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic() {
        let a = stagger_offset("job-1", 30_000);
        let b = stagger_offset("job-1", 30_000);
        assert_eq!(a, b);
    }

    #[test]
    fn within_window() {
        for i in 0..100 {
            let id = format!("job-{i}");
            let offset = stagger_offset(&id, 30_000);
            assert!(offset < 30_000);
        }
    }

    #[test]
    fn different_jobs_differ() {
        let a = stagger_offset("job-alpha", 60_000);
        let b = stagger_offset("job-beta", 60_000);
        // Extremely unlikely to collide
        assert_ne!(a, b);
    }

    #[test]
    fn zero_window() {
        assert_eq!(stagger_offset("any", 0), 0);
    }

    #[test]
    fn apply_adds_offset() {
        let base = 1_000_000u64;
        let result = apply_stagger(base, "job-x", 30_000);
        assert!(result >= base);
        assert!(result < base + 30_000);
    }
}
