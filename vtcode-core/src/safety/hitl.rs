use std::time::SystemTime;

use anyhow::Result;

use crate::tools::registry::RiskLevel;

/// Decision rendered by the human-in-the-loop gate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OversightDecision {
    Allow,
    Deny,
    RequireApproval,
}

/// Configurable gate that maps risk into oversight requirements.
#[derive(Debug, Clone)]
pub struct HitlGate {
    pub require_explainability: bool,
    pub emergency_override_enabled: bool,
}

impl HitlGate {
    pub fn new(require_explainability: bool, emergency_override_enabled: bool) -> Self {
        Self {
            require_explainability,
            emergency_override_enabled,
        }
    }

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
#[derive(Debug, Clone)]
pub struct HitlEvent {
    pub decision: OversightDecision,
    pub reason: String,
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
            at: SystemTime::now(),
        });
    }

    pub fn events(&self) -> &[HitlEvent] {
        &self.events
    }
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
}
