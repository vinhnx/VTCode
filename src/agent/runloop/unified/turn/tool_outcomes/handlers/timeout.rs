use anyhow::Result;
use std::sync::Arc;
use std::time::Instant;
use vtcode_core::core::decision_tracker::DecisionOutcome;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use crate::agent::runloop::unified::turn::ui_sync::{redraw_with_sync, wait_for_redraw_complete};
use crate::agent::runloop::unified::turn::utils::safe_force_redraw;

use super::super::helpers::push_tool_response;

#[allow(dead_code)]
pub(crate) struct RunTurnHandleToolTimeoutParams<'a> {
    pub name: &'a str,
    pub error: anyhow::Error,
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a vtcode_core::ui::tui::InlineHandle,
    pub session_stats: &'a mut SessionStats,
    pub traj: &'a vtcode_core::core::trajectory::TrajectoryLogger,
    pub working_history: &'a mut Vec<uni::Message>,
    pub call_id: &'a str,
    pub dec_id: &'a str,
    pub decision_ledger:
        &'a Arc<tokio::sync::RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>,
}

#[allow(dead_code)]
pub(crate) struct RunTurnHandleToolCancelledParams<'a> {
    pub name: &'a str,
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a vtcode_core::ui::tui::InlineHandle,
    pub session_stats: &'a mut SessionStats,
    pub working_history: &'a mut Vec<uni::Message>,
    pub call_id: &'a str,
    pub dec_id: &'a str,
    pub decision_ledger:
        &'a Arc<tokio::sync::RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>,
}

#[allow(dead_code)]
pub(crate) async fn run_turn_handle_tool_timeout(
    params: RunTurnHandleToolTimeoutParams<'_>,
) -> Result<()> {
    params.handle.force_redraw();
    wait_for_redraw_complete().await?;

    params.session_stats.record_tool(params.name);
    params.renderer.line_if_not_empty(MessageStyle::Output)?;
    params.renderer.line(
        MessageStyle::Error,
        &format!("Tool {} timed out after 5 minutes.", params.name),
    )?;
    params.traj.log_tool_call(
        params.working_history.len(),
        params.name,
        &serde_json::json!({}),
        false,
    );

    let error_message = params.error.to_string();
    let err_json = serde_json::json!({ "error": error_message });
    let timeout_content = serde_json::to_string(&err_json).unwrap_or_else(|_| "{}".to_string());

    push_tool_response(
        params.working_history,
        params.call_id.to_string(),
        timeout_content,
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

#[allow(dead_code)]
pub(crate) async fn run_turn_handle_tool_cancelled(
    params: RunTurnHandleToolCancelledParams<'_>,
) -> Result<TurnLoopResult> {
    safe_force_redraw(params.handle, &mut Instant::now());
    redraw_with_sync(params.handle).await?;

    params.session_stats.record_tool(params.name);

    params.renderer.line_if_not_empty(MessageStyle::Output)?;
    params.renderer.line(
        MessageStyle::Info,
        "Operation cancelled by user. Stopping current turn.",
    )?;

    let err_json = serde_json::json!({ "error": "Tool execution cancelled by user" });

    push_tool_response(
        params.working_history,
        params.call_id.to_string(),
        serde_json::to_string(&err_json).unwrap_or_else(|_| "{}".to_string()),
        params.name,
    );

    {
        let mut ledger = params.decision_ledger.write().await;
        ledger.record_outcome(
            params.dec_id,
            DecisionOutcome::Failure {
                error: "Cancelled by user".to_string(),
                recovery_attempts: 0,
                context_preserved: true,
            },
        );
    }

    Ok(TurnLoopResult::Cancelled)
}
