use std::path::{Path, PathBuf};
use tokio::task::JoinHandle;

pub struct SessionReaper {
    pub retention_hours: u64,
    pub sweep_interval_mins: u64,
}

pub struct ReaperResult {
    pub pruned: usize,
    pub errors: Vec<String>,
}

impl Default for SessionReaper {
    fn default() -> Self {
        Self {
            retention_hours: 24,
            sweep_interval_mins: 5,
        }
    }
}

impl SessionReaper {
    pub async fn sweep(&self, sessions_dir: &Path) -> ReaperResult {
        let mut pruned = 0;
        let mut errors = Vec::new();

        let cutoff = chrono::Utc::now() - chrono::Duration::hours(self.retention_hours as i64);

        let mut entries = match tokio::fs::read_dir(sessions_dir).await {
            Ok(e) => e,
            Err(e) => {
                errors.push(format!("Failed to read sessions dir: {}", e));
                return ReaperResult { pruned, errors };
            }
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }

            let meta = match tokio::fs::metadata(&path).await {
                Ok(m) => m,
                Err(e) => {
                    errors.push(format!("{}: {}", path.display(), e));
                    continue;
                }
            };

            let modified = match meta.modified() {
                Ok(t) => t,
                Err(_) => continue,
            };

            let mod_time: chrono::DateTime<chrono::Utc> = modified.into();
            if mod_time < cutoff {
                // Safety: skip files that are currently locked (being written to)
                let lock_path = path.with_extension("lock");
                if lock_path.exists() {
                    tracing::debug!("Skipping active session (lock exists): {}", path.display());
                    continue;
                }
                // Double-check: re-read mtime to avoid TOCTOU race
                if let Ok(m2) = tokio::fs::metadata(&path).await
                    && let Ok(t2) = m2.modified()
                {
                    let t2: chrono::DateTime<chrono::Utc> = t2.into();
                    if t2 >= cutoff {
                        tracing::debug!(
                            "Skipping session (modified during sweep): {}",
                            path.display()
                        );
                        continue;
                    }
                }
                match tokio::fs::remove_file(&path).await {
                    Ok(_) => {
                        tracing::info!("Reaped session: {}", path.display());
                        pruned += 1;
                    }
                    Err(e) => {
                        errors.push(format!("Failed to remove {}: {}", path.display(), e));
                    }
                }
            }
        }

        ReaperResult { pruned, errors }
    }

    pub fn start_background(self, sessions_dir: PathBuf) -> JoinHandle<()> {
        tokio::spawn(async move {
            let interval = tokio::time::Duration::from_secs(self.sweep_interval_mins * 60);
            loop {
                tokio::time::sleep(interval).await;
                let result = self.sweep(&sessions_dir).await;
                if result.pruned > 0 {
                    tracing::info!("Session reaper: pruned {} file(s)", result.pruned);
                }
                for err in &result.errors {
                    tracing::warn!("Session reaper error: {}", err);
                }
            }
        })
    }
}
