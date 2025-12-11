use std::collections::HashMap;

use anyhow::Result;
use tokio::sync::Mutex;

use crate::config::ActorCriticConfig;

use super::policy::{PolicyContext, PolicyDecision, ReinforcementPolicy};
use super::signals::RewardSignal;

#[derive(Debug, Default)]
struct ActorCriticState {
    value: HashMap<String, f32>,
    advantage: HashMap<String, f32>,
}

/// Lightweight actor-critic variant for prioritizing actions.
#[derive(Debug)]
pub struct ActorCriticPolicy {
    config: ActorCriticConfig,
    state: Mutex<ActorCriticState>,
}

impl ActorCriticPolicy {
    pub fn new(config: ActorCriticConfig) -> Self {
        Self {
            config,
            state: Mutex::new(ActorCriticState::default()),
        }
    }
}

#[async_trait::async_trait]
impl ReinforcementPolicy for ActorCriticPolicy {
    async fn select_action(
        &self,
        actions: &[String],
        _context: &PolicyContext,
    ) -> Result<PolicyDecision> {
        let mut state = self.state.lock().await;
        if actions.is_empty() {
            return Err(anyhow::anyhow!("no actions provided to actor-critic"));
        }

        for action in actions {
            state.value.entry(action.clone()).or_insert(0.0);
            state.advantage.entry(action.clone()).or_insert(0.0);
        }

        let chosen = actions
            .iter()
            .max_by(|a, b| {
                let score_a =
                    state.value.get(*a).unwrap_or(&0.0) + state.advantage.get(*a).unwrap_or(&0.0);
                let score_b =
                    state.value.get(*b).unwrap_or(&0.0) + state.advantage.get(*b).unwrap_or(&0.0);
                score_a
                    .partial_cmp(&score_b)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .cloned()
            .unwrap_or_else(|| actions[0].clone());

        let priority = state.value.get(&chosen).copied().unwrap_or(0.0)
            + state.advantage.get(&chosen).copied().unwrap_or(0.0);

        Ok(PolicyDecision {
            action: chosen,
            priority,
        })
    }

    async fn update_reward(&self, action: &str, signal: &RewardSignal) -> Result<()> {
        let mut state = self.state.lock().await;
        let current_value = *state.value.entry(action.to_string()).or_insert(0.0);
        let reward = signal.reward_value(1.0, -1.0, 0.0);
        let delta = reward + (self.config.discount_factor * current_value) - current_value;

        let value_slot = state.value.entry(action.to_string()).or_insert(0.0);
        *value_slot = current_value + self.config.learning_rate * delta;

        let adv_entry = state.advantage.entry(action.to_string()).or_insert(0.0);
        *adv_entry = (*adv_entry * (1.0 - self.config.trace_decay)) + delta;

        Ok(())
    }
}
