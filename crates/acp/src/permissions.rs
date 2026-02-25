//! ACP tool permissions — controls which tools a session can invoke.

use std::collections::HashSet;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AcpPermissions {
    allowed_tools: HashSet<String>,
    denied_tools: HashSet<String>,
    require_approval: bool,
}

impl AcpPermissions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allow_all() -> Self {
        Self {
            allowed_tools: HashSet::from(["*".to_string()]),
            denied_tools: HashSet::new(),
            require_approval: false,
        }
    }

    pub fn allow(&mut self, tool: &str) {
        self.allowed_tools.insert(tool.to_string());
    }

    pub fn deny(&mut self, tool: &str) {
        self.denied_tools.insert(tool.to_string());
    }

    pub fn set_require_approval(&mut self, require: bool) {
        self.require_approval = require;
    }

    pub fn check(&self, tool_name: &str) -> PermissionDecision {
        if self.denied_tools.contains(tool_name) {
            return PermissionDecision::Denied;
        }
        if self.allowed_tools.contains("*") || self.allowed_tools.contains(tool_name) {
            if self.require_approval {
                return PermissionDecision::NeedsApproval;
            }
            return PermissionDecision::Allowed;
        }
        PermissionDecision::Denied
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    Allowed,
    Denied,
    NeedsApproval,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allow_all() {
        let perms = AcpPermissions::allow_all();
        assert_eq!(perms.check("bash"), PermissionDecision::Allowed);
        assert_eq!(perms.check("anything"), PermissionDecision::Allowed);
    }

    #[test]
    fn test_deny_overrides() {
        let mut perms = AcpPermissions::allow_all();
        perms.deny("bash");
        assert_eq!(perms.check("bash"), PermissionDecision::Denied);
        assert_eq!(perms.check("web_search"), PermissionDecision::Allowed);
    }

    #[test]
    fn test_require_approval() {
        let mut perms = AcpPermissions::allow_all();
        perms.set_require_approval(true);
        assert_eq!(perms.check("bash"), PermissionDecision::NeedsApproval);
    }
}
