#![allow(clippy::too_many_arguments)]
mod slash_commands;

use anyhow::{Context, Result};
use chrono::Local;
use slash_commands::{SlashCommandContext, SlashCommandControl};
use std::collections::{BTreeSet, HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Notify;
use tokio::task;

#[cfg(debug_assertions)]
use tracing::debug;
use tracing::warn;
use vtcode_core::config::constants::{defaults, tools, ui};
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::{AgentConfig as CoreAgentConfig, UiSurfacePreference};
use vtcode_core::core::agent::snapshots::{SnapshotConfig, SnapshotManager};
use vtcode_core::core::decision_tracker::{Action as DTAction, DecisionOutcome, ResponseType};
use vtcode_core::core::router::{Router, TaskClass};
use vtcode_core::core::token_constants::{
    THRESHOLD_ALERT, THRESHOLD_COMPACT, THRESHOLD_EMERGENCY, THRESHOLD_WARNING,
};
use vtcode_core::llm::error_display;
use vtcode_core::llm::provider::{self as uni};
use vtcode_core::tools::ApprovalRecorder;
use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError};
use vtcode_core::ui::theme;
use vtcode_core::ui::tui::{InlineEvent, InlineEventCallback, spawn_session, theme_from_styles};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_core::utils::at_pattern::parse_at_patterns;
use vtcode_core::utils::session_archive::{SessionArchive, SessionArchiveMetadata, SessionMessage};
// Note: specific tool error helpers and style helpers were used during stepwise refactor
// and have been removed to simplify outcome handlers. Keep generic error handling.
use vtcode_core::utils::transcript;

fn should_trigger_turn_balancer(
    step_count: usize,
    max_tool_loops: usize,
    repeated: usize,
    repeat_limit: usize,
) -> bool {
    step_count > max_tool_loops / 2 && repeated >= repeat_limit
}

use crate::agent::runloop::ResumeSession;
use crate::agent::runloop::git::confirm_changes_with_git_diff;
use crate::agent::runloop::model_picker::{ModelPickerProgress, ModelPickerState};
use crate::agent::runloop::prompt::refine_user_prompt_if_enabled;
use crate::agent::runloop::slash_commands::handle_slash_command;
use crate::agent::runloop::text_tools::{detect_textual_tool_call, extract_code_fence_blocks};
use crate::agent::runloop::tool_output::render_code_fence_blocks;
use crate::agent::runloop::ui::{build_inline_header_context, render_session_banner};
use crate::agent::runloop::unified::extract_action_from_messages;
use crate::agent::runloop::unified::mcp_tool_manager::McpToolManager;
use crate::agent::runloop::unified::ui_interaction::{
    PlaceholderSpinner, stream_and_render_response,
};

use super::finalization::finalize_session;
use super::harmony::strip_harmony_syntax;
use super::utils::{render_hook_messages, safe_force_redraw};
use super::workspace::{load_workspace_files, refresh_vt_config};
use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::async_mcp_manager::McpInitStatus;
use crate::agent::runloop::unified::context_manager::ContextManager;
use crate::agent::runloop::unified::turn::ui_sync::{redraw_with_sync, wait_for_redraw_complete};

use crate::agent::runloop::unified::display::{display_user_message, ensure_turn_bottom_gap};
use crate::agent::runloop::unified::inline_events::{
    InlineEventLoopResources, InlineInterruptCoordinator, InlineLoopAction, poll_inline_loop_action,
};
use crate::agent::runloop::unified::loop_detection::{
    LoopDetectionResponse, LoopDetector, prompt_for_loop_detection,
};
use crate::agent::runloop::unified::model_selection::finalize_model_selection;
use crate::agent::runloop::unified::palettes::{ActivePalette, apply_prompt_style};
use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::session_setup::{
    SessionState, build_mcp_tool_definitions, initialize_session,
};
use crate::agent::runloop::unified::shell::{
    derive_recent_tool_output, should_short_circuit_shell,
};
use crate::agent::runloop::unified::state::{CtrlCSignal, CtrlCState, SessionStats};
use crate::agent::runloop::unified::status_line::{
    InputStatusState, update_context_efficiency, update_input_status_if_changed,
};
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use crate::agent::runloop::unified::tool_pipeline::{
    ToolExecutionStatus, execute_tool_with_timeout_ref,
};
use crate::agent::runloop::unified::tool_routing::{ToolPermissionFlow, ensure_tool_permission};
use crate::agent::runloop::unified::tool_summary::{
    describe_tool_action, humanize_tool_name, render_tool_call_summary_with_status,
};
use crate::agent::runloop::unified::workspace_links::LinkedDirectory;
use crate::hooks::lifecycle::{LifecycleHookEngine, SessionEndReason, SessionStartTrigger};
use crate::ide_context::IdeContextBridge;
use vtcode_core::tools::autonomous_executor::{AutonomousExecutor, AutonomousPolicy};

#[allow(dead_code)]
pub enum TurnLoopResult {
    Completed,
    Aborted,
    Cancelled,
    Blocked { reason: Option<String> },
}

#[allow(dead_code)]
pub enum PrepareToolCallResult {
    Approved,
    Denied,
    Exit,
    Interrupted,
}

#[allow(dead_code)]
const SELF_REVIEW_MIN_LENGTH: usize = 240;

#[allow(dead_code)]
pub(crate) async fn run_turn_prepare_tool_call<
    S: vtcode_core::core::interfaces::ui::UiSession + ?Sized,
>(
    tool_registry: &mut vtcode_core::tools::registry::ToolRegistry,
    name: &str,
    args_val: Option<&serde_json::Value>,
    renderer: &mut AnsiRenderer,
    handle: &vtcode_core::ui::tui::InlineHandle,
    session: &mut S,
    default_placeholder: Option<String>,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    hooks: Option<&LifecycleHookEngine>,
    justification: Option<&vtcode_core::tools::ToolJustification>,
    approval_recorder: Option<&ApprovalRecorder>,
    decision_ledger: Option<
        &Arc<tokio::sync::RwLock<vtcode_core::core::decision_tracker::DecisionTracker>>,
    >,
    mcp_panel_state: Option<&mut crate::agent::runloop::mcp_events::McpPanelState>,
    tool_result_cache: Option<&Arc<tokio::sync::RwLock<vtcode_core::tools::ToolResultCache>>>,
    tool_permission_cache: Option<&Arc<tokio::sync::RwLock<vtcode_core::acp::ToolPermissionCache>>>,
    session_stats: &mut crate::agent::runloop::unified::state::SessionStats,
    traj: &vtcode_core::core::trajectory::TrajectoryLogger,
    working_history: &mut Vec<vtcode_core::llm::provider::Message>,
    call_id: &str,
    dec_id: &str,
    vt_cfg: Option<&VTCodeConfig>,
    token_budget: &vtcode_core::core::token_budget::TokenBudgetManager,
    last_forced_redraw: &mut Instant,
) -> Result<PrepareToolCallResult> {
    match ensure_tool_permission(
        tool_registry,
        name,
        args_val,
        renderer,
        handle,
        session,
        default_placeholder,
        ctrl_c_state,
        ctrl_c_notify,
        hooks,
        justification,
        approval_recorder,
        decision_ledger,
        tool_permission_cache,
    )
    .await
    {
        Ok(ToolPermissionFlow::Approved) => Ok(PrepareToolCallResult::Approved),
        Ok(ToolPermissionFlow::Denied) => {
            // Force redraw after modal closes
            safe_force_redraw(handle, last_forced_redraw);
            redraw_with_sync(handle).await?;

            session_stats.record_tool(name);
            let denial = ToolExecutionError::new(
                name.to_string(),
                ToolErrorType::PolicyViolation,
                format!("Tool '{}' execution denied by policy", name),
            )
            .to_json_value();
            traj.log_tool_call(
                working_history.len(),
                name,
                args_val.unwrap_or(&serde_json::json!({})),
                false,
            );
            // Build a ToolPipelineOutcome wrapper for the denial, and render via adapter
            use crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output_renderer;
            use crate::agent::runloop::unified::tool_pipeline::{
                ToolExecutionStatus, ToolPipelineOutcome,
            };
            let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
                output: denial.clone(),
                stdout: None,
                modified_files: vec![],
                command_success: false,
                has_more: false,
            });
            // Prepare an optional disabled panel if none is passed
            let mut disabled_mcp = crate::agent::runloop::mcp_events::McpPanelState::disabled();
            let mcp_ref: &mut crate::agent::runloop::mcp_events::McpPanelState =
                match mcp_panel_state {
                    Some(m) => m,
                    None => &mut disabled_mcp,
                };
            handle_pipeline_output_renderer(
                renderer,
                session_stats,
                mcp_ref,
                tool_result_cache,
                decision_ledger,
                name,
                args_val.unwrap_or(&serde_json::json!({})),
                &outcome,
                vt_cfg,
                token_budget,
            )
            .await?;
            let content = serde_json::to_string(&denial).unwrap_or("{}".to_string());
            working_history.push(
                vtcode_core::llm::provider::Message::tool_response_with_origin(
                    call_id.to_string(),
                    content,
                    name.to_string(),
                ),
            );
            if let Some(ledger) = decision_ledger {
                let mut ledger = ledger.write().await;
                ledger.record_outcome(
                    dec_id,
                    DecisionOutcome::Failure {
                        error: format!("Tool '{}' execution denied by policy", name),
                        recovery_attempts: 0,
                        context_preserved: true,
                    },
                );
            }
            Ok(PrepareToolCallResult::Denied)
        }
        Ok(ToolPermissionFlow::Exit) => Ok(PrepareToolCallResult::Exit),
        Ok(ToolPermissionFlow::Interrupted) => Ok(PrepareToolCallResult::Interrupted),
        Err(err) => {
            // Force redraw after modal closes
            safe_force_redraw(handle, last_forced_redraw);
            redraw_with_sync(handle).await?;

            traj.log_tool_call(
                working_history.len(),
                name,
                args_val.unwrap_or(&serde_json::json!({})),
                false,
            );
            renderer.line(
                MessageStyle::Error,
                &format!("Failed to evaluate policy for tool '{}': {}", name, err),
            )?;
            let err_json = serde_json::json!({
                "error": format!("Policy evaluation error for '{}' : {}", name, err)
            });
            working_history.push(
                vtcode_core::llm::provider::Message::tool_response_with_origin(
                    call_id.to_string(),
                    err_json.to_string(),
                    name.to_string(),
                ),
            );
            if let Some(ledger) = decision_ledger {
                let mut ledger = ledger.write().await;
                ledger.record_outcome(
                    dec_id,
                    DecisionOutcome::Failure {
                        error: format!("Failed to evaluate policy for '{}': {}", name, err),
                        recovery_attempts: 0,
                        context_preserved: true,
                    },
                );
            }
            Ok(PrepareToolCallResult::Denied)
        }
    }
}

#[allow(dead_code)]
pub(crate) async fn run_turn_execute_tool(
    tool_registry: &mut vtcode_core::tools::registry::ToolRegistry,
    name: &str,
    args_val: &serde_json::Value,
    is_read_only_tool: bool,
    tool_result_cache: &Arc<tokio::sync::RwLock<vtcode_core::tools::ToolResultCache>>,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    progress_reporter: Option<&ProgressReporter>,
    handle: &vtcode_core::ui::tui::InlineHandle,
    last_forced_redraw: &mut Instant,
) -> ToolExecutionStatus {
    use vtcode_core::tools::result_cache::ToolCacheKey;

    // Try to get from cache first for read-only tools
    if is_read_only_tool {
        let _params_str = serde_json::to_string(args_val).unwrap_or_default();
        let cache_key = ToolCacheKey::from_json(name, args_val, "");
        {
            let mut tool_cache = tool_result_cache.write().await;
            if let Some(cached_output) = tool_cache.get(&cache_key) {
                #[cfg(debug_assertions)]
                tracing::debug!("Cache hit for tool: {}", name);

                // Return cached result wrapped as tool success
                let cached_json: serde_json::Value =
                    serde_json::from_str(&cached_output).unwrap_or(serde_json::json!({}));
                return ToolExecutionStatus::Success {
                    output: cached_json,
                    stdout: None,
                    modified_files: vec![],
                    command_success: true,
                    has_more: false,
                };
            }
        }
        // Force TUI refresh to ensure display stability before executing
        safe_force_redraw(handle, last_forced_redraw);

        let result = execute_tool_with_timeout_ref(
            tool_registry,
            name,
            args_val,
            ctrl_c_state,
            ctrl_c_notify,
            progress_reporter,
        )
        .await;

        // Cache successful read-only results
        if let ToolExecutionStatus::Success { ref output, .. } = result {
            let output_json = serde_json::to_string(output).unwrap_or_else(|_| "{}".to_string());
            let mut cache = tool_result_cache.write().await;
            cache.insert_arc(cache_key, Arc::new(output_json));
        }

        return result;
    }

    // Non-cached path for write tools
    safe_force_redraw(handle, last_forced_redraw);

    execute_tool_with_timeout_ref(
        tool_registry,
        name,
        args_val,
        ctrl_c_state,
        ctrl_c_notify,
        progress_reporter,
    )
    .await
}

#[allow(dead_code)]
pub(crate) async fn run_turn_handle_tool_success(
    name: &str,
    output: serde_json::Value,
    stdout: Option<String>,
    modified_files: Vec<String>,
    command_success: bool,
    has_more: bool,
    renderer: &mut AnsiRenderer,
    handle: &vtcode_core::ui::tui::InlineHandle,
    session_stats: &mut crate::agent::runloop::unified::state::SessionStats,
    // repeated_tool_attempts is managed by the caller; not required here
    traj: &vtcode_core::core::trajectory::TrajectoryLogger,
    mcp_panel_state: &mut mcp_events::McpPanelState,
    tool_result_cache: &Arc<tokio::sync::RwLock<vtcode_core::tools::ToolResultCache>>,
    vt_cfg: Option<&VTCodeConfig>,
    token_budget: &vtcode_core::core::token_budget::TokenBudgetManager,
    token_counter: &Arc<tokio::sync::RwLock<vtcode_core::llm::TokenCounter>>,
    working_history: &mut Vec<uni::Message>,
    call_id: &str,
    dec_id: &str,
    decision_ledger: &Arc<
        tokio::sync::RwLock<vtcode_core::core::decision_tracker::DecisionTracker>,
    >,
    last_tool_stdout: &mut Option<String>,
    any_write_effect: &mut bool,
    turn_modified_files: &mut std::collections::BTreeSet<std::path::PathBuf>,
    skip_confirmations: bool,
    lifecycle_hooks: &Option<LifecycleHookEngine>,
    bottom_gap_applied: &mut bool,
    last_forced_redraw: &mut Instant,
    input: &str,
) -> Result<Option<TurnLoopResult>> {
    // Mirror original success handling but return Some(TurnLoopResult) when we need to break the outer loop.
    safe_force_redraw(handle, last_forced_redraw);
    redraw_with_sync(handle).await?;

    session_stats.record_tool(name);
    // repeated_tool_attempts is mutated by caller; remove signature elsewhere as original code did before calling helper
    // Note: caller must manage repeated_tool_attempts removal.
    traj.log_tool_call(
        working_history.len(),
        name,
        &serde_json::to_value(&output).unwrap_or(serde_json::json!({})),
        true,
    );

    // Handle MCP events
    if let Some(tool_name) = name.strip_prefix("mcp_") {
        let mut mcp_event = mcp_events::McpEvent::new(
            "mcp".to_string(),
            tool_name.to_string(),
            Some(output.to_string()),
        );
        mcp_event.success(None);
        mcp_panel_state.add_event(mcp_event);
    } else {
        // Render tool summary with status
        let exit_code = output.get("exit_code").and_then(|v| v.as_i64());
        let status_icon = if command_success { "✓" } else { "✗" };
        render_tool_call_summary_with_status(
            renderer,
            name,
            &serde_json::to_value(&output).unwrap_or(serde_json::json!({})),
            status_icon,
            exit_code,
        )?;
    }

    // Render unified tool output via generic minimal adapter, to ensure consistent handling
    let _ = crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output_renderer(
        renderer,
        session_stats,
        mcp_panel_state,
        Some(tool_result_cache),
        Some(decision_ledger),
        name,
        &serde_json::json!({}),
        &crate::agent::runloop::unified::tool_pipeline::ToolPipelineOutcome::from_status(
            crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus::Success {
                output: output.clone(),
                stdout: stdout.clone(),
                modified_files: modified_files.clone(),
                command_success,
                has_more,
            },
        ),
        vt_cfg,
        token_budget,
    )
    .await?;

    *last_tool_stdout = if command_success {
        stdout.clone()
    } else {
        None
    };

    if matches!(
        name,
        "write_file" | "edit_file" | "create_file" | "delete_file"
    ) {
        *any_write_effect = true;
    }

    if !modified_files.is_empty() {
        if confirm_changes_with_git_diff(&modified_files, skip_confirmations).await? {
            renderer.line(MessageStyle::Info, "Changes applied successfully.")?;
            for f in &modified_files {
                turn_modified_files.insert(std::path::PathBuf::from(f));
            }
            // Invalidate cache for modified files
            for file_path in &modified_files {
                let mut cache = tool_result_cache.write().await;
                cache.invalidate_for_path(file_path);
            }
        } else {
            renderer.line(MessageStyle::Info, "Changes discarded.")?;
        }
    }

    let mut notice_lines: Vec<String> = Vec::new();
    if !modified_files.is_empty() {
        notice_lines.push("Files touched:".to_string());
        for file in &modified_files {
            notice_lines.push(format!("  - {}", file));
        }
        if let Some(stdout_preview) = &*last_tool_stdout {
            let preview: String = stdout_preview.chars().take(80).collect();
            notice_lines.push(format!("stdout preview: {}", preview));
        }
    }
    if let Some(notice) = output.get("notice").and_then(|value| value.as_str())
        && !notice.trim().is_empty()
    {
        notice_lines.push(notice.trim().to_string());
    }
    if !notice_lines.is_empty() {
        renderer.line(MessageStyle::Info, "")?;
        for line in notice_lines {
            renderer.line(MessageStyle::Info, &line)?;
        }
    }

    let content = serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_string());

    // Track token usage for this tool result
    {
        let mut counter = token_counter.write().await;
        counter.count_with_profiling("tool_output", &content);
    }

    working_history.push(uni::Message::tool_response_with_origin(
        call_id.to_string(),
        content,
        name.to_string(),
    ));

    let mut hook_block_reason: Option<String> = None;

    if let Some(hooks) = lifecycle_hooks {
        match hooks
            .run_post_tool_use(
                name,
                Some(&serde_json::to_value(&output).unwrap_or(serde_json::json!({}))),
                &output,
            )
            .await
        {
            Ok(outcome) => {
                render_hook_messages(renderer, &outcome.messages)?;
                for context in outcome.additional_context {
                    if !context.trim().is_empty() {
                        working_history.push(uni::Message::system(context));
                    }
                }
                if let Some(reason) = outcome.block_reason {
                    let trimmed = reason.trim();
                    if !trimmed.is_empty() {
                        renderer.line(MessageStyle::Info, trimmed)?;
                        hook_block_reason = Some(trimmed.to_string());
                    }
                }
            }
            Err(err) => {
                renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to run post-tool hooks: {}", err),
                )?;
            }
        }
    }

    if let Some(reason) = hook_block_reason {
        let blocked_message = format!("Tool execution blocked by lifecycle hooks: {}", reason);
        working_history.push(uni::Message::system(blocked_message));
        {
            let mut ledger = decision_ledger.write().await;
            ledger.record_outcome(
                dec_id,
                DecisionOutcome::Failure {
                    error: reason.clone(),
                    recovery_attempts: 0,
                    context_preserved: true,
                },
            );
        }
        // Signal session end and break outer loop
        return Ok(Some(TurnLoopResult::Blocked {
            reason: Some(reason),
        }));
    }

    {
        let mut ledger = decision_ledger.write().await;
        ledger.record_outcome(
            dec_id,
            DecisionOutcome::Success {
                result: "tool_ok".to_string(),
                metrics: Default::default(),
            },
        );
    }

    let allow_short_circuit = !has_more
        && command_success
        && should_short_circuit_shell(
            input,
            name,
            &serde_json::to_value(&output).unwrap_or(serde_json::json!({})),
        );

    if allow_short_circuit {
        let reply = derive_recent_tool_output(working_history)
            .unwrap_or_else(|| "Command completed successfully.".to_string());
        renderer.line(MessageStyle::Response, &reply)?;
        ensure_turn_bottom_gap(renderer, bottom_gap_applied)?;
        working_history.push(uni::Message::assistant(reply));
        let _ = last_tool_stdout.take();
        return Ok(Some(TurnLoopResult::Completed));
    }

    Ok(None)
}

#[allow(dead_code)]
pub(crate) async fn run_turn_handle_tool_failure(
    name: &str,
    error: anyhow::Error,
    renderer: &mut AnsiRenderer,
    handle: &vtcode_core::ui::tui::InlineHandle,
    session_stats: &mut crate::agent::runloop::unified::state::SessionStats,
    traj: &vtcode_core::core::trajectory::TrajectoryLogger,
    working_history: &mut Vec<uni::Message>,
    call_id: &str,
    dec_id: &str,
    mcp_panel_state: &mut mcp_events::McpPanelState,
    token_counter: &Arc<tokio::sync::RwLock<vtcode_core::llm::TokenCounter>>,
    decision_ledger: &Arc<
        tokio::sync::RwLock<vtcode_core::core::decision_tracker::DecisionTracker>,
    >,
    tool_result_cache: Option<&Arc<tokio::sync::RwLock<vtcode_core::tools::ToolResultCache>>>,
    vt_cfg: Option<&VTCodeConfig>,
    token_budget: &vtcode_core::core::token_budget::TokenBudgetManager,
) -> Result<()> {
    // Finish spinner / ensure redraw is caller's responsibility
    safe_force_redraw(handle, &mut Instant::now());
    redraw_with_sync(handle).await?;

    session_stats.record_tool(name);

    // Display a simple failure message and log
    let failure_msg = format!("Tool '{}' failed: {}", name, error);
    renderer.line(MessageStyle::Error, &failure_msg)?;
    // Provide simple recovery hint to reduce repeated failures
    let recovery_hint = match name {
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
    renderer.line(MessageStyle::Info, recovery_hint)?;
    working_history.push(uni::Message::system(format!(
        "Tool '{}' failed. Hint: {}",
        name, recovery_hint
    )));

    traj.log_tool_call(working_history.len(), name, &serde_json::json!({}), false);

    let error_message = error.to_string();
    let error_json = serde_json::json!({ "error": error_message });

    if let Some(tool_name) = name.strip_prefix("mcp_") {
        renderer.line_if_not_empty(MessageStyle::Output)?;
        renderer.line(
            MessageStyle::Error,
            &format!("MCP tool {} failed: {}", tool_name, error_message),
        )?;
        handle.force_redraw();
        wait_for_redraw_complete().await?;

        let mut mcp_event = mcp_events::McpEvent::new(
            "mcp".to_string(),
            tool_name.to_string(),
            Some(serde_json::to_string(&error_json).unwrap_or_default()),
        );
        mcp_event.failure(Some(error_message.clone()));
        mcp_panel_state.add_event(mcp_event);
    }

    renderer.line(MessageStyle::Error, &error_message)?;
    // Render via the renderer adapter so all cache invalidation and MCP events are handled
    use crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output_renderer;
    use crate::agent::runloop::unified::tool_pipeline::{ToolExecutionStatus, ToolPipelineOutcome};
    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
        output: error_json.clone(),
        stdout: None,
        modified_files: vec![],
        command_success: false,
        has_more: false,
    });
    handle_pipeline_output_renderer(
        renderer,
        session_stats,
        mcp_panel_state,
        tool_result_cache,
        Some(decision_ledger),
        name,
        &serde_json::json!({}),
        &outcome,
        vt_cfg,
        token_budget,
    )
    .await?;

    // Track error token usage
    {
        let mut counter = token_counter.write().await;
        let error_content = serde_json::to_string(&error_json).unwrap_or_else(|_| "{}".to_string());
        counter.count_with_profiling("tool_output", &error_content);
    }

    working_history.push(uni::Message::tool_response_with_origin(
        call_id.to_string(),
        serde_json::to_string(&error_json).unwrap_or_default(),
        name.to_string(),
    ));

    {
        let mut ledger = decision_ledger.write().await;
        ledger.record_outcome(
            dec_id,
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
pub(crate) async fn run_turn_handle_tool_timeout(
    name: &str,
    error: anyhow::Error,
    renderer: &mut AnsiRenderer,
    handle: &vtcode_core::ui::tui::InlineHandle,
    session_stats: &mut crate::agent::runloop::unified::state::SessionStats,
    traj: &vtcode_core::core::trajectory::TrajectoryLogger,
    working_history: &mut Vec<uni::Message>,
    call_id: &str,
    dec_id: &str,
    token_counter: &Arc<tokio::sync::RwLock<vtcode_core::llm::TokenCounter>>,
    decision_ledger: &Arc<
        tokio::sync::RwLock<vtcode_core::core::decision_tracker::DecisionTracker>,
    >,
) -> Result<()> {
    // Timeout handling mirrors original behavior
    handle.force_redraw();
    wait_for_redraw_complete().await?;

    session_stats.record_tool(name);
    renderer.line_if_not_empty(MessageStyle::Output)?;
    renderer.line(
        MessageStyle::Error,
        &format!("Tool {} timed out after 5 minutes.", name),
    )?;
    traj.log_tool_call(working_history.len(), name, &serde_json::json!({}), false);

    let error_message = error.to_string();
    let err_json = serde_json::json!({ "error": error_message });
    let timeout_content = serde_json::to_string(&err_json).unwrap_or_else(|_| "{}".to_string());

    // Track timeout error token usage
    {
        let mut counter = token_counter.write().await;
        counter.count_with_profiling("tool_output", &timeout_content);
    }

    working_history.push(uni::Message::tool_response_with_origin(
        call_id.to_string(),
        timeout_content,
        name.to_string(),
    ));

    {
        let mut ledger = decision_ledger.write().await;
        ledger.record_outcome(
            dec_id,
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
    name: &str,
    renderer: &mut AnsiRenderer,
    handle: &vtcode_core::ui::tui::InlineHandle,
    session_stats: &mut crate::agent::runloop::unified::state::SessionStats,
    working_history: &mut Vec<uni::Message>,
    call_id: &str,
    dec_id: &str,
    decision_ledger: &Arc<
        tokio::sync::RwLock<vtcode_core::core::decision_tracker::DecisionTracker>,
    >,
) -> Result<TurnLoopResult> {
    safe_force_redraw(handle, &mut Instant::now());
    redraw_with_sync(handle).await?;

    session_stats.record_tool(name);

    renderer.line_if_not_empty(MessageStyle::Output)?;
    renderer.line(
        MessageStyle::Info,
        "Operation cancelled by user. Stopping current turn.",
    )?;

    let err_json = serde_json::json!({ "error": "Tool execution cancelled by user" });

    working_history.push(uni::Message::tool_response_with_origin(
        call_id.to_string(),
        serde_json::to_string(&err_json).unwrap_or_else(|_| "{}".to_string()),
        name.to_string(),
    ));

    {
        let mut ledger = decision_ledger.write().await;
        ledger.record_outcome(
            dec_id,
            DecisionOutcome::Failure {
                error: "Cancelled by user".to_string(),
                recovery_attempts: 0,
                context_preserved: true,
            },
        );
    }

    Ok(TurnLoopResult::Cancelled)
}

#[allow(dead_code)]
pub(crate) async fn run_single_agent_loop_unified(
    config: &CoreAgentConfig,
    mut vt_cfg: Option<VTCodeConfig>,
    skip_confirmations: bool,
    full_auto: bool,
    resume: Option<ResumeSession>,
) -> Result<()> {
    // Set up panic handler to ensure MCP cleanup on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        eprintln!("Application panic occurred: {:?}", panic_info);
        // Note: We can't easily access the MCP client here due to move semantics
        // The cleanup will happen in the Drop implementations
        original_hook(panic_info);
    }));

    // Note: The original hook will not be restored during this session
    // but Rust runtime should handle this appropriately
    let mut config = config.clone();
    let mut resume_state = resume;

    loop {
        let resume_ref = resume_state.as_ref();

        let session_trigger = if resume_ref.is_some() {
            SessionStartTrigger::Resume
        } else {
            SessionStartTrigger::Startup
        };
        let lifecycle_hooks = if let Some(vt) = vt_cfg.as_ref() {
            LifecycleHookEngine::new(config.workspace.clone(), &vt.hooks, session_trigger)?
        } else {
            None
        };

        let SessionState {
            session_bootstrap,
            mut provider_client,
            mut tool_registry,
            tools,
            trim_config,
            mut conversation_history,
            decision_ledger,
            pruning_ledger,
            trajectory: traj,
            base_system_prompt,
            full_auto_allowlist,
            async_mcp_manager,
            mut mcp_panel_state,
            token_budget,
            token_budget_enabled,
            token_counter,
            tool_result_cache,
            tool_permission_cache,
            search_metrics: _,
            custom_prompts,
            loaded_skills,
        } = initialize_session(&config, vt_cfg.as_ref(), full_auto, resume_ref).await?;

        // Restore skills from previous session if resuming
        if let Some(resume_session) = resume_ref {
            let skill_names_to_restore = &resume_session.snapshot.metadata.loaded_skills;
            if !skill_names_to_restore.is_empty() {
                use std::sync::Arc;
                use vtcode_core::config::types::CapabilityLevel;
                use vtcode_core::skills::executor::SkillToolAdapter;
                use vtcode_core::skills::loader::EnhancedSkillLoader;
                use vtcode_core::tools::ToolRegistration;

                let mut skill_loader = EnhancedSkillLoader::new(config.workspace.clone());
                for skill_name in skill_names_to_restore {
                    match skill_loader.get_skill(skill_name).await {
                        Ok(enhanced_skill) => match enhanced_skill {
                            vtcode_core::skills::loader::EnhancedSkill::Traditional(skill) => {
                                let adapter = SkillToolAdapter::new(skill.clone());
                                let adapter_arc = Arc::new(adapter);
                                let name_static: &'static str =
                                    Box::leak(Box::new(skill_name.clone()));
                                let registration = ToolRegistration::from_tool(
                                    name_static,
                                    CapabilityLevel::Bash,
                                    adapter_arc,
                                );
                                if let Err(e) = tool_registry.register_tool(registration) {
                                    tracing::warn!(
                                        "Failed to restore skill '{}': {}",
                                        skill_name,
                                        e
                                    );
                                } else {
                                    loaded_skills
                                        .write()
                                        .await
                                        .insert(skill_name.clone(), skill);
                                }
                            }
                            vtcode_core::skills::loader::EnhancedSkill::CliTool(_bridge) => {
                                tracing::warn!(
                                    "Cannot restore CLI tool skill '{}' as traditional skill",
                                    skill_name
                                );
                            }
                        },
                        Err(e) => {
                            tracing::warn!(
                                "Failed to load skill '{}' during resume: {}",
                                skill_name,
                                e
                            );
                        }
                    }
                }
            }
        }

        let mut session_end_reason = SessionEndReason::Completed;

        let mut context_manager = ContextManager::new(
            base_system_prompt,
            trim_config,
            token_budget.clone(),
            token_budget_enabled,
        );
        let trim_config = context_manager.trim_config();
        let token_budget_enabled = context_manager.token_budget_enabled();

        let active_styles = theme::active_styles();
        let theme_spec = theme_from_styles(&active_styles);
        let mut default_placeholder = session_bootstrap
            .placeholder
            .clone()
            .or_else(|| Some(ui::CHAT_INPUT_PLACEHOLDER_BOOTSTRAP.to_string()));
        let mut follow_up_placeholder = if session_bootstrap.placeholder.is_none() {
            Some(ui::CHAT_INPUT_PLACEHOLDER_FOLLOW_UP.to_string())
        } else {
            None
        };
        let inline_rows = vt_cfg
            .as_ref()
            .map(|cfg| cfg.ui.inline_viewport_rows)
            .unwrap_or(ui::DEFAULT_INLINE_VIEWPORT_ROWS);
        let show_timeline_pane = vt_cfg
            .as_ref()
            .map(|cfg| cfg.ui.show_timeline_pane)
            .unwrap_or(ui::INLINE_SHOW_TIMELINE_PANE);

        // Set environment variable to indicate TUI mode is active
        // This prevents CLI dialoguer prompts from corrupting the TUI display
        // SAFETY: Setting a process-local environment variable is safe; the OS copies the value.
        unsafe {
            std::env::set_var("VTCODE_TUI_MODE", "1");
        }

        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());
        let interrupt_callback: InlineEventCallback = {
            let state = ctrl_c_state.clone();
            let notify = ctrl_c_notify.clone();
            Arc::new(move |event: &InlineEvent| {
                if matches!(event, InlineEvent::Interrupt) {
                    let _ = state.register_signal();
                    notify.notify_waiters();
                }
            })
        };

        let mut session = spawn_session(
            theme_spec.clone(),
            default_placeholder.clone(),
            config.ui_surface,
            inline_rows,
            show_timeline_pane,
            Some(interrupt_callback),
        )
        .context("failed to launch inline session")?;
        let handle = session.clone_inline_handle();
        let highlight_config = vt_cfg
            .as_ref()
            .map(|cfg| cfg.syntax_highlighting.clone())
            .unwrap_or_default();

        // Set the inline handle for the message queue system
        transcript::set_inline_handle(Arc::new(handle.clone()));

        let mut ide_context_bridge = IdeContextBridge::from_env();
        let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), highlight_config);

        let workspace_for_indexer = config.workspace.clone();
        let workspace_for_palette = config.workspace.clone();
        let handle_for_indexer = handle.clone();
        // Spawn background task for file palette loading. See: https://ratatui.rs/faq/
        let _file_palette_task = tokio::spawn(async move {
            match load_workspace_files(workspace_for_indexer).await {
                Ok(files) => {
                    if !files.is_empty() {
                        handle_for_indexer.load_file_palette(files, workspace_for_palette);
                    } else {
                        tracing::debug!("No files found in workspace for file palette");
                    }
                }
                Err(err) => {
                    tracing::warn!("Failed to load workspace files for file palette: {}", err);
                }
            }
        });

        transcript::clear();

        if let Some(session) = resume_state.as_ref() {
            let ended_local = session
                .snapshot
                .ended_at
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M");
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Resuming session {} · ended {} · {} messages",
                    session.identifier,
                    ended_local,
                    session.message_count()
                ),
            )?;
            renderer.line(
                MessageStyle::Info,
                &format!("Previous archive: {}", session.path.display()),
            )?;
            renderer.line_if_not_empty(MessageStyle::Output)?;
        }

        let workspace_label = config
            .workspace
            .file_name()
            .and_then(|component| component.to_str())
            .map(|value| value.to_string())
            .unwrap_or_else(|| "workspace".to_string());
        let workspace_path = config.workspace.to_string_lossy().into_owned();
        let provider_label = if config.provider.trim().is_empty() {
            provider_client.name().to_string()
        } else {
            config.provider.clone()
        };
        let header_provider_label = provider_label.clone();
        let archive_metadata = SessionArchiveMetadata::new(
            workspace_label,
            workspace_path,
            config.model.clone(),
            provider_label,
            config.theme.clone(),
            config.reasoning_effort.as_str().to_string(),
        );
        let mut session_archive_error: Option<String> = None;
        let mut session_archive = match SessionArchive::new(archive_metadata).await {
            Ok(archive) => Some(archive),
            Err(err) => {
                session_archive_error = Some(err.to_string());
                None
            }
        };

        if let (Some(hooks), Some(archive)) = (&lifecycle_hooks, session_archive.as_ref()) {
            hooks
                .update_transcript_path(Some(archive.path().to_path_buf()))
                .await;
        }

        let mut checkpoint_config = SnapshotConfig::new(config.workspace.clone());
        checkpoint_config.enabled = config.checkpointing_enabled;
        checkpoint_config.storage_dir = config.checkpointing_storage_dir.clone();
        checkpoint_config.max_snapshots = config.checkpointing_max_snapshots;
        checkpoint_config.max_age_days = config.checkpointing_max_age_days;

        let checkpoint_manager = match SnapshotManager::new(checkpoint_config) {
            Ok(manager) => Some(manager),
            Err(err) => {
                warn!("Failed to initialize checkpoint manager: {}", err);
                None
            }
        };
        let mut next_checkpoint_turn = checkpoint_manager
            .as_ref()
            .and_then(|manager| manager.next_turn_number().ok())
            .unwrap_or(1);

        handle.set_theme(theme_spec);
        apply_prompt_style(&handle);
        handle.set_placeholder(default_placeholder.clone());

        let reasoning_label = vt_cfg
            .as_ref()
            .map(|cfg| cfg.agent.reasoning_effort.as_str().to_string())
            .unwrap_or_else(|| config.reasoning_effort.as_str().to_string());

        // Render the session banner, now enriched with Git branch and status information.
        render_session_banner(
            &mut renderer,
            &config,
            &session_bootstrap,
            &config.model,
            &reasoning_label,
        )?;

        if let Some(bridge) = ide_context_bridge.as_mut() {
            match bridge.snapshot() {
                Ok(Some(context)) => {
                    conversation_history.push(uni::Message::system(context));
                }
                Ok(None) => {}
                Err(err) => {
                    warn!("Failed to update IDE context snapshot: {}", err);
                }
            }
        }

        if let Some(hooks) = &lifecycle_hooks {
            match hooks.run_session_start().await {
                Ok(outcome) => {
                    render_hook_messages(&mut renderer, &outcome.messages)?;
                    for context in outcome.additional_context {
                        if !context.trim().is_empty() {
                            conversation_history.push(uni::Message::system(context));
                        }
                    }
                }
                Err(err) => {
                    renderer.line(
                        MessageStyle::Error,
                        &format!("Failed to run session start hooks: {}", err),
                    )?;
                }
            }
        }
        let mode_label = match (config.ui_surface, full_auto) {
            (UiSurfacePreference::Inline, true) => "auto".to_string(),
            (UiSurfacePreference::Inline, false) => "inline".to_string(),
            (UiSurfacePreference::Alternate, _) => "alt".to_string(),
            (UiSurfacePreference::Auto, true) => "auto".to_string(),
            (UiSurfacePreference::Auto, false) => "std".to_string(),
        };
        let header_context = build_inline_header_context(
            &config,
            &session_bootstrap,
            header_provider_label,
            config.model.clone(),
            mode_label,
            reasoning_label.clone(),
        )
        .await?;
        handle.set_header_context(header_context);
        // MCP events are now rendered as message blocks in the conversation history

        if let Some(message) = session_archive_error.take() {
            renderer.line(
                MessageStyle::Info,
                &format!("Session archiving disabled: {}", message),
            )?;
            renderer.line_if_not_empty(MessageStyle::Output)?;
        }

        if full_auto && let Some(allowlist) = full_auto_allowlist.as_ref() {
            if allowlist.is_empty() {
                renderer.line(
                    MessageStyle::Info,
                    "Full-auto mode enabled with no tool permissions; tool calls will be skipped.",
                )?;
            } else {
                renderer.line(
                    MessageStyle::Info,
                    &format!(
                        "Full-auto mode enabled. Permitted tools: {}",
                        allowlist.join(", ")
                    ),
                )?;
            }
        }

        let async_mcp_manager_for_signal = async_mcp_manager.clone();
        {
            let state = ctrl_c_state.clone();
            let notify = ctrl_c_notify.clone();
            let handle_for_signal = handle.clone();
            // Spawn Ctrl+C signal handler (background task)
            // See: https://ratatui.rs/faq/#when-should-i-use-tokio-and-async--await-
            let _signal_handler = tokio::spawn(async move {
                loop {
                    if tokio::signal::ctrl_c().await.is_err() {
                        break;
                    }

                    let signal = state.register_signal();
                    notify.notify_waiters();

                    // Send shutdown command to session to ensure terminal cleanup
                    handle_for_signal.shutdown();

                    // Shutdown MCP client on interrupt using async manager
                    if let Some(mcp_manager) = &async_mcp_manager_for_signal
                        && let Err(e) = mcp_manager.shutdown().await
                    {
                        let error_msg = e.to_string();
                        if error_msg.contains("EPIPE")
                            || error_msg.contains("Broken pipe")
                            || error_msg.contains("write EPIPE")
                        {
                            eprintln!(
                                "Info: MCP client shutdown encountered pipe errors during interrupt (normal): {}",
                                e
                            );
                        } else {
                            eprintln!("Warning: Failed to shutdown MCP client on interrupt: {}", e);
                        }
                    }

                    if matches!(signal, CtrlCSignal::Exit) {
                        break;
                    }
                }
            });
        }

        let mut session_stats = SessionStats::default();
        let cache_dir = std::env::var("HOME")
            .ok()
            .map(|home| PathBuf::from(home).join(".vtcode").join("cache"))
            .unwrap_or_else(|| PathBuf::from(".vtcode/cache"));
        let approval_recorder = Arc::new(ApprovalRecorder::new(cache_dir));
        let mut linked_directories: Vec<LinkedDirectory> = Vec::new();
        let mut model_picker_state: Option<ModelPickerState> = None;
        let mut palette_state: Option<ActivePalette> = None;
        let mut last_forced_redraw = Instant::now();
        let mut input_status_state = InputStatusState::default();
        let mut queued_inputs: VecDeque<String> = VecDeque::new();
        let mut ctrl_c_notice_displayed = false;
        let mut mcp_catalog_initialized = tool_registry.mcp_client().is_some();
        let mut last_known_mcp_tools: Vec<String> = Vec::new();
        let mut last_mcp_refresh = std::time::Instant::now();
        const MCP_REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

        // Report MCP initialization status if available and there's an error
        if let Some(mcp_manager) = &async_mcp_manager {
            let mcp_status = mcp_manager.get_status().await;
            if mcp_status.is_error() {
                if let Some(error_msg) = mcp_status.get_error_message() {
                    renderer.line(MessageStyle::Error, &format!("MCP Error: {}", error_msg))?;
                    renderer.line(
                        MessageStyle::Info,
                        "Use /mcp to check status or update your vtcode.toml configuration.",
                    )?;
                }
            } else if mcp_status.is_initializing() {
                renderer.line(
                    MessageStyle::Info,
                    "MCP is still initializing in the background...",
                )?;
            }
        }

        loop {
            if let Err(error) = update_input_status_if_changed(
                &handle,
                &config.workspace,
                &config.model,
                config.reasoning_effort.as_str(),
                vt_cfg.as_ref().map(|cfg| &cfg.ui.status_line),
                &mut input_status_state,
            )
            .await
            {
                warn!(
                    workspace = %config.workspace.display(),
                    error = ?error,
                    "Failed to refresh status line"
                );
            }

            // Update context efficiency metrics in status line
            if let Some(efficiency) = context_manager.last_efficiency() {
                update_context_efficiency(
                    &mut input_status_state,
                    efficiency.context_utilization_percent,
                    efficiency.total_tokens,
                    efficiency.semantic_value_per_token,
                );
            }

            if ctrl_c_state.is_exit_requested() {
                session_end_reason = SessionEndReason::Exit;
                break;
            }

            let interrupts = InlineInterruptCoordinator::new(ctrl_c_state.as_ref());

            if let Some(mcp_manager) = &async_mcp_manager {
                // Handle initial MCP client setup
                if !mcp_catalog_initialized {
                    match mcp_manager.get_status().await {
                        McpInitStatus::Ready { client } => {
                            tool_registry.set_mcp_client(Arc::clone(&client));
                            match tool_registry.refresh_mcp_tools().await {
                                Ok(()) => {
                                    let mut registered_tools = 0usize;
                                    match tool_registry.list_mcp_tools().await {
                                        Ok(mcp_tools) => {
                                            let new_definitions =
                                                build_mcp_tool_definitions(&mcp_tools);
                                            registered_tools = new_definitions.len();
                                            let _updated_snapshot = {
                                                let mut guard = tools.write().await;
                                                guard.retain(|tool| {
                                                    !tool
                                                        .function
                                                        .as_ref()
                                                        .unwrap()
                                                        .name
                                                        .starts_with("mcp_")
                                                });
                                                guard.extend(new_definitions);
                                                guard.clone()
                                            };

                                            // Enumerate MCP tools after initial setup (silently)
                                            McpToolManager::enumerate_mcp_tools_after_initial_setup(
                                                &mut tool_registry,
                                                &tools,
                                                mcp_tools,
                                                &mut last_known_mcp_tools,
                                            ).await?;
                                        }
                                        Err(err) => {
                                            warn!(
                                                "Failed to enumerate MCP tools after refresh: {err}"
                                            );
                                        }
                                    }

                                    renderer.line(
                                            MessageStyle::Info,
                                            &format!(
                                                "MCP tools ready ({} registered). Use /mcp tools to inspect the catalog.",
                                                registered_tools
                                            ),
                                        )?;
                                    renderer.line_if_not_empty(MessageStyle::Output)?;
                                }
                                Err(err) => {
                                    warn!(
                                        "Failed to refresh MCP tools after initialization: {err}"
                                    );
                                    renderer.line(
                                        MessageStyle::Error,
                                        &format!("Failed to index MCP tools: {}", err),
                                    )?;
                                    renderer.line_if_not_empty(MessageStyle::Output)?;
                                }
                            }
                            mcp_catalog_initialized = true;
                        }
                        McpInitStatus::Error { message } => {
                            renderer
                                .line(MessageStyle::Error, &format!("MCP Error: {}", message))?;
                            renderer.line_if_not_empty(MessageStyle::Output)?;
                            mcp_catalog_initialized = true;
                        }
                        McpInitStatus::Initializing { .. } | McpInitStatus::Disabled => {}
                    }
                }

                // Dynamic MCP tool refresh - check for new/updated tools after initialization
                if mcp_catalog_initialized && last_mcp_refresh.elapsed() >= MCP_REFRESH_INTERVAL {
                    last_mcp_refresh = std::time::Instant::now();

                    if let Ok(known_tools) = tool_registry.list_mcp_tools().await {
                        let current_tool_keys: Vec<String> = known_tools
                            .iter()
                            .map(|t| format!("{}-{}", t.provider, t.name))
                            .collect();

                        // Check if there are new or changed tools
                        if current_tool_keys != last_known_mcp_tools {
                            match tool_registry.refresh_mcp_tools().await {
                                Ok(()) => {
                                    match tool_registry.list_mcp_tools().await {
                                        Ok(new_mcp_tools) => {
                                            let new_definitions =
                                                build_mcp_tool_definitions(&new_mcp_tools);
                                            let _updated_snapshot = {
                                                let mut guard = tools.write().await;
                                                guard.retain(|tool| {
                                                    !tool
                                                        .function
                                                        .as_ref()
                                                        .unwrap()
                                                        .name
                                                        .starts_with("mcp_")
                                                });
                                                guard.extend(new_definitions);
                                                guard.clone()
                                            };

                                            // Enumerate MCP tools after refresh (silently)
                                            McpToolManager::enumerate_mcp_tools_after_refresh(
                                                &mut tool_registry,
                                                &tools,
                                                &mut last_known_mcp_tools,
                                            )
                                            .await?;
                                        }
                                        Err(err) => {
                                            warn!(
                                                "Failed to enumerate MCP tools after refresh: {err}"
                                            );
                                        }
                                    }
                                }
                                Err(err) => {
                                    warn!(
                                        "Failed to refresh MCP tools during dynamic update: {err}"
                                    );
                                }
                            }
                        }
                    }
                }
            }

            let resources = InlineEventLoopResources {
                renderer: &mut renderer,
                handle: &handle,
                interrupts,
                ctrl_c_notice_displayed: &mut ctrl_c_notice_displayed,
                default_placeholder: &default_placeholder,
                queued_inputs: &mut queued_inputs,
                model_picker_state: &mut model_picker_state,
                palette_state: &mut palette_state,
                config: &mut config,
                vt_cfg: &mut vt_cfg,
                provider_client: &mut provider_client,
                session_bootstrap: &session_bootstrap,
                full_auto,
            };

            let mut input_owned =
                match poll_inline_loop_action(&mut session, &ctrl_c_notify, resources).await? {
                    InlineLoopAction::Continue => continue,
                    InlineLoopAction::Submit(text) => text,
                    InlineLoopAction::Exit(reason) => {
                        session_end_reason = reason;
                        break;
                    }
                };

            if input_owned.is_empty() {
                continue;
            }

            if let Err(err) = refresh_vt_config(&config.workspace, &config, &mut vt_cfg).await {
                warn!(
                    workspace = %config.workspace.display(),
                    error = ?err,
                    "Failed to refresh workspace configuration"
                );
                renderer.line(
                    MessageStyle::Error,
                    &format!("Failed to reload configuration: {err}"),
                )?;
            }

            // Check for MCP status changes and report errors
            if let Some(mcp_manager) = &async_mcp_manager {
                let mcp_status = mcp_manager.get_status().await;
                if mcp_status.is_error()
                    && let Some(error_msg) = mcp_status.get_error_message()
                {
                    renderer.line(MessageStyle::Error, &format!("MCP Error: {}", error_msg))?;
                    renderer.line(
                        MessageStyle::Info,
                        "Use /mcp to check status or update your vtcode.toml configuration.",
                    )?;
                }
            }

            if let Some(next_placeholder) = follow_up_placeholder.take() {
                handle.set_placeholder(Some(next_placeholder.clone()));
                default_placeholder = Some(next_placeholder);
            }

            match input_owned.as_str() {
                "" => continue,
                "exit" | "quit" => {
                    renderer.line(MessageStyle::Info, "Goodbye!")?;
                    session_end_reason = SessionEndReason::Exit;
                    break;
                }
                "help" => {
                    renderer.line(MessageStyle::Info, "Commands: exit, help")?;
                    continue;
                }
                input if input.starts_with('/') => {
                    // Handle slash commands
                    if let Some(command_input) = input.strip_prefix('/') {
                        let outcome =
                            handle_slash_command(command_input, &mut renderer, &custom_prompts)
                                .await?;
                        let command_result = slash_commands::handle_outcome(
                            outcome,
                            SlashCommandContext {
                                renderer: &mut renderer,
                                handle: &handle,
                                session: &mut session,
                                config: &mut config,
                                vt_cfg: &mut vt_cfg,
                                provider_client: &mut provider_client,
                                session_bootstrap: &session_bootstrap,
                                model_picker_state: &mut model_picker_state,
                                palette_state: &mut palette_state,
                                tool_registry: &mut tool_registry,
                                conversation_history: &mut conversation_history,
                                decision_ledger: &decision_ledger,
                                pruning_ledger: &pruning_ledger,
                                context_manager: &mut context_manager,
                                session_stats: &mut session_stats,
                                tools: &tools,
                                token_budget_enabled,
                                trim_config: &trim_config,
                                async_mcp_manager: async_mcp_manager.as_ref(),
                                mcp_panel_state: &mut mcp_panel_state,
                                linked_directories: &mut linked_directories,
                                ctrl_c_state: &ctrl_c_state,
                                ctrl_c_notify: &ctrl_c_notify,
                                default_placeholder: &default_placeholder,
                                lifecycle_hooks: lifecycle_hooks.as_ref(),
                                full_auto,
                                approval_recorder: Some(&approval_recorder),
                                tool_permission_cache: &tool_permission_cache,
                                loaded_skills: &loaded_skills,
                            },
                        )
                        .await?;
                        match command_result {
                            SlashCommandControl::SubmitPrompt(prompt) => {
                                input_owned = prompt;
                            }
                            SlashCommandControl::Continue => continue,
                            SlashCommandControl::BreakWithReason(reason) => {
                                session_end_reason = reason;
                                break;
                            }
                            SlashCommandControl::BreakWithoutReason => break,
                        }
                    }
                }
                _ => {}
            }

            if let Some(hooks) = &lifecycle_hooks {
                match hooks.run_user_prompt_submit(input_owned.as_str()).await {
                    Ok(outcome) => {
                        render_hook_messages(&mut renderer, &outcome.messages)?;
                        if !outcome.allow_prompt {
                            handle.clear_input();
                            continue;
                        }
                        for context in outcome.additional_context {
                            if !context.trim().is_empty() {
                                conversation_history.push(uni::Message::system(context));
                            }
                        }
                    }
                    Err(err) => {
                        renderer.line(
                            MessageStyle::Error,
                            &format!("Failed to run prompt hooks: {}", err),
                        )?;
                    }
                }
            }

            if let Some(picker) = model_picker_state.as_mut() {
                let progress = picker.handle_input(&mut renderer, input_owned.as_str())?;
                match progress {
                    ModelPickerProgress::InProgress => continue,
                    ModelPickerProgress::NeedsRefresh => {
                        picker.refresh_dynamic_models(&mut renderer).await?;
                        continue;
                    }
                    ModelPickerProgress::Cancelled => {
                        model_picker_state = None;
                        continue;
                    }
                    ModelPickerProgress::Completed(selection) => {
                        let picker_state = model_picker_state.take().unwrap();
                        if let Err(err) = finalize_model_selection(
                            &mut renderer,
                            &picker_state,
                            selection,
                            &mut config,
                            &mut vt_cfg,
                            &mut provider_client,
                            &session_bootstrap,
                            &handle,
                            full_auto,
                        )
                        .await
                        {
                            renderer.line(
                                MessageStyle::Error,
                                &format!("Failed to apply model selection: {}", err),
                            )?;
                        }
                        continue;
                    }
                }
            }

            let input = input_owned.as_str();

            // Check for explicit "run <command>" pattern BEFORE processing
            // This bypasses LLM interpretation and executes the command directly
            if let Some((tool_name, tool_args)) =
                crate::agent::runloop::unified::shell::detect_explicit_run_command(input)
            {
                // Display the user message
                display_user_message(&mut renderer, input)?;

                // Add user message to history
                conversation_history.push(uni::Message::user(input.to_string()));

                // Execute the tool directly via tool registry
                let tool_call_id = format!("explicit_run_{}", conversation_history.len());
                match tool_registry.execute_tool_ref(&tool_name, &tool_args).await {
                    Ok(result) => {
                        // Render the command output using the standard tool output renderer
                        crate::agent::runloop::tool_output::render_tool_output(
                            &mut renderer,
                            Some(&tool_name),
                            &result,
                            vt_cfg.as_ref(),
                            None,
                        )
                        .await?;

                        // Add tool response to history
                        let result_str = serde_json::to_string(&result).unwrap_or_default();
                        conversation_history.push(uni::Message::tool_response(
                            tool_call_id.clone(),
                            result_str,
                        ));
                    }
                    Err(err) => {
                        renderer.line(MessageStyle::Error, &format!("Command failed: {}", err))?;
                        conversation_history.push(uni::Message::tool_response(
                            tool_call_id.clone(),
                            format!("{{\"error\": \"{}\"}}", err),
                        ));
                    }
                }

                // Clear input and continue to next iteration
                handle.clear_input();
                handle.set_placeholder(default_placeholder.clone());
                continue;
            }

            // Process @ patterns to embed images as base64 content
            let processed_content = match parse_at_patterns(input, &config.workspace).await {
                Ok(content) => content,
                Err(e) => {
                    // Log the error but continue with original input as text
                    tracing::warn!("Failed to parse @ patterns: {}", e);
                    uni::MessageContent::text(input.to_string())
                }
            };

            // Apply prompt refinement if enabled
            let refined_content = match &processed_content {
                uni::MessageContent::Text(text) => {
                    let refined_text =
                        refine_user_prompt_if_enabled(text, &config, vt_cfg.as_ref()).await;
                    uni::MessageContent::text(refined_text)
                }
                uni::MessageContent::Parts(parts) => {
                    let mut refined_parts = Vec::new();
                    for part in parts {
                        match part {
                            uni::ContentPart::Text { text } => {
                                let refined_text =
                                    refine_user_prompt_if_enabled(text, &config, vt_cfg.as_ref())
                                        .await;
                                refined_parts.push(uni::ContentPart::text(refined_text));
                            }
                            _ => refined_parts.push(part.clone()),
                        }
                    }
                    uni::MessageContent::parts(refined_parts)
                }
            };

            // Extract text from the message once for display and skill matching
            let input_text = match &refined_content {
                uni::MessageContent::Text(text) => text.clone(),
                uni::MessageContent::Parts(parts) => parts
                    .iter()
                    .filter_map(|p| {
                        if let uni::ContentPart::Text { text } = p {
                            Some(text.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" "),
            };

            // Display the user message with inline border decoration
            display_user_message(&mut renderer, &input_text)?;

            // Spinner is displayed via the input status in the inline handle
            // No need to show a separate message here

            // Create user message with processed content using the appropriate constructor
            let user_message = match refined_content {
                uni::MessageContent::Text(text) => uni::Message::user(text),
                uni::MessageContent::Parts(parts) => uni::Message::user_with_parts(parts),
            };

            conversation_history.push(user_message);
            // Removed: Tool response pruning
            // Removed: Context window enforcement to respect token limits

            // Check if any loaded skill should handle this request
            {
                use vtcode_core::skills::execute_skill_with_sub_llm;
                use vtcode_core::skills::loader::EnhancedSkillLoader;

                let input_lower = input_text.to_lowercase();
                let mut matched_skill: Option<(String, vtcode_core::skills::types::Skill)> = None;

                // First check loaded skills
                {
                    let loaded_skills_lock = loaded_skills.read().await;

                    // Check if request matches any loaded skill's purpose
                    for (skill_name, skill) in loaded_skills_lock.iter() {
                        let description_lower = skill.description().to_lowercase();
                        let skill_name_lower = skill_name.to_lowercase();

                        // Priority 1: Direct skill name match (e.g., "spreadsheet-generator")
                        if input_lower.contains(&skill_name_lower)
                            || input_lower.contains(&skill_name.replace("-", " "))
                        {
                            matched_skill = Some((skill_name.clone(), skill.clone()));
                            break;
                        }

                        // Priority 2: Keyword matching on description
                        // For spreadsheet/excel/xlsx skills
                        if (description_lower.contains("spreadsheet")
                            || description_lower.contains("excel"))
                            && (input_lower.contains("spreadsheet")
                                || input_lower.contains("excel")
                                || input_lower.contains("xlsx")
                                || input_lower.contains("sheet"))
                        {
                            matched_skill = Some((skill_name.clone(), skill.clone()));
                            break;
                        }
                        // For document/word skills
                        if (description_lower.contains("word")
                            || description_lower.contains("document"))
                            && (input_lower.contains("word")
                                || input_lower.contains("document")
                                || input_lower.contains(".docx"))
                        {
                            matched_skill = Some((skill_name.clone(), skill.clone()));
                            break;
                        }
                        // For PDF skills
                        if description_lower.contains("pdf")
                            && (input_lower.contains("pdf") || input_lower.contains("report"))
                        {
                            matched_skill = Some((skill_name.clone(), skill.clone()));
                            break;
                        }
                    }
                }

                // If no loaded skill matched, check if user mentioned a skill by name and try to load it
                if matched_skill.is_none() {
                    let mut skill_loader = EnhancedSkillLoader::new(config.workspace.clone());

                    // Look for skill name in input (e.g., "spreadsheet-generator")
                    match skill_loader.discover_all_skills().await {
                        Ok(discovery_result) => {
                            for skill_ctx in &discovery_result.traditional_skills {
                                let manifest = skill_ctx.manifest();
                                let skill_name_lower = manifest.name.to_lowercase();

                                // Check if user explicitly mentioned this skill
                                if input_lower.contains(&skill_name_lower)
                                    || input_lower.contains(&skill_name_lower.replace("-", " "))
                                {
                                    // Try to load the skill
                                    match skill_loader.get_skill(&manifest.name).await {
                                        Ok(enhanced_skill) => {
                                            // Extract the actual skill from EnhancedSkill
                                            match enhanced_skill {
                                                vtcode_core::skills::loader::EnhancedSkill::Traditional(skill) => {
                                                    // Add to loaded skills
                                                    loaded_skills.write().await.insert(manifest.name.clone(), skill.clone());
                                                    renderer.line(
                                                        MessageStyle::Info,
                                                        &format!("Loaded skill: {} - {}", manifest.name, manifest.description),
                                                    )?;
                                                    matched_skill = Some((manifest.name.clone(), skill));
                                                    break;
                                                }
                                                vtcode_core::skills::loader::EnhancedSkill::CliTool(_) => {
                                                    renderer.line(
                                                        MessageStyle::Info,
                                                        &format!("Skill '{}' is a CLI tool, not a traditional skill", manifest.name),
                                                    )?;
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            renderer.line(
                                                MessageStyle::Error,
                                                &format!(
                                                    "Failed to load skill '{}': {}",
                                                    manifest.name, e
                                                ),
                                            )?;
                                        }
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            // Skill discovery failed, but don't block normal processing
                            #[cfg(debug_assertions)]
                            eprintln!("Failed to discover skills: {}", e);
                        }
                    }
                }

                if let Some((skill_name, skill)) = matched_skill {
                    // Auto-route to skill execution
                    renderer.line(
                        MessageStyle::Info,
                        &format!("Using {} skill...", skill_name),
                    )?;

                    let available_tools = tools.read().await.clone();
                    let model = config.model.clone();

                    match execute_skill_with_sub_llm(
                        &skill,
                        input_text.clone(),
                        provider_client.as_ref(),
                        &mut tool_registry,
                        available_tools,
                        model,
                    )
                    .await
                    {
                        Ok(result) => {
                            renderer.line(MessageStyle::Output, &result)?;
                            conversation_history.push(uni::Message::assistant(result.clone()));
                            continue;
                        }
                        Err(e) => {
                            renderer.line(
                                MessageStyle::Error,
                                &format!("Skill execution failed: {}", e),
                            )?;
                            // Fall through to normal processing if skill fails
                        }
                    }
                }
            }

            // Use copy-on-write pattern to avoid cloning entire history
            let working_history = &mut conversation_history;
            let max_tool_loops = vt_cfg
                .as_ref()
                .map(|cfg| cfg.tools.max_tool_loops)
                .filter(|&value| value > 0)
                .unwrap_or(defaults::DEFAULT_MAX_TOOL_LOOPS);

            let mut step_count = 0usize;
            let mut allow_follow_up = true;
            let mut any_write_effect = false;
            let mut last_tool_stdout: Option<String> = None;
            let mut bottom_gap_applied = false;
            let mut turn_modified_files: BTreeSet<PathBuf> = BTreeSet::new();
            let mut budget_warned_75 = false;
            let mut budget_warned_85 = false;
            let mut budget_warned_90 = false;
            let mut budget_warned_emergency = false;
            let tool_repeat_limit = vt_cfg
                .as_ref()
                .map(|cfg| cfg.tools.max_repeated_tool_calls)
                .filter(|&value| value > 0)
                .unwrap_or(defaults::DEFAULT_MAX_REPEATED_TOOL_CALLS);
            let mut repeated_tool_attempts: HashMap<String, usize> = HashMap::new();

            // Initialize loop detection
            let loop_detection_enabled = vt_cfg
                .as_ref()
                .map(|cfg| !cfg.model.skip_loop_detection)
                .unwrap_or(true);
            // Use a lower threshold (2) to catch repeated calls faster.
            // count > threshold means: 1st call (1>2=false), 2nd call (2>2=false), 3rd (3>2=true) = triggers at 3rd
            let loop_detection_threshold = vt_cfg
                .as_ref()
                .map(|cfg| cfg.model.loop_detection_threshold)
                .unwrap_or(2);
            let loop_detection_interactive = vt_cfg
                .as_ref()
                .map(|cfg| cfg.model.loop_detection_interactive)
                .unwrap_or(false);
            let mut loop_detector = LoopDetector::new(
                loop_detection_threshold,
                loop_detection_enabled,
                loop_detection_interactive,
            );
            let mut loop_detection_disabled_for_session = false;
            let mut tool_call_safety = ToolCallSafetyValidator::new();
            tool_call_safety.set_limits(max_tool_loops, max_tool_loops.saturating_mul(3));
            // Rate-limit tool calls per second, bounded by the per-turn cap so we do not exceed
            // the configured loop ceiling even under aggressive retries.
            let per_second_cap = vt_cfg
                .as_ref()
                .and_then(|cfg| cfg.tools.max_tool_rate_per_second)
                .filter(|v| *v > 0)
                .unwrap_or_else(|| vt_cfg
                    .as_ref()
                    .map(|cfg| cfg.tools.max_tool_loops.max(1))
                    .unwrap_or(max_tool_loops.max(1)));
            let current_rate_limit = tool_call_safety.rate_limit_per_second();
            tool_call_safety.set_rate_limit_per_second(current_rate_limit.min(per_second_cap));
                    // Coordinate with registry minute-level limit when configured (env-driven)
                    tool_call_safety.set_rate_limit_per_minute(tool_registry.rate_limit_per_minute());
            tool_call_safety.start_turn();
            let mut autonomous_executor = AutonomousExecutor::new();
            autonomous_executor.set_workspace_dir(config.workspace.clone());

            async fn run_turn_preamble(
                ctrl_c_state: &Arc<crate::agent::runloop::unified::state::CtrlCState>,
                renderer: &mut AnsiRenderer,
                step_count: &mut usize,
                allow_follow_up: &mut bool,
                bottom_gap_applied: &mut bool,
                max_tool_loops: usize,
                working_history: &mut Vec<uni::Message>,
            ) -> Result<Option<TurnLoopResult>> {
                if ctrl_c_state.is_cancel_requested() {
                    renderer.line_if_not_empty(MessageStyle::Output)?;
                    renderer.line(MessageStyle::Info, "Cancelling current operation...")?;
                    return Ok(Some(TurnLoopResult::Cancelled));
                }
                if *step_count > 0 && !*allow_follow_up {
                    ensure_turn_bottom_gap(renderer, bottom_gap_applied)?;
                    return Ok(Some(TurnLoopResult::Completed));
                }
                if *step_count == 0 {
                    renderer.line_if_not_empty(MessageStyle::Output)?;
                }
                *step_count += 1;
                *allow_follow_up = false;
                if *step_count > max_tool_loops {
                    if !*bottom_gap_applied {
                        renderer.line(MessageStyle::Output, "")?;
                    }
                    let notice = format!(
                        "I reached the configured tool-call limit of {} for this turn and paused further tool execution. Increase `tools.max_tool_loops` in vtcode.toml if you need more, then ask me to continue.",
                        max_tool_loops
                    );
                    renderer.line(MessageStyle::Error, &notice)?;
                    ensure_turn_bottom_gap(renderer, bottom_gap_applied)?;
                    working_history.push(uni::Message::assistant(notice));
                    return Ok(Some(TurnLoopResult::Completed));
                }
                Ok(None)
            }

            async fn run_turn_decision(
                vt_cfg: Option<&VTCodeConfig>,
                config: &CoreAgentConfig,
                input: &str,
            ) -> vtcode_core::core::router::RouteDecision {
                if let Some(cfg) = vt_cfg.filter(|cfg| cfg.router.enabled) {
                    Router::route_async(cfg, config, &config.api_key, input).await
                } else {
                    Router::route(&VTCodeConfig::default(), config, input)
                }
            }

            let turn_result = 'outer: loop {
                if let Some(res) = run_turn_preamble(
                    &ctrl_c_state,
                    &mut renderer,
                    &mut step_count,
                    &mut allow_follow_up,
                    &mut bottom_gap_applied,
                    max_tool_loops,
                    working_history,
                )
                .await?
                {
                    break res;
                }

                // Adaptive context trim near budget thresholds
                if let Ok(outcome) = context_manager
                    .adaptive_trim(working_history, None, step_count)
                    .await
                {
                    if outcome.is_trimmed() {
                        let note = format!(
                            "Context trimmed ({:?}, removed {}).",
                            outcome.phase, outcome.removed_messages
                        );
                        working_history.push(uni::Message::system(note.clone()));
                        let mut ledger = decision_ledger.write().await;
                        let decision_id = ledger.record_decision(
                            "Context compaction for budget".to_string(),
                            DTAction::Response {
                                content: note,
                                response_type: ResponseType::ContextSummary,
                            },
                            None,
                        );
                        ledger.record_outcome(
                            &decision_id,
                            DecisionOutcome::Success {
                                result: "trimmed".to_string(),
                                metrics: Default::default(),
                            },
                        );
                    }
                }

                // Turn balancer: cap low-signal churn and request compaction if looping
                if should_trigger_turn_balancer(
                    step_count,
                    max_tool_loops,
                    repeated_tool_attempts.values().copied().max().unwrap_or(0),
                    tool_repeat_limit,
                ) {
                    renderer.line(
                        MessageStyle::Status,
                        "Turn balancer: pausing due to repeated low-signal calls; compacting context.",
                    )?;
                    let _ = context_manager
                        .adaptive_trim(working_history, None, step_count)
                        .await;
                    working_history.push(uni::Message::system(
                        "Turn balancer paused turn after repeated low-signal calls.".to_string(),
                    ));
                    let mut ledger = decision_ledger.write().await;
                    let decision_id = ledger.record_decision(
                        "Turn balancer triggered compaction".to_string(),
                        DTAction::Response {
                            content: "Turn balancer paused after repeats".to_string(),
                            response_type: ResponseType::ContextSummary,
                        },
                        None,
                    );
                    ledger.record_outcome(
                        &decision_id,
                        DecisionOutcome::Success {
                            result: "turn_balancer_pause".to_string(),
                            metrics: Default::default(),
                        },
                    );
                    break TurnLoopResult::Completed;
                }

                // Token budget warnings (Requirement 2.1/2.2/5.5)
                // Using unified thresholds from token_constants
                if token_budget_enabled {
                    let usage = token_budget.usage_ratio().await;
                    if usage >= THRESHOLD_EMERGENCY && !budget_warned_emergency {
                        let msg = format!(
                            "Token budget critical: {:.1}% used (emergency threshold {}%). Checkpoint immediately.",
                            usage * 100.0,
                            (THRESHOLD_EMERGENCY * 100.0) as u32
                        );
                        renderer.line(MessageStyle::Error, &msg)?;
                        working_history.push(uni::Message::system(msg.clone()));
                        budget_warned_emergency = true;
                        budget_warned_90 = true;
                        budget_warned_85 = true;
                        budget_warned_75 = true;
                    } else if usage >= THRESHOLD_COMPACT && !budget_warned_90 {
                        let msg = format!(
                            "Token budget compact mode: {:.1}% used (compact threshold {}%). Pruning context and truncating tool outputs.",
                            usage * 100.0,
                            (THRESHOLD_COMPACT * 100.0) as u32
                        );
                        renderer.line(MessageStyle::Status, &msg)?;
                        working_history.push(uni::Message::system(msg.clone()));
                        budget_warned_90 = true;
                        budget_warned_85 = true;
                        budget_warned_75 = true;
                    } else if usage >= THRESHOLD_ALERT && !budget_warned_85 {
                        let msg = format!(
                            "Token budget high: {:.1}% used (alert threshold {}%). Checkpoint or compaction recommended.",
                            usage * 100.0,
                            (THRESHOLD_ALERT * 100.0) as u32
                        );
                        renderer.line(MessageStyle::Error, &msg)?;
                        working_history.push(uni::Message::system(msg.clone()));
                        budget_warned_85 = true;
                        budget_warned_75 = true; // implied
                    } else if usage >= THRESHOLD_WARNING && !budget_warned_75 {
                        let msg = format!(
                            "Token budget warning: {:.1}% used (warning threshold {}%). I will compact outputs.",
                            usage * 100.0,
                            (THRESHOLD_WARNING * 100.0) as u32
                        );
                        renderer.line(MessageStyle::Info, &msg)?;
                        working_history.push(uni::Message::system(msg.clone()));
                        budget_warned_75 = true;
                    }
                }

                let decision = run_turn_decision(vt_cfg.as_ref(), &config, input).await;
                traj.log_route(
                    working_history.len(),
                    &decision.selected_model,
                    match decision.class {
                        TaskClass::Simple => "simple",
                        TaskClass::Standard => "standard",
                        TaskClass::Complex => "complex",
                        TaskClass::CodegenHeavy => "codegen_heavy",
                        TaskClass::RetrievalHeavy => "retrieval_heavy",
                    },
                    &input.chars().take(120).collect::<String>(),
                );

                let active_model = decision.selected_model;
                let (max_tokens_opt, parallel_cfg_opt) = if let Some(vt) = vt_cfg.as_ref() {
                    let key = match decision.class {
                        TaskClass::Simple => "simple",
                        TaskClass::Standard => "standard",
                        TaskClass::Complex => "complex",
                        TaskClass::CodegenHeavy => "codegen_heavy",
                        TaskClass::RetrievalHeavy => "retrieval_heavy",
                    };
                    let budget = vt.router.budgets.get(key);
                    let max_tokens = budget.and_then(|b| b.max_tokens).map(|value| value as u32);
                    let parallel = budget.and_then(|b| b.max_parallel_tools).map(|value| {
                        vtcode_core::llm::provider::ParallelToolConfig {
                            disable_parallel_tool_use: value <= 1,
                            max_parallel_tools: Some(value),
                            encourage_parallel: value > 1,
                        }
                    });
                    (max_tokens, parallel)
                } else {
                    (None, None)
                };

                async fn run_turn_ledger(
                    decision_ledger: &Arc<
                        tokio::sync::RwLock<vtcode_core::core::decision_tracker::DecisionTracker>,
                    >,
                    working_history: &[uni::Message],
                    tools: &Arc<tokio::sync::RwLock<Vec<uni::ToolDefinition>>>,
                ) {
                    let mut ledger = decision_ledger.write().await;
                    ledger.start_turn(
                        working_history.len(),
                        working_history
                            .last()
                            .map(|message| message.content.as_text().into_owned()),
                    );
                    let tool_names: Vec<String> = {
                        let snapshot = tools.read().await;
                        snapshot
                            .iter()
                            .map(|tool| tool.function.as_ref().unwrap().name.clone())
                            .collect()
                    };
                    ledger.update_available_tools(tool_names);
                }

                run_turn_ledger(&decision_ledger, working_history, &tools).await;

                let _conversation_len = working_history.len();

                async fn run_turn_pruning(
                    trim_config_semantic: bool,
                    pruning_ledger: &Arc<
                        tokio::sync::RwLock<
                            vtcode_core::core::pruning_decisions::PruningDecisionLedger,
                        >,
                    >,
                    context_manager: &mut ContextManager,
                    working_history: &mut Vec<uni::Message>,
                    step_count: usize,
                ) {
                    if trim_config_semantic {
                        let mut pruning_ledger_mut = pruning_ledger.write().await;
                        context_manager.prune_with_semantic_priority(
                            working_history,
                            Some(&mut *pruning_ledger_mut),
                            step_count,
                        );
                    }
                }

                run_turn_pruning(
                    trim_config.semantic_compression,
                    &pruning_ledger,
                    &mut context_manager,
                    working_history,
                    step_count,
                )
                .await;

                async fn run_turn_build_system_prompt(
                    context_manager: &mut ContextManager,
                    request_history: &[uni::Message],
                    step_count: usize,
                ) -> Result<String> {
                    context_manager.reset_token_budget().await;
                    let system_prompt = context_manager
                        .build_system_prompt(request_history, step_count)
                        .await?;
                    Ok(system_prompt)
                }

                let system_prompt =
                    run_turn_build_system_prompt(&mut context_manager, working_history, step_count)
                        .await?;

                let use_streaming = provider_client.supports_streaming();
                let reasoning_effort = vt_cfg.as_ref().and_then(|cfg| {
                    if provider_client.supports_reasoning_effort(&active_model) {
                        Some(cfg.agent.reasoning_effort)
                    } else {
                        None
                    }
                });
                let current_tools = tools.read().await.clone();
                let request = uni::LLMRequest {
                    messages: working_history.clone(),
                    system_prompt: Some(system_prompt),
                    tools: Some(current_tools),
                    model: active_model.clone(),
                    max_tokens: max_tokens_opt.or(Some(2000)),
                    temperature: Some(0.7),
                    stream: use_streaming,
                    tool_choice: Some(uni::ToolChoice::auto()),
                    parallel_tool_calls: None,
                    parallel_tool_config: if provider_client
                        .supports_parallel_tool_config(&active_model)
                    {
                        parallel_cfg_opt.clone()
                    } else {
                        None
                    },
                    reasoning_effort,
                    output_format: None,
                    verbosity: None,
                };

                let action_suggestion = extract_action_from_messages(working_history);
                let thinking_spinner = PlaceholderSpinner::new(
                    &handle,
                    input_status_state.left.clone(),
                    input_status_state.right.clone(),
                    action_suggestion,
                );
                task::yield_now().await;
                #[cfg(debug_assertions)]
                let request_timer = Instant::now();
                #[cfg(debug_assertions)]
                {
                    let tool_count = request.tools.as_ref().map_or(0, |tools| tools.len());
                    debug!(
                        target = "vtcode::agent::llm",
                        model = %request.model,
                        streaming = use_streaming,
                        step = step_count,
                        messages = request.messages.len(),
                        tools = tool_count,
                        "Dispatching provider request"
                    );
                }
                let llm_result = if use_streaming {
                    stream_and_render_response(
                        provider_client.as_ref(),
                        request,
                        &thinking_spinner,
                        &mut renderer,
                        &ctrl_c_state,
                        &ctrl_c_notify,
                    )
                    .await
                } else {
                    let provider_name = provider_client.name().to_string();

                    if ctrl_c_state.is_cancel_requested() || ctrl_c_state.is_exit_requested() {
                        thinking_spinner.finish();
                        Err(uni::LLMError::Provider {
                            message: error_display::format_llm_error(
                                &provider_name,
                                "Interrupted by user",
                            ),
                            metadata: None,
                        })
                    } else {
                        // Get LLM request timeout from config (default: 120 seconds)
                        let llm_timeout_secs = vt_cfg
                            .as_ref()
                            .map(|cfg| cfg.timeouts.streaming_ceiling_seconds)
                            .unwrap_or(120);
                        let llm_timeout = tokio::time::Duration::from_secs(llm_timeout_secs);

                        let generate_future = provider_client.generate(request);
                        tokio::pin!(generate_future);
                        let cancel_notifier = ctrl_c_notify.notified();
                        tokio::pin!(cancel_notifier);
                        let timeout_future = tokio::time::sleep(llm_timeout);
                        tokio::pin!(timeout_future);

                        let outcome = tokio::select! {
                            res = &mut generate_future => {
                                thinking_spinner.finish();
                                res.map(|resp| (resp, false))
                            }
                            _ = &mut cancel_notifier => {
                                thinking_spinner.finish();
                                Err(uni::LLMError::Provider {
                                    message: error_display::format_llm_error(
                                        &provider_name,
                                        "Interrupted by user",
                                    ),
                                    metadata: None,
                                })
                            }
                            _ = &mut timeout_future => {
                                thinking_spinner.finish();
                                Err(uni::LLMError::Provider {
                                    message: error_display::format_llm_error(
                                        &provider_name,
                                        &format!("Request timed out after {} seconds. The LLM is taking too long to respond. Try a simpler prompt or check your network connection.", llm_timeout_secs),
                                    ),
                                    metadata: None,
                                })
                            }
                        };
                        outcome
                    }
                };

                #[cfg(debug_assertions)]
                {
                    debug!(
                        target = "vtcode::agent::llm",
                        model = %active_model,
                        streaming = use_streaming,
                        step = step_count,
                        elapsed_ms = request_timer.elapsed().as_millis(),
                        succeeded = llm_result.is_ok(),
                        "Provider request finished"
                    );
                }

                let (response, response_streamed) = match llm_result {
                    Ok(payload) => {
                        if ctrl_c_state.is_cancel_requested() {
                            renderer.line_if_not_empty(MessageStyle::Output)?;
                            renderer.line(MessageStyle::Info, "Operation cancelled by user.")?;
                            break 'outer TurnLoopResult::Cancelled;
                        }
                        payload
                    }
                    Err(error) => {
                        // Finish spinner before rendering error to remove it from transcript
                        thinking_spinner.finish();

                        if ctrl_c_state.is_cancel_requested() {
                            renderer.line_if_not_empty(MessageStyle::Output)?;
                            renderer.line(MessageStyle::Info, "Operation cancelled by user.")?;
                            break 'outer TurnLoopResult::Cancelled;
                        }

                        let error_text = error.to_string();
                        // Removed: Context overflow handling and automatic retry logic

                        let has_recent_tool = working_history
                            .iter()
                            .rev()
                            .take_while(|msg| msg.role != uni::MessageRole::Assistant)
                            .any(|msg| msg.role == uni::MessageRole::Tool);

                        if has_recent_tool {
                            let reply = derive_recent_tool_output(working_history)
                                .unwrap_or_else(|| "Command completed successfully.".to_string());
                            renderer.line(MessageStyle::Response, &reply)?;
                            ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                            working_history.push(uni::Message::assistant(reply));
                            let _ = last_tool_stdout.take();
                            break 'outer TurnLoopResult::Completed;
                        }

                        renderer.line(
                            MessageStyle::Error,
                            &format!("Provider error: {error_text}"),
                        )?;
                        ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                        break 'outer TurnLoopResult::Aborted;
                    }
                };

                let assistant_reasoning = response.reasoning.clone();

                fn parse_response_for_tools(
                    response: &uni::LLMResponse,
                    renderer: &mut AnsiRenderer,
                    conversation_len: usize,
                ) -> Result<(Option<String>, Vec<uni::ToolCall>, bool)> {
                    let mut final_text = response.content.clone();
                    let mut tool_calls = response.tool_calls.clone().unwrap_or_default();
                    let mut interpreted_textual_call = false;

                    // Strip harmony syntax from displayed content if present
                    if let Some(ref text) = final_text
                        && (text.contains("<|start|>")
                            || text.contains("<|channel|>")
                            || text.contains("<|call|>"))
                    {
                        let cleaned = strip_harmony_syntax(text);
                        if !cleaned.trim().is_empty() {
                            final_text = Some(cleaned);
                        } else {
                            final_text = None;
                        }
                    }

                    if tool_calls.is_empty()
                        && let Some(text) = final_text.clone()
                        && !text.trim().is_empty()
                        && let Some((name, args)) = detect_textual_tool_call(&text)
                    {
                        let args_json =
                            serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
                        let code_blocks = extract_code_fence_blocks(&text);
                        if !code_blocks.is_empty() {
                            render_code_fence_blocks(renderer, &code_blocks)?;
                            renderer.line(MessageStyle::Output, "")?;
                        }
                        let (headline, _) = describe_tool_action(&name, &args);
                        let notice = if headline.is_empty() {
                            format!("Detected {} request", humanize_tool_name(&name))
                        } else {
                            format!("Detected {headline}")
                        };
                        renderer.line(MessageStyle::Info, &notice)?;
                        let call_id = format!("call_textual_{}", conversation_len);
                        tool_calls.push(uni::ToolCall::function(
                            call_id.clone(),
                            name.clone(),
                            args_json.clone(),
                        ));
                        interpreted_textual_call = true;
                        final_text = None;
                    }

                    Ok((final_text, tool_calls, interpreted_textual_call))
                }

                let (mut final_text, tool_calls, interpreted_textual_call) =
                    parse_response_for_tools(&response, &mut renderer, working_history.len())?;

                if !tool_calls.is_empty() {
                    let assistant_text = if interpreted_textual_call {
                        String::new()
                    } else {
                        final_text.clone().unwrap_or_default()
                    };
                    let message =
                        uni::Message::assistant_with_tools(assistant_text, tool_calls.clone())
                            .with_reasoning(assistant_reasoning.clone());
                    working_history.push(message);
                    // Clear final_text since it was used for assistant_text
                    // This prevents the loop from breaking after tool execution
                    let _ = final_text.take();
                    for call in &tool_calls {
                        let name = call
                            .function
                            .as_ref()
                            .expect("Tool call must have function")
                            .name
                            .as_str();
                        let args_val = call
                            .parsed_arguments()
                            .unwrap_or_else(|_| serde_json::json!({}));

                        // Normalize args for loop detection: strip pagination params and normalize paths
                        let normalized_args = if let Some(obj) = args_val.as_object() {
                            let mut normalized = obj.clone();
                            normalized.remove("page");
                            normalized.remove("per_page");

                            // For list_files: normalize root path variations to catch loops
                            if name == tools::LIST_FILES {
                                if let Some(path) = normalized.get("path").and_then(|v| v.as_str())
                                {
                                    let path_trimmed =
                                        path.trim_start_matches("./").trim_start_matches('/');
                                    if path_trimmed.is_empty() || path_trimmed == "." {
                                        // Normalize all root variations to the same key
                                        normalized.insert(
                                            "path".to_string(),
                                            serde_json::json!("__ROOT__"),
                                        );
                                    }
                                } else {
                                    // No path = root
                                    normalized
                                        .insert("path".to_string(), serde_json::json!("__ROOT__"));
                                }
                            }

                            serde_json::Value::Object(normalized)
                        } else {
                            args_val.clone()
                        };

                        let signature_key = format!(
                            "{}::{}",
                            name,
                            serde_json::to_string(&normalized_args)
                                .unwrap_or_else(|_| "{}".to_string())
                        );

                        // Autonomous executor safety: block early if loop or destructive
                        if let Some(block_reason) =
                            autonomous_executor.should_block(name, &args_val)
                        {
                            renderer.line(MessageStyle::Error, &block_reason)?;
                            working_history.push(uni::Message::system(block_reason));
                            continue;
                        }

                        if let Err(err) = autonomous_executor.validate_args(name, &args_val) {
                            let msg = format!("Blocked '{}' due to invalid args: {}", name, err);
                            renderer.line(MessageStyle::Error, &msg)?;
                            working_history.push(uni::Message::system(msg));
                            continue;
                        }

                        if let Some(loop_notice) =
                            autonomous_executor.record_tool_call(name, &args_val)
                        {
                            renderer.line(MessageStyle::Error, &loop_notice)?;
                            working_history.push(uni::Message::system(loop_notice));
                            continue;
                        }

                        // Autonomous policy feedback (Requirement 3.x/5.x)
                        let policy = autonomous_executor.get_policy(name, &args_val);
                        match policy {
                            AutonomousPolicy::AutoExecute => {
                                // no-op for verbosity
                            }
                            AutonomousPolicy::VerifyThenExecute => {
                                let msg = format!(
                                    "Policy: '{}' requires verify-then-execute. Proceeding with caution.",
                                    name
                                );
                                renderer.line(MessageStyle::Info, &msg)?;
                                working_history.push(uni::Message::system(msg));
                            }
                            AutonomousPolicy::RequireConfirmation => {
                                let msg = format!(
                                    "Policy: '{}' requires confirmation. Skipping this call to avoid destructive action.",
                                    name
                                );
                                renderer.line(MessageStyle::Error, &msg)?;
                                working_history.push(uni::Message::system(msg));
                                // Skip this tool call entirely
                                continue;
                            }
                        }

                        // Safety validation: rate limits, per-turn/session limits, destructive checks
                        let validation = match tool_call_safety.validate_call(name) {
                            Ok(v) => v,
                            Err(err) => {
                                let msg = format!("Tool safety blocked '{}': {}", name, err);
                                renderer.line(MessageStyle::Error, &msg)?;
                                working_history.push(uni::Message::system(msg));
                                ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                                break 'outer TurnLoopResult::Completed;
                            }
                        };
                        if validation.requires_confirmation && skip_confirmations {
                            let msg = format!(
                                "Skipping '{}' because confirmations are disabled and it is marked destructive.",
                                name
                            );
                            renderer.line(MessageStyle::Error, &msg)?;
                            working_history.push(uni::Message::system(msg));
                            continue;
                        }

                        // Check for loop hang detection
                        let (is_loop_detected, repeat_count) =
                            if !loop_detection_disabled_for_session {
                                loop_detector.record_tool_call(&signature_key)
                            } else {
                                (false, 0)
                            };

                        if is_loop_detected {
                            if loop_detection_interactive {
                                // Get user's choice with context information (interactive mode)
                                match prompt_for_loop_detection(
                                    loop_detection_interactive,
                                    &signature_key,
                                    repeat_count,
                                ) {
                                    Ok(LoopDetectionResponse::KeepEnabled) => {
                                        renderer.line(
                                            MessageStyle::Info,
                                            "Loop detection remains enabled. Skipping this tool call.",
                                        )?;
                                        // Add feedback to conversation history so LLM doesn't repeat the same tool
                                        let loop_feedback = format!(
                                            "LOOP DETECTED: Tool '{}' called {} times with same arguments. STOP calling this tool. Change your approach: use a different tool, different arguments, or ask user for clarification.",
                                            name, repeat_count
                                        );
                                        working_history
                                            .push(uni::Message::system(loop_feedback.clone()));
                                        renderer.line(MessageStyle::Error, &loop_feedback)?;
                                        // Reset only this signature for fresh monitoring
                                        loop_detector.reset_signature(&signature_key);
                                        continue; // Skip processing this tool call
                                    }
                                    Ok(LoopDetectionResponse::DisableForSession) => {
                                        renderer.line(MessageStyle::Info, "Loop detection disabled for this session. Proceeding with tool call.")?;
                                        loop_detection_disabled_for_session = true;
                                        // Clear all tracking for fresh start after user override
                                        loop_detector.reset();
                                        // Continue processing the tool call below
                                    }
                                    Err(e) => {
                                        warn!("Loop detection prompt failed: {}", e);
                                        // Graceful degradation: disable detection and continue
                                        loop_detection_disabled_for_session = true;
                                        loop_detector.reset();
                                    }
                                }
                            } else {
                                // Non-interactive mode: add system message and break
                                let alternative = match name {
                                    tools::LIST_FILES => {
                                        "Instead of listing root repeatedly, target subdirectories or use grep_file for patterns."
                                    }
                                    tools::GREP_FILE => {
                                        "Refine the search pattern or open specific files with read_file."
                                    }
                                    tools::READ_FILE => {
                                        "Narrow to specific ranges or search first with grep_file."
                                    }
                                    _ => {
                                        "Try a different tool, change arguments, or ask for clarification."
                                    }
                                };
                                let loop_msg = format!(
                                    "LOOP DETECTED: Tool '{}' called {} times with identical arguments. Session stopped to prevent infinite loop. {}",
                                    name, repeat_count, alternative
                                );
                                working_history.push(uni::Message::system(loop_msg.clone()));
                                renderer.line(MessageStyle::Error, &loop_msg)?;
                                ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                                break 'outer TurnLoopResult::Completed;
                            }
                        }

                        let failed_attempts = repeated_tool_attempts
                            .entry(signature_key.clone())
                            .or_insert(0);
                        if *failed_attempts >= tool_repeat_limit {
                            let abort_msg = format!(
                                "REPEATED FAILURE: Tool '{}' failed {} times with same arguments. Stopping to prevent loop. Read the error messages above and try a different approach.",
                                name, *failed_attempts
                            );
                            renderer.line(MessageStyle::Error, &abort_msg)?;
                            ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                            working_history.push(uni::Message::system(abort_msg));
                            break 'outer TurnLoopResult::Completed;
                        }

                        // Render MCP tool calls as assistant messages instead of user input
                        if let Some(tool_name) = name.strip_prefix("mcp_") {
                            // Remove "mcp_" prefix
                            let (headline, _) = describe_tool_action(tool_name, &args_val);

                            // Render MCP tool call as a single message block
                            renderer.line(MessageStyle::Info, &headline)?;
                            renderer.line(MessageStyle::Info, &format!("MCP: {}", tool_name))?;

                            // Force immediate TUI refresh to ensure proper layout
                            handle.force_redraw();
                            wait_for_redraw_complete().await?;

                            // Also capture for logging
                            {
                                let mut mcp_event = mcp_events::McpEvent::new(
                                    "mcp".to_string(),
                                    tool_name.to_string(),
                                    Some(args_val.to_string()),
                                );
                                mcp_event.success(None);
                                mcp_panel_state.add_event(mcp_event);
                            }
                        }
                        // Note: tool summary will be rendered after execution with status
                        let dec_id = {
                            let mut ledger = decision_ledger.write().await;
                            ledger.record_decision(
                                format!("Execute tool '{}' to progress task", name),
                                DTAction::ToolCall {
                                    name: name.to_string(),
                                    args: args_val.clone(),
                                    expected_outcome: "Use tool output to decide next step"
                                        .to_string(),
                                },
                                None,
                            )
                        };

                        match run_turn_prepare_tool_call(
                            &mut tool_registry,
                            name,
                            Some(&args_val),
                            &mut renderer,
                            &handle,
                            &mut session,
                            default_placeholder.clone(),
                            &ctrl_c_state,
                            &ctrl_c_notify,
                            lifecycle_hooks.as_ref(),
                            None, // justification from agent - TODO: extract from context
                            Some(&approval_recorder),
                            Some(&decision_ledger),
                            Some(&mut mcp_panel_state),
                            Some(&tool_result_cache),
                            Some(&tool_permission_cache),
                            &mut session_stats,
                            &traj,
                            working_history,
                            &call.id,
                            &dec_id,
                            vt_cfg.as_ref(),
                            &token_budget,
                            &mut last_forced_redraw,
                        )
                        .await
                        {
                            Ok(PrepareToolCallResult::Approved) => {
                                // Force redraw immediately after modal closes to clear artifacts
                                safe_force_redraw(&handle, &mut last_forced_redraw);
                                // Wait for redraw completion to ensure modal is fully cleared before spinner starts
                                redraw_with_sync(&handle).await?;

                                if ctrl_c_state.is_cancel_requested() {
                                    renderer.line_if_not_empty(MessageStyle::Output)?;
                                    renderer.line(
                                        MessageStyle::Info,
                                        "Tool execution cancelled by user.",
                                    )?;
                                    break 'outer TurnLoopResult::Cancelled;
                                }

                                // Check if this is a read-only tool that can be cached
                                let is_read_only_tool = matches!(
                                    name,
                                    tools::READ_FILE
                                        | tools::LIST_FILES
                                        | "grep_search"
                                        | "find_files"
                                        | "tree_sitter_analyze"
                                );

                                // Create a progress reporter for the tool execution
                                let progress_reporter = ProgressReporter::new();

                                // Set initial progress and total
                                progress_reporter.set_total(100).await;
                                progress_reporter.set_progress(0).await;
                                progress_reporter
                                    .set_message(format!("Starting {}...", name))
                                    .await;

                                let tool_spinner = PlaceholderSpinner::with_progress(
                                    &handle,
                                    input_status_state.left.clone(),
                                    input_status_state.right.clone(),
                                    format!("Running tool: {}", name),
                                    Some(&progress_reporter),
                                );

                                // Execute tool (cache-aware for read-only tools)
                                let pipeline_outcome = run_turn_execute_tool(
                                    &mut tool_registry,
                                    name,
                                    &args_val,
                                    is_read_only_tool,
                                    &tool_result_cache,
                                    &ctrl_c_state,
                                    &ctrl_c_notify,
                                    Some(&progress_reporter),
                                    &handle,
                                    &mut last_forced_redraw,
                                )
                                .await;

                                match pipeline_outcome {
                                    ToolExecutionStatus::Progress(progress) => {
                                        // Handle progress updates
                                        progress_reporter.set_message(progress.message).await;
                                        // Progress is already a u8 between 0-100
                                        if progress.progress <= 100 {
                                            progress_reporter
                                                .set_progress(progress.progress as u64)
                                                .await;
                                        }
                                        continue;
                                    }
                                    ToolExecutionStatus::Success {
                                        output,
                                        stdout,
                                        modified_files,
                                        command_success,
                                        has_more,
                                    } => {
                                        tool_spinner.finish();
                                        autonomous_executor.record_execution(name, true);
                                        // Reset the repeat counter on successful execution
                                        repeated_tool_attempts.remove(&signature_key);
                                        if let Some(res) = run_turn_handle_tool_success(
                                            name,
                                            output,
                                            stdout,
                                            modified_files,
                                            command_success,
                                            has_more,
                                            &mut renderer,
                                            &handle,
                                            &mut session_stats,
                                            &traj,
                                            &mut mcp_panel_state,
                                            &tool_result_cache,
                                            vt_cfg.as_ref(),
                                            &token_budget,
                                            &token_counter,
                                            working_history,
                                            &call.id,
                                            &dec_id,
                                            &decision_ledger,
                                            &mut last_tool_stdout,
                                            &mut any_write_effect,
                                            &mut turn_modified_files,
                                            skip_confirmations,
                                            &lifecycle_hooks,
                                            &mut bottom_gap_applied,
                                            &mut last_forced_redraw,
                                            input,
                                        )
                                        .await?
                                        {
                                            break 'outer res;
                                        }
                                    }
                                    ToolExecutionStatus::Failure { error } => {
                                        tool_spinner.finish();
                                        autonomous_executor.record_execution(name, false);

                                        // Increment failure counter for this tool signature
                                        let failed_attempts = repeated_tool_attempts
                                            .entry(signature_key.clone())
                                            .or_insert(0);
                                        *failed_attempts += 1;

                                        // Convert the tool error into anyhow for the helper
                                        let any_err = anyhow::anyhow!(format!("{:?}", error));
                                        // Call the centralized failure handler
                                        run_turn_handle_tool_failure(
                                            name,
                                            any_err,
                                            &mut renderer,
                                            &handle,
                                            &mut session_stats,
                                            &traj,
                                            working_history,
                                            &call.id,
                                            &dec_id,
                                            &mut mcp_panel_state,
                                            &token_counter,
                                            &decision_ledger,
                                            Some(&tool_result_cache),
                                            vt_cfg.as_ref(),
                                            &token_budget,
                                        )
                                        .await?;
                                        // continue the outer loop
                                        continue;
                                    }
                                    ToolExecutionStatus::Timeout { error } => {
                                        tool_spinner.finish();
                                        autonomous_executor.record_execution(name, false);

                                        // Increment failure counter for timeout as well
                                        let failed_attempts = repeated_tool_attempts
                                            .entry(signature_key.clone())
                                            .or_insert(0);
                                        *failed_attempts += 1;

                                        // Convert and delegate to timeout handler
                                        let any_err = anyhow::anyhow!(format!("{:?}", error));
                                        run_turn_handle_tool_timeout(
                                            name,
                                            any_err,
                                            &mut renderer,
                                            &handle,
                                            &mut session_stats,
                                            &traj,
                                            working_history,
                                            &call.id,
                                            &dec_id,
                                            &token_counter,
                                            &decision_ledger,
                                        )
                                        .await?;
                                        continue;
                                    }
                                    ToolExecutionStatus::Cancelled => {
                                        tool_spinner.finish();
                                        autonomous_executor.record_execution(name, false);

                                        let res = run_turn_handle_tool_cancelled(
                                            name,
                                            &mut renderer,
                                            &handle,
                                            &mut session_stats,
                                            working_history,
                                            &call.id,
                                            &dec_id,
                                            &decision_ledger,
                                        )
                                        .await?;
                                        break 'outer res;
                                    }
                                }
                            }
                            Ok(PrepareToolCallResult::Denied) => {
                                // Helper already rendered denial and recorded ledger outcome.
                                continue;
                            }
                            Ok(PrepareToolCallResult::Exit) => {
                                // Force redraw after modal closes
                                safe_force_redraw(&handle, &mut last_forced_redraw);
                                redraw_with_sync(&handle).await?;

                                renderer.line(MessageStyle::Info, "Goodbye!")?;
                                session_end_reason = SessionEndReason::Exit;
                                break 'outer TurnLoopResult::Cancelled;
                            }
                            Ok(PrepareToolCallResult::Interrupted) => {
                                // Force redraw after modal closes
                                safe_force_redraw(&handle, &mut last_forced_redraw);
                                redraw_with_sync(&handle).await?;

                                break 'outer TurnLoopResult::Cancelled;
                            }
                            Err(err) => {
                                // Force redraw after modal closes
                                safe_force_redraw(&handle, &mut last_forced_redraw);
                                redraw_with_sync(&handle).await?;

                                traj.log_tool_call(working_history.len(), name, &args_val, false);
                                renderer.line(
                                    MessageStyle::Error,
                                    &format!(
                                        "Failed to evaluate policy for tool '{}': {}",
                                        name, err
                                    ),
                                )?;
                                let err_json = serde_json::json!({
                                    "error": format!(
                                        "Policy evaluation error for '{}' : {}",
                                        name, err
                                    )
                                });
                                working_history.push(uni::Message::tool_response_with_origin(
                                    call.id.clone(),
                                    err_json.to_string(),
                                    name.to_string(),
                                ));
                                let _ = last_tool_stdout.take();
                                {
                                    let mut ledger = decision_ledger.write().await;
                                    ledger.record_outcome(
                                        &dec_id,
                                        DecisionOutcome::Failure {
                                            error: format!(
                                                "Failed to evaluate policy for tool '{}': {}",
                                                name, err
                                            ),
                                            recovery_attempts: 0,
                                            context_preserved: true,
                                        },
                                    );
                                }
                                continue;
                            }
                        }
                    }
                    allow_follow_up = true;
                    continue 'outer;
                }

                if let Some(mut text) = final_text {
                    // Store the original response content before self-review modifies it.
                    // This is needed to correctly detect if content was already streamed.
                    let original_text = text.clone();

                    let do_review = vt_cfg
                        .as_ref()
                        .map(|cfg| cfg.agent.enable_self_review)
                        .unwrap_or(false);
                    let review_passes = vt_cfg
                        .as_ref()
                        .map(|cfg| cfg.agent.max_review_passes)
                        .unwrap_or(1)
                        .max(1);
                    let should_run_review =
                        do_review && (text.len() >= SELF_REVIEW_MIN_LENGTH || text.contains("```"));
                    if should_run_review {
                        let review_system = "You are the agent's critical code reviewer. Improve clarity, correctness, and add missing test or validation guidance. Return only the improved final answer (no meta commentary).".to_string();
                        for _ in 0..review_passes {
                            let review_req = uni::LLMRequest {
                                messages: vec![uni::Message::user(format!(
                                    "Please review and refine the following response. Return only the improved response.\n\n{}",
                                    text
                                ))],
                                system_prompt: Some(review_system.clone()),
                                tools: None,
                                model: config.model.clone(),
                                max_tokens: Some(2000),
                                temperature: Some(0.5),
                                stream: false,
                                tool_choice: Some(uni::ToolChoice::none()),
                                parallel_tool_calls: None,
                                parallel_tool_config: None,
                                reasoning_effort: vt_cfg.as_ref().and_then(|cfg| {
                                    if provider_client.supports_reasoning_effort(&active_model) {
                                        Some(cfg.agent.reasoning_effort)
                                    } else {
                                        None
                                    }
                                }),
                                output_format: None,
                                verbosity: None,
                            };
                            let rr = provider_client.generate(review_req).await.ok();
                            if let Some(r) = rr.and_then(|result| result.content)
                                && !r.trim().is_empty()
                            {
                                text = r;
                            }
                        }
                    }
                    let trimmed = text.trim();
                    let suppress_response = trimmed.is_empty()
                        || last_tool_stdout
                            .as_ref()
                            .map(|stdout| stdout == trimmed)
                            .unwrap_or(false);

                    // Empty responses mean we're done; avoid spinning another iteration.
                    if trimmed.is_empty() {
                        break TurnLoopResult::Completed;
                    }

                    // Check if the original content (before self-review) was already streamed.
                    // This prevents duplicate rendering when self-review modifies the response.
                    let streamed_matches_output = response_streamed
                        && response
                            .content
                            .as_ref()
                            .map(|original| original == &original_text)
                            .unwrap_or(false);

                    if !suppress_response && !streamed_matches_output {
                        renderer.line(MessageStyle::Response, &text)?;
                    }
                    ensure_turn_bottom_gap(&mut renderer, &mut bottom_gap_applied)?;
                    working_history.push(
                        uni::Message::assistant(text.clone())
                            .with_reasoning(assistant_reasoning.clone()),
                    );
                    let _ = last_tool_stdout.take();
                    break TurnLoopResult::Completed;
                }
                continue;
            };

            match turn_result {
                TurnLoopResult::Cancelled => {
                    if ctrl_c_state.is_exit_requested() {
                        session_end_reason = SessionEndReason::Exit;
                        break;
                    }

                    renderer.line_if_not_empty(MessageStyle::Output)?;
                    renderer.line(
                        MessageStyle::Info,
                        "Interrupted current task. Press Ctrl+C again to exit.",
                    )?;
                    handle.clear_input();
                    handle.set_placeholder(default_placeholder.clone());
                    ctrl_c_state.clear_cancel();
                    session_end_reason = SessionEndReason::Cancelled;
                    continue;
                }
                TurnLoopResult::Aborted => {
                    let _ = conversation_history.pop();
                    continue;
                }
                TurnLoopResult::Blocked { reason: _ } => {
                    conversation_history = working_history.clone();
                    handle.clear_input();
                    handle.set_placeholder(default_placeholder.clone());
                    continue;
                }
                TurnLoopResult::Completed => {
                    conversation_history = working_history.clone();

                    // Removed: Tool response pruning after completion
                    // Removed: Context window enforcement after completion

                    if let Some(last) = conversation_history.last()
                        && last.role == uni::MessageRole::Assistant
                    {
                        let text = last.content.as_text();
                        let claims_write = text.contains("I've updated")
                            || text.contains("I have updated")
                            || text.contains("updated the `");
                        if claims_write && !any_write_effect {
                            renderer.line_if_not_empty(MessageStyle::Output)?;
                            renderer.line(
                                MessageStyle::Info,
                                "Note: The assistant mentioned edits but no write tool ran.",
                            )?;
                        }
                    }

                    if let Some(manager) = checkpoint_manager.as_ref() {
                        let conversation_snapshot: Vec<SessionMessage> = conversation_history
                            .iter()
                            .map(SessionMessage::from)
                            .collect();
                        let turn_number = next_checkpoint_turn;
                        let description = conversation_history
                            .last()
                            .map(|msg| msg.content.as_text())
                            .unwrap_or_default();
                        let description = description.trim().to_string();
                        match manager
                            .create_snapshot(
                                turn_number,
                                description.as_str(),
                                &conversation_snapshot,
                                &turn_modified_files,
                            )
                            .await
                        {
                            Ok(Some(meta)) => {
                                next_checkpoint_turn = meta.turn_number.saturating_add(1);
                            }
                            Ok(None) => {}
                            Err(err) => {
                                warn!(
                                    "Failed to create checkpoint for turn {}: {}",
                                    turn_number, err
                                );
                            }
                        }
                    }
                }
            }
        }

        // Capture loaded skills before finalizing session
        if let Some(archive) = session_archive.as_mut() {
            let skill_names: Vec<String> = loaded_skills.read().await.keys().cloned().collect();
            archive.set_loaded_skills(skill_names);
        }

        finalize_session(
            &mut renderer,
            lifecycle_hooks.as_ref(),
            session_end_reason,
            &mut session_archive,
            &session_stats,
            &conversation_history,
            linked_directories,
            async_mcp_manager.as_deref(),
            &handle,
            Some(&pruning_ledger),
        )
        .await?;

        // If the session ended with NewSession, restart the loop with fresh config
        if matches!(session_end_reason, SessionEndReason::NewSession) {
            // Reload config to pick up any changes
            vt_cfg =
                vtcode_core::config::loader::ConfigManager::load_from_workspace(&config.workspace)
                    .ok()
                    .map(|manager| manager.config().clone());
            resume_state = None;
            continue;
        }

        break;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::should_trigger_turn_balancer;

    #[test]
    fn balancer_triggers_only_after_halfway_and_repeats() {
        assert!(should_trigger_turn_balancer(11, 20, 3, 3));
        assert!(!should_trigger_turn_balancer(9, 20, 3, 3));
        assert!(!should_trigger_turn_balancer(12, 20, 2, 3));
    }
}
