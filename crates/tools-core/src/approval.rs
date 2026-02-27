//! Tool execution approval system

use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalPolicy {
    pub require_approval: HashSet<String>,
    pub auto_approve: HashSet<String>,
    pub deny: HashSet<String>,
}

impl Default for ApprovalPolicy {
    fn default() -> Self {
        let mut require = HashSet::new();
        require.insert("bash".into());
        require.insert("write_file".into());
        Self {
            require_approval: require,
            auto_approve: HashSet::new(),
            deny: HashSet::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalDecision {
    Approved,
    Denied,
    Pending,
}

/// Default TTL for approval requests: 5 minutes.
const APPROVAL_TTL_MS: u64 = 5 * 60 * 1000;

pub struct ApprovalGate {
    policy: ApprovalPolicy,
    pending: Arc<RwLock<Vec<ApprovalRequest>>>,
    auto_approve_all: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApprovalRequest {
    pub id: String,
    pub tool: String,
    pub arguments_summary: String,
    pub decision: ApprovalDecision,
    pub created_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_by: Option<String>,
}

impl ApprovalGate {
    pub fn new(policy: ApprovalPolicy) -> Self {
        Self {
            policy,
            pending: Arc::new(RwLock::new(Vec::new())),
            auto_approve_all: false,
        }
    }

    pub fn auto_approve(mut self) -> Self {
        self.auto_approve_all = true;
        self
    }

    pub fn check(&self, tool_name: &str) -> ApprovalDecision {
        if self.policy.deny.contains(tool_name) {
            return ApprovalDecision::Denied;
        }
        if self.auto_approve_all || self.policy.auto_approve.contains(tool_name) {
            return ApprovalDecision::Approved;
        }
        if self.policy.require_approval.contains(tool_name) {
            return ApprovalDecision::Pending;
        }
        ApprovalDecision::Approved
    }

    pub async fn request_approval(&self, tool: &str, args: &str) -> ApprovalRequest {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let req = ApprovalRequest {
            id: uuid::Uuid::new_v4().to_string(),
            tool: tool.into(),
            arguments_summary: args.chars().take(200).collect(),
            decision: ApprovalDecision::Pending,
            created_at_ms: now,
            resolved_at_ms: None,
            resolved_by: None,
        };
        self.pending.write().await.push(req.clone());
        req
    }

    pub async fn approve(&self, id: &str) -> bool {
        self.resolve(id, ApprovalDecision::Approved, None).await
    }

    pub async fn deny(&self, id: &str) -> bool {
        self.resolve(id, ApprovalDecision::Denied, None).await
    }

    pub async fn resolve(
        &self,
        id: &str,
        decision: ApprovalDecision,
        resolved_by: Option<String>,
    ) -> bool {
        let mut pending = self.pending.write().await;
        if let Some(req) = pending.iter_mut().find(|r| r.id == id) {
            req.decision = decision;
            if decision != ApprovalDecision::Pending {
                req.resolved_at_ms = Some(Self::now_ms());
                req.resolved_by = resolved_by;
            } else {
                req.resolved_at_ms = None;
                req.resolved_by = None;
            }
            true
        } else {
            false
        }
    }

    pub async fn get_request(&self, id: &str) -> Option<ApprovalRequest> {
        self.evict_expired_pending().await;
        let pending = self.pending.read().await;
        pending.iter().find(|r| r.id == id).cloned()
    }

    pub async fn list_requests(&self) -> Vec<ApprovalRequest> {
        self.evict_expired_pending().await;
        self.pending.read().await.clone()
    }

    pub async fn pending_requests(&self) -> Vec<ApprovalRequest> {
        self.evict_expired_pending().await;
        let pending = self.pending.read().await;
        pending
            .iter()
            .filter(|r| r.decision == ApprovalDecision::Pending)
            .cloned()
            .collect()
    }

    async fn evict_expired_pending(&self) {
        let now = Self::now_ms();
        let mut pending = self.pending.write().await;
        pending.retain(|r| {
            r.decision != ApprovalDecision::Pending
                || now.saturating_sub(r.created_at_ms) < APPROVAL_TTL_MS
        });
    }

    fn now_ms() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
}

impl Default for ApprovalGate {
    fn default() -> Self {
        Self::new(ApprovalPolicy::default())
    }
}

impl Serialize for ApprovalDecision {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(match self {
            Self::Approved => "approved",
            Self::Denied => "denied",
            Self::Pending => "pending",
        })
    }
}
