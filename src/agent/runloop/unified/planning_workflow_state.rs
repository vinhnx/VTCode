use crate::agent::runloop::unified::state::SessionStats;
use anyhow::Result;
use vtcode_core::core::interfaces::session::PlanningEntrySource;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_ui::tui::app::InlineHandle;

#[derive(Default)]
pub(crate) struct PlanningWorkflowSessionState {
    interview_shown: bool,
    interview_pending: bool,
    turns: usize,
    interview_cycles_completed: usize,
    last_interview_cancelled: bool,
    entry_source: Option<PlanningEntrySource>,
    /// Set when the session budget is exhausted during planning. Prevents
    /// the interview from being re-forced on the next turn, which would
    /// loop forever because no further LLM calls are possible.
    budget_exhausted: bool,
    /// Set when the post-tool recovery cycle cap is reached during planning
    /// (repeated tool-free synthesis failures because the planning context is
    /// saturated). Prevents the interview from being re-forced on the next
    /// turn, which would re-research the still-huge context and fail again —
    /// looping forever across turns.
    recovery_exhausted: bool,
    /// Set when a `request_user_input` tool call is denied by a permanent
    /// capability/policy failure (e.g. the tool is not available in the
    /// current runtime) rather than the user cancelling the modal. Unlike
    /// cancellation, a policy denial will recur on every retry — this flag
    /// permanently stops the interview from being re-forced for the rest of
    /// the planning session, falling back to autonomous plan synthesis
    /// instead of looping (see checkpoint turn_655/turn_660).
    interview_denied: bool,
}

impl PlanningWorkflowSessionState {
    pub(crate) fn enter(&mut self, entry_source: PlanningEntrySource) {
        self.interview_shown = false;
        self.interview_pending = false;
        self.turns = 0;
        self.interview_cycles_completed = 0;
        self.last_interview_cancelled = false;
        self.entry_source = Some(entry_source);
        self.budget_exhausted = false;
        self.recovery_exhausted = false;
        self.interview_denied = false;
    }

    pub(crate) fn exit(&mut self) {
        self.entry_source = None;
        self.budget_exhausted = false;
        self.recovery_exhausted = false;
        self.interview_denied = false;
    }

    #[allow(dead_code)]
    #[cfg(test)]
    pub(crate) fn interview_shown(&self) -> bool {
        self.interview_shown
    }

    #[allow(dead_code)]
    pub(crate) fn mark_interview_shown(&mut self) {
        self.interview_shown = true;
        self.interview_pending = false;
    }

    pub(crate) fn turns(&self) -> usize {
        self.turns
    }

    pub(crate) fn increment_turns(&mut self) {
        self.turns = self.turns.saturating_add(1);
    }

    #[allow(dead_code)]
    pub(crate) fn interview_pending(&self) -> bool {
        self.interview_pending
    }

    #[allow(dead_code)]
    pub(crate) fn mark_interview_pending(&mut self) {
        self.interview_pending = true;
    }

    pub(crate) fn record_interview_result(&mut self, answered_questions: usize, cancelled: bool) {
        let answered_questions = answered_questions.min(3);
        self.last_interview_cancelled = cancelled || answered_questions == 0;
        self.interview_pending = false;

        if !self.last_interview_cancelled {
            self.interview_cycles_completed = self.interview_cycles_completed.saturating_add(1);
            self.interview_shown = true;
        } else {
            self.interview_shown = false;
        }
    }

    #[allow(dead_code)]
    pub(crate) fn interview_cycles_completed(&self) -> usize {
        self.interview_cycles_completed
    }

    #[allow(dead_code)]
    pub(crate) fn last_interview_cancelled(&self) -> bool {
        self.last_interview_cancelled
    }

    pub(crate) fn mark_budget_exhausted(&mut self) {
        self.budget_exhausted = true;
    }

    pub(crate) fn is_budget_exhausted(&self) -> bool {
        self.budget_exhausted
    }

    pub(crate) fn mark_recovery_exhausted(&mut self) {
        self.recovery_exhausted = true;
    }

    pub(crate) fn is_recovery_exhausted(&self) -> bool {
        self.recovery_exhausted
    }

    /// Record that `request_user_input` was denied by a permanent
    /// capability/policy failure this session. Once set, the interview must
    /// never be re-forced — see the field doc comment for why this differs
    /// from `record_interview_result(0, cancelled=true)`.
    pub(crate) fn mark_interview_denied(&mut self) {
        self.interview_denied = true;
        self.interview_pending = false;
    }

    pub(crate) fn is_interview_denied(&self) -> bool {
        self.interview_denied
    }
}

pub(crate) const PLANNING_WORKFLOW_REVIEW_AND_EXECUTE_HINT: &str =
    "Planning workflow: review the plan, then type `implement` (or `yes`/`continue`/`go`/`start`) to execute.";
pub(crate) const PLANNING_WORKFLOW_SHORT_CONFIRMATION_HINT: &str = "Planning workflow: type `implement` (or `yes`/`continue`/`go`/`start`) to execute, or say `keep planning` to revise.";
pub(crate) const PLANNING_WORKFLOW_KEEP_PLANNING_HINT: &str =
    "To keep planning, say `keep planning` and describe what to revise.";
pub(crate) const PLANNING_WORKFLOW_MANUAL_SWITCH_FALLBACK_HINT: &str =
    "If the plan is not shown automatically, type `implement` to present it for approval.";

pub(crate) fn short_confirmation_hint_with_fallback() -> String {
    format!("{PLANNING_WORKFLOW_SHORT_CONFIRMATION_HINT} {PLANNING_WORKFLOW_MANUAL_SWITCH_FALLBACK_HINT}")
}

pub(crate) fn render_planning_workflow_next_step_hint(renderer: &mut AnsiRenderer) -> Result<()> {
    renderer.line(MessageStyle::Info, PLANNING_WORKFLOW_REVIEW_AND_EXECUTE_HINT)?;
    renderer.line(MessageStyle::Info, PLANNING_WORKFLOW_KEEP_PLANNING_HINT)?;
    renderer.line(MessageStyle::Info, PLANNING_WORKFLOW_MANUAL_SWITCH_FALLBACK_HINT)?;
    Ok(())
}

pub(crate) async fn transition_to_planning_workflow(
    tool_registry: &ToolRegistry,
    session_stats: &mut SessionStats,
    plan_session: &mut PlanningWorkflowSessionState,
    handle: &InlineHandle,
    entry_source: PlanningEntrySource,
    reset_plan_file: bool,
    reset_plan_baseline: bool,
) {
    tool_registry.enable_planning();
    let plan_state = tool_registry.planning_workflow_state();
    // `enable_planning()` above already sets the active flag on
    // `PlanningWorkflowState` (the single source of truth), so we do not call
    // `plan_state.enable()` again here.
    if reset_plan_file {
        plan_state.set_plan_file(None).await;
    }
    if reset_plan_baseline {
        plan_state.set_plan_baseline(None).await;
    }

    session_stats.reset_for_planning_workflow_entry();
    plan_session.enter(entry_source);
    handle.force_redraw();
}

pub(crate) async fn finish_planning_workflow(
    tool_registry: &ToolRegistry,
    plan_session: &mut PlanningWorkflowSessionState,
    handle: &InlineHandle,
    clear_plan_file: bool,
) {
    tool_registry.disable_planning();
    let plan_state = tool_registry.planning_workflow_state();
    plan_state.disable();
    if clear_plan_file {
        plan_state.set_plan_file(None).await;
    }

    plan_session.exit();
    handle.force_redraw();
}

#[cfg(test)]
mod tests {
    use super::PlanningWorkflowSessionState;
    use vtcode_core::core::interfaces::session::PlanningEntrySource;

    #[test]
    fn interview_result_updates_cycle_metrics() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);

        state.record_interview_result(2, false);
        assert_eq!(state.interview_cycles_completed(), 1);
        assert!(!state.last_interview_cancelled());

        state.record_interview_result(0, true);
        assert_eq!(state.interview_cycles_completed(), 1);
        assert!(state.last_interview_cancelled());
    }

    #[test]
    fn entering_resets_interview_cycle_metrics() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);
        state.record_interview_result(1, false);
        assert_eq!(state.interview_cycles_completed(), 1);

        state.exit();
        state.enter(PlanningEntrySource::UserRequest);
        assert_eq!(state.interview_cycles_completed(), 0);
        assert!(!state.last_interview_cancelled());
    }

    #[test]
    fn mark_interview_denied_is_permanent_until_reset() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);
        assert!(!state.is_interview_denied());

        state.mark_interview_pending();
        state.mark_interview_denied();
        assert!(state.is_interview_denied());
        // A denial also clears any pending interview request — re-forcing it
        // would just repeat the same policy failure.
        assert!(!state.interview_pending());

        // Re-entering the planning workflow (a fresh session) clears the flag.
        state.exit();
        state.enter(PlanningEntrySource::UserRequest);
        assert!(!state.is_interview_denied());
    }

    #[test]
    fn budget_and_recovery_exhaustion_cleared_by_enter_and_exit() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);

        state.mark_budget_exhausted();
        state.mark_recovery_exhausted();
        assert!(state.is_budget_exhausted());
        assert!(state.is_recovery_exhausted());

        // exit() clears both exhaustion flags.
        state.exit();
        assert!(!state.is_budget_exhausted());
        assert!(!state.is_recovery_exhausted());

        // Re-apply and verify enter() also clears them.
        state.mark_budget_exhausted();
        state.mark_recovery_exhausted();
        state.enter(PlanningEntrySource::UserRequest);
        assert!(!state.is_budget_exhausted());
        assert!(!state.is_recovery_exhausted());
    }

    #[test]
    fn record_interview_result_treats_zero_answered_as_cancelled() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);

        // answered_questions=0 with cancelled=false should still count as cancelled.
        state.record_interview_result(0, false);
        assert!(state.last_interview_cancelled());
        assert_eq!(state.interview_cycles_completed(), 0);
    }

    #[test]
    fn record_interview_result_clamps_answered_questions_to_three() {
        let mut state = PlanningWorkflowSessionState::default();
        state.enter(PlanningEntrySource::UserRequest);

        state.record_interview_result(10, false);
        assert_eq!(state.interview_cycles_completed(), 1);
        assert!(!state.last_interview_cancelled());
    }
}
