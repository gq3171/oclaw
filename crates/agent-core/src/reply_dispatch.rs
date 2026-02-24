use tokio::sync::mpsc;

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
        if result.is_ok() {
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
