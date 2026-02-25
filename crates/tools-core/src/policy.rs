use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::profiles::ToolProfile;
use crate::groups;

/// Per-tool allow/deny policy.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ToolPolicy {
    /// If `Some`, only these tools are allowed. `None` = all allowed.
    pub allow: Option<Vec<String>>,
    /// Always denied tools.
    pub deny: Vec<String>,
    /// Tools that require human approval before execution.
    pub require_approval: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolPolicyDecision {
    Allowed,
    Denied(String),
    NeedsApproval,
}

impl ToolPolicy {
    pub fn is_allowed(&self, tool_name: &str) -> ToolPolicyDecision {
        if self.deny.iter().any(|d| d == tool_name) {
            return ToolPolicyDecision::Denied(format!("Tool '{}' is denied by policy", tool_name));
        }
        if let Some(ref allow) = self.allow
            && !allow.iter().any(|a| a == tool_name)
        {
            return ToolPolicyDecision::Denied(format!(
                "Tool '{}' is not in the allow list",
                tool_name
            ));
        }
        if self.require_approval.iter().any(|r| r == tool_name) {
            return ToolPolicyDecision::NeedsApproval;
        }
        ToolPolicyDecision::Allowed
    }

    /// Merge two policies. The `override_policy` takes precedence.
    pub fn merge(&self, override_policy: &ToolPolicy) -> ToolPolicy {
        let allow = override_policy.allow.clone().or_else(|| self.allow.clone());
        let mut deny = self.deny.clone();
        for d in &override_policy.deny {
            if !deny.contains(d) {
                deny.push(d.clone());
            }
        }
        let mut require_approval = self.require_approval.clone();
        for r in &override_policy.require_approval {
            if !require_approval.contains(r) {
                require_approval.push(r.clone());
            }
        }
        ToolPolicy { allow, deny, require_approval }
    }
}

/// Layered policy pipeline: global → channel → session.
pub struct ToolPolicyPipeline {
    pub global_policy: ToolPolicy,
    pub channel_policies: HashMap<String, ToolPolicy>,
    pub session_policies: HashMap<String, ToolPolicy>,
}

impl ToolPolicyPipeline {
    pub fn new(global: ToolPolicy) -> Self {
        Self {
            global_policy: global,
            channel_policies: HashMap::new(),
            session_policies: HashMap::new(),
        }
    }

    pub fn check(
        &self,
        tool_name: &str,
        channel: Option<&str>,
        session: Option<&str>,
    ) -> ToolPolicyDecision {
        let mut merged = self.global_policy.clone();
        if let Some(ch) = channel
            && let Some(cp) = self.channel_policies.get(ch)
        {
            merged = merged.merge(cp);
        }
        if let Some(sess) = session
            && let Some(sp) = self.session_policies.get(sess)
        {
            merged = merged.merge(sp);
        }
        merged.is_allowed(tool_name)
    }
}

// ─── 7-Layer Policy Pipeline ───────────────────────────────────────

/// Context passed to the policy pipeline for evaluation.
#[derive(Debug, Clone, Default)]
pub struct PolicyContext {
    pub channel: Option<String>,
    pub session: Option<String>,
    pub agent_id: Option<String>,
    pub provider: Option<String>,
}

/// A single policy layer in the 7-layer pipeline.
#[derive(Debug, Clone)]
pub enum PolicyLayer {
    /// Layer 1: Base profile (Minimal, Coding, Messaging, Full).
    Profile(ToolProfile),
    /// Layer 2: Per-provider profile override.
    ProviderProfile(String, ToolProfile),
    /// Layer 3: Global allow/deny list (supports group refs).
    GlobalAllow(Vec<String>),
    /// Layer 4: Per-provider global allow list.
    GlobalProviderAllow(String, Vec<String>),
    /// Layer 5: Per-agent allow list.
    AgentAllow(String, Vec<String>),
    /// Layer 6: Per-agent per-provider allow list.
    AgentProviderAllow(String, String, Vec<String>),
    /// Layer 7: Channel/group level allow list.
    GroupAllow(String, Vec<String>),
}

/// 7-layer policy pipeline that evaluates layers in order.
///
/// Evaluation logic:
/// - Each layer can either ALLOW or be silent (not applicable).
/// - A tool is allowed if at least one applicable layer explicitly allows it.
/// - If no layer allows the tool, it falls back to the base ToolPolicy check.
pub struct LayeredPolicyPipeline {
    layers: Vec<PolicyLayer>,
    fallback: ToolPolicy,
}

impl LayeredPolicyPipeline {
    pub fn new(fallback: ToolPolicy) -> Self {
        Self {
            layers: Vec::new(),
            fallback,
        }
    }

    pub fn add_layer(&mut self, layer: PolicyLayer) {
        self.layers.push(layer);
    }

    pub fn with_layer(mut self, layer: PolicyLayer) -> Self {
        self.layers.push(layer);
        self
    }

    /// Evaluate whether a tool is allowed given the context.
    ///
    /// Later layers override earlier ones, so more-specific layers
    /// (e.g. GroupAllow) can override less-specific ones (e.g. Profile).
    /// Explicit deny in the fallback policy always wins.
    pub fn evaluate(&self, tool_name: &str, ctx: &PolicyContext) -> ToolPolicyDecision {
        // Deny in fallback always wins
        if self.fallback.deny.iter().any(|d| d == tool_name) {
            return ToolPolicyDecision::Denied(
                format!("Tool '{}' is denied by policy", tool_name),
            );
        }

        // Walk layers; later applicable layers override earlier ones
        let mut last_decision: Option<ToolPolicyDecision> = None;
        for layer in &self.layers {
            if let Some(decision) = self.evaluate_layer(layer, tool_name, ctx) {
                last_decision = Some(decision);
            }
        }

        // Use last applicable layer decision, or fall back to base policy
        last_decision.unwrap_or_else(|| self.fallback.is_allowed(tool_name))
    }

    fn evaluate_layer(
        &self,
        layer: &PolicyLayer,
        tool_name: &str,
        ctx: &PolicyContext,
    ) -> Option<ToolPolicyDecision> {
        match layer {
            PolicyLayer::Profile(profile) => {
                if profile.allows_tool(tool_name) {
                    Some(ToolPolicyDecision::Allowed)
                } else {
                    Some(ToolPolicyDecision::Denied(
                        format!("Tool '{}' not in profile {:?}", tool_name, profile),
                    ))
                }
            }
            PolicyLayer::ProviderProfile(provider, profile) => {
                if ctx.provider.as_deref() != Some(provider) {
                    return None;
                }
                if profile.allows_tool(tool_name) {
                    Some(ToolPolicyDecision::Allowed)
                } else {
                    Some(ToolPolicyDecision::Denied(
                        format!("Tool '{}' not in provider profile", tool_name),
                    ))
                }
            }
            PolicyLayer::GlobalAllow(list) => {
                let expanded = groups::expand_tool_list(list);
                if expanded.iter().any(|t| t == tool_name || t == "*") {
                    Some(ToolPolicyDecision::Allowed)
                } else {
                    None
                }
            }
            PolicyLayer::GlobalProviderAllow(provider, list) => {
                if ctx.provider.as_deref() != Some(provider) {
                    return None;
                }
                let expanded = groups::expand_tool_list(list);
                if expanded.iter().any(|t| t == tool_name || t == "*") {
                    Some(ToolPolicyDecision::Allowed)
                } else {
                    None
                }
            }
            PolicyLayer::AgentAllow(agent_id, list) => {
                if ctx.agent_id.as_deref() != Some(agent_id) {
                    return None;
                }
                let expanded = groups::expand_tool_list(list);
                if expanded.iter().any(|t| t == tool_name || t == "*") {
                    Some(ToolPolicyDecision::Allowed)
                } else {
                    None
                }
            }
            PolicyLayer::AgentProviderAllow(agent_id, provider, list) => {
                if ctx.agent_id.as_deref() != Some(agent_id)
                    || ctx.provider.as_deref() != Some(provider)
                {
                    return None;
                }
                let expanded = groups::expand_tool_list(list);
                if expanded.iter().any(|t| t == tool_name || t == "*") {
                    Some(ToolPolicyDecision::Allowed)
                } else {
                    None
                }
            }
            PolicyLayer::GroupAllow(channel, list) => {
                if ctx.channel.as_deref() != Some(channel) {
                    return None;
                }
                let expanded = groups::expand_tool_list(list);
                if expanded.iter().any(|t| t == tool_name || t == "*") {
                    Some(ToolPolicyDecision::Allowed)
                } else {
                    None
                }
            }
        }
    }

    pub fn layers(&self) -> &[PolicyLayer] {
        &self.layers
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_allows_all() {
        let p = ToolPolicy::default();
        assert_eq!(p.is_allowed("bash"), ToolPolicyDecision::Allowed);
    }

    #[test]
    fn deny_takes_precedence() {
        let p = ToolPolicy {
            allow: Some(vec!["bash".into()]),
            deny: vec!["bash".into()],
            require_approval: vec![],
        };
        assert!(matches!(p.is_allowed("bash"), ToolPolicyDecision::Denied(_)));
    }

    #[test]
    fn allow_list_filters() {
        let p = ToolPolicy {
            allow: Some(vec!["read_file".into()]),
            deny: vec![],
            require_approval: vec![],
        };
        assert_eq!(p.is_allowed("read_file"), ToolPolicyDecision::Allowed);
        assert!(matches!(p.is_allowed("bash"), ToolPolicyDecision::Denied(_)));
    }

    #[test]
    fn merge_combines() {
        let base = ToolPolicy {
            allow: None,
            deny: vec!["dangerous".into()],
            require_approval: vec!["bash".into()],
        };
        let over = ToolPolicy {
            allow: Some(vec!["bash".into(), "read_file".into()]),
            deny: vec!["write_file".into()],
            require_approval: vec![],
        };
        let merged = base.merge(&over);
        assert!(merged.deny.contains(&"dangerous".into()));
        assert!(merged.deny.contains(&"write_file".into()));
        assert!(merged.allow.is_some());
    }

    #[test]
    fn pipeline_layered() {
        let global = ToolPolicy::default();
        let mut pipeline = ToolPolicyPipeline::new(global);
        pipeline.channel_policies.insert(
            "telegram".into(),
            ToolPolicy { allow: None, deny: vec!["bash".into()], require_approval: vec![] },
        );
        assert!(matches!(
            pipeline.check("bash", Some("telegram"), None),
            ToolPolicyDecision::Denied(_)
        ));
        assert_eq!(
            pipeline.check("bash", Some("slack"), None),
            ToolPolicyDecision::Allowed,
        );
    }

    #[test]
    fn layered_profile_allows() {
        let pipeline = LayeredPolicyPipeline::new(ToolPolicy::default())
            .with_layer(PolicyLayer::Profile(ToolProfile::Coding));
        let ctx = PolicyContext::default();
        assert_eq!(pipeline.evaluate("bash", &ctx), ToolPolicyDecision::Allowed);
        assert_eq!(pipeline.evaluate("read_file", &ctx), ToolPolicyDecision::Allowed);
        assert!(matches!(pipeline.evaluate("message", &ctx), ToolPolicyDecision::Denied(_)));
    }

    #[test]
    fn layered_profile_minimal_restricts() {
        let pipeline = LayeredPolicyPipeline::new(ToolPolicy::default())
            .with_layer(PolicyLayer::Profile(ToolProfile::Minimal));
        let ctx = PolicyContext::default();
        assert_eq!(pipeline.evaluate("session_status", &ctx), ToolPolicyDecision::Allowed);
        assert!(matches!(pipeline.evaluate("bash", &ctx), ToolPolicyDecision::Denied(_)));
    }

    #[test]
    fn layered_deny_overrides_profile() {
        let fallback = ToolPolicy {
            allow: None,
            deny: vec!["bash".into()],
            require_approval: vec![],
        };
        let pipeline = LayeredPolicyPipeline::new(fallback)
            .with_layer(PolicyLayer::Profile(ToolProfile::Full));
        let ctx = PolicyContext::default();
        assert!(matches!(pipeline.evaluate("bash", &ctx), ToolPolicyDecision::Denied(_)));
    }

    #[test]
    fn layered_group_allow_channel() {
        let pipeline = LayeredPolicyPipeline::new(ToolPolicy::default())
            .with_layer(PolicyLayer::Profile(ToolProfile::Minimal))
            .with_layer(PolicyLayer::GroupAllow(
                "telegram".into(),
                vec!["bash".into()],
            ));
        let ctx_tg = PolicyContext {
            channel: Some("telegram".into()),
            ..Default::default()
        };
        let ctx_dc = PolicyContext {
            channel: Some("discord".into()),
            ..Default::default()
        };
        assert_eq!(pipeline.evaluate("bash", &ctx_tg), ToolPolicyDecision::Allowed);
        assert!(matches!(pipeline.evaluate("bash", &ctx_dc), ToolPolicyDecision::Denied(_)));
    }
}
