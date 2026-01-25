use anyhow::Result;
use std::sync::Arc;
use std::time::Instant;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::decision_tracker::DecisionOutcome;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::ToolResultCache;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::agent::runloop::git::confirm_changes_with_git_diff;
use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::display::ensure_turn_bottom_gap;
use crate::agent::runloop::unified::shell::{
    derive_recent_tool_output, should_short_circuit_shell,
};
use crate::agent::runloop::unified::state::SessionStats;
use crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output_renderer;
use crate::agent::runloop::unified::tool_pipeline::{
    ToolExecutionStatus, ToolPipelineOutcome,
};
use crate::agent::runloop::unified::tool_summary::{
    render_tool_call_summary, stream_label_from_output,
};
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use crate::agent::runloop::unified::turn::ui_sync::redraw_with_sync;
use crate::agent::runloop::unified::turn::utils::{
    render_hook_messages, safe_force_redraw,
};
use crate::hooks::lifecycle::LifecycleHookEngine;

use super::super::helpers::{push_assistant_message, push_tool_response};

pub(crate) struct RunTurnHandleToolSuccessParams<'a> {
    pub name: &'a str,
    pub output: serde_json::Value,
    pub stdout: Option<String>,
    pub modified_files: Vec<String>,
    pub command_success: bool,
    pub has_more: bool,
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a vtcode_core::ui::tui::InlineHandle,
    pub session_stats: &'a mut SessionStats,
    pub traj: &'a vtcode_core::core::trajectory::TrajectoryLogger,
    pub mcp_panel_state: &'a mut mcp_events::McpPanelState,
    pub tool_result_cache: &'a Arc<tokio::sync::RwLock<ToolResultCache>>,
    pub vt_cfg: Option<&'a VTCodeConfig>,
    pub working_history: &'a mut Vec<uni::Message>,
    pub call_id: &'a str,
    pub dec_id: &'a str,
    pub decision_ledger:
        &'a Arc<tokio::sync::RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>,
    pub last_tool_stdout: &'a mut Option<String>,
    pub any_write_effect: &'a mut bool,
    pub turn_modified_files: &'a mut std::collections::BTreeSet<std::path::PathBuf>,
    pub skip_confirmations: bool,
    pub lifecycle_hooks: &'a Option<LifecycleHookEngine>,
    pub bottom_gap_applied: &'a mut bool,
    pub last_forced_redraw: &'a mut Instant,
    pub input: &'a str,
}

#[allow(dead_code)]
pub(crate) async fn run_turn_handle_tool_success(
    params: RunTurnHandleToolSuccessParams<'_>,
) -> Result<Option<TurnLoopResult>> {
    safe_force_redraw(params.handle, params.last_forced_redraw);
    redraw_with_sync(params.handle).await?;

    params.session_stats.record_tool(params.name);
    params.traj.log_tool_call(
        params.working_history.len(),
        params.name,
        &serde_json::to_value(&params.output).unwrap_or(serde_json::json!({})),
        true,
    );

    if let Some(tool_name) = params.name.strip_prefix("mcp_") {
        let mut mcp_event = mcp_events::McpEvent::new(
            "mcp".to_string(),
            tool_name.to_string(),
            Some(params.output.to_string()),
        );
        mcp_event.success(None);
        params.mcp_panel_state.add_event(mcp_event);
    } else {
        let output_json = serde_json::to_value(&params.output).unwrap_or(serde_json::json!({}));
        let stream_label = stream_label_from_output(&output_json, params.command_success);
        render_tool_call_summary(params.renderer, params.name, &output_json, stream_label)?;
    }

    let _ = handle_pipeline_output_renderer(
        params.renderer,
        params.session_stats,
        params.mcp_panel_state,
        Some(params.tool_result_cache),
        Some(params.decision_ledger),
        params.name,
        &serde_json::json!({}),
        &ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: params.output.clone(),
            stdout: params.stdout.clone(),
            modified_files: params.modified_files.clone(),
            command_success: params.command_success,
            has_more: params.has_more,
        }),
        params.vt_cfg,
    )
    .await?;

    *params.last_tool_stdout = if params.command_success {
        params.stdout.clone()
    } else {
        None
    };

    if matches!(
        params.name,
        "write_file" | "edit_file" | "create_file" | "delete_file"
    ) {
        *params.any_write_effect = true;
    }

    if !params.modified_files.is_empty() {
        if confirm_changes_with_git_diff(&params.modified_files, params.skip_confirmations).await? {
            params
                .renderer
                .line(MessageStyle::Info, "Changes applied successfully.")?;
            for f in &params.modified_files {
                params
                    .turn_modified_files
                    .insert(std::path::PathBuf::from(f));
            }
            for file_path in &params.modified_files {
                let mut cache = params.tool_result_cache.write().await;
                cache.invalidate_for_path(file_path);
            }
        } else {
            params
                .renderer
                .line(MessageStyle::Info, "Changes discarded.")?;
        }
    }

    let mut notice_lines: Vec<String> = Vec::with_capacity(params.modified_files.len() + 3);
    if !params.modified_files.is_empty() {
        notice_lines.push("Files touched:".to_string());
        for file in &params.modified_files {
            notice_lines.push(format!("  - {}", file));
        }
        if let Some(stdout_preview) = &*params.last_tool_stdout {
            let preview: String = stdout_preview.chars().take(80).collect();
            notice_lines.push(format!("stdout preview: {}", preview));
        }
    }
    if let Some(notice) = params.output.get("notice").and_then(|value| value.as_str())
        && !notice.trim().is_empty()
    {
        notice_lines.push(notice.trim().to_string());
    }
    if !notice_lines.is_empty() {
        params.renderer.line(MessageStyle::Info, "")?;
        for line in notice_lines {
            params.renderer.line(MessageStyle::Info, &line)?;
        }
    }

    let content = serde_json::to_string(&params.output).unwrap_or_else(|_| "{}".to_string());

    push_tool_response(
        params.working_history,
        params.call_id.to_string(),
        content,
        params.name,
    );

    let mut hook_block_reason: Option<String> = None;

    if let Some(hooks) = params.lifecycle_hooks {
        match hooks
            .run_post_tool_use(
                params.name,
                Some(&serde_json::to_value(&params.output).unwrap_or(serde_json::json!({}))),
                &params.output,
            )
            .await
        {
            Ok(outcome) => {
                render_hook_messages(params.renderer, &outcome.messages)?;
                for context in outcome.additional_context {
                    if !context.trim().is_empty() {
                        params.working_history.push(uni::Message::system(context));
                    }
                }
                if let Some(reason) = outcome.block_reason {
                    let trimmed = reason.trim();
                    if !trimmed.is_empty() {
                        params.renderer.line(MessageStyle::Info, trimmed)?;
                        hook_block_reason = Some(trimmed.to_string());
                    }
                }
            }
            Err(err) => {
                params.renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to run post-tool hooks: {}", err),
                )?;
            }
        }
    }

    if let Some(reason) = hook_block_reason {
        let blocked_message = format!("Tool execution blocked by lifecycle hooks: {}", reason);
        params
            .working_history
            .push(uni::Message::system(blocked_message));
        {
            let mut ledger = params.decision_ledger.write().await;
            ledger.record_outcome(
                params.dec_id,
                DecisionOutcome::Failure {
                    error: reason.clone(),
                    recovery_attempts: 0,
                    context_preserved: true,
                },
            );
        }
        return Ok(Some(TurnLoopResult::Blocked {
            reason: Some(reason),
        }));
    }

    {
        let mut ledger = params.decision_ledger.write().await;
        ledger.record_outcome(
            params.dec_id,
            DecisionOutcome::Success {
                result: "tool_ok".to_string(),
                metrics: Default::default(),
            },
        );
    }

    let allow_short_circuit = !params.has_more
        && params.command_success
        && should_short_circuit_shell(
            params.input,
            params.name,
            &serde_json::to_value(&params.output).unwrap_or(serde_json::json!({})),
        );

    if allow_short_circuit {
        let reply = derive_recent_tool_output(params.working_history)
            .unwrap_or_else(|| "Command completed successfully.".to_string());
        params.renderer.line(MessageStyle::Response, &reply)?;
        ensure_turn_bottom_gap(params.renderer, params.bottom_gap_applied)?;
        push_assistant_message(params.working_history, uni::Message::assistant(reply));
        let _ = params.last_tool_stdout.take();
        return Ok(Some(TurnLoopResult::Completed));
    }

    Ok(None)
}
