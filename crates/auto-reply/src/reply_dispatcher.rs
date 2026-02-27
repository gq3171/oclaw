//! Auto-reply level dispatcher — routes tool/block/final payloads.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::mpsc;

use crate::types::ReplyPayload;

/// Dispatch kind for reply payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ReplyDispatchKind {
    Tool,
    Block,
    Final,
}

/// Dispatcher that routes reply payloads by kind (tool/block/final).
pub struct AutoReplyDispatcher {
    tool_tx: mpsc::Sender<ReplyPayload>,
    block_tx: mpsc::Sender<ReplyPayload>,
    final_tx: mpsc::Sender<ReplyPayload>,
    pending: Arc<AtomicUsize>,
}

/// Receiver side of the dispatcher.
pub struct AutoReplyReceiver {
    pub tool_rx: mpsc::Receiver<ReplyPayload>,
    pub block_rx: mpsc::Receiver<ReplyPayload>,
    pub final_rx: mpsc::Receiver<ReplyPayload>,
}

impl AutoReplyDispatcher {
    /// Create a new dispatcher with the given buffer size per channel.
    pub fn new(buffer: usize) -> (Self, AutoReplyReceiver) {
        let (tool_tx, tool_rx) = mpsc::channel(buffer);
        let (block_tx, block_rx) = mpsc::channel(buffer);
        let (final_tx, final_rx) = mpsc::channel(buffer);
        let pending = Arc::new(AtomicUsize::new(1)); // reservation
        (
            Self {
                tool_tx,
                block_tx,
                final_tx,
                pending,
            },
            AutoReplyReceiver {
                tool_rx,
                block_rx,
                final_rx,
            },
        )
    }

    pub fn send_tool_result(&self, payload: ReplyPayload) -> bool {
        self.pending.fetch_add(1, Ordering::Relaxed);
        match self.tool_tx.try_send(payload) {
            Ok(()) => {
                self.pending.fetch_sub(1, Ordering::Relaxed);
                true
            }
            Err(_) => {
                self.pending.fetch_sub(1, Ordering::Relaxed);
                false
            }
        }
    }

    pub fn send_block_reply(&self, payload: ReplyPayload) -> bool {
        self.pending.fetch_add(1, Ordering::Relaxed);
        match self.block_tx.try_send(payload) {
            Ok(()) => {
                self.pending.fetch_sub(1, Ordering::Relaxed);
                true
            }
            Err(_) => {
                self.pending.fetch_sub(1, Ordering::Relaxed);
                false
            }
        }
    }

    pub fn send_final_reply(&self, payload: ReplyPayload) -> bool {
        self.pending.fetch_add(1, Ordering::Relaxed);
        match self.final_tx.try_send(payload) {
            Ok(()) => {
                self.pending.fetch_sub(1, Ordering::Relaxed);
                true
            }
            Err(_) => {
                self.pending.fetch_sub(1, Ordering::Relaxed);
                false
            }
        }
    }

    /// Signal that no more replies are incoming.
    pub fn mark_complete(&self) {
        self.pending.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn is_idle(&self) -> bool {
        self.pending.load(Ordering::Relaxed) == 0
    }
}
