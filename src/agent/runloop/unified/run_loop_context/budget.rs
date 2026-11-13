//! Turn budget accounting: tool-call/wall-clock exhaustion notices and metrics.

use super::*;

impl HarnessTurnState {
    /// Keep the runtime-owned budget cause, without the model-only synthesis
    /// instructions. Never infer a budget failure from conversation text.
    pub(crate) fn budget_recovery_reason(&self) -> Option<&str> {
        budget_exhaustion_cause(self.recovery_prompt_reason()?)
    }
}

pub(crate) fn budget_exhaustion_cause(reason: &str) -> Option<&str> {
    [
        "Tool loop budget exhausted",
        "Tool wall-clock budget exhausted for this turn",
    ]
    .iter()
    .any(|prefix| reason.starts_with(prefix))
    .then(|| reason.split_once(". ").map_or(reason, |(cause, _)| cause))
}

/// Minimal probe for the exhaustion marker. Decoding only the single control
/// flag avoids materializing the full tool payload (`Value` IR) on every
/// response when all we need is one bool.
#[derive(serde::Deserialize)]
pub(super) struct PreviewBudgetExhaustedProbe {
    #[serde(default)]
    pub(super) preview_budget_exhausted: Option<bool>,
}

/// Whether the turn loop owes the model a loop-budget warning this
/// iteration: at or past `threshold` of `max_tool_loops`, once per turn.
/// Unlimited (`usize::MAX`, the `UNLIMITED_TOOL_LOOPS` sentinel) and
/// disabled (`0`) budgets never warn.
#[must_use]
pub(crate) fn tool_loop_warning_due(
    step_count: usize,
    max_tool_loops: usize,
    threshold: f64,
    already_emitted: bool,
) -> bool {
    if already_emitted || max_tool_loops == 0 || max_tool_loops == usize::MAX {
        return false;
    }
    step_count as f64 / max_tool_loops as f64 >= threshold
}

/// Tool-loop pressure warning: same shape and batching advice the model
/// already knows from the loop-budget contract.
#[must_use]
pub(crate) fn tool_loop_warning_message(used: usize, max: usize) -> String {
    format!(
        "Tool-loop budget warning: {}/{} used; {} remaining for this turn. Use targeted extraction/batching before additional tool calls.",
        used,
        max,
        max.saturating_sub(used),
    )
}

/// Shared tail of the wall-clock budget exhaustion directive.
pub(crate) const BUDGET_EXHAUSTED_SYNTHESIS_NOTE: &str = "Tools are disabled for the rest of this turn, so further tool calls are \
skipped. Synthesize your final answer now from the tool outputs already gathered in this conversation.";

/// Budget kinds that can fail a turn closed. Recorded in trajectory telemetry
/// so post-hoc diagnosis can tell which ceiling fired; the model-facing
/// recovery directives already cover the live behavior.
pub(crate) mod budget_kind {
    /// Per-turn tool-loop iterations (`max_tool_loops` hard cap).
    pub(crate) const TOOL_LOOP: &str = "tool_loop";
    /// Per-turn aggregate tool wall-clock time (`max_tool_wall_clock_secs`).
    pub(crate) const WALL_CLOCK: &str = "wall_clock_secs";
}

/// Snapshot of a failed-closed turn budget for trajectory telemetry.
/// Mirrors the `tool_catalog_cache_metrics` record shape.
#[derive(Copy, Clone)]
pub(crate) struct BudgetExhaustedMetrics {
    pub budget: &'static str,
    pub used: usize,
    pub max: usize,
    pub step_count: Option<usize>,
    pub planning_active: bool,
    pub tool_calls: usize,
}

/// Log one JSONL `budget_exhausted` record. Best effort like all trajectory
/// logging: a disabled logger drops the record. Unknown `kind` values are
/// ignored by trajectory consumers, so this adds no schema burden.
pub(crate) fn emit_budget_exhausted_metric(traj: &TrajectoryLogger, metrics: BudgetExhaustedMetrics) {
    traj.log(&budget_exhausted_record(metrics));
}

/// Serializable `budget_exhausted` record behind
/// [`emit_budget_exhausted_metric`], kept separate so tests can assert exact
/// field values without filesystem I/O (the async line writer behind
/// `traj.log` is only flushable inside `vtcode-core` unit tests).
#[derive(serde::Serialize)]
pub(crate) struct BudgetExhaustedRecord {
    kind: &'static str,
    budget: &'static str,
    used: usize,
    max: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    step_count: Option<usize>,
    planning_active: bool,
    tool_calls: usize,
    ts: i64,
}

/// Pure record constructor behind [`emit_budget_exhausted_metric`].
pub(crate) fn budget_exhausted_record(metrics: BudgetExhaustedMetrics) -> BudgetExhaustedRecord {
    BudgetExhaustedRecord {
        kind: "budget_exhausted",
        budget: metrics.budget,
        used: metrics.used,
        max: metrics.max,
        step_count: metrics.step_count,
        planning_active: metrics.planning_active,
        tool_calls: metrics.tool_calls,
        ts: chrono::Utc::now().timestamp(),
    }
}

impl ToolWallClockExhaustion {
    pub(crate) fn policy_violation_message(self) -> String {
        format!("Policy violation: exceeded tool wall clock budget ({}s)", self.max_secs)
    }

    /// Compact stub returned for the 2nd+ rejected calls in the same batch so
    /// the full policy message isn't repeated N times and context stays clean.
    pub(crate) fn skipped_call_message(self) -> String {
        "Tool wall-clock budget exhausted for this turn; call skipped.".to_string()
    }

    /// System directive pushed once (after all tool responses in the batch)
    /// telling the model that tools are disabled for the rest of the turn and
    /// it must synthesize a final answer from already-gathered outputs. This is
    /// the in-turn synthesis nudge that the raw per-call policy errors lack.
    pub(crate) fn synthesis_directive_message(self) -> String {
        format!(
            "Tool wall-clock budget exhausted for this turn ({}s). {BUDGET_EXHAUSTED_SYNTHESIS_NOTE}",
            self.max_secs
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_loop_warning_fires_once_at_threshold() {
        const THRESHOLD: f64 = 0.75;
        // Both sides of the 45/60 boundary.
        assert!(!tool_loop_warning_due(44, 60, THRESHOLD, false));
        assert!(tool_loop_warning_due(45, 60, THRESHOLD, false));
        assert!(tool_loop_warning_due(59, 60, THRESHOLD, false));
        // One-shot latch.
        assert!(!tool_loop_warning_due(59, 60, THRESHOLD, true));
        // Unlimited and disabled budgets never warn (and must not divide by zero).
        assert!(!tool_loop_warning_due(usize::MAX, usize::MAX, THRESHOLD, false));
        assert!(!tool_loop_warning_due(usize::MAX, 0, THRESHOLD, false));
        assert!(!tool_loop_warning_due(0, 60, THRESHOLD, false));
    }

    #[test]
    fn tool_loop_warning_message_mirrors_call_warning_shape() {
        let message = tool_loop_warning_message(45, 60);
        assert!(message.starts_with("Tool-loop budget warning: 45/60 used; 15 remaining for this turn."));
        assert!(message.contains("Use targeted extraction/batching before additional tool calls."));
    }
}
