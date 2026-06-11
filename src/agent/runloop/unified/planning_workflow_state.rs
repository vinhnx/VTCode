use crate::agent::runloop::unified::state::SessionStats;
use anyhow::Result;
use vtcode_core::core::interfaces::session::PlanningEntrySource;
use vtcode_core::tools::handlers::planning_workflow::PlanLifecyclePhase;
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
}

impl PlanningWorkflowSessionState {
    pub(crate) fn enter(&mut self, entry_source: PlanningEntrySource) {
        self.interview_shown = false;
        self.interview_pending = false;
        self.turns = 0;
        self.interview_cycles_completed = 0;
        self.last_interview_cancelled = false;
        self.entry_source = Some(entry_source);
    }

    pub(crate) fn exit(&mut self) {
        self.entry_source = None;
    }

    #[cfg(test)]
    pub(crate) fn interview_shown(&self) -> bool {
        self.interview_shown
    }

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

    pub(crate) fn interview_pending(&self) -> bool {
        self.interview_pending
    }

    pub(crate) fn mark_interview_pending(&mut self) {
        self.interview_pending = true;
    }

    pub(crate) fn clear_interview_pending(&mut self) {
        self.interview_pending = false;
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

    pub(crate) fn interview_cycles_completed(&self) -> usize {
        self.interview_cycles_completed
    }

    pub(crate) fn last_interview_cancelled(&self) -> bool {
        self.last_interview_cancelled
    }
}

pub(crate) const PLANNING_WORKFLOW_REVIEW_AND_EXECUTE_HINT: &str = "Planning workflow: review the plan, then type `implement` (or `yes`/`continue`/`go`/`start`) to execute.";
pub(crate) const PLANNING_WORKFLOW_SHORT_CONFIRMATION_HINT: &str = "Planning workflow: type `implement` (or `yes`/`continue`/`go`/`start`) to execute, or say `keep planning` to revise.";
pub(crate) const PLANNING_WORKFLOW_KEEP_PLANNING_HINT: &str =
    "To keep planning, say `keep planning` and describe what to revise.";
pub(crate) const PLANNING_WORKFLOW_MANUAL_SWITCH_FALLBACK_HINT: &str =
    "If automatic planning handoff fails, call `finish_planning` to present the plan again.";
pub(crate) const PLANNING_WORKFLOW_STILL_ACTIVE_PREFIX: &str = "Planning workflow is still active. Call `finish_planning` to review/refine the plan before retrying.";

pub(crate) fn short_confirmation_hint_with_fallback() -> String {
    format!(
        "{} {}",
        PLANNING_WORKFLOW_SHORT_CONFIRMATION_HINT, PLANNING_WORKFLOW_MANUAL_SWITCH_FALLBACK_HINT
    )
}

pub(crate) fn planning_still_active_hint_with_fallback() -> String {
    format!(
        "{} {}",
        PLANNING_WORKFLOW_STILL_ACTIVE_PREFIX, PLANNING_WORKFLOW_MANUAL_SWITCH_FALLBACK_HINT
    )
}

pub(crate) fn render_planning_workflow_next_step_hint(renderer: &mut AnsiRenderer) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        PLANNING_WORKFLOW_REVIEW_AND_EXECUTE_HINT,
    )?;
    renderer.line(MessageStyle::Info, PLANNING_WORKFLOW_KEEP_PLANNING_HINT)?;
    renderer.line(
        MessageStyle::Info,
        PLANNING_WORKFLOW_MANUAL_SWITCH_FALLBACK_HINT,
    )?;
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
    plan_state.enable();
    plan_state.set_phase(PlanLifecyclePhase::ActiveDrafting);
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
}
