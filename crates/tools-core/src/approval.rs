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
}

impl ApprovalGate {
    pub fn new(policy: ApprovalPolicy) -> Self {
        Self { policy, pending: Arc::new(RwLock::new(Vec::new())), auto_approve_all: false }
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
        let req = ApprovalRequest {
            id: uuid::Uuid::new_v4().to_string(),
            tool: tool.into(),
            arguments_summary: args.chars().take(200).collect(),
            decision: ApprovalDecision::Pending,
        };
        self.pending.write().await.push(req.clone());
        req
    }

    pub async fn approve(&self, id: &str) -> bool {
        let mut pending = self.pending.write().await;
        if let Some(req) = pending.iter_mut().find(|r| r.id == id) {
            req.decision = ApprovalDecision::Approved;
            true
        } else {
            false
        }
    }

    pub async fn deny(&self, id: &str) -> bool {
        let mut pending = self.pending.write().await;
        if let Some(req) = pending.iter_mut().find(|r| r.id == id) {
            req.decision = ApprovalDecision::Denied;
            true
        } else {
            false
        }
    }

    pub async fn pending_requests(&self) -> Vec<ApprovalRequest> {
        self.pending.read().await.iter()
            .filter(|r| r.decision == ApprovalDecision::Pending)
            .cloned()
            .collect()
    }
}

impl Default for ApprovalGate {
    fn default() -> Self { Self::new(ApprovalPolicy::default()) }
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
