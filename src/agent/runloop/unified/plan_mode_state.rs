use crate::agent::runloop::unified::state::SessionStats;
use anyhow::Result;
use vtcode_core::tools::handlers::plan_mode::PlanLifecyclePhase;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::{EditingMode, InlineHandle};

pub(crate) const PLAN_MODE_REVIEW_AND_EXECUTE_HINT: &str = "Plan Mode: review the plan, then type `implement` (or `yes`/`continue`/`go`/`start`) to execute.";
pub(crate) const PLAN_MODE_SHORT_CONFIRMATION_HINT: &str = "Plan Mode: type `implement` (or `yes`/`continue`/`go`/`start`) to execute, or say `stay in plan mode` to revise.";
pub(crate) const PLAN_MODE_KEEP_PLANNING_HINT: &str =
    "To keep planning, say `stay in plan mode` and describe what to revise.";
pub(crate) const PLAN_MODE_MANUAL_SWITCH_FALLBACK_HINT: &str = "If automatic Plan->Edit switching fails, manually switch with `/plan off` or `/mode` (or `Shift+Tab`/`Alt+M`).";
pub(crate) const PLAN_MODE_STILL_ACTIVE_PREFIX: &str =
    "Plan Mode still active. Call `exit_plan_mode` to review/refine the plan before retrying.";

pub(crate) fn short_confirmation_hint_with_fallback() -> String {
    format!(
        "{} {}",
        PLAN_MODE_SHORT_CONFIRMATION_HINT, PLAN_MODE_MANUAL_SWITCH_FALLBACK_HINT
    )
}

pub(crate) fn plan_mode_still_active_hint_with_fallback() -> String {
    format!(
        "{} {}",
        PLAN_MODE_STILL_ACTIVE_PREFIX, PLAN_MODE_MANUAL_SWITCH_FALLBACK_HINT
    )
}

pub(crate) fn render_plan_mode_next_step_hint(renderer: &mut AnsiRenderer) -> Result<()> {
    renderer.line(MessageStyle::Info, PLAN_MODE_REVIEW_AND_EXECUTE_HINT)?;
    renderer.line(MessageStyle::Info, PLAN_MODE_KEEP_PLANNING_HINT)?;
    renderer.line(MessageStyle::Info, PLAN_MODE_MANUAL_SWITCH_FALLBACK_HINT)?;
    Ok(())
}

pub(crate) async fn transition_to_plan_mode(
    tool_registry: &ToolRegistry,
    session_stats: &mut SessionStats,
    handle: &InlineHandle,
    reset_plan_file: bool,
    reset_plan_baseline: bool,
) {
    tool_registry.enable_plan_mode();
    let plan_state = tool_registry.plan_mode_state();
    plan_state.enable();
    plan_state.set_phase(PlanLifecyclePhase::ActiveDrafting);
    if reset_plan_file {
        plan_state.set_plan_file(None).await;
    }
    if reset_plan_baseline {
        plan_state.set_plan_baseline(None).await;
    }

    session_stats.set_plan_mode(true);
    handle.set_editing_mode(EditingMode::Plan);
}

pub(crate) async fn transition_to_edit_mode(
    tool_registry: &ToolRegistry,
    session_stats: &mut SessionStats,
    handle: &InlineHandle,
    clear_plan_file: bool,
) {
    tool_registry.disable_plan_mode();
    let plan_state = tool_registry.plan_mode_state();
    plan_state.disable();
    if clear_plan_file {
        plan_state.set_plan_file(None).await;
    }

    session_stats.set_plan_mode(false);
    handle.set_editing_mode(EditingMode::Edit);
}
