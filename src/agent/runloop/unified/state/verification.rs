//! Bounded continuation budgets and verification-recovery episodes.
//!
//! Extracted from state.rs beside the pre-existing preflight_recovery,
//! request_envelope, and ctrl_c submodules. No behavior change: verifier,
//! tracker-continuation, and plan-continuation budgets plus the
//! verification failure record are verbatim, and VerificationFailureSummary
//! is re-exported from the parent.

use super::SessionStats;

/// Bounded record of a failed harness auto-verification, kept for handoff
/// enrichment so the eventual manual blocker names the failing command and
/// shows its output tail instead of repeating the generic "run a verifier"
/// recipe.
#[derive(Debug, Clone, Default)]
pub(crate) struct VerificationFailureSummary {
    /// Verifier command that failed (e.g. `cargo check --locked`).
    pub command: String,
    /// Observed terminal status, retained separately from the bounded tail.
    pub exit_code: Option<i64>,
    /// Tail excerpt of the tool output, bounded to
    /// `VERIFICATION_FAILURE_EXCERPT_CHARS` chars at record time.
    pub excerpt_tail: String,
    /// Consecutive failure count including this failure.
    pub consecutive_failures: u8,
}

impl SessionStats {
    /// Preserve the legacy snapshot shape; the retired edit allowance is zero.
    pub(crate) fn verification_snapshot(&self) -> (bool, u8) {
        (self.verification_pending, 0)
    }

    pub(crate) fn set_verification_snapshot(&mut self, snapshot: (bool, u8)) {
        self.verification_pending = snapshot.0;
    }

    /// Config-aware cross-turn recovery recorder; prefer it at call sites with
    /// workspace config access so `[agent.harness.verification].cross_turn_turns`
    /// is honored. Returns `true` when budget remained (the caller should
    /// queue a recovery turn instead of writing a blocked handoff); `false`
    /// once exhausted and manual `continue` is required.
    pub(crate) fn record_verification_auto_recovery_turn_with_limit(&mut self, max_turns: u8) -> bool {
        if self.verification_auto_recovery_turns >= max_turns {
            return false;
        }
        self.verification_auto_recovery_turns = self.verification_auto_recovery_turns.saturating_add(1);
        true
    }

    pub(crate) fn verification_auto_recovery_turns(&self) -> u8 {
        self.verification_auto_recovery_turns
    }

    /// Record one tracker auto-continue turn. Returns `true` when budget
    /// remained (`max_turns`); `false` once exhausted.
    pub(crate) fn record_tracker_continuation_turn_with_limit(&mut self, max_turns: u8) -> bool {
        if self.tracker_continuation_turns >= max_turns {
            return false;
        }
        self.tracker_continuation_turns = self.tracker_continuation_turns.saturating_add(1);
        true
    }

    pub(crate) fn tracker_continuation_turns(&self) -> u8 {
        self.tracker_continuation_turns
    }

    pub(crate) fn reset_tracker_continuation_budget(&mut self) {
        self.tracker_continuation_turns = 0;
    }

    /// Apply a live tracker probe to the incomplete-items cache.
    ///
    /// Successful `Complete` probes **clear** the cache so auto-continue stops
    /// after the tracker finishes. `Unavailable` keeps the last value.
    pub(crate) fn apply_tracker_probe(
        &mut self,
        probe: crate::agent::runloop::unified::turn::tool_outcomes::helpers::TrackerProbeOutcome,
    ) -> Option<&[String]> {
        crate::agent::runloop::unified::turn::tool_outcomes::helpers::apply_tracker_probe_to_cache(
            &mut self.last_incomplete_tracker_items,
            probe,
        )
    }

    /// Only new completion progress restores the auto-continue episode.
    /// Recreating a checklist or toggling statuses must not refresh retries.
    pub(crate) fn note_tracker_completed_count(&mut self, completed: u32) -> bool {
        let progressed = completed > self.tracker_completed_count_high_water;
        if progressed {
            self.tracker_completed_count_high_water = completed;
        }
        progressed
    }

    /// Record one plan-mode auto-continue turn against its own budget.
    pub(crate) fn record_plan_continuation_turn_with_limit(&mut self, max_turns: u8) -> bool {
        if self.plan_continuation_turns >= max_turns {
            return false;
        }
        self.plan_continuation_turns = self.plan_continuation_turns.saturating_add(1);
        true
    }

    pub(crate) fn plan_continuation_turns(&self) -> u8 {
        self.plan_continuation_turns
    }

    pub(crate) fn reset_plan_continuation_budget(&mut self) {
        self.plan_continuation_turns = 0;
    }

    /// Record a failed harness auto-verification of `command`, keeping a
    /// bounded tail of its output for the escalated handoff. Returns the new
    /// consecutive-failure count so callers can compare against the
    /// configured escalation threshold without a second accessor call.
    pub(crate) fn record_verification_auto_failure(&mut self, command: String, excerpt: &str) -> u8 {
        const BOUND: usize =
            crate::agent::runloop::unified::turn::tool_outcomes::helpers::VERIFICATION_FAILURE_EXCERPT_CHARS;
        // Keep the last BOUND chars (the tail usually holds the error
        // summary) without materializing the full char vector.
        let start = excerpt
            .char_indices()
            .rev()
            .nth(BOUND.saturating_sub(1))
            .map_or(0, |(idx, _)| idx);
        let tail = excerpt[start..].to_string();
        self.verification_consecutive_failures = self.verification_consecutive_failures.saturating_add(1);
        let consecutive_failures = self.verification_consecutive_failures;
        self.last_verification_failure = Some(VerificationFailureSummary {
            command,
            exit_code: serde_json::from_str::<serde_json::Value>(excerpt)
                .ok()
                .and_then(|value| value.get("exit_code").and_then(serde_json::Value::as_i64)),
            excerpt_tail: tail,
            consecutive_failures,
        });
        consecutive_failures
    }

    /// Record a successful harness auto-verification: the suite is green, so
    /// the consecutive-failure count and any stored failure tail are dropped.
    pub(crate) fn record_verification_auto_success(&mut self) {
        self.verification_consecutive_failures = 0;
        self.last_verification_failure = None;
    }

    pub(crate) fn verification_consecutive_failures(&self) -> u8 {
        self.verification_consecutive_failures
    }

    pub(crate) fn last_verification_failure(&self) -> Option<&VerificationFailureSummary> {
        self.last_verification_failure.as_ref()
    }

    /// Clear terminal diagnostics without replenishing request-scoped recovery.
    /// Only genuine fresh input restores the corrective-pass allowance.
    pub(crate) fn reset_verification_recovery_episode(&mut self) {
        self.verification_consecutive_failures = 0;
        self.last_verification_failure = None;
        // Tracker/plan continuation budgets are independent episode counters;
        // do not wipe them here (Completed-turn verification resets must not
        // silently restore full tracker auto-continue budgets).
    }
}
