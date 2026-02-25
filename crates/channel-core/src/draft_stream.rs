use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::time::{Duration, Instant};

/// Throttled draft-streaming loop for channels that support message editing.
/// Accumulates text and periodically flushes to a sender callback.
pub struct DraftStreamLoop {
    _throttle: Duration,
    _buffer: Arc<Mutex<String>>,
    _tx: mpsc::Sender<String>,
    _stopped: Arc<AtomicBool>,
}

pub struct DraftStreamHandle {
    buffer: Arc<Mutex<String>>,
    stopped: Arc<AtomicBool>,
    _task: tokio::task::JoinHandle<()>,
}

impl DraftStreamLoop {
    /// Create a new draft stream loop with the given throttle interval.
    /// Returns a handle for pushing updates and a receiver for consuming flushed text.
    pub fn start(throttle_ms: u64) -> (DraftStreamHandle, mpsc::Receiver<String>) {
        let (tx, rx) = mpsc::channel(64);
        let buffer = Arc::new(Mutex::new(String::new()));
        let stopped = Arc::new(AtomicBool::new(false));

        let loop_buf = buffer.clone();
        let loop_stopped = stopped.clone();
        let loop_tx = tx.clone();
        let throttle = Duration::from_millis(throttle_ms);

        let task = tokio::spawn(async move {
            let mut _last_flush = Instant::now();
            loop {
                tokio::time::sleep(throttle).await;

                if loop_stopped.load(Ordering::Relaxed) {
                    // Final flush
                    let text = {
                        let mut buf = loop_buf.lock().await;
                        std::mem::take(&mut *buf)
                    };
                    if !text.is_empty() {
                        let _ = loop_tx.send(text).await;
                    }
                    break;
                }

                let text = {
                    let mut buf = loop_buf.lock().await;
                    if buf.is_empty() {
                        continue;
                    }
                    std::mem::take(&mut *buf)
                };
                let _ = loop_tx.send(text).await;
                _last_flush = Instant::now();
            }
        });

        let handle = DraftStreamHandle {
            buffer,
            stopped,
            _task: task,
        };
        (handle, rx)
    }
}

impl DraftStreamHandle {
    /// Append text to the buffer (will be flushed on next throttle tick).
    pub async fn update(&self, text: &str) {
        let mut buf = self.buffer.lock().await;
        buf.push_str(text);
    }

    /// Signal the loop to stop after the next flush.
    pub fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}
