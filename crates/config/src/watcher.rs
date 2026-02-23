use std::path::PathBuf;
use std::time::{Duration, SystemTime};
use tokio::sync::watch;
use tracing::info;

/// Watches a config file for changes and sends reload signals.
pub struct ConfigWatcher {
    path: PathBuf,
    poll_interval: Duration,
    debounce: Duration,
}

impl ConfigWatcher {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            poll_interval: Duration::from_secs(2),
            debounce: Duration::from_millis(500),
        }
    }

    pub fn with_interval(mut self, interval: Duration) -> Self {
        self.poll_interval = interval;
        self
    }

    /// Starts watching. Returns a receiver that fires `true` on each detected change.
    pub fn watch(&self) -> watch::Receiver<bool> {
        let (tx, rx) = watch::channel(false);
        let path = self.path.clone();
        let interval = self.poll_interval;
        let debounce = self.debounce;

        tokio::spawn(async move {
            let mut last_modified = file_mtime(&path);

            loop {
                tokio::time::sleep(interval).await;
                let current = file_mtime(&path);
                if current != last_modified {
                    tokio::time::sleep(debounce).await;
                    last_modified = file_mtime(&path);
                    info!("Config file changed: {:?}", path);
                    if tx.send(true).is_err() {
                        break;
                    }
                }
            }
        });

        rx
    }
}

fn file_mtime(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).ok().and_then(|m| m.modified().ok())
}
