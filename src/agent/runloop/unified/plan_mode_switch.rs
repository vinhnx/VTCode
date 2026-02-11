use anyhow::Result;
use serde_json::Value;
use vtcode_core::tools::registry::ToolRegistry;
use vtcode_core::ui::tui::InlineHandle;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::agent::runloop::unified::plan_mode_state::{
    transition_to_edit_mode, transition_to_plan_mode,
};
use crate::agent::runloop::unified::state::SessionStats;

pub(crate) async fn maybe_disable_plan_mode_for_tool(
    session_stats: &mut SessionStats,
    tool_registry: &ToolRegistry,
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
    transition_to_edit_mode(tool_registry, session_stats, handle, false).await;

    Ok(true)
}

pub(crate) async fn restore_plan_mode_after_tool(
    session_stats: &mut SessionStats,
    tool_registry: &ToolRegistry,
    renderer: &mut AnsiRenderer,
    handle: &InlineHandle,
    restore_plan_mode: bool,
) -> Result<()> {
    if !restore_plan_mode {
        return Ok(());
    }

    transition_to_plan_mode(tool_registry, session_stats, handle, false, false).await;
    renderer.line(MessageStyle::Info, "Returned to Plan Mode (read-only).")?;

    Ok(())
}
