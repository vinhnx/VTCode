//! Reward signal and selection-strategy types for the RL optimization loop.

/// Selection strategy for the RL engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RlStrategy {
    /// Upper-confidence-bound / epsilon-greedy bandit (default).
    Bandit,
    /// Actor-critic stand-in (currently routes through the bandit core).
    ActorCritic,
}

impl RlStrategy {
    /// Parse a strategy name from config (`actor_critic` → `ActorCritic`).
    #[must_use]
    pub fn parse(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "actor_critic" | "actor-critic" => RlStrategy::ActorCritic,
            _ => RlStrategy::Bandit,
        }
    }
}

/// A scalar outcome signal for one action execution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RewardSignal {
    /// Whether the action succeeded.
    pub success: bool,
    /// Wall-clock latency in seconds.
    pub latency_secs: f64,
    /// Estimated cost in USD.
    pub cost_usd: f64,
}

impl RewardSignal {
    /// Combined reward in `(-1.0, 1.0]`: `1.0` for a fast, successful,
    /// cost-free action; negative for failures. `latency_weight` trades off
    /// speed against success at *record* time.
    #[must_use]
    pub fn score(&self, latency_weight: f64) -> f64 {
        let w = latency_weight.clamp(0.0, 1.0);
        let success_term = if self.success { 1.0 } else { -1.0 };
        // Latency penalty: 0 at instant, approaching -1 as latency grows large.
        let latency_penalty = -(self.latency_secs / (1.0 + self.latency_secs));
        let cost_penalty = -(self.cost_usd / (1.0 + self.cost_usd));
        success_term * (1.0 - w) + latency_penalty * w + cost_penalty * 0.1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strategy_parse_is_case_insensitive() {
        assert_eq!(RlStrategy::parse("actor_critic"), RlStrategy::ActorCritic);
        assert_eq!(RlStrategy::parse("Actor-Critic"), RlStrategy::ActorCritic);
        assert_eq!(RlStrategy::parse("bandit"), RlStrategy::Bandit);
        assert_eq!(RlStrategy::parse("anything"), RlStrategy::Bandit);
    }

    #[test]
    fn score_rewards_fast_success_and_penalizes_failure() {
        let good = RewardSignal {
            success: true,
            latency_secs: 0.1,
            cost_usd: 0.0,
        };
        let bad = RewardSignal {
            success: false,
            latency_secs: 10.0,
            cost_usd: 1.0,
        };
        assert!(good.score(0.5) > 0.0);
        assert!(bad.score(0.5) < 0.0);
    }
}
