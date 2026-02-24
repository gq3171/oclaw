//! Heartbeat wake scheduling system.
//!
//! Manages priority-based wake requests with coalescing, retry backoff,
//! and per-agent/session targeting. Mirrors the Node OpenClaw heartbeat-wake pattern.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

const DEFAULT_COALESCE_MS: u64 = 250;
const DEFAULT_RETRY_MS: u64 = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WakePriority {
    Retry = 0,
    Interval = 1,
    Default = 2,
    Action = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReasonKind {
    Retry,
    Interval,
    Manual,
    ExecEvent,
    Wake,
    Cron,
    Hook,
    Other,
}

pub fn resolve_reason_kind(reason: &str) -> ReasonKind {
    let trimmed = reason.trim();
    match trimmed {
        "retry" => ReasonKind::Retry,
        "interval" => ReasonKind::Interval,
        "manual" => ReasonKind::Manual,
        "exec-event" => ReasonKind::ExecEvent,
        "wake" => ReasonKind::Wake,
        _ if trimmed.starts_with("cron:") => ReasonKind::Cron,
        _ if trimmed.starts_with("hook:") => ReasonKind::Hook,
        _ => ReasonKind::Other,
    }
}

pub fn is_action_wake_reason(reason: &str) -> bool {
    matches!(
        resolve_reason_kind(reason),
        ReasonKind::Manual | ReasonKind::ExecEvent | ReasonKind::Hook
    )
}

fn resolve_priority(reason: &str) -> WakePriority {
    match resolve_reason_kind(reason) {
        ReasonKind::Retry => WakePriority::Retry,
        ReasonKind::Interval => WakePriority::Interval,
        _ if is_action_wake_reason(reason) => WakePriority::Action,
        _ => WakePriority::Default,
    }
}

fn normalize_reason(reason: Option<&str>) -> String {
    let trimmed = reason.unwrap_or("").trim().to_string();
    if trimmed.is_empty() {
        "requested".to_string()
    } else {
        trimmed
    }
}

fn normalize_target(value: Option<&str>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn wake_target_key(agent_id: Option<&str>, session_key: Option<&str>) -> String {
    format!(
        "{}::{}",
        agent_id.unwrap_or(""),
        session_key.unwrap_or("")
    )
}

#[derive(Debug, Clone)]
pub enum HeartbeatRunResult {
    Ran { duration_ms: u64 },
    Skipped { reason: String },
    Failed { reason: String },
}

#[derive(Debug, Clone)]
pub struct WakeRequest {
    pub reason: Option<String>,
    pub agent_id: Option<String>,
    pub session_key: Option<String>,
    pub coalesce_ms: Option<u64>,
}

#[derive(Debug, Clone)]
struct PendingWake {
    reason: String,
    priority: WakePriority,
    requested_at: u64,
    agent_id: Option<String>,
    session_key: Option<String>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Async callback type for handling heartbeat wake events.
pub type WakeHandler = Arc<
    dyn Fn(String, Option<String>, Option<String>) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = HeartbeatRunResult> + Send>,
    > + Send
        + Sync,
>;

struct SchedulerInner {
    pending: HashMap<String, PendingWake>,
    handler: Option<WakeHandler>,
    running: bool,
    generation: u64,
}

/// Priority-based heartbeat wake scheduler with coalescing and retry backoff.
pub struct HeartbeatWakeScheduler {
    inner: Arc<Mutex<SchedulerInner>>,
    notify: Arc<tokio::sync::Notify>,
}

impl HeartbeatWakeScheduler {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(SchedulerInner {
                pending: HashMap::new(),
                handler: None,
                running: false,
                generation: 0,
            })),
            notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Register a wake handler. Returns the generation ID for disposal.
    pub async fn set_handler(&self, handler: Option<WakeHandler>) -> u64 {
        let mut inner = self.inner.lock().await;
        inner.generation += 1;
        inner.handler = handler;
        inner.running = false;
        let generation = inner.generation;

        if inner.handler.is_some() && !inner.pending.is_empty() {
            self.notify.notify_one();
        }
        generation
    }

    /// Clear handler only if generation matches (stale disposer safety).
    pub async fn clear_handler(&self, generation: u64) {
        let mut inner = self.inner.lock().await;
        if inner.generation == generation {
            inner.generation += 1;
            inner.handler = None;
        }
    }

    /// Queue a wake request with priority-based coalescing.
    pub async fn request_wake(&self, req: WakeRequest) {
        let reason = normalize_reason(req.reason.as_deref());
        let agent_id = normalize_target(req.agent_id.as_deref());
        let session_key = normalize_target(req.session_key.as_deref());
        let key = wake_target_key(agent_id.as_deref(), session_key.as_deref());
        let priority = resolve_priority(&reason);
        let requested_at = now_ms();

        let mut inner = self.inner.lock().await;
        let should_insert = match inner.pending.get(&key) {
            None => true,
            Some(prev) => {
                priority > prev.priority
                    || (priority == prev.priority && requested_at >= prev.requested_at)
            }
        };

        if should_insert {
            inner.pending.insert(
                key,
                PendingWake {
                    reason,
                    priority,
                    requested_at,
                    agent_id,
                    session_key,
                },
            );
        }

        if inner.handler.is_some() {
            drop(inner);
            // Schedule after coalesce delay
            let delay = req.coalesce_ms.unwrap_or(DEFAULT_COALESCE_MS);
            let notify = self.notify.clone();
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                notify.notify_one();
            });
        }
    }

    pub async fn has_handler(&self) -> bool {
        self.inner.lock().await.handler.is_some()
    }

    pub async fn has_pending(&self) -> bool {
        !self.inner.lock().await.pending.is_empty()
    }

    /// Run the scheduler loop. Call this from a spawned task.
    pub async fn run(&self) {
        loop {
            self.notify.notified().await;

            let (batch, handler) = {
                let mut inner = self.inner.lock().await;
                if inner.handler.is_none() || inner.pending.is_empty() || inner.running {
                    continue;
                }
                inner.running = true;
                let batch: Vec<PendingWake> = inner.pending.drain().map(|(_, v)| v).collect();
                let handler = inner.handler.clone().unwrap();
                (batch, handler)
            };

            for wake in &batch {
                let result = handler(
                    wake.reason.clone(),
                    wake.agent_id.clone(),
                    wake.session_key.clone(),
                )
                .await;

                if let HeartbeatRunResult::Skipped { ref reason } = result {
                    if reason == "requests-in-flight" {
                        // Re-queue for retry
                        let mut inner = self.inner.lock().await;
                        let key = wake_target_key(
                            wake.agent_id.as_deref(),
                            wake.session_key.as_deref(),
                        );
                        inner.pending.insert(
                            key,
                            PendingWake {
                                reason: wake.reason.clone(),
                                priority: WakePriority::Retry,
                                requested_at: now_ms(),
                                agent_id: wake.agent_id.clone(),
                                session_key: wake.session_key.clone(),
                            },
                        );
                    }
                }
            }

            // Mark not running, re-notify if more pending
            let has_more = {
                let mut inner = self.inner.lock().await;
                inner.running = false;
                !inner.pending.is_empty()
            };
            if has_more {
                let notify = self.notify.clone();
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(DEFAULT_RETRY_MS)).await;
                    notify.notify_one();
                });
            }
        }
    }
}

impl Default for HeartbeatWakeScheduler {
    fn default() -> Self {
        Self::new()
    }
}
