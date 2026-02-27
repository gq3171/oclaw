use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuditEventKind {
    AuthAttempt,
    AuthSuccess,
    AuthFailure,
    SessionCreate,
    SessionRevoke,
    ToolCall,
    ToolDenied,
    MessageSend,
    MessageReceive,
    ConfigChange,
    PluginLoad,
    PluginError,
    RateLimited,
    ContentFiltered,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEvent {
    pub id: String,
    pub kind: AuditEventKind,
    pub timestamp: DateTime<Utc>,
    pub actor: Option<String>,
    pub target: Option<String>,
    pub detail: Option<String>,
    pub ip_address: Option<String>,
    pub success: bool,
}

impl AuditEvent {
    pub fn new(kind: AuditEventKind) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            kind,
            timestamp: Utc::now(),
            actor: None,
            target: None,
            detail: None,
            ip_address: None,
            success: true,
        }
    }

    pub fn actor(mut self, actor: &str) -> Self {
        self.actor = Some(actor.to_string());
        self
    }

    pub fn target(mut self, target: &str) -> Self {
        self.target = Some(target.to_string());
        self
    }

    pub fn detail(mut self, detail: &str) -> Self {
        self.detail = Some(detail.to_string());
        self
    }

    pub fn failed(mut self) -> Self {
        self.success = false;
        self
    }
}

/// In-memory audit log with optional capacity limit.
pub struct AuditLog {
    events: Arc<RwLock<Vec<AuditEvent>>>,
    max_events: usize,
}

impl AuditLog {
    pub fn new(max_events: usize) -> Self {
        Self {
            events: Arc::new(RwLock::new(Vec::new())),
            max_events,
        }
    }

    pub async fn record(&self, event: AuditEvent) {
        tracing::info!(
            kind = ?event.kind,
            actor = ?event.actor,
            target = ?event.target,
            success = event.success,
            "audit: {}",
            event.detail.as_deref().unwrap_or("")
        );
        let mut events = self.events.write().await;
        events.push(event);
        let overflow = events.len().saturating_sub(self.max_events);
        if overflow > 0 {
            events.drain(..overflow);
        }
    }

    pub async fn query(&self, kind: Option<AuditEventKind>, limit: usize) -> Vec<AuditEvent> {
        let events = self.events.read().await;
        events
            .iter()
            .rev()
            .filter(|e| kind.is_none_or(|k| e.kind == k))
            .take(limit)
            .cloned()
            .collect()
    }

    pub async fn count(&self) -> usize {
        self.events.read().await.len()
    }

    pub async fn clear(&self) {
        self.events.write().await.clear();
    }
}

impl Default for AuditLog {
    fn default() -> Self {
        Self::new(10_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_audit_event_builder() {
        let event = AuditEvent::new(AuditEventKind::AuthAttempt)
            .actor("user1")
            .target("session-1")
            .detail("login attempt")
            .failed();
        assert_eq!(event.kind, AuditEventKind::AuthAttempt);
        assert_eq!(event.actor.as_deref(), Some("user1"));
        assert_eq!(event.target.as_deref(), Some("session-1"));
        assert!(!event.success);
    }

    #[tokio::test]
    async fn test_audit_log_record_and_query() {
        let log = AuditLog::new(100);
        log.record(AuditEvent::new(AuditEventKind::ToolCall).detail("bash"))
            .await;
        log.record(AuditEvent::new(AuditEventKind::AuthSuccess))
            .await;

        assert_eq!(log.count().await, 2);

        let tool_events = log.query(Some(AuditEventKind::ToolCall), 10).await;
        assert_eq!(tool_events.len(), 1);

        let all = log.query(None, 10).await;
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_audit_log_capacity() {
        let log = AuditLog::new(3);
        for i in 0..5 {
            log.record(AuditEvent::new(AuditEventKind::MessageSend).detail(&i.to_string()))
                .await;
        }
        assert_eq!(log.count().await, 3);
    }

    #[tokio::test]
    async fn test_audit_log_clear() {
        let log = AuditLog::new(100);
        log.record(AuditEvent::new(AuditEventKind::ConfigChange))
            .await;
        assert_eq!(log.count().await, 1);
        log.clear().await;
        assert_eq!(log.count().await, 0);
    }
}
