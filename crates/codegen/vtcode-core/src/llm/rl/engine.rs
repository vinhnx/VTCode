//! RL engine: adaptive action selection.

use serde::{Deserialize, Serialize};

use vtcode_config::optimization::OptimizationConfig;

use super::ledger::RewardLedger;
use super::signal::{RewardSignal, RlStrategy};

/// A candidate action for the selector.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Action {
    /// Stable action identifier (e.g. `"executor:edge"` vs `"executor:cloud"`).
    pub id: String,
}

/// Per-call context for [`RlEngine::select`].
///
/// Only carries what selection needs at decision time; the latency-vs-success
/// trade-off is a record-time concern owned by the engine (from config).
#[derive(Debug, Clone, Copy)]
pub struct PolicyContext {
    /// Exploration constant for the bandit (higher = more exploration).
    pub exploration: f64,
}

/// Serializable snapshot of an [`RlEngine`] for inspection/telemetry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RlSnapshot {
    /// Active strategy (debug-formatted enum).
    pub strategy: String,
    /// Number of recorded reward signals.
    pub recorded: u64,
}

/// RL engine selecting actions from learned reward statistics.
#[derive(Debug, Clone)]
pub struct RlEngine {
    strategy: RlStrategy,
    exploration: f64,
    latency_weight: f64,
    ledger: RewardLedger,
}

impl RlEngine {
    /// Build an engine from the `[optimization]` config block.
    pub fn from_config(cfg: &OptimizationConfig) -> Self {
        Self {
            strategy: RlStrategy::parse(&cfg.rl.strategy),
            exploration: cfg.rl.epsilon.max(0.0),
            latency_weight: cfg.rl.latency_weight.clamp(0.0, 1.0),
            ledger: RewardLedger::default(),
        }
    }

    /// Select the index of the best action from `actions` given `ctx`.
    /// Returns `None` when `actions` is empty.
    #[must_use]
    pub fn select(&self, actions: &[Action], ctx: &PolicyContext) -> Option<usize> {
        if actions.is_empty() {
            return None;
        }
        let total = self.ledger.total().max(1) as f64;
        let exploration = if self.strategy == RlStrategy::ActorCritic {
            // Actor-critic stand-in: dampen exploration once data exists.
            self.exploration * (1.0 / (1.0 + total))
        } else {
            // Bandit: the per-call policy context drives exploration.
            ctx.exploration.max(0.0)
        };
        let mut best_idx = 0usize;
        let mut best_ucb = f64::NEG_INFINITY;
        for (idx, action) in actions.iter().enumerate() {
            let ucb = self.ledger.ucb_for(&action.id, total, exploration);
            if ucb > best_ucb {
                best_ucb = ucb;
                best_idx = idx;
            }
        }
        Some(best_idx)
    }

    /// Feed a reward signal back for an executed action.
    pub fn apply_reward(&mut self, action_id: &str, signal: RewardSignal) {
        self.ledger.record(action_id, signal, self.latency_weight);
    }

    /// Borrow the underlying reward ledger.
    #[must_use]
    pub fn ledger(&self) -> &RewardLedger {
        &self.ledger
    }

    /// Build a telemetry snapshot of the engine's current state.
    #[must_use]
    pub fn snapshot(&self) -> RlSnapshot {
        RlSnapshot {
            strategy: format!("{:?}", self.strategy),
            recorded: self.ledger.total(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vtcode_config::optimization::OptimizationConfig;

    fn cfg() -> OptimizationConfig {
        OptimizationConfig::default()
    }

    #[test]
    fn select_prefers_learned_success_over_untried() {
        let mut engine = RlEngine::from_config(&cfg());
        let actions = vec![Action { id: "edge".to_string() }, Action { id: "cloud".to_string() }];
        let ctx = PolicyContext { exploration: 0.0 };
        // Both untried → first (UCB INFINITY) wins; learn that "cloud" fails.
        assert_eq!(engine.select(&actions, &ctx), Some(0));
        engine.apply_reward("cloud", RewardSignal { success: false, latency_secs: 5.0, cost_usd: 0.1 });
        // Now "edge" (untried) should be preferred over the failed "cloud".
        assert_eq!(engine.select(&actions, &ctx), Some(0));
        engine.apply_reward("edge", RewardSignal { success: true, latency_secs: 0.2, cost_usd: 0.001 });
        assert_eq!(engine.select(&actions, &ctx), Some(0));
    }

    #[test]
    fn empty_actions_returns_none() {
        let engine = RlEngine::from_config(&cfg());
        assert_eq!(engine.select(&[], &PolicyContext { exploration: 0.1 }), None);
    }

    #[test]
    fn snapshot_reports_recorded_count() {
        let mut engine = RlEngine::from_config(&cfg());
        engine.apply_reward("x", RewardSignal { success: true, latency_secs: 0.1, cost_usd: 0.0 });
        let snap = engine.snapshot();
        assert_eq!(snap.recorded, 1);
        assert_eq!(snap.strategy, "Bandit");
    }
}
