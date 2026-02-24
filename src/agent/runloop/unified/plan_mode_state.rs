use crate::agent::runloop::unified::state::SessionStats;
use anyhow::Result;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::ui::tui::{EditingMode, InlineHandle};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

pub(crate) fn render_plan_mode_next_step_hint(renderer: &mut AnsiRenderer) -> Result<()> {
    renderer.line(
        MessageStyle::Info,
        "Plan Mode: review the plan, then type `implement` (or `yes`/`continue`/`go`/`start`) to execute.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "To keep planning, say `stay in plan mode` and describe what to revise.",
    )?;
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
    if reset_plan_file {
        plan_state.set_plan_file(None).await;
    }
    if reset_plan_baseline {
        plan_state.set_plan_baseline(None).await;
    }

    session_stats.set_plan_mode(true);
    session_stats.switch_to_planner();
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
    session_stats.switch_to_coder();
    handle.set_editing_mode(EditingMode::Edit);
}
