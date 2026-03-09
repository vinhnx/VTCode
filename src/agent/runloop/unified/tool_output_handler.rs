use crate::agent::runloop::mcp_events::McpPanelState;
use crate::agent::runloop::unified::state::SessionStats;
use anyhow::Result;
use std::path::PathBuf;
use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::tools::tool_intent;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_core::utils::transcript;
use vtcode_tui::{InlineHandle, InlineMessageKind, InlineSegment, InlineTextStyle};

use crate::agent::runloop::unified::run_loop_context::RunLoopContext;
use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};

fn record_mcp_success_event(
    mcp_panel_state: &mut McpPanelState,
    tool_name: &str,
    args_val: &serde_json::Value,
) {
    let mut mcp_event = crate::agent::runloop::mcp_events::McpEvent::new(
        "mcp".to_string(),
        tool_name.to_string(),
        Some(args_val.to_string()),
    );
    mcp_event.success(None);
    mcp_panel_state.add_event(mcp_event);
}

fn collect_modified_files(modified_files: &[String]) -> Vec<PathBuf> {
    modified_files.iter().map(PathBuf::from).collect()
}

fn is_run_pty_tool(name: &str, args_val: &serde_json::Value) -> bool {
    tool_intent::is_command_run_tool_call(name, args_val)
}

fn compact_run_completion_line(
    output: &serde_json::Value,
    command_success: bool,
) -> Option<String> {
    if let Some(exit_code) = output.get("exit_code").and_then(serde_json::Value::as_i64) {
        if exit_code == 0 {
            return Some("✓ run completed (exit code: 0)".to_string());
        }
        return Some(format!("✗ run error, exit code: {}", exit_code));
    }

    if output.get("is_exited").and_then(serde_json::Value::as_bool) == Some(true) {
        if command_success {
            return Some("✓ done".to_string());
        }
        return Some("✗ failed".to_string());
    }

    None
}

fn is_git_diff_payload(output: &serde_json::Value) -> bool {
    output
        .get("content_type")
        .and_then(serde_json::Value::as_str)
        .is_some_and(|content_type| content_type == "git_diff")
}

fn has_renderable_stream_content(output: &serde_json::Value) -> bool {
    ["output", "stdout", "stderr"].iter().any(|key| {
        output
            .get(*key)
            .and_then(serde_json::Value::as_str)
            .is_some_and(|s| !s.trim().is_empty())
    })
}

fn is_task_tracker_tool(name: &str) -> bool {
    matches!(name, tools::TASK_TRACKER | tools::PLAN_TASK_TRACKER)
}

fn task_tracker_call_lines(args_val: &serde_json::Value) -> Vec<String> {
    let mut lines = vec!["• Task tracker".to_string()];

    if let Some(action) = args_val.get("action").and_then(serde_json::Value::as_str) {
        lines.push(format!("  └ Action: {action}"));
    }
    if let Some(title) = args_val.get("title").and_then(serde_json::Value::as_str) {
        lines.push(format!("  └ Title: {title}"));
    }
    if let Some(index) = args_val.get("index").and_then(serde_json::Value::as_u64) {
        lines.push(format!("  └ Index: {index}"));
    } else if let Some(index_path) = args_val
        .get("index_path")
        .and_then(serde_json::Value::as_str)
    {
        lines.push(format!("  └ Index: {index_path}"));
    }
    if let Some(status) = args_val.get("status").and_then(serde_json::Value::as_str) {
        lines.push(format!("  └ Status: {status}"));
    }
    if let Some(items) = args_val.get("items").and_then(serde_json::Value::as_array)
        && !items.is_empty()
    {
        let preview = items
            .iter()
            .filter_map(serde_json::Value::as_str)
            .take(2)
            .collect::<Vec<_>>();
        let suffix = match items.len().saturating_sub(preview.len()) {
            0 => String::new(),
            remaining => format!(" +{remaining} more"),
        };
        if !preview.is_empty() {
            lines.push(format!("  └ Items: {}{}", preview.join(", "), suffix));
        }
    }

    lines
}

fn task_tracker_block_lines(
    args_val: &serde_json::Value,
    output: &serde_json::Value,
) -> Vec<String> {
    let mut lines = task_tracker_call_lines(args_val);
    lines.extend(crate::agent::runloop::tool_output::tracker_view_lines(
        output,
    ));
    lines
}

fn task_tracker_block_segments(lines: &[String]) -> Vec<Vec<InlineSegment>> {
    let style = std::sync::Arc::new(InlineTextStyle::default());
    lines
        .iter()
        .map(|line| {
            vec![InlineSegment {
                text: line.clone(),
                style: style.clone(),
            }]
        })
        .collect()
}

fn apply_task_tracker_block(
    handle: &InlineHandle,
    harness_state: &mut crate::agent::runloop::unified::run_loop_context::HarnessTurnState,
    lines: Vec<String>,
) {
    let replace_count = harness_state.replaceable_task_tracker_count();
    let segments = task_tracker_block_segments(&lines);

    if let Some(count) = replace_count {
        handle.replace_last(count, InlineMessageKind::Tool, segments);
        transcript::replace_last(count, &lines);
    } else {
        for (segments, plain_line) in segments.into_iter().zip(lines.iter()) {
            handle.append_line(InlineMessageKind::Tool, segments);
            transcript::append(plain_line);
        }
    }

    harness_state.remember_task_tracker_block(lines);
}

async fn render_tool_output_common(
    renderer: &mut AnsiRenderer,
    name: &str,
    args_val: &serde_json::Value,
    output: &serde_json::Value,
    command_success: bool,
    vt_config: Option<&VTCodeConfig>,
) -> Result<()> {
    let inline_run_tool = renderer.supports_inline_ui() && is_run_pty_tool(name, args_val);
    let git_diff_payload = is_git_diff_payload(output);

    if inline_run_tool && !git_diff_payload {
        let has_stream_content = has_renderable_stream_content(output);
        if !has_stream_content {
            if command_success {
                renderer.line(MessageStyle::ToolDetail, "(no output)")?;
            } else if let Some(completion) = compact_run_completion_line(output, command_success) {
                renderer.line(MessageStyle::ToolDetail, &completion)?;
            }
            return Ok(());
        }
    }

    // Inline PTY streaming already renders a "• Ran <command>" header. Avoid duplicating
    // it for git_diff payloads in post-tool summary rendering.
    if !(inline_run_tool && git_diff_payload) {
        let stream_label = crate::agent::runloop::unified::tool_summary::stream_label_from_output(
            output,
            command_success,
        );
        crate::agent::runloop::unified::tool_summary::render_tool_call_summary(
            renderer,
            name,
            args_val,
            stream_label,
        )?;
    }

    crate::agent::runloop::tool_output::render_tool_output(renderer, Some(name), output, vt_config)
        .await
}

fn render_error_common(
    renderer: &mut AnsiRenderer,
    name: &str,
    error: &str,
    error_type: &str,
) -> Result<()> {
    let err_msg = format!("Tool '{}' {}: {}", name, error_type, error);
    renderer.line(vtcode_core::utils::ansi::MessageStyle::Error, &err_msg)?;
    Ok(())
}

#[derive(Default)]
struct OutcomeState {
    turn_modified_files: Vec<PathBuf>,
    last_tool_stdout: Option<String>,
}

impl OutcomeState {
    fn into_tuple(self) -> (Vec<PathBuf>, Option<String>) {
        (self.turn_modified_files, self.last_tool_stdout)
    }
}

struct OutcomeContext<'a> {
    session_stats: &'a mut SessionStats,
    renderer: &'a mut AnsiRenderer,
    handle: &'a InlineHandle,
    harness_state: &'a mut crate::agent::runloop::unified::run_loop_context::HarnessTurnState,
    mcp_panel_state: &'a mut McpPanelState,
    vt_config: Option<&'a VTCodeConfig>,
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

    if let Some(tool_name) = name.strip_prefix("mcp_") {
        let tool_name = tool_name.trim_start_matches('_');
        let tool_name = tool_name.split("__").last().unwrap_or(tool_name);
        record_mcp_success_event(ctx.mcp_panel_state, tool_name, args_val);
    } else if is_task_tracker_tool(name) && ctx.renderer.supports_inline_ui() {
        let block_lines = task_tracker_block_lines(args_val, payload.output);
        apply_task_tracker_block(ctx.handle, ctx.harness_state, block_lines);
    } else {
        render_tool_output_common(
            ctx.renderer,
            name,
            args_val,
            payload.output,
            payload.command_success,
            ctx.vt_config,
        )
        .await?;
    }

    state.last_tool_stdout = if payload.command_success {
        payload.stdout.clone()
    } else {
        None
    };

    if !payload.modified_files.is_empty() {
        state
            .turn_modified_files
            .extend(collect_modified_files(payload.modified_files));
    }

    Ok(())
}

fn handle_non_success_common(
    ctx: &mut OutcomeContext<'_>,
    name: &str,
    status: &ToolExecutionStatus,
) -> Result<()> {
    match status {
        ToolExecutionStatus::Failure { error } => {
            render_error_common(ctx.renderer, name, &error.to_string(), "failure")?;
        }
        ToolExecutionStatus::Timeout { error } => {
            render_error_common(ctx.renderer, name, &error.message, "timed out")?;
        }
        ToolExecutionStatus::Cancelled => {
            ctx.renderer
                .line(MessageStyle::Info, "Tool execution cancelled")?;
        }
        ToolExecutionStatus::Success { .. } => {}
    }

    Ok(())
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
            output,
            stdout,
            modified_files,
            command_success,
            has_more: _,
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
        _ => handle_non_success_common(ctx, name, &outcome.status)?,
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
    let mut output_ctx = OutcomeContext {
        session_stats: ctx.session_stats,
        renderer: ctx.renderer,
        handle: ctx.handle,
        harness_state: ctx.harness_state,
        mcp_panel_state: ctx.mcp_panel_state,
        vt_config,
    };
    let state = process_outcome_common(&mut output_ctx, name, args_val, outcome).await?;
    Ok(state.into_tuple())
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
    handle_pipeline_output(&mut run_ctx, name, args_val, outcome, vt_config).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runloop::tui_compat::inline_theme_from_core_styles;
    use std::io::{IsTerminal, stdin};
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::sync::{RwLock, mpsc::unbounded_channel};
    use vtcode_core::acp::ToolPermissionCache;
    use vtcode_core::config::loader::VTCodeConfig;
    use vtcode_core::core::decision_tracker::DecisionTracker;
    use vtcode_core::core::trajectory::TrajectoryLogger;
    use vtcode_core::tools::ApprovalRecorder;
    use vtcode_core::tools::registry::ToolRegistry;
    use vtcode_core::tools::result_cache::{ToolCacheKey, ToolResultCache};
    use vtcode_core::ui::theme;
    use vtcode_tui::{InlineCommand, InlineHandle, SessionOptions, spawn_session_with_options};

    fn build_harness_state() -> crate::agent::runloop::unified::run_loop_context::HarnessTurnState {
        crate::agent::runloop::unified::run_loop_context::HarnessTurnState::new(
            crate::agent::runloop::unified::run_loop_context::TurnRunId("test-run".to_string()),
            crate::agent::runloop::unified::run_loop_context::TurnId("test-turn".to_string()),
            4,
            60,
            0,
        )
    }

    fn dummy_handle() -> InlineHandle {
        InlineHandle::new_for_tests(unbounded_channel().0)
    }

    // Use Tokio runtime for async test blocks
    #[tokio::test]
    async fn test_renderer_records_tool_and_collects_modified_files() {
        // Setup a stdout renderer
        let mut renderer = vtcode_core::utils::ansi::AnsiRenderer::stdout();

        // Prepare session stats and mcp state
        let mut stats = SessionStats::default();
        let mut mcp = McpPanelState::default();

        // Create an outcome that indicates write to /tmp/foo.txt
        let output_json = serde_json::json!({"result":"ok"});
        let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: output_json.clone(),
            stdout: None,
            modified_files: vec!["/tmp/foo.txt".to_string()],
            command_success: true,
            has_more: false,
        });

        // Invoke the shared outcome processor via a minimal output context.
        let handle = dummy_handle();
        let mut harness_state = build_harness_state();
        let mut output_ctx = OutcomeContext {
            session_stats: &mut stats,
            renderer: &mut renderer,
            handle: &handle,
            harness_state: &mut harness_state,
            mcp_panel_state: &mut mcp,
            vt_config: None::<&VTCodeConfig>,
        };
        let (mod_files, _last_stdout) = process_outcome_common(
            &mut output_ctx,
            "write_file",
            &serde_json::json!({}),
            &outcome,
        )
        .await
        .expect("render should succeed")
        .into_tuple();

        // Confirm the function recorded the tool call
        let recorded = stats.sorted_tools();
        assert!(recorded.contains(&"write_file".to_string()));

        // Confirm the modified files list contains our path
        assert_eq!(mod_files, vec![PathBuf::from("/tmp/foo.txt")]);
    }

    #[tokio::test]
    async fn test_renderer_records_mcp_event_for_mcp_tool() {
        let mut renderer = vtcode_core::utils::ansi::AnsiRenderer::stdout();

        // Note: tests involving `apply_turn_outcome` live in `turn/turn_loop.rs` and can be added there
        let mut stats = SessionStats::default();
        let mut mcp = McpPanelState::new(32, true); // enabled

        let output_json = serde_json::json!({"exit_code":0});
        let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: output_json.clone(),
            stdout: Some("ok".to_string()),
            modified_files: vec![],
            command_success: true,
            has_more: false,
        });

        let handle = dummy_handle();
        let mut harness_state = build_harness_state();
        let mut output_ctx = OutcomeContext {
            session_stats: &mut stats,
            renderer: &mut renderer,
            handle: &handle,
            harness_state: &mut harness_state,
            mcp_panel_state: &mut mcp,
            vt_config: None::<&VTCodeConfig>,
        };
        let (_mod_files, _last_stdout) = process_outcome_common(
            &mut output_ctx,
            "mcp_example",
            &serde_json::json!({}),
            &outcome,
        )
        .await
        .expect("render should succeed")
        .into_tuple();

        // Ensure mcp panel recorded an event
        assert!(mcp.event_count() > 0);
    }

    #[tokio::test]
    async fn test_handle_pipeline_output_collects_modified_files_and_records_stats() {
        if !stdin().is_terminal() {
            eprintln!("Skipping TUI-dependent test in non-interactive environment");
            return;
        }

        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path().to_path_buf();

        let mut registry = ToolRegistry::new(workspace.clone()).await;
        let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));

        let mut session = spawn_session_with_options(
            inline_theme_from_core_styles(&theme::active_styles()),
            SessionOptions {
                inline_rows: 10,
                workspace_root: Some(workspace.clone()),
                ..SessionOptions::default()
            },
        )
        .unwrap();
        let handle = session.clone_inline_handle();
        let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

        let cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
        let key = ToolCacheKey::new("read_file", "{}", "/tmp/foo.txt");
        {
            let mut c = cache.write().await;
            c.insert_arc(key.clone(), Arc::new("{}".to_string()));
            assert!(c.get(&key).is_some());
        }

        let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
        let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
        let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
        let approval_recorder = ApprovalRecorder::new(workspace.clone());
        let traj = TrajectoryLogger::new(&workspace);
        let tools = Arc::new(RwLock::new(Vec::new()));

        let mut harness_state = build_harness_state();
        let mut ctx = RunLoopContext::new(
            &mut renderer,
            &handle,
            &mut registry,
            &tools,
            &cache,
            &permission_cache_arc,
            &decision_ledger,
            &mut session_stats,
            &mut mcp_panel,
            &approval_recorder,
            &mut session,
            None,
            &traj,
            &mut harness_state,
            None,
        );

        let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({"ok": true}),
            stdout: None,
            modified_files: vec!["/tmp/foo.txt".to_string()],
            command_success: true,
            has_more: false,
        });

        let (mod_files, _last_stdout) = handle_pipeline_output(
            &mut ctx,
            "read_file",
            &serde_json::json!({}),
            &outcome,
            None::<&VTCodeConfig>,
        )
        .await
        .expect("handle should succeed");

        assert_eq!(mod_files, vec![PathBuf::from("/tmp/foo.txt")]);

        // Cache invalidation is handled in execution side-effects, not output rendering.
        {
            let c = cache.write().await;
            assert!(c.get(&key).is_some());
        }

        // Ensure session stats were updated
        let rec = session_stats.sorted_tools();
        assert!(rec.contains(&"read_file".to_string()));
    }

    #[tokio::test]
    async fn task_tracker_updates_replace_previous_inline_block() {
        transcript::clear();

        let (sender, mut receiver) = unbounded_channel();
        let handle = InlineHandle::new_for_tests(sender);
        let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
        let mut stats = SessionStats::default();
        let mut mcp = McpPanelState::default();
        let mut harness_state = build_harness_state();

        let first = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({
                "status": "updated",
                "view": {
                    "title": "Respond to user greeting and assess next steps",
                    "lines": [
                        {"display": "├ ✔ Greet user and summarize current workspace state"},
                        {"display": "├ > Ask what task they'd like to tackle"},
                        {"display": "└ • Offer to provide workspace tour if needed"}
                    ]
                },
                "checklist": {
                    "title": "Respond to user greeting and assess next steps",
                    "total": 3,
                    "completed": 1,
                    "in_progress": 1,
                    "pending": 1,
                    "blocked": 0,
                    "progress_percent": 33,
                    "items": [
                        {"index": 1, "description": "Greet user and summarize current workspace state", "status": "completed"},
                        {"index": 2, "description": "Ask what task they'd like to tackle", "status": "in_progress"},
                        {"index": 3, "description": "Offer to provide workspace tour if needed", "status": "pending"}
                    ]
                },
                "message": "Item 2 status changed: pending → in_progress"
            }),
            stdout: None,
            modified_files: vec![],
            command_success: true,
            has_more: false,
        });
        let second = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({
                "status": "updated",
                "view": {
                    "title": "Respond to user greeting and assess next steps",
                    "lines": [
                        {"display": "├ ✔ Greet user and summarize current workspace state"},
                        {"display": "├ ✔ Ask what task they'd like to tackle"},
                        {"display": "└ • Offer to provide workspace tour if needed"}
                    ]
                },
                "checklist": {
                    "title": "Respond to user greeting and assess next steps",
                    "total": 3,
                    "completed": 2,
                    "in_progress": 0,
                    "pending": 1,
                    "blocked": 0,
                    "progress_percent": 67,
                    "items": [
                        {"index": 1, "description": "Greet user and summarize current workspace state", "status": "completed"},
                        {"index": 2, "description": "Ask what task they'd like to tackle", "status": "completed"},
                        {"index": 3, "description": "Offer to provide workspace tour if needed", "status": "pending"}
                    ]
                },
                "message": "Item 2 status changed: in_progress → completed"
            }),
            stdout: None,
            modified_files: vec![],
            command_success: true,
            has_more: false,
        });

        let args = serde_json::json!({"action": "update", "index": 2, "status": "in_progress"});
        let mut output_ctx = OutcomeContext {
            session_stats: &mut stats,
            renderer: &mut renderer,
            handle: &handle,
            harness_state: &mut harness_state,
            mcp_panel_state: &mut mcp,
            vt_config: None::<&VTCodeConfig>,
        };

        process_outcome_common(&mut output_ctx, tools::TASK_TRACKER, &args, &first)
            .await
            .expect("first tracker render should succeed");

        let args = serde_json::json!({"action": "update", "index": 2, "status": "completed"});
        process_outcome_common(&mut output_ctx, tools::TASK_TRACKER, &args, &second)
            .await
            .expect("second tracker render should succeed");

        let mut saw_replace = false;
        while let Ok(command) = receiver.try_recv() {
            if matches!(
                command,
                InlineCommand::ReplaceLast {
                    kind: InlineMessageKind::Tool,
                    ..
                }
            ) {
                saw_replace = true;
            }
        }

        let transcript_lines = transcript::snapshot();
        assert!(
            saw_replace,
            "expected later tracker update to replace prior block"
        );
        assert_eq!(
            transcript_lines
                .iter()
                .filter(|line| line.contains("• Task tracker"))
                .count(),
            1
        );
        assert!(transcript_lines.iter().any(|line| line.contains("67%)")));
        assert!(!transcript_lines.iter().any(|line| line.contains("33%)")));

        transcript::clear();
    }

    #[tokio::test]
    async fn test_handle_pipeline_output_mcp_events() {
        if !stdin().is_terminal() {
            eprintln!("Skipping TUI-dependent test in non-interactive environment");
            return;
        }

        let tmp = TempDir::new().unwrap();
        let workspace = tmp.path().to_path_buf();

        let mut registry = ToolRegistry::new(workspace.clone()).await;
        let permission_cache_arc = Arc::new(tokio::sync::RwLock::new(ToolPermissionCache::new()));

        let mut session = spawn_session_with_options(
            inline_theme_from_core_styles(&theme::active_styles()),
            SessionOptions {
                inline_rows: 10,
                workspace_root: Some(workspace.clone()),
                ..SessionOptions::default()
            },
        )
        .unwrap();
        let handle = session.clone_inline_handle();
        let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());

        let cache = Arc::new(RwLock::new(ToolResultCache::new(8)));
        let decision_ledger = Arc::new(RwLock::new(DecisionTracker::new()));
        let mut session_stats = crate::agent::runloop::unified::state::SessionStats::default();
        let mut mcp_panel = crate::agent::runloop::mcp_events::McpPanelState::new(10, true);
        let approval_recorder = ApprovalRecorder::new(workspace.clone());
        let traj = TrajectoryLogger::new(&workspace);
        let tools = Arc::new(RwLock::new(Vec::new()));

        let mut harness_state = build_harness_state();
        let mut ctx = RunLoopContext::new(
            &mut renderer,
            &handle,
            &mut registry,
            &tools,
            &cache,
            &permission_cache_arc,
            &decision_ledger,
            &mut session_stats,
            &mut mcp_panel,
            &approval_recorder,
            &mut session,
            None,
            &traj,
            &mut harness_state,
            None,
        );

        let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: serde_json::json!({"exit_code": 0}),
            stdout: Some("ok".to_string()),
            modified_files: vec![],
            command_success: true,
            has_more: false,
        });

        let (_mod_files, _last_stdout) = handle_pipeline_output(
            &mut ctx,
            "mcp_example",
            &serde_json::json!({}),
            &outcome,
            None::<&VTCodeConfig>,
        )
        .await
        .expect("handle should succeed");

        assert!(ctx.mcp_panel_state.event_count() > 0);
    }
}
