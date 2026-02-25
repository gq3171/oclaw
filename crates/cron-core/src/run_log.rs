use crate::runner::DeliveryResult;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

const MAX_LOG_LINES: usize = 500;
const OUTPUT_PREVIEW_MAX: usize = 500;

/// A single run-log entry persisted as one JSONL line.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunLogEntry {
    pub timestamp_ms: u64,
    pub status: String,
    pub duration_ms: u64,
    pub output_preview: String,
    pub error: Option<String>,
    pub deliveries: Vec<DeliveryResult>,
}

/// JSONL-based run log. Each job gets its own file, auto-pruned to MAX_LOG_LINES.
pub struct RunLog {
    dir: PathBuf,
}

impl RunLog {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    fn log_path(&self, job_id: &str) -> PathBuf {
        self.dir.join(format!("{}.jsonl", job_id))
    }

    pub async fn append(&self, job_id: &str, entry: &RunLogEntry) -> anyhow::Result<()> {
        tokio::fs::create_dir_all(&self.dir).await?;
        let path = self.log_path(job_id);
        let mut line = serde_json::to_string(entry)?;
        line.push('\n');
        tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?
            .write_all(line.as_bytes())
            .await?;
        self.prune(&path, MAX_LOG_LINES).await?;
        Ok(())
    }

    pub async fn read(&self, job_id: &str, limit: usize) -> anyhow::Result<Vec<RunLogEntry>> {
        let path = self.log_path(job_id);
        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let entries: Vec<RunLogEntry> = content
            .lines()
            .rev()
            .take(limit)
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        Ok(entries)
    }

    async fn prune(&self, path: &Path, max_lines: usize) -> anyhow::Result<()> {
        let content = tokio::fs::read_to_string(path).await?;
        let lines: Vec<&str> = content.lines().collect();
        if lines.len() <= max_lines {
            return Ok(());
        }
        let keep = &lines[lines.len() - max_lines..];
        let mut pruned = keep.join("\n");
        pruned.push('\n');
        tokio::fs::write(path, pruned).await?;
        Ok(())
    }

    /// Truncate output to a preview-safe length.
    pub fn truncate_preview(s: &str) -> String {
        if s.len() <= OUTPUT_PREVIEW_MAX {
            s.to_string()
        } else {
            format!("{}…", &s[..OUTPUT_PREVIEW_MAX])
        }
    }
}
