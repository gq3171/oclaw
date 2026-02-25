use crate::types::CronJob;
use std::path::PathBuf;

pub struct CronStore {
    path: PathBuf,
}

impl CronStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path() -> PathBuf {
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("oclaws")
            .join("cron")
            .join("jobs.json")
    }

    pub async fn load(&self) -> Vec<CronJob> {
        match tokio::fs::read_to_string(&self.path).await {
            Ok(content) => {
                serde_json::from_str(&content).unwrap_or_default()
            }
            Err(_) => Vec::new(),
        }
    }

    pub async fn save(&self, jobs: &[CronJob]) -> anyhow::Result<()> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp = self.path.with_extension("tmp");
        let content = serde_json::to_string_pretty(jobs)?;
        tokio::fs::write(&tmp, &content).await?;
        tokio::fs::rename(&tmp, &self.path).await?;
        Ok(())
    }
}
