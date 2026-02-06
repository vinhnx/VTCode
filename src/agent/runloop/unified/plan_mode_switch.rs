use anyhow::Result;
use serde_json::Value;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::ui::tui::{EditingMode, InlineHandle};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::agent::runloop::unified::state::SessionStats;

pub(crate) fn maybe_disable_plan_mode_for_tool(
    session_stats: &mut SessionStats,
    tool_registry: &mut ToolRegistry,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    tool_name: &str,
    args_val: &Value,
) -> Result<bool> {
    if !session_stats.is_plan_mode() {
        return Ok(false);
    }

    if tool_registry.is_plan_mode_allowed(tool_name, args_val) {
        return Ok(false);
    }

    renderer.line(
        MessageStyle::Info,
        "Plan Mode active: enabling full tools for discovery, then returning to Plan Mode.",
    )?;
    tool_registry.disable_plan_mode();
    let plan_state = tool_registry.plan_mode_state();
    plan_state.disable();
    session_stats.switch_to_coder();
    handle.set_editing_mode(EditingMode::Edit);

    Ok(true)
}

pub(crate) fn restore_plan_mode_after_tool(
    session_stats: &mut SessionStats,
    tool_registry: &mut ToolRegistry,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    restore_plan_mode: bool,
) -> Result<()> {
    if !restore_plan_mode {
        return Ok(());
    }

    tool_registry.enable_plan_mode();
    let plan_state = tool_registry.plan_mode_state();
    plan_state.enable();
    session_stats.switch_to_planner();
    handle.set_editing_mode(EditingMode::Plan);
    renderer.line(MessageStyle::Info, "Returned to Plan Mode (read-only).")?;

    Ok(())
}
