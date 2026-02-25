use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, Semaphore};

#[derive(Debug, Clone)]
pub enum ReplyKind {
    ToolResult,
    Block,
    Final,
}

#[derive(Debug, Clone)]
pub struct ReplyPayload {
    pub kind: ReplyKind,
    pub content: String,
    pub session_key: String,
}

/// Serialized reply dispatcher — ensures tool/block/final order.
pub struct ReplyDispatcher {
    tx: mpsc::Sender<ReplyPayload>,
    pending: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl ReplyDispatcher {
    pub fn new(buffer: usize) -> (Self, mpsc::Receiver<ReplyPayload>) {
        let (tx, rx) = mpsc::channel(buffer);
        let pending = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(1)); // reservation
        (Self { tx, pending }, rx)
    }

    pub async fn send(&self, payload: ReplyPayload) -> Result<(), mpsc::error::SendError<ReplyPayload>> {
        self.pending.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let result = self.tx.send(payload).await;
        if result.is_err() {
            // Only decrement on failure; successful sends are decremented by the consumer
            self.pending.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        }
        result
    }

    /// Signal no more replies incoming — clears the reservation.
    pub fn mark_complete(&self) {
        self.pending.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn pending_count(&self) -> usize {
        self.pending.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub fn is_idle(&self) -> bool {
        self.pending_count() == 0
    }
}

/// Per-session lane dispatcher — allows concurrent processing across sessions
/// while maintaining ordering within each session.
pub struct LaneDispatcher {
    lanes: HashMap<String, mpsc::Sender<ReplyPayload>>,
    semaphore: Arc<Semaphore>,
}

impl LaneDispatcher {
    pub fn new(max_concurrent: usize) -> Self {
        Self {
            lanes: HashMap::new(),
            semaphore: Arc::new(Semaphore::new(max_concurrent)),
        }
    }

    pub async fn dispatch(&mut self, session_key: &str, payload: ReplyPayload) {
        if let Some(tx) = self.lanes.get(session_key) {
            if tx.send(payload.clone()).await.is_ok() {
                return;
            }
            // Channel closed — remove stale lane and recreate below
            self.lanes.remove(session_key);
        }
        // Create a new lane
        let (tx, mut rx) = mpsc::channel::<ReplyPayload>(32);
        let _ = tx.send(payload).await;
        self.lanes.insert(session_key.to_string(), tx);

        let sem = self.semaphore.clone();
        let lane_key = session_key.to_string();
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let _permit = sem.acquire().await;
                // Actual delivery is injected by the orchestrator layer at runtime.
                // This task only serialises ordering within a lane.
                tracing::debug!(
                    session = %lane_key,
                    kind = ?msg.kind,
                    "LaneDispatcher: forwarding reply"
                );
            }
        });
    }

    pub fn lane_count(&self) -> usize {
        self.lanes.len()
    }

    pub fn is_lane_active(&self, session_key: &str) -> bool {
        self.lanes.contains_key(session_key)
    }

    pub fn drain_lane(&mut self, session_key: &str) {
        self.lanes.remove(session_key);
    }
}
