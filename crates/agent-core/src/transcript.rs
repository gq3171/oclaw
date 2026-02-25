use std::path::PathBuf;
use anyhow::Result;
use oclaws_llm_core::chat::ChatMessage;
use tokio::io::AsyncWriteExt;

pub struct Transcript {
    path: PathBuf,
}

impl Transcript {
    pub fn new(session_id: &str) -> Self {
        let base = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".oclaw")
            .join("sessions");
        Self {
            path: base.join(format!("{}.jsonl", session_id)),
        }
    }

    pub fn open(path: PathBuf) -> Self {
        Self { path }
    }

    pub async fn exists(&self) -> bool {
        tokio::fs::metadata(&self.path).await.is_ok()
    }

    pub async fn load(&self) -> Vec<ChatMessage> {
        let Ok(data) = tokio::fs::read_to_string(&self.path).await else {
            return Vec::new();
        };
        let mut messages = Vec::new();
        let mut discarded = 0usize;
        for line in data.lines() {
            match serde_json::from_str::<ChatMessage>(line) {
                Ok(msg) => messages.push(msg),
                Err(_) => discarded += 1,
            }
        }
        if discarded > 0 {
            tracing::warn!(
                "Transcript {}: discarded {} corrupt/non-message line(s)",
                self.path.display(),
                discarded
            );
        }
        messages
    }

    pub async fn append(&self, message: &ChatMessage) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let mut line = serde_json::to_string(message)?;
        line.push('\n');
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        file.write_all(line.as_bytes()).await?;
        Ok(())
    }

    pub async fn append_compaction(&self, summary: &str, _kept_from_id: Option<&str>) -> Result<()> {
        let marker = serde_json::json!({
            "type": "compaction",
            "summary": summary,
            "timestamp": chrono::Utc::now().to_rfc3339(),
        });
        let mut line = serde_json::to_string(&marker)?;
        line.push('\n');
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .await?;
        file.write_all(line.as_bytes()).await?;
        Ok(())
    }
}
