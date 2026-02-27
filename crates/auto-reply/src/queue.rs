//! Message queue — handles concurrent inbound messages per session.

use crate::context::FinalizedMsgContext;
use std::collections::VecDeque;

fn now_ms() -> u64 {
    let ts = chrono::Utc::now().timestamp_millis();
    if ts <= 0 { 0 } else { ts as u64 }
}

fn summarize_queue_text(text: &str, limit: usize) -> String {
    let compact = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if compact.chars().count() <= limit {
        return compact;
    }
    let mut out = compact
        .chars()
        .take(limit.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out
}

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

/// Queue overflow drop policy.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum QueueDropPolicy {
    #[default]
    Summarize,
    Old,
    New,
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
    debounce_ms: u64,
    cap: usize,
    drop_policy: QueueDropPolicy,
    dropped_count: usize,
    summary_lines: VecDeque<String>,
    last_enqueued_at_ms: u64,
}

impl MessageQueue {
    pub fn new(mode: QueueMode) -> Self {
        Self {
            mode,
            pending: VecDeque::new(),
            has_active_run: false,
            collect_window_ms: 2000,
            debounce_ms: 0,
            cap: usize::MAX,
            drop_policy: QueueDropPolicy::Summarize,
            dropped_count: 0,
            summary_lines: VecDeque::new(),
            last_enqueued_at_ms: 0,
        }
    }

    pub fn with_collect_window(mut self, ms: u64) -> Self {
        self.collect_window_ms = ms;
        self
    }

    pub fn set_collect_window_ms(&mut self, ms: u64) {
        self.collect_window_ms = ms.max(1);
    }

    pub fn set_debounce_ms(&mut self, ms: u64) {
        self.debounce_ms = ms;
    }

    pub fn set_cap(&mut self, cap: usize) {
        self.cap = cap.max(1);
    }

    pub fn set_drop_policy(&mut self, policy: QueueDropPolicy) {
        self.drop_policy = policy;
    }

    fn push_pending(&mut self, ctx: FinalizedMsgContext) -> bool {
        self.last_enqueued_at_ms = now_ms();
        if self.cap != usize::MAX && self.pending.len() >= self.cap {
            if self.drop_policy == QueueDropPolicy::New {
                return false;
            }
            let drop_count = self
                .pending
                .len()
                .saturating_sub(self.cap)
                .saturating_add(1);
            for _ in 0..drop_count {
                if let Some(dropped) = self.pending.pop_front()
                    && self.drop_policy == QueueDropPolicy::Summarize
                {
                    self.dropped_count = self.dropped_count.saturating_add(1);
                    self.summary_lines
                        .push_back(summarize_queue_text(&dropped.body_for_agent, 160));
                    while self.summary_lines.len() > self.cap.min(256) {
                        let _ = self.summary_lines.pop_front();
                    }
                }
            }
        }
        self.pending.push_back(ctx);
        true
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
                let _ = self.push_pending(ctx);
                QueueAction::Queued
            }
            QueueMode::Followup => {
                let _ = self.push_pending(ctx);
                QueueAction::Queued
            }
            QueueMode::Collect => {
                let _ = self.push_pending(ctx);
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

    /// Mark the current run complete and return the next queued message, if any.
    ///
    /// If a next message exists, a new run is considered active immediately.
    pub fn complete_and_take_next(&mut self) -> Option<FinalizedMsgContext> {
        if let Some(next) = self.pending.pop_front() {
            self.has_active_run = true;
            Some(next)
        } else {
            self.has_active_run = false;
            None
        }
    }

    /// Drain pending messages as a single batch for collect mode.
    /// Returns None if there is no pending message.
    pub fn take_collect_batch(&mut self) -> Option<Vec<FinalizedMsgContext>> {
        if self.pending.is_empty() {
            self.has_active_run = false;
            return None;
        }
        self.has_active_run = true;
        Some(self.pending.drain(..).collect())
    }

    pub fn take_summary_prompt(&mut self, noun: &str) -> Option<String> {
        if self.drop_policy != QueueDropPolicy::Summarize || self.dropped_count == 0 {
            return None;
        }
        let plural = if self.dropped_count == 1 { "" } else { "s" };
        let mut lines = vec![format!(
            "[Queue overflow] Dropped {} {}{} due to queue cap.",
            self.dropped_count, noun, plural
        )];
        if !self.summary_lines.is_empty() {
            lines.push("Summary:".to_string());
            for line in self.summary_lines.drain(..) {
                lines.push(format!("- {}", line));
            }
        }
        self.dropped_count = 0;
        Some(lines.join("\n"))
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

    pub fn collect_window_ms(&self) -> u64 {
        self.collect_window_ms
    }

    pub fn debounce_ms(&self) -> u64 {
        self.debounce_ms
    }

    pub fn cap(&self) -> usize {
        self.cap
    }

    pub fn drop_policy(&self) -> QueueDropPolicy {
        self.drop_policy
    }

    pub fn last_enqueued_at_ms(&self) -> u64 {
        self.last_enqueued_at_ms
    }

    pub fn set_mode(&mut self, mode: QueueMode) {
        self.mode = mode;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{ChatType, MsgContext, finalize_inbound_context};

    fn make_ctx(body: &str) -> FinalizedMsgContext {
        finalize_inbound_context(MsgContext {
            body: body.to_string(),
            raw_body: None,
            from: "u".to_string(),
            from_name: Some("U".to_string()),
            to: "chat".to_string(),
            provider: "telegram".to_string(),
            surface: Some("telegram".to_string()),
            chat_type: ChatType::Direct,
            session_key: "s".to_string(),
            message_id: None,
            thread_id: None,
            was_mentioned: false,
            media_paths: vec![],
            timestamp_ms: 0,
            raw: serde_json::Value::Null,
        })
    }

    #[test]
    fn drop_policy_new_rejects_incoming_when_full() {
        let mut q = MessageQueue::new(QueueMode::Followup);
        q.set_cap(2);
        q.set_drop_policy(QueueDropPolicy::New);
        assert!(matches!(
            q.enqueue(make_ctx("first")),
            QueueAction::RunNow(_)
        ));
        let _ = q.enqueue(make_ctx("a"));
        let _ = q.enqueue(make_ctx("b"));
        let _ = q.enqueue(make_ctx("c"));
        assert_eq!(q.pending_count(), 2);
    }

    #[test]
    fn drop_policy_old_evicts_oldest() {
        let mut q = MessageQueue::new(QueueMode::Followup);
        q.set_cap(2);
        q.set_drop_policy(QueueDropPolicy::Old);
        assert!(matches!(
            q.enqueue(make_ctx("first")),
            QueueAction::RunNow(_)
        ));
        let _ = q.enqueue(make_ctx("a"));
        let _ = q.enqueue(make_ctx("b"));
        let _ = q.enqueue(make_ctx("c"));
        let drained = q.drain();
        assert_eq!(drained.len(), 2);
        assert_eq!(drained[0].body_for_agent, "b");
        assert_eq!(drained[1].body_for_agent, "c");
    }

    #[test]
    fn drop_policy_summarize_emits_prompt() {
        let mut q = MessageQueue::new(QueueMode::Followup);
        q.set_cap(1);
        q.set_drop_policy(QueueDropPolicy::Summarize);
        assert!(matches!(
            q.enqueue(make_ctx("first")),
            QueueAction::RunNow(_)
        ));
        let _ = q.enqueue(make_ctx("message one"));
        let _ = q.enqueue(make_ctx("message two"));
        let summary = q.take_summary_prompt("message").expect("summary");
        assert!(summary.contains("Dropped 1 message"));
        assert!(summary.contains("message one"));
    }
}
