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
        "Tool-call budget exhausted for this turn",
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

impl ToolBudgetWarning {
    pub(crate) fn system_message(self) -> String {
        format!(
            "Tool-call budget warning: {}/{} used; {} remaining for this turn. Use targeted extraction/batching before additional tool calls.",
            self.used, self.max, self.remaining
        )
    }

    pub(crate) fn log_threshold_reached(self, path: &'static str) {
        tracing::info!(used = self.used, max = self.max, remaining = self.remaining, "{path}");
    }
}

/// Shared tail of the tool-call and wall-clock budget exhaustion directives.
pub(crate) const BUDGET_EXHAUSTED_SYNTHESIS_NOTE: &str = "Tools are disabled for the rest of this turn, so further tool calls are \
skipped. Synthesize your final answer now from the tool outputs already gathered in this conversation.";

impl ToolBudgetExhaustion {
    pub(crate) fn policy_violation_message(self) -> String {
        format!("Policy violation: exceeded max tool calls per turn ({})", self.max)
    }

    /// Compact stub returned for the 2nd+ rejected calls in the same batch so
    /// the full policy message isn't repeated N times and context stays clean.
    pub(crate) fn skipped_call_message(self) -> String {
        "Tool-call budget exhausted for this turn; call skipped.".to_string()
    }

    /// System directive pushed once (after all tool responses in the batch)
    /// telling the model that tools are disabled for the rest of the turn and
    /// it must synthesize a final answer from already-gathered outputs.
    /// Mirrors `ToolWallClockExhaustion::synthesis_directive_message`.
    pub(crate) fn synthesis_directive_message(self) -> String {
        debug_assert!(self.max > 0, "disabled tool-call caps must not emit exhaustion");
        format!(
            "Tool-call budget exhausted for this turn ({}/{}). {BUDGET_EXHAUSTED_SYNTHESIS_NOTE}",
            self.used, self.max
        )
    }
}

/// Budget kinds that can fail a turn closed. Recorded in trajectory telemetry
/// so post-hoc diagnosis can tell which ceiling fired; the model-facing
/// recovery directives already cover the live behavior.
pub(crate) mod budget_kind {
    /// Per-turn tool-call count (`max_tool_calls_per_turn`).
    pub(crate) const TOOL_CALLS: &str = "tool_calls";
    /// Per-turn tool-loop iterations (`max_tool_loops` hard cap).
    pub(crate) const TOOL_LOOP: &str = "tool_loop";
    /// Session-wide tool-call count (safety gateway + auto-grant headroom).
    pub(crate) const SESSION_CALLS: &str = "session_calls";
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
