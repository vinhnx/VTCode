use crate::agent::runloop::git::normalize_workspace_path;
use crate::agent::runloop::mcp_events::McpPanelState;
use crate::agent::runloop::tool_output::TrackerLine;
use crate::agent::runloop::unified::state::SessionStats;
use anstyle::Effects;
use anyhow::Result;
use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;
use vtcode_commons::ui_protocol::TaskItemStatus;
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::tools::tool_intent;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::style_helpers::ColorPalette;
use vtcode_core::utils::transcript;
use vtcode_ui::tui::app::{InlineHandle, InlineMessageKind, InlineSegment, InlineTextStyle, ToolOutputId};

use crate::agent::runloop::unified::run_loop_context::RunLoopContext;
use crate::agent::runloop::unified::tool_pipeline::{
    ToolDisplayStatus, ToolExecutionStatus, ToolPipelineOutcome, is_exec_session_call, renders_pty_command_header,
    streams_pty_output,
};
use crate::agent::runloop::unified::tool_summary_helpers::{
    COMPACT_PREVIEW_LEN, display_command_text, preview_command, preview_full_command, relativize_command_paths,
};
use vtcode_commons::canonicalize;

fn render_error_common(renderer: &mut AnsiRenderer, name: &str, error: &str, error_type: &str) -> Result<()> {
    let err_msg = format!("Tool '{name}' {error_type}: {error}");
    renderer.line(MessageStyle::Error, &err_msg)?;
    Ok(())
}

#[derive(Default)]
struct OutcomeState {
    turn_modified_files: Vec<PathBuf>,
    turn_touched_files: Vec<PathBuf>,
    last_tool_stdout: Option<String>,
}

impl OutcomeState {
    fn into_full_tuple(self) -> (Vec<PathBuf>, Vec<PathBuf>, Option<String>) {
        (self.turn_modified_files, self.turn_touched_files, self.last_tool_stdout)
    }
}

struct OutcomeContext<'a> {
    session_stats: &'a mut SessionStats,
    renderer: &'a mut AnsiRenderer,
    handle: &'a InlineHandle,
    mcp_panel_state: &'a mut McpPanelState,
    vt_config: Option<&'a VTCodeConfig>,
    workspace_root: Option<&'a Path>,
}

struct SuccessPayload<'a> {
    output: &'a serde_json::Value,
    stdout: &'a Option<String>,
    modified_files: &'a [String],
    command_success: bool,
}

async fn handle_success_common(
    ctx: &mut OutcomeContext<'_>,
    name: &str,
    args_val: &serde_json::Value,
    payload: SuccessPayload<'_>,
    state: &mut OutcomeState,
) -> Result<()> {
    ctx.session_stats.record_tool(name);

    // Record the MCP panel event for every MCP spelling (canonical
    // `mcp::provider::tool`, `mcp__provider__tool`, and legacy `mcp_tool`).
    // The inline transcript output is still rendered by the shared path below:
    // canonical names previously fell through to `render_tool_output_common`
    // (the legacy `mcp_` prefix never matched them), which draws provider
    // output such as context7 snippets or sequential-reasoning summaries.
    if let Some(tool_name) = crate::agent::runloop::unified::tool_summary_helpers::mcp_tool_display_name(name) {
        ctx.renderer.flush_compact_command_group();
        record_mcp_outcome_event(ctx.mcp_panel_state, tool_name, args_val, payload.command_success);
    }

    if is_task_tracker_tool(name) && ctx.renderer.supports_inline_ui() {
        ctx.renderer.flush_compact_command_group();
        // Display-mode split: compact transcript shows header plus the current
        // task; expanded appends the truncated tree. The docked panel body
        // always keeps the full tree with per-row statuses for text-styling.
        let expanded = !ctx.renderer.is_compact_display();
        let (panel_lines, panel_statuses, panel_current) =
            crate::agent::runloop::tool_output::tracker_panel_rows(payload.output);
        let progress_lines = task_tracker_block_lines(payload.output, expanded);
        if !panel_lines.is_empty() || !progress_lines.is_empty() {
            ctx.handle.update_task_panel_with_statuses(
                panel_lines,
                panel_statuses,
                panel_current,
                crate::agent::runloop::tool_output::tracker_panel_metadata(payload.output),
            );
            if !progress_lines.is_empty() {
                apply_task_tracker_block(ctx.handle, progress_lines);
            }
        }
    } else {
        render_tool_output_common(
            ctx.renderer,
            ctx.handle,
            name,
            args_val,
            payload.output,
            payload.command_success,
            ctx.vt_config,
            ctx.workspace_root,
        )
        .await?;
    }

    state.last_tool_stdout = if payload.command_success {
        payload.stdout.clone()
    } else {
        None
    };

    if !payload.modified_files.is_empty() {
        state.turn_modified_files.extend(collect_modified_files(payload.modified_files));
    }

    // Track read/touched paths for checkpoint replay even when nothing was
    // modified. Reuses the existing activity-path extractor so read_file,
    // grep, and search turns leave visible evidence without snapshotting
    // file contents.
    if let Some(workspace_root) = ctx.workspace_root {
        let touched =
            collect_instruction_activity_paths(workspace_root, args_val, payload.output, payload.modified_files);
        if !touched.is_empty() {
            ctx.session_stats
                .record_touched_files(touched.iter().map(|path| path.display().to_string()));
            state.turn_touched_files.extend(touched);
        }
    }

    Ok(())
}

fn handle_non_success_common(
    ctx: &mut OutcomeContext<'_>,
    name: &str,
    args_val: &serde_json::Value,
    status: &ToolExecutionStatus,
) -> Result<()> {
    ctx.renderer.flush_compact_command_group();

    // Expanded PTY tools already rendered "• Ran ..." in the pre-execution
    // inline block. Compact PTY tools suppress that block, so retain the
    // command summary for failures and cancellations instead of relying on a
    // header that was never emitted.
    let has_live_pty_preview =
        ctx.renderer.supports_inline_ui() && is_run_pty_tool(name, args_val) && !ctx.renderer.is_compact_display();

    match status {
        ToolExecutionStatus::Failure { error } | ToolExecutionStatus::Timeout { error } => {
            let user_message = error.user_message();
            let viewer_id = if ctx.renderer.supports_inline_ui() && is_command_output_call(name, args_val) {
                Some(ctx.handle.record_tool_output(vec![
                    command_output_header(name, args_val, ctx.workspace_root),
                    format!(
                        "    {}: {}",
                        if matches!(status, ToolExecutionStatus::Timeout { .. }) {
                            "timed out"
                        } else {
                            "failed"
                        },
                        user_message
                    ),
                ]))
            } else {
                None
            };
            if !has_live_pty_preview {
                if let Some(viewer_id) = viewer_id {
                    ctx.renderer.set_next_tool_output_anchor(viewer_id);
                }
                render_non_success_summary(
                    ctx.renderer,
                    name,
                    args_val,
                    Some("error"),
                    ctx.workspace_root,
                    ToolDisplayStatus::Failure,
                )?;
            }
            render_error_common(
                ctx.renderer,
                name,
                &user_message,
                if matches!(status, ToolExecutionStatus::Timeout { .. }) {
                    "timed out"
                } else {
                    "failure"
                },
            )?;
        }
        ToolExecutionStatus::Cancelled => {
            let viewer_id = if ctx.renderer.supports_inline_ui() && is_command_output_call(name, args_val) {
                Some(ctx.handle.record_tool_output(vec![
                    command_output_header(name, args_val, ctx.workspace_root),
                    "    warning: tool execution cancelled".to_string(),
                ]))
            } else {
                None
            };
            if !has_live_pty_preview {
                if let Some(viewer_id) = viewer_id {
                    ctx.renderer.set_next_tool_output_anchor(viewer_id);
                }
                render_non_success_summary(
                    ctx.renderer,
                    name,
                    args_val,
                    Some("cancelled"),
                    ctx.workspace_root,
                    ToolDisplayStatus::Warning,
                )?;
            }
            ctx.renderer.line(MessageStyle::Info, "Tool execution cancelled")?;
        }
        ToolExecutionStatus::Success { .. } => {}
    };

    Ok(())
}

fn render_non_success_summary(
    renderer: &mut AnsiRenderer,
    name: &str,
    args_val: &serde_json::Value,
    stream_label: Option<&str>,
    workspace_root: Option<&Path>,
    status: ToolDisplayStatus,
) -> Result<()> {
    let summary_ctx = crate::agent::runloop::unified::tool_summary::ToolSummaryRenderContext { workspace_root };
    crate::agent::runloop::unified::tool_summary::render_expanded_tool_call_summary(
        renderer,
        name,
        args_val,
        stream_label,
        &summary_ctx,
        status.color(ColorPalette::default()),
    )
}

async fn process_outcome_common(
    ctx: &mut OutcomeContext<'_>,
    name: &str,
    args_val: &serde_json::Value,
    outcome: &ToolPipelineOutcome,
) -> Result<OutcomeState> {
    let mut state = OutcomeState::default();

    match &outcome.status {
        ToolExecutionStatus::Success {
            output, stdout, modified_files, command_success, ..
        } => {
            handle_success_common(
                ctx,
                name,
                args_val,
                SuccessPayload {
                    output,
                    stdout,
                    modified_files,
                    command_success: *command_success,
                },
                &mut state,
            )
            .await?;
        }
        _ => handle_non_success_common(ctx, name, args_val, &outcome.status)?,
    }

    Ok(state)
}

pub(crate) async fn handle_pipeline_output(
    ctx: &mut RunLoopContext<'_>,
    name: &str,
    args_val: &serde_json::Value,
    outcome: &ToolPipelineOutcome,
    vt_config: Option<&VTCodeConfig>,
) -> Result<(Vec<PathBuf>, Option<String>)> {
    let (modified, _touched, stdout) = handle_pipeline_output_full(ctx, name, args_val, outcome, vt_config).await?;
    Ok((modified, stdout))
}

pub(crate) async fn handle_pipeline_output_full(
    ctx: &mut RunLoopContext<'_>,
    name: &str,
    args_val: &serde_json::Value,
    outcome: &ToolPipelineOutcome,
    vt_config: Option<&VTCodeConfig>,
) -> Result<(Vec<PathBuf>, Vec<PathBuf>, Option<String>)> {
    // The registry owns the workspace used by the executor and the spooler.
    // Use it here even on the Copilot path, whose lightweight run-loop
    // context intentionally does not carry an auto-permission context.
    let workspace_root = Some(ctx.tool_registry.workspace_root().as_path());
    let mut output_ctx = OutcomeContext {
        session_stats: ctx.session_stats,
        renderer: ctx.renderer,
        handle: ctx.handle,
        mcp_panel_state: ctx.mcp_panel_state,
        vt_config,
        workspace_root,
    };
    let state = process_outcome_common(&mut output_ctx, name, args_val, outcome).await?;
    Ok(state.into_full_tuple())
}

// Adapter for TurnLoopContext (to avoid duplication when handling tool output in the turn loop)
pub(crate) async fn handle_pipeline_output_from_turn_ctx(
    ctx: &mut crate::agent::runloop::unified::turn::TurnLoopContext<'_>,
    name: &str,
    args_val: &serde_json::Value,
    outcome: &ToolPipelineOutcome,
    vt_config: Option<&VTCodeConfig>,
) -> Result<(Vec<PathBuf>, Option<String>)> {
    let mut run_ctx = ctx.as_run_loop_context();
    // `handle_pipeline_output_full` already records touched files into
    // `session_stats` via `handle_success_common` (same underlying stats
    // object); do not record a second time here. Modified files remain the
    // snapshot content source while touched files only feed checkpoint replay.
    let (modified_files, _touched_files, last_stdout) =
        handle_pipeline_output_full(&mut run_ctx, name, args_val, outcome, vt_config).await?;

    if let ToolExecutionStatus::Success { output, modified_files, command_success: true, .. } = &outcome.status {
        let activity_paths =
            collect_instruction_activity_paths(ctx.config.workspace.as_path(), args_val, output, modified_files);
        if !activity_paths.is_empty() {
            ctx.context_manager.record_instruction_activity_paths(activity_paths);
        }
    }

    Ok((modified_files, last_stdout))
}

use activity_paths::*;
use capture_lines::*;
use command_summary::*;
use pipe_streams::*;
use render_common::*;
use tracker_block::*;

pub(crate) use tracker_block::write_tracker_progress_transcript;

mod activity_paths;
mod capture_lines;
mod command_summary;
mod pipe_streams;
mod render_common;
mod tracker_block;

#[cfg(test)]
mod tests;
