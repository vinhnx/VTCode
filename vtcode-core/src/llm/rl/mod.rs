//! Reinforcement learning primitives for adaptive tool/plan selection.

mod actor_critic;
mod bandit;
mod policy;
mod signals;

use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use tokio::sync::Mutex;

pub use actor_critic::ActorCriticPolicy;
pub use bandit::EpsilonGreedyBandit;
pub use policy::{PolicyContext, PolicyDecision, ReinforcementPolicy};
pub use signals::{RewardLedger, RewardSignal};

use crate::config::{
    ActorCriticConfig, BanditConfig, ReinforcementLearningConfig, RlStrategy, RewardShapingConfig,
};

/// Wraps a reinforcement policy with reward tracking.
#[derive(Debug)]
pub struct RlEngine {
    policy: Arc<dyn ReinforcementPolicy>,
    reward_ledger: Mutex<RewardLedger>,
    reward_config: RewardShapingConfig,
}

impl RlEngine {
    pub fn from_config(config: &ReinforcementLearningConfig) -> Self {
        let policy: Arc<dyn ReinforcementPolicy> = match config.strategy {
            RlStrategy::Bandit => Arc::new(EpsilonGreedyBandit::new(config.bandit.clone())),
            RlStrategy::ActorCritic => Arc::new(ActorCriticPolicy::new(config.actor_critic.clone())),
        };

        Self {
            policy,
            reward_ledger: Mutex::new(RewardLedger::new(256)),
            reward_config: config.reward_shaping.clone(),
        }
    }

    pub async fn select(&self, actions: &[String], context: PolicyContext) -> Result<PolicyDecision> {
        ensure!(!actions.is_empty(), "no candidate actions provided");
        self.policy.select_action(actions, &context).await
    }

    pub async fn apply_reward(&self, signal: RewardSignal) -> Result<()> {
        {
            let mut ledger = self.reward_ledger.lock().await;
            ledger.push(signal.clone());
        }

        let reward_value = signal.reward_value(
            self.reward_config.success_reward,
            self.reward_config.timeout_penalty,
            self.reward_config.latency_penalty_weight,
        );

        let weighted_signal = RewardSignal {
            action: signal.action.clone(),
            success: signal.success,
            latency_ms: signal.latency_ms,
            tokens_used: signal.tokens_used,
            emitted_at: signal.emitted_at,
        };

        self.policy
            .update_reward(&signal.action, &weighted_signal)
            .await
            .with_context(|| format!("failed to update reward for {}", signal.action))?;

        tracing::debug!(
            action = signal.action,
            success = signal.success,
            reward = reward_value,
            "applied RL reward"
        );

        Ok(())
    }

    pub async fn into_policy(self) -> Arc<dyn ReinforcementPolicy> {
        self.policy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bandit_selects_action() {
        let engine = RlEngine::from_config(&ReinforcementLearningConfig {
            enabled: true,
            ..ReinforcementLearningConfig::default()
        });

        let decision = engine
            .select(
                &[String::from("a"), String::from("b")],
                PolicyContext::default(),
            )
            .await
            .expect("decision should succeed");
        assert!(!decision.action.is_empty());
    }
}
