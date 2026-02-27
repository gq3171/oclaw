use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

pub enum WatchEvent {
    Modified(PathBuf),
    Created(PathBuf),
    Removed(PathBuf),
}

pub struct MemoryWatcher {
    directories: Vec<PathBuf>,
    poll_interval_secs: u64,
}

impl MemoryWatcher {
    pub fn new(directories: Vec<PathBuf>) -> Self {
        Self {
            directories,
            poll_interval_secs: 30,
        }
    }

    pub fn with_interval(mut self, secs: u64) -> Self {
        self.poll_interval_secs = secs;
        self
    }

    /// Start polling for file changes. Returns a receiver of watch events.
    pub fn start(self) -> mpsc::Receiver<WatchEvent> {
        let (tx, rx) = mpsc::channel(256);

        tokio::spawn(async move {
            let mut snapshots = std::collections::HashMap::<PathBuf, u64>::new();

            // Initial scan
            for dir in &self.directories {
                if let Ok(entries) = collect_files(dir) {
                    for (path, mtime) in entries {
                        snapshots.insert(path, mtime);
                    }
                }
            }
            info!(
                "Memory watcher started: {} dirs, {} files tracked",
                self.directories.len(),
                snapshots.len()
            );

            loop {
                tokio::time::sleep(std::time::Duration::from_secs(self.poll_interval_secs)).await;

                let mut current = std::collections::HashMap::new();
                for dir in &self.directories {
                    if let Ok(entries) = collect_files(dir) {
                        for (path, mtime) in entries {
                            current.insert(path, mtime);
                        }
                    }
                }

                // Detect changes
                for (path, mtime) in &current {
                    match snapshots.get(path) {
                        None => {
                            debug!("New file: {:?}", path);
                            let _ = tx.send(WatchEvent::Created(path.clone())).await;
                        }
                        Some(old_mtime) if old_mtime != mtime => {
                            debug!("Modified file: {:?}", path);
                            let _ = tx.send(WatchEvent::Modified(path.clone())).await;
                        }
                        _ => {}
                    }
                }

                // Detect removals
                for path in snapshots.keys() {
                    if !current.contains_key(path) {
                        debug!("Removed file: {:?}", path);
                        let _ = tx.send(WatchEvent::Removed(path.clone())).await;
                    }
                }

                snapshots = current;
            }
        });

        rx
    }
}

fn collect_files(dir: &Path) -> std::io::Result<Vec<(PathBuf, u64)>> {
    let mut results = Vec::new();
    collect_files_recursive(dir, &mut results)?;
    Ok(results)
}

fn collect_files_recursive(dir: &Path, results: &mut Vec<(PathBuf, u64)>) -> std::io::Result<()> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            warn!("Cannot read dir {:?}: {}", dir, e);
            return Ok(());
        }
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            // Skip hidden dirs and common non-source dirs
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }
            collect_files_recursive(&path, results)?;
        } else if path.is_file()
            && let Ok(meta) = path.metadata()
        {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            results.push((path, mtime));
        }
    }
    Ok(())
}
