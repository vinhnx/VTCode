use anyhow::Result;
use async_trait::async_trait;

use super::signals::RewardSignal;

/// Minimal context passed to a policy when ranking actions.
#[derive(Debug, Clone, Default)]
pub struct PolicyContext {
    /// Optional latency budget hint (milliseconds).
    pub latency_budget_ms: Option<u64>,
    /// Optional token budget hint.
    pub token_budget: Option<u32>,
}

/// Decision returned by a policy.
#[derive(Debug, Clone)]
pub struct PolicyDecision {
    pub action: String,
    pub priority: f32,
}

#[async_trait]
pub trait ReinforcementPolicy: Send + Sync {
    async fn select_action(
        &self,
        actions: &[String],
        context: &PolicyContext,
    ) -> Result<PolicyDecision>;
    async fn update_reward(&self, action: &str, signal: &RewardSignal) -> Result<()>;
}
