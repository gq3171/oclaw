//! Workspace directory management — resolves paths and ensures structure.

use std::path::{Path, PathBuf};
use tokio::fs;
use tracing::info;

const DEFAULT_WORKSPACE_DIR: &str = ".oclaw/workspace";

/// Represents an agent's workspace directory on disk.
#[derive(Debug, Clone)]
pub struct Workspace {
    root: PathBuf,
}

impl Workspace {
    /// Create a workspace rooted at the given directory.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Create a workspace using the default location (~/.oclaw/workspace).
    pub fn default_location() -> anyhow::Result<Self> {
        let home = dirs_home().ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?;
        Ok(Self::new(home.join(DEFAULT_WORKSPACE_DIR)))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Ensure the workspace directory structure exists.
    pub async fn ensure_dirs(&self) -> anyhow::Result<()> {
        fs::create_dir_all(&self.root).await?;
        fs::create_dir_all(self.memory_dir()).await?;
        info!("Workspace initialized at {}", self.root.display());
        Ok(())
    }

    // ── Well-known file paths ──

    pub fn soul_path(&self) -> PathBuf {
        self.root.join("SOUL.md")
    }

    pub fn identity_path(&self) -> PathBuf {
        self.root.join("IDENTITY.md")
    }

    pub fn heartbeat_path(&self) -> PathBuf {
        self.root.join("HEARTBEAT.md")
    }

    pub fn memory_path(&self) -> PathBuf {
        self.root.join("MEMORY.md")
    }

    pub fn memory_dir(&self) -> PathBuf {
        self.root.join("memory")
    }

    /// Daily memory log path: memory/YYYY-MM-DD.md
    pub fn daily_memory_path(&self) -> PathBuf {
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        self.memory_dir().join(format!("{}.md", today))
    }

    // ── File I/O helpers ──

    /// Read a workspace file, returning None if it doesn't exist.
    pub async fn read_file(&self, path: &Path) -> anyhow::Result<Option<String>> {
        match fs::read_to_string(path).await {
            Ok(content) => Ok(Some(content)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Write content to a workspace file, creating parent dirs as needed.
    pub async fn write_file(&self, path: &Path, content: &str) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        fs::write(path, content).await?;
        Ok(())
    }

    /// Append content to a workspace file.
    pub async fn append_file(&self, path: &Path, content: &str) -> anyhow::Result<()> {
        use tokio::io::AsyncWriteExt;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        file.write_all(content.as_bytes()).await?;
        Ok(())
    }

    /// Check whether a well-known file exists.
    pub async fn has_soul(&self) -> bool {
        fs::metadata(self.soul_path()).await.is_ok()
    }

    pub async fn has_identity(&self) -> bool {
        fs::metadata(self.identity_path()).await.is_ok()
    }

    pub async fn has_heartbeat(&self) -> bool {
        fs::metadata(self.heartbeat_path()).await.is_ok()
    }

    /// Check if bootstrap has already completed (SOUL.md exists).
    pub async fn is_bootstrapped(&self) -> bool {
        self.has_soul().await && self.has_identity().await
    }
}

fn dirs_home() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("USERPROFILE").ok().map(PathBuf::from)
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME").ok().map(PathBuf::from)
    }
}
