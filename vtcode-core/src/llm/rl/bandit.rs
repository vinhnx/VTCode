use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use tokio::sync::Mutex;

use crate::config::BanditConfig;

use super::policy::{PolicyContext, PolicyDecision, ReinforcementPolicy};
use super::signals::RewardSignal;

#[derive(Debug, Default)]
struct BanditState {
    scores: HashMap<String, f32>,
    counts: HashMap<String, u32>,
}

/// Simple epsilon-greedy bandit for action selection.
#[derive(Debug)]
pub struct EpsilonGreedyBandit {
    config: BanditConfig,
    state: Mutex<BanditState>,
}

impl EpsilonGreedyBandit {
    pub fn new(config: BanditConfig) -> Self {
        Self {
            config,
            state: Mutex::new(BanditState::default()),
        }
    }

    fn pseudo_random_index(&self, len: usize) -> usize {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        (nanos as usize) % len
    }
}

#[async_trait::async_trait]
impl ReinforcementPolicy for EpsilonGreedyBandit {
    async fn select_action(
        &self,
        actions: &[String],
        _context: &PolicyContext,
    ) -> Result<PolicyDecision> {
        let mut state = self.state.lock().await;
        if actions.is_empty() {
            return Err(anyhow::anyhow!("no actions provided to bandit"));
        }

        for action in actions {
            state.scores.entry(action.clone()).or_insert(0.0);
            state.counts.entry(action.clone()).or_insert(0);
        }

        let explore =
            self.pseudo_random_index(1000) as f32 / 1000.0 <= self.config.exploration_epsilon;
        let chosen = if explore {
            let idx = self.pseudo_random_index(actions.len());
            actions[idx].clone()
        } else {
            actions
                .iter()
                .max_by(|a, b| {
                    state
                        .scores
                        .get(*a)
                        .unwrap_or(&0.0)
                        .partial_cmp(state.scores.get(*b).unwrap_or(&0.0))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .cloned()
                .unwrap_or_else(|| actions[0].clone())
        };

        let priority = *state.scores.get(&chosen).unwrap_or(&0.0);
        Ok(PolicyDecision {
            action: chosen,
            priority,
        })
    }

    async fn update_reward(&self, action: &str, signal: &RewardSignal) -> Result<()> {
        let mut state = self.state.lock().await;
        let reward = signal.reward_value(1.0, -1.0, self.config.latency_weight);
        let key = action.to_string();

        let count_value = {
            let count_entry = state.counts.entry(key.clone()).or_insert(0);
            *count_entry += 1;
            *count_entry
        };

        let entry = state.scores.entry(key.clone()).or_insert(0.0);
        let weight = 1.0 / (count_value as f32 + 1.0);
        *entry = (*entry * (1.0 - weight)) + (reward * weight);

        if self.config.rolling_window > 0
            && count_value as usize > self.config.rolling_window
            && let Some(count_entry) = state.counts.get_mut(&key)
        {
            *count_entry = self.config.rolling_window as u32;
        }

        Ok(())
    }
}
