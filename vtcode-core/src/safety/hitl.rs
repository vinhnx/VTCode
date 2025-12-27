use std::collections::HashSet;
use std::time::SystemTime;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::tools::registry::RiskLevel;

/// Decision rendered by the human-in-the-loop gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OversightDecision {
    Allow,
    Deny,
    RequireApproval,
}

/// Formal HITL policy configuration with explicit whitelist/blacklist
#[derive(Debug, Clone, Default)]
pub struct HitlPolicy {
    /// Tools that are automatically approved without user confirmation
    pub auto_approve_tools: HashSet<String>,
    /// Tools that are always denied regardless of other settings
    pub always_deny_tools: HashSet<String>,
    /// Whether to require approval for tools not in either list
    pub default_require_approval: bool,
}

impl HitlPolicy {
    pub fn new() -> Self {
        Self {
            auto_approve_tools: HashSet::new(),
            always_deny_tools: HashSet::new(),
            default_require_approval: true,
        }
    }

    /// Create a permissive policy (auto-approve common safe tools)
    pub fn permissive() -> Self {
        let mut policy = Self::new();
        policy.default_require_approval = false;
        // Common safe read-only tools
        for tool in &["list_files", "read_file", "grep_search", "view_file"] {
            policy.auto_approve_tools.insert(tool.to_string());
        }
        policy
    }

    /// Create a strict policy (require approval for everything)
    pub fn strict() -> Self {
        Self {
            auto_approve_tools: HashSet::new(),
            always_deny_tools: HashSet::new(),
            default_require_approval: true,
        }
    }

    /// Check policy for a specific tool
    pub fn check_tool(&self, tool_name: &str) -> OversightDecision {
        if self.always_deny_tools.contains(tool_name) {
            OversightDecision::Deny
        } else if self.auto_approve_tools.contains(tool_name) {
            OversightDecision::Allow
        } else if self.default_require_approval {
            OversightDecision::RequireApproval
        } else {
            OversightDecision::Allow
        }
    }

    /// Add a tool to the auto-approve whitelist
    pub fn whitelist_tool(&mut self, tool_name: impl Into<String>) {
        let name = tool_name.into();
        self.always_deny_tools.remove(&name);
        self.auto_approve_tools.insert(name);
    }

    /// Add a tool to the always-deny blacklist
    pub fn blacklist_tool(&mut self, tool_name: impl Into<String>) {
        let name = tool_name.into();
        self.auto_approve_tools.remove(&name);
        self.always_deny_tools.insert(name);
    }
}

/// Configurable gate that maps risk into oversight requirements.
#[derive(Debug, Clone)]
pub struct HitlGate {
    pub require_explainability: bool,
    pub emergency_override_enabled: bool,
    pub policy: HitlPolicy,
}

impl HitlGate {
    pub fn new(require_explainability: bool, emergency_override_enabled: bool) -> Self {
        Self {
            require_explainability,
            emergency_override_enabled,
            policy: HitlPolicy::new(),
        }
    }

    /// Create gate with a specific policy
    pub fn with_policy(policy: HitlPolicy) -> Self {
        Self {
            require_explainability: true,
            emergency_override_enabled: false,
            policy,
        }
    }

    /// Decide based on risk level (original behavior)
    pub fn decide(&self, risk: RiskLevel) -> OversightDecision {
        match risk {
            RiskLevel::High => OversightDecision::RequireApproval,
            RiskLevel::Medium => {
                if self.require_explainability {
                    OversightDecision::RequireApproval
                } else {
                    OversightDecision::Allow
                }
            }
            RiskLevel::Low => OversightDecision::Allow,
            RiskLevel::Critical => OversightDecision::RequireApproval,
        }
    }

    /// Decide for a specific tool, considering both risk and policy
    pub fn decide_for_tool(&self, tool_name: &str, risk: RiskLevel) -> OversightDecision {
        // Policy blacklist takes absolute precedence
        let policy_decision = self.policy.check_tool(tool_name);
        if policy_decision == OversightDecision::Deny {
            return OversightDecision::Deny;
        }

        // Policy whitelist overrides risk-based decision
        if policy_decision == OversightDecision::Allow {
            return OversightDecision::Allow;
        }

        // Fall back to risk-based decision
        self.decide(risk)
    }

    pub fn override_decision(
        &self,
        decision: OversightDecision,
        reason: impl Into<String>,
        trail: &mut HitlAuditTrail,
    ) -> Result<()> {
        if !self.emergency_override_enabled {
            anyhow::bail!("emergency override disabled");
        }

        trail.record(decision, reason);
        Ok(())
    }
}

/// Audit record for HITL decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HitlEvent {
    pub decision: OversightDecision,
    pub reason: String,
    pub tool_name: Option<String>,
    pub at: SystemTime,
}

#[derive(Debug, Default, Clone)]
pub struct HitlAuditTrail {
    events: Vec<HitlEvent>,
}

impl HitlAuditTrail {
    pub fn record(&mut self, decision: OversightDecision, reason: impl Into<String>) {
        self.events.push(HitlEvent {
            decision,
            reason: reason.into(),
            tool_name: None,
            at: SystemTime::now(),
        });
    }

    /// Record a decision for a specific tool
    pub fn record_tool_decision(
        &mut self,
        tool_name: impl Into<String>,
        decision: OversightDecision,
        reason: impl Into<String>,
    ) {
        self.events.push(HitlEvent {
            decision,
            reason: reason.into(),
            tool_name: Some(tool_name.into()),
            at: SystemTime::now(),
        });
    }

    pub fn events(&self) -> &[HitlEvent] {
        &self.events
    }

    /// Get events for a specific tool
    pub fn events_for_tool(&self, tool_name: &str) -> Vec<&HitlEvent> {
        self.events
            .iter()
            .filter(|e| e.tool_name.as_deref() == Some(tool_name))
            .collect()
    }

    /// Export audit trail as JSON for security logging
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string_pretty(&self.events).map_err(Into::into)
    }

    /// Get statistics about decisions
    pub fn statistics(&self) -> HitlStatistics {
        let mut stats = HitlStatistics::default();
        for event in &self.events {
            match event.decision {
                OversightDecision::Allow => stats.allowed += 1,
                OversightDecision::Deny => stats.denied += 1,
                OversightDecision::RequireApproval => stats.required_approval += 1,
            }
        }
        stats.total = self.events.len();
        stats
    }

    /// Clear old events (for memory management)
    pub fn prune_old_events(&mut self, max_count: usize) -> usize {
        if self.events.len() <= max_count {
            return 0;
        }
        let excess = self.events.len() - max_count;
        self.events.drain(0..excess);
        excess
    }
}

/// Statistics about HITL decisions
#[derive(Debug, Default, Clone)]
pub struct HitlStatistics {
    pub total: usize,
    pub allowed: usize,
    pub denied: usize,
    pub required_approval: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_required_for_high_risk() {
        let gate = HitlGate::new(true, true);
        assert_eq!(
            gate.decide(RiskLevel::High),
            OversightDecision::RequireApproval
        );
    }

    #[test]
    fn policy_whitelist_auto_approves() {
        let mut policy = HitlPolicy::new();
        policy.whitelist_tool("read_file");

        assert_eq!(policy.check_tool("read_file"), OversightDecision::Allow);
        assert_eq!(
            policy.check_tool("write_file"),
            OversightDecision::RequireApproval
        );
    }

    #[test]
    fn policy_blacklist_denies() {
        let mut policy = HitlPolicy::new();
        policy.blacklist_tool("dangerous_tool");

        assert_eq!(policy.check_tool("dangerous_tool"), OversightDecision::Deny);
    }

    #[test]
    fn blacklist_overrides_whitelist() {
        let mut policy = HitlPolicy::new();
        policy.whitelist_tool("tool");
        policy.blacklist_tool("tool");

        // Blacklist should win
        assert_eq!(policy.check_tool("tool"), OversightDecision::Deny);
    }

    #[test]
    fn decide_for_tool_respects_policy() {
        let policy = HitlPolicy::permissive();
        let gate = HitlGate::with_policy(policy);

        // Whitelisted tool is allowed even with high risk
        assert_eq!(
            gate.decide_for_tool("read_file", RiskLevel::High),
            OversightDecision::Allow
        );
    }

    #[test]
    fn audit_trail_statistics() {
        let mut trail = HitlAuditTrail::default();
        trail.record(OversightDecision::Allow, "allowed 1");
        trail.record(OversightDecision::Allow, "allowed 2");
        trail.record(OversightDecision::Deny, "denied 1");

        let stats = trail.statistics();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.allowed, 2);
        assert_eq!(stats.denied, 1);
    }

    #[test]
    fn audit_trail_json_export() {
        let mut trail = HitlAuditTrail::default();
        trail.record_tool_decision("test_tool", OversightDecision::Allow, "test reason");

        let json = trail.to_json().unwrap();
        assert!(json.contains("test_tool"));
        assert!(json.contains("Allow"));
    }
}
