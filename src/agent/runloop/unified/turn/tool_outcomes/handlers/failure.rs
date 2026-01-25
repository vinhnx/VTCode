use anyhow::Result;
use std::sync::Arc;
use std::time::Instant;
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::decision_tracker::DecisionOutcome;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::ToolResultCache;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output_renderer;
use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
use crate::agent::runloop::unified::turn::ui_sync::{redraw_with_sync, wait_for_redraw_complete};
use crate::agent::runloop::unified::turn::utils::safe_force_redraw;

use super::super::helpers::push_tool_response;

#[allow(dead_code)]
pub(crate) struct RunTurnHandleToolFailureParams<'a> {
    pub name: &'a str,
    pub error: anyhow::Error,
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a vtcode_core::ui::tui::InlineHandle,
    pub session_stats: &'a mut SessionStats,
    pub traj: &'a vtcode_core::core::trajectory::TrajectoryLogger,
    pub working_history: &'a mut Vec<uni::Message>,
    pub call_id: &'a str,
    pub dec_id: &'a str,
    pub mcp_panel_state: &'a mut mcp_events::McpPanelState,
    pub decision_ledger:
        &'a Arc<tokio::sync::RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>,
    pub tool_result_cache: Option<&'a Arc<tokio::sync::RwLock<ToolResultCache>>>,
    pub vt_cfg: Option<&'a VTCodeConfig>,
}

#[allow(dead_code)]
pub(crate) async fn run_turn_handle_tool_failure(
    params: RunTurnHandleToolFailureParams<'_>,
) -> Result<()> {
    safe_force_redraw(params.handle, &mut Instant::now());
    redraw_with_sync(params.handle).await?;

    params.session_stats.record_tool(params.name);

    let failure_msg = format!("Tool '{}' failed: {}", params.name, params.error);
    params.renderer.line(MessageStyle::Error, &failure_msg)?;
    let recovery_hint = match params.name {
        tools::GREP_FILE => {
            "Try narrowing the pattern or limiting files (e.g., use glob or specific paths)."
        }
        tools::LIST_FILES => {
            "Specify a subdirectory instead of root; avoid repeating on '.' or './'."
        }
        tools::READ_FILE => {
            "Ensure the path exists and is inside the workspace; try providing a line range."
        }
        _ => "Adjust arguments, try a smaller scope, or use a different tool.",
    };
    params.renderer.line(MessageStyle::Info, recovery_hint)?;
    params.working_history.push(uni::Message::system(format!(
        "Tool '{}' failed. Hint: {}",
        params.name, recovery_hint
    )));

    params.traj.log_tool_call(
        params.working_history.len(),
        params.name,
        &serde_json::json!({}),
        false,
    );

    let error_message = params.error.to_string();
    let error_json = serde_json::json!({ "error": error_message });

    if let Some(tool_name) = params.name.strip_prefix("mcp_") {
        params.renderer.line_if_not_empty(MessageStyle::Output)?;
        params.renderer.line(
            MessageStyle::Error,
            &format!("MCP tool {} failed: {}", tool_name, error_message),
        )?;
        params.handle.force_redraw();
        wait_for_redraw_complete().await?;

        let mut mcp_event = mcp_events::McpEvent::new(
            "mcp".to_string(),
            tool_name.to_string(),
            Some(serde_json::to_string(&error_json).unwrap_or_default()),
        );
        mcp_event.failure(Some(error_message.clone()));
        params.mcp_panel_state.add_event(mcp_event);
    }

    params.renderer.line(MessageStyle::Error, &error_message)?;
    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: error_json.clone(),
        stdout: None,
        modified_files: vec![],
        command_success: false,
        has_more: false,
    });
    handle_pipeline_output_renderer(
        params.renderer,
        params.session_stats,
        params.mcp_panel_state,
        params.tool_result_cache,
        Some(params.decision_ledger),
        params.name,
        &serde_json::json!({}),
        &outcome,
        params.vt_cfg,
    )
    .await?;

    push_tool_response(
        params.working_history,
        params.call_id.to_string(),
        serde_json::to_string(&error_json).unwrap_or_default(),
        params.name,
    );

    {
        let mut ledger = params.decision_ledger.write().await;
        ledger.record_outcome(
            params.dec_id,
            DecisionOutcome::Failure {
                error: error_message,
                recovery_attempts: 0,
                context_preserved: true,
            },
        );
    }

    Ok(())
}
