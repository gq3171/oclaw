//! Message queue — handles concurrent inbound messages per session.

use std::collections::VecDeque;
use crate::context::FinalizedMsgContext;

/// Queue mode determining how concurrent messages are handled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum QueueMode {
    /// Abort current run, start new one immediately.
    Interrupt,
    /// Inject message into the active run's context.
    Steer,
    /// Queue for next turn after current completes.
    #[default]
    Followup,
    /// Collect multiple messages before processing.
    Collect,
    /// Steer + queue backlog for later.
    SteerBacklog,
}

/// Action returned by the queue when a message is enqueued.
#[derive(Debug)]
pub enum QueueAction {
    /// Run this message immediately (no active run).
    RunNow(FinalizedMsgContext),
    /// Message was queued for later.
    Queued,
    /// Active run was interrupted; run this message now.
    Interrupted(FinalizedMsgContext),
    /// Collected batch ready for processing.
    Collected(Vec<FinalizedMsgContext>),
}

/// Per-session message queue.
pub struct MessageQueue {
    mode: QueueMode,
    pending: VecDeque<FinalizedMsgContext>,
    has_active_run: bool,
    collect_window_ms: u64,
}

impl MessageQueue {
    pub fn new(mode: QueueMode) -> Self {
        Self {
            mode,
            pending: VecDeque::new(),
            has_active_run: false,
            collect_window_ms: 2000,
        }
    }

    pub fn with_collect_window(mut self, ms: u64) -> Self {
        self.collect_window_ms = ms;
        self
    }

    /// Enqueue a message and determine the action to take.
    pub fn enqueue(&mut self, ctx: FinalizedMsgContext) -> QueueAction {
        if !self.has_active_run {
            self.has_active_run = true;
            return QueueAction::RunNow(ctx);
        }

        match self.mode {
            QueueMode::Interrupt => {
                self.pending.clear();
                QueueAction::Interrupted(ctx)
            }
            QueueMode::Steer | QueueMode::SteerBacklog => {
                self.pending.push_back(ctx);
                QueueAction::Queued
            }
            QueueMode::Followup => {
                self.pending.push_back(ctx);
                QueueAction::Queued
            }
            QueueMode::Collect => {
                self.pending.push_back(ctx);
                QueueAction::Queued
            }
        }
    }

    /// Drain all pending messages (called when active run completes).
    pub fn drain(&mut self) -> Vec<FinalizedMsgContext> {
        self.has_active_run = false;
        self.pending.drain(..).collect()
    }

    /// Mark the active run as complete without draining.
    pub fn mark_run_complete(&mut self) {
        self.has_active_run = false;
    }

    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    pub fn has_active_run(&self) -> bool {
        self.has_active_run
    }

    pub fn mode(&self) -> QueueMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: QueueMode) {
        self.mode = mode;
    }
}

