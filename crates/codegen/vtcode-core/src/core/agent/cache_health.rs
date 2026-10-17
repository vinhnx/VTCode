//! Session-level prompt-cache health monitoring.
//!
//! The harness already warns about individual cache-breaking events (reasoning
//! effort changes, idle-gap expiry, planning transitions). This module adds the
//! missing aggregate view: it observes normalized per-turn usage and fires a
//! session-scoped alert when the *hit rate* stays degraded, mirroring the
//! "monitor cache hit rate like uptime" discipline. Both runloops (the
//! `vtcode-core` `AgentRunner` and the binary unified runloop) feed the same
//! monitor through their usage-merge paths so thresholds and wording stay
//! identical.
//!
//! The monitor consumes the canonical harness [`Usage`] (as produced by
//! `normalized_turn_usage`), where `input_tokens` is always the total prompt
//! volume and `cached_input_tokens`/`cache_creation_tokens` carry the cache
//! signal, so provider reporting differences are already normalized away.

use crate::exec::events::Usage;

/// Minimum input tokens for a turn to count as cache signal.
///
/// Tiny turns re-pay little even on a full miss; counting them would let
/// cheap noise trip (or mask) the sustained-miss detector.
pub const MIN_INPUT_TOKENS_FOR_SIGNAL: u64 = 1_024;

/// Consecutive degraded turns that fire the sustained-miss alert.
pub const SUSTAINED_MISS_TURNS: u32 = 3;

/// Measured turns before the hit-rate alert arms.
pub const HIT_RATE_WINDOW_TURNS: u32 = 8;

/// Hit-rate floor (percent) that fires the low-hit-rate alert.
pub const HIT_RATE_FLOOR_PERCENT: f64 = 25.0;

/// A turn counts as degraded when its cache hit rate is below this floor.
/// Partial reuse below this threshold counts as degraded, too.
pub const DEGRADED_TURN_HIT_RATE_PERCENT: f64 = 50.0;

/// Session-scoped prompt-cache health alert.
#[derive(Debug, Clone, PartialEq)]
pub enum CacheHealthAlert {
    /// Several consecutive measured requests reused less than half their input.
    SustainedMisses {
        consecutive: u32,
        hit_rate: f64,
        measured_turns: u32,
    },
    /// The hit rate over recent measured turns fell below the floor.
    LowHitRate { hit_rate: f64, measured_turns: u32 },
}

impl CacheHealthAlert {
    /// User-facing warning text. Mirrors the tone of the existing cache-gap
    /// and reasoning-effort advisories: what happened, what it costs, and the
    /// most likely causes to check.
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::SustainedMisses { consecutive, hit_rate, measured_turns } => format!(
                "Prompt cache reuse stayed below {DEGRADED_TURN_HIT_RATE_PERCENT:.0}% for {consecutive} consecutive requests (overall hit rate {hit_rate:.0}% over {measured_turns} measured requests). \
                 Check earlier warnings for model/reasoning switches, MCP refreshes, planning toggles, or idle-gap expiry, or inspect the trajectory log for prefix/catalog hash changes. Cache misses alone do not establish client-side prefix churn."
            ),
            Self::LowHitRate { hit_rate, measured_turns } => format!(
                "Prompt cache hit rate is {hit_rate:.0}% over {measured_turns} measured turns (floor {HIT_RATE_FLOOR_PERCENT:.0}%). \
                 Requests are re-paying input cost; check for prompt/tool-catalog churn or idle gaps expiring the provider cache."
            ),
        }
    }
}

/// Rolling prompt-cache health for one session.
///
/// Fed with normalized per-turn [`Usage`]; turns without provider cache
/// metrics (providers that do not report them) or below the volume floor are
/// ignored so unmeasurable providers never trip the detector. Each alert
/// fires at most once per session, matching the one-shot style of the budget
/// and cache-gap warnings.
#[derive(Debug, Clone, Default)]
pub struct PromptCacheHealthMonitor {
    measured_turns: u32,
    window_input_tokens: u64,
    window_read_tokens: u64,
    consecutive_degraded: u32,
    sustained_miss_fired: bool,
    low_rate_fired: bool,
}

impl PromptCacheHealthMonitor {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Successful compaction replaces the reusable history. Start a new
    /// consecutive streak while retaining session totals and one-shot alerts.
    pub fn record_compaction_boundary(&mut self) {
        self.consecutive_degraded = 0;
    }

    /// Record one turn's normalized usage. Returns an alert the first time a
    /// degraded pattern is confirmed, `None` otherwise.
    pub fn record_turn(&mut self, usage: &Usage) -> Option<CacheHealthAlert> {
        if !Self::is_measured(usage) {
            return None;
        }
        self.measured_turns = self.measured_turns.saturating_add(1);
        self.window_input_tokens = self.window_input_tokens.saturating_add(usage.input_tokens);
        self.window_read_tokens = self.window_read_tokens.saturating_add(usage.cached_input_tokens);

        if Self::turn_hit_rate(usage) < DEGRADED_TURN_HIT_RATE_PERCENT {
            self.consecutive_degraded = self.consecutive_degraded.saturating_add(1);
        } else {
            self.consecutive_degraded = 0;
        }

        if !self.sustained_miss_fired && self.consecutive_degraded >= SUSTAINED_MISS_TURNS {
            self.sustained_miss_fired = true;
            return Some(CacheHealthAlert::SustainedMisses {
                consecutive: self.consecutive_degraded,
                hit_rate: self.rolling_hit_rate(),
                measured_turns: self.measured_turns,
            });
        }

        if !self.low_rate_fired
            && self.measured_turns >= HIT_RATE_WINDOW_TURNS
            && self.rolling_hit_rate() < HIT_RATE_FLOOR_PERCENT
        {
            self.low_rate_fired = true;
            return Some(CacheHealthAlert::LowHitRate {
                hit_rate: self.rolling_hit_rate(),
                measured_turns: self.measured_turns,
            });
        }

        None
    }

    /// Hit rate over all measured turns this session: cached input over total
    /// input. Uses the same `cached/input` definition as the per-turn check so
    /// turns with large uncached tails cannot inflate the rolling figure.
    #[must_use]
    pub fn rolling_hit_rate(&self) -> f64 {
        if self.window_input_tokens == 0 {
            return 100.0;
        }
        (self.window_read_tokens as f64 / self.window_input_tokens as f64) * 100.0
    }

    fn is_measured(usage: &Usage) -> bool {
        usage.input_tokens >= MIN_INPUT_TOKENS_FOR_SIGNAL
            && (usage.cached_input_tokens > 0 || usage.cache_creation_tokens > 0)
    }

    fn turn_hit_rate(usage: &Usage) -> f64 {
        usage.cache_hit_rate().map_or(100.0, |rate| rate * 100.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input_tokens: u64, cached: u64, creation: u64) -> Usage {
        Usage {
            input_tokens,
            cached_input_tokens: cached,
            cache_creation_tokens: creation,
            output_tokens: 10,
        }
    }

    #[test]
    fn compaction_separates_observed_session_misses_without_losing_totals() {
        let mut monitor = PromptCacheHealthMonitor::new();
        assert!(monitor.record_turn(&usage(12645, 0, 12642)).is_none());
        assert!(monitor.record_turn(&usage(12669, 5710, 5575)).is_none());
        monitor.record_compaction_boundary();
        let miss = usage(12740, 0, 5212);
        assert!(monitor.record_turn(&miss).is_none());
        assert_eq!(monitor.window_input_tokens, 38054);
        assert_eq!(monitor.window_read_tokens, 5710);
        assert!(monitor.record_turn(&miss).is_none());
        assert!(matches!(
            monitor.record_turn(&miss),
            Some(CacheHealthAlert::SustainedMisses { consecutive: 3, measured_turns: 5, .. })
        ));
        monitor.record_compaction_boundary();
        assert!(monitor.record_turn(&miss).is_none());
        assert!(monitor.record_turn(&miss).is_none());
        assert!(matches!(monitor.record_turn(&miss), Some(CacheHealthAlert::LowHitRate { measured_turns: 8, .. })));
        assert!(monitor.record_turn(&miss).is_none());
    }

    #[test]
    fn partial_hits_are_reported_as_low_reuse_not_full_cost_misses() {
        let mut monitor = PromptCacheHealthMonitor::new();
        assert!(monitor.record_turn(&usage(40000, 12000, 0)).is_none());
        assert!(monitor.record_turn(&usage(40000, 18000, 0)).is_none());
        let message = monitor.record_turn(&usage(40000, 16000, 0)).expect("degraded streak").message();
        assert!(message.contains("below 50%"));
        assert!(message.contains("38%"));
        assert!(!message.contains("near-full"));
        assert!(!message.contains("full input cost"));
    }

    #[test]
    fn observed_constant_reuse_plateau_reports_weighted_session_rate() {
        // First six requests in the 2026-10-15T06:35:33 session: the reused
        // prefix plateaus while input grows. This measures reuse, not churn.
        let mut monitor = PromptCacheHealthMonitor::new();
        for (input, cached) in [
            (19126, 0),
            (20309, 13708),
            (25677, 13708),
            (30443, 13708),
            (30686, 13708),
        ] {
            assert!(monitor.record_turn(&usage(input, cached, input - cached - 3)).is_none());
        }
        let alert = monitor
            .record_turn(&usage(30932, 13708, 17221))
            .expect("three low-reuse requests");
        let CacheHealthAlert::SustainedMisses { consecutive, hit_rate, measured_turns } = &alert else {
            panic!("expected sustained low reuse");
        };
        assert_eq!((*consecutive, *measured_turns), (3, 6));
        assert!((hit_rate - 43.6079988293).abs() < 0.000001);
        assert!(alert.message().contains("44% over 6 measured requests"));
        assert!(
            alert
                .message()
                .contains("Cache misses alone do not establish client-side prefix churn")
        );
    }

    #[test]
    fn ignores_turns_without_cache_metrics() {
        let mut monitor = PromptCacheHealthMonitor::new();
        for _ in 0..10 {
            assert_eq!(monitor.record_turn(&usage(50_000, 0, 0)), None);
        }
        assert_eq!(monitor.measured_turns, 0);
    }

    #[test]
    fn ignores_small_turns_below_volume_floor() {
        let mut monitor = PromptCacheHealthMonitor::new();
        for _ in 0..10 {
            assert_eq!(monitor.record_turn(&usage(100, 0, 200)), None);
        }
        assert_eq!(monitor.measured_turns, 0);
    }

    #[test]
    fn fires_sustained_miss_after_consecutive_degraded_turns() {
        let mut monitor = PromptCacheHealthMonitor::new();
        assert_eq!(monitor.record_turn(&usage(50_000, 0, 5_000)), None);
        assert_eq!(monitor.record_turn(&usage(50_000, 0, 5_000)), None);
        let alert = monitor.record_turn(&usage(50_000, 0, 5_000));
        assert!(matches!(alert, Some(CacheHealthAlert::SustainedMisses { consecutive: 3, .. })));
    }

    #[test]
    fn healthy_turn_resets_consecutive_counter() {
        let mut monitor = PromptCacheHealthMonitor::new();
        assert_eq!(monitor.record_turn(&usage(50_000, 0, 5_000)), None);
        assert_eq!(monitor.record_turn(&usage(50_000, 0, 5_000)), None);
        assert_eq!(monitor.record_turn(&usage(50_000, 45_000, 5_000)), None);
        assert_eq!(monitor.record_turn(&usage(50_000, 0, 5_000)), None);
        assert_eq!(monitor.record_turn(&usage(50_000, 0, 5_000)), None);
        assert!(matches!(
            monitor.record_turn(&usage(50_000, 0, 5_000)),
            Some(CacheHealthAlert::SustainedMisses { consecutive: 3, .. })
        ));
    }

    #[test]
    fn alerts_are_one_shot_per_session() {
        let mut monitor = PromptCacheHealthMonitor::new();
        for _ in 0..3 {
            monitor.record_turn(&usage(50_000, 0, 5_000));
        }
        // Stay below the hit-rate window so only the sustained-miss alert
        // (already fired) could recur; it must not.
        for _ in 0..4 {
            assert_eq!(monitor.record_turn(&usage(50_000, 0, 5_000)), None);
        }
    }

    #[test]
    fn no_alert_when_rolling_rate_stays_above_floor() {
        let mut monitor = PromptCacheHealthMonitor::new();
        // Interleaved healthy turns keep the consecutive counter (needs 3 in a
        // row) at bay: D,D,H,D,D,H,D,D → max streak 2, rolling rate
        // (6*10000 + 2*30000) / (8*50000) = 30%, above the 25% floor.
        let mut alert = None;
        for index in 0..8 {
            let turn = if index % 3 == 2 {
                usage(50_000, 30_000, 20_000)
            } else {
                usage(50_000, 10_000, 40_000)
            };
            alert = monitor.record_turn(&turn).or(alert);
        }
        assert_eq!(alert, None);
    }

    #[test]
    fn low_hit_rate_fires_below_floor() {
        let mut monitor = PromptCacheHealthMonitor::new();
        // Barely-healthy turns (60%) reset the consecutive counter while the
        // rolling rate stays near zero: max streak 2, rolling rate after 8
        // measured turns (60000 read / 400000) = 15%.
        let mut alert = None;
        for index in 0..10 {
            let turn = if index % 3 == 2 {
                usage(50_000, 30_000, 20_000)
            } else {
                usage(50_000, 0, 50_000)
            };
            alert = monitor.record_turn(&turn).or(alert);
        }
        assert!(matches!(alert, Some(CacheHealthAlert::LowHitRate { .. })));
    }

    #[test]
    fn rolling_rate_uses_total_input_not_cache_traffic() {
        // Asymmetric uncached tail: cached/(cached+creation) looks healthy
        // while cached/input is degraded. Rolling must follow cached/input.
        let mut monitor = PromptCacheHealthMonitor::new();
        // D,D,H x2 + D,D → max streak 2 (no sustained miss), rolling
        // (6*5000 + 2*26000) / (8*50000) = 20.5%, below the 25% floor.
        // Old read/(read+creation) math gave 68% and missed this.
        let mut alert = None;
        for index in 0..8 {
            let turn = if index % 3 == 2 {
                usage(50_000, 26_000, 4_000)
            } else {
                usage(50_000, 5_000, 5_000)
            };
            alert = monitor.record_turn(&turn).or(alert);
        }
        assert!(matches!(alert, Some(CacheHealthAlert::LowHitRate { .. })));
        assert!(
            monitor.rolling_hit_rate() < HIT_RATE_FLOOR_PERCENT,
            "rolling {} should be below floor",
            monitor.rolling_hit_rate()
        );
    }

    #[test]
    fn alert_messages_mention_likely_causes() {
        let sustained =
            CacheHealthAlert::SustainedMisses { consecutive: 3, hit_rate: 5.0, measured_turns: 3 }.message();
        assert!(sustained.contains("consecutive"));
        assert!(sustained.contains("5%"));
        assert!(sustained.contains("trajectory log"));
        let low = CacheHealthAlert::LowHitRate { hit_rate: 10.0, measured_turns: 9 }.message();
        assert!(low.contains("10%"));
        assert!(low.contains("idle gaps"));
    }
}
