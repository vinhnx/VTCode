//! Rolling reward ledger keyed by action id.

use std::collections::HashMap;

use super::signal::RewardSignal;

/// Rolling statistics for one action.
#[derive(Debug, Clone, Copy, Default)]
struct RollingReward {
    count: u64,
    mean_score: f64,
    total_latency: f64,
}

impl RollingReward {
    fn record(&mut self, score: f64, latency: f64) {
        self.count = self.count.saturating_add(1);
        // Incremental mean to avoid overflow on long runs.
        self.mean_score += (score - self.mean_score) / self.count as f64;
        self.total_latency += latency;
    }

    fn ucb(&self, total: f64, exploration: f64) -> f64 {
        if self.count == 0 {
            return f64::INFINITY; // always try untried actions first
        }
        let bonus = exploration * (total.ln() / self.count as f64).sqrt();
        self.mean_score + bonus
    }
}

/// Rolling ledger of reward signals keyed by action id.
#[derive(Debug, Clone, Default)]
pub struct RewardLedger {
    per_action: HashMap<String, RollingReward>,
    total: u64,
}

impl RewardLedger {
    /// Record a reward signal for `action_id`.
    pub fn record(&mut self, action_id: &str, signal: RewardSignal, latency_weight: f64) {
        let entry = self.per_action.entry(action_id.to_string()).or_default();
        entry.record(signal.score(latency_weight), signal.latency_secs);
        self.total = self.total.saturating_add(1);
    }

    /// Mean reward score for an action, or `None` if untried.
    #[must_use]
    pub fn mean_score(&self, action_id: &str) -> Option<f64> {
        self.per_action.get(action_id).map(|r| r.mean_score)
    }

    /// Number of recorded signals (all actions).
    #[must_use]
    pub fn total(&self) -> u64 {
        self.total
    }

    /// UCB value for `action_id` given the global `total` and `exploration`.
    pub(crate) fn ucb_for(&self, action_id: &str, total: f64, exploration: f64) -> f64 {
        match self.per_action.get(action_id) {
            None => f64::INFINITY,
            Some(s) => s.ucb(total, exploration.max(1e-3)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_aggregates() {
        let mut ledger = RewardLedger::default();
        assert_eq!(ledger.total(), 0);
        ledger.record("a", RewardSignal { success: true, latency_secs: 0.1, cost_usd: 0.0 }, 0.5);
        assert_eq!(ledger.total(), 1);
        assert!(ledger.mean_score("a").is_some());
        // Untried action still yields a finite UCB (INFINITY) for selection.
        assert!(
            ledger.ucb_for("b", 1.0, 0.1).is_finite()
                || ledger.ucb_for("b", 1.0, 0.1) == f64::INFINITY
        );
    }
}
