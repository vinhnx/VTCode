use vtcode_core::llm::provider as uni;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_ui::tui::app::InlineHandle;

use crate::agent::runloop::unified::planning_workflow::{
    PlanningIntent, assistant_recently_prompted_implementation, detect_planning_intent,
};
use crate::agent::runloop::unified::planning_workflow_state::{
    PlanningWorkflowSessionState, finish_planning_workflow, short_confirmation_hint_with_fallback,
};
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use vtcode_config::builtin_primary_build_agent;

const PLANNING_WORKFLOW_EXIT_TRIGGER_STATUS: &str = "Planning workflow: implementation intent detected from your message. Exiting planning mode and proceeding with execution.";

/// Outcome of checking whether the planning workflow should exit this turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlanningTransition {
    /// No planning transition; continue the turn normally.
    None,
    /// User approved the plan; proceed with execution.
    ExitAndImplement,
    /// User wants to stay in planning mode.
    StayInPlanning,
}

impl PlanningTransition {
    /// Convert this transition into the `TurnLoopResult::Completed` variant
    /// and an optional primary-agent switch command.
    #[inline]
    pub(crate) fn into_result_and_agent(self) -> (TurnLoopResult, Option<String>) {
        match self {
            PlanningTransition::None => (TurnLoopResult::Completed { plan_approved_execution_pending: false }, None),
            PlanningTransition::ExitAndImplement => (
                TurnLoopResult::Completed { plan_approved_execution_pending: true },
                Some(builtin_primary_build_agent().name),
            ),
            PlanningTransition::StayInPlanning => {
                (TurnLoopResult::Completed { plan_approved_execution_pending: false }, None)
            }
        }
    }

    /// Whether the turn loop should break after this transition.
    #[inline]
    pub(crate) fn should_break(self) -> bool {
        !matches!(self, PlanningTransition::None)
    }
}

/// Check whether the last user message signals a planning-workflow exit (approve,
/// implement, switch-to-build/auto) and execute the transition if so.
///
/// Returns the detected transition. The caller checks `should_break()` to decide
/// whether to break the turn loop.
pub(crate) async fn maybe_handle_planning_exit_trigger(
    renderer: &mut AnsiRenderer,
    tool_registry: &mut ToolRegistry,
    plan_session: &mut PlanningWorkflowSessionState,
    handle: &InlineHandle,
    working_history: &[uni::Message],
    auto_finish_planning_attempted: &mut bool,
) -> anyhow::Result<PlanningTransition> {
    if !tool_registry.is_planning_active() {
        return Ok(PlanningTransition::None);
    }

    if *auto_finish_planning_attempted {
        return Ok(PlanningTransition::None);
    }

    let Some(last_user_msg) = working_history.iter().rev().find(|msg| msg.role == uni::MessageRole::User) else {
        return Ok(PlanningTransition::None);
    };

    let text = last_user_msg.content.as_text();
    let assistant_prompted = assistant_recently_prompted_implementation(working_history);
    let intent = detect_planning_intent(&text, assistant_prompted);

    let transition = match intent {
        PlanningIntent::ExitAndImplement => {
            *auto_finish_planning_attempted = true;
            display_status(renderer, PLANNING_WORKFLOW_EXIT_TRIGGER_STATUS)?;
            finish_planning_workflow(tool_registry, plan_session, handle, true).await;
            PlanningTransition::ExitAndImplement
        }
        PlanningIntent::StayInPlanning => {
            display_status(renderer, &short_confirmation_hint_with_fallback())?;
            PlanningTransition::StayInPlanning
        }
        PlanningIntent::None => PlanningTransition::None,
    };

    Ok(transition)
}

fn display_status(renderer: &mut AnsiRenderer, message: &str) -> anyhow::Result<()> {
    renderer.line(vtcode_core::utils::ansi::MessageStyle::Status, message)
}
