//! Tool outcome handlers for the agent turn loop.
//!
//! This module contains the functions for handling tool execution outcomes:
//! - Permission checking (prepare)
//! - Execution with caching
//! - Success/failure/timeout/cancelled handling

use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnOutcomeContext, TurnProcessingContext,
};
use crate::agent::runloop::unified::turn::guards::handle_turn_balancer;
use crate::agent::runloop::unified::turn::turn_loop::TurnLoopOutcome;
use anyhow::Result;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Notify;
use vtcode_core::utils::session_archive::SessionMessage;

use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::decision_tracker::DecisionOutcome;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use crate::agent::runloop::git::confirm_changes_with_git_diff;
use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::shell::{
    derive_recent_tool_output, should_short_circuit_shell,
};
use crate::agent::runloop::unified::state::{CtrlCState, SessionStats};
use crate::agent::runloop::unified::tool_pipeline::{
    ToolExecutionStatus, ToolPipelineOutcome, execute_tool_with_timeout_ref,
};
use crate::agent::runloop::unified::tool_routing::{ToolPermissionFlow, ensure_tool_permission};
use crate::agent::runloop::unified::tool_summary::render_tool_call_summary_with_status;
use crate::agent::runloop::unified::turn::ui_sync::{redraw_with_sync, wait_for_redraw_complete};
use crate::hooks::lifecycle::LifecycleHookEngine;

use super::utils::{render_hook_messages, safe_force_redraw};
use crate::agent::runloop::unified::display::ensure_turn_bottom_gap;
use crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output_renderer;

pub async fn apply_turn_outcome(
    outcome: &TurnLoopOutcome,
    ctx: TurnOutcomeContext<'_>,
) -> Result<()> {
    match outcome.result {
        TurnLoopResult::Cancelled => {
            if ctx.ctrl_c_state.is_exit_requested() {
                *ctx.session_end_reason = crate::hooks::lifecycle::SessionEndReason::Exit;
                return Ok(());
            }
            ctx.renderer.line_if_not_empty(MessageStyle::Output)?;
            ctx.renderer.line(
                MessageStyle::Info,
                "Interrupted current task. Press Ctrl+C again to exit.",
            )?;
            ctx.handle.clear_input();
            ctx.handle.set_placeholder(ctx.default_placeholder.clone());
            ctx.ctrl_c_state.clear_cancel();
            *ctx.session_end_reason = crate::hooks::lifecycle::SessionEndReason::Cancelled;
            Ok(())
        }
        TurnLoopResult::Aborted => {
            if let Some(last) = ctx.conversation_history.last() {
                match last.role {
                    uni::MessageRole::Assistant | uni::MessageRole::Tool => {
                        let _ = ctx.conversation_history.pop();
                    }
                    _ => {}
                }
            }
            Ok(())
        }
        TurnLoopResult::Blocked { reason: _ } => {
            *ctx.conversation_history = outcome.working_history.clone();
            ctx.handle.clear_input();
            ctx.handle.set_placeholder(ctx.default_placeholder.clone());
            Ok(())
        }
        TurnLoopResult::Completed => {
            *ctx.conversation_history = outcome.working_history.clone();
            if let Some(manager) = ctx.checkpoint_manager {
                let conversation_snapshot: Vec<SessionMessage> = outcome
                    .working_history
                    .iter()
                    .map(SessionMessage::from)
                    .collect();
                let turn_number = *ctx.next_checkpoint_turn;
                let description = outcome
                    .working_history
                    .last()
                    .map(|msg| msg.content.as_text())
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                match manager
                    .create_snapshot(
                        turn_number,
                        description.as_str(),
                        &conversation_snapshot,
                        &outcome.turn_modified_files,
                    )
                    .await
                {
                    Ok(Some(meta)) => {
                        *ctx.next_checkpoint_turn = meta.turn_number.saturating_add(1);
                    }
                    Ok(None) => {}
                    Err(err) => tracing::warn!(
                        "Failed to create checkpoint for turn {}: {}",
                        turn_number,
                        err
                    ),
                }
            }
            Ok(())
        }
    }
}

#[allow(dead_code)]
pub enum PrepareToolCallResult {
    Approved,
    Denied,
    Exit,
    Interrupted,
}

pub(crate) async fn handle_tool_call(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call: &uni::ToolCall,
    repeated_tool_attempts: &mut std::collections::HashMap<String, usize>,
    turn_modified_files: &mut std::collections::BTreeSet<std::path::PathBuf>,
    traj: &vtcode_core::core::trajectory::TrajectoryLogger,
) -> Result<Option<TurnHandlerOutcome>> {
    use vtcode_core::utils::ansi::MessageStyle;

    let function = tool_call
        .function
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Tool call has no function definition"))?;
    let tool_name = &function.name;
    let args_val = tool_call
        .parsed_arguments()
        .unwrap_or_else(|_| serde_json::json!({}));

    // HP-4: Validate tool call safety before execution
    {
        let mut validator = ctx.safety_validator.write().await;
        if let Err(err) = validator.validate_call(tool_name) {
            ctx.renderer.line(
                MessageStyle::Error,
                &format!("Safety validation failed: {}", err),
            )?;
            ctx.working_history
                .push(uni::Message::tool_response_with_origin(
                    tool_call.id.clone(),
                    serde_json::json!({"error": format!("Safety validation failed: {}", err)})
                        .to_string(),
                    tool_name.clone(),
                ));
            return Ok(None); // Continue to next tool or break (managed by caller loop)
        }
    }

    // Ensure tool permission
    match ensure_tool_permission(
        ctx.tool_registry,
        tool_name,
        Some(&args_val),
        ctx.renderer,
        ctx.handle,
        ctx.session,
        ctx.default_placeholder.clone(),
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
        ctx.lifecycle_hooks,
        None, // justification
        Some(ctx.approval_recorder.as_ref()),
        Some(ctx.decision_ledger),
        Some(ctx.tool_permission_cache),
        ctx.vt_cfg
            .map(|cfg| cfg.security.hitl_notification_bell)
            .unwrap_or(true),
    )
    .await
    {
        Ok(ToolPermissionFlow::Approved) => {
            let signature_key = format!("{}:{}", tool_name, args_val);
            let current_count = repeated_tool_attempts.entry(signature_key).or_insert(0);
            *current_count += 1;

            let progress_reporter = ProgressReporter::new();
            let _spinner =
                crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner::with_progress(
                    ctx.handle,
                    ctx.input_status_state.left.clone(),
                    ctx.input_status_state.right.clone(),
                    format!("Executing {}...", tool_name),
                    Some(&progress_reporter),
                );

            let progress_reporter_clone = progress_reporter.clone();
            ctx.tool_registry
                .set_progress_callback(Arc::new(move |_name, output| {
                    let reporter = progress_reporter_clone.clone();
                    let output_owned = output.to_string();
                    tokio::spawn(async move {
                        if let Some(last_line) = output_owned.lines().last() {
                            let clean_line = vtcode_core::utils::ansi_parser::strip_ansi(last_line);
                            let trimmed = clean_line.trim();
                            if !trimmed.is_empty() {
                                reporter.set_message(trimmed.to_string()).await;
                            }
                        }
                    });
                }));

            let tool_result = execute_tool_with_timeout_ref(
                ctx.tool_registry,
                tool_name,
                &args_val,
                ctx.ctrl_c_state,
                ctx.ctrl_c_notify,
                Some(&progress_reporter),
            )
            .await;

            ctx.tool_registry.clear_progress_callback();

            // Handle the result
            handle_tool_execution_result(
                &mut crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext {
                    renderer: ctx.renderer,
                    handle: ctx.handle,
                    session: ctx.session,
                    session_stats: ctx.session_stats,
                    mcp_panel_state: ctx.mcp_panel_state,
                    tool_result_cache: ctx.tool_result_cache,
                    approval_recorder: ctx.approval_recorder,
                    decision_ledger: ctx.decision_ledger,
                    pruning_ledger: ctx.pruning_ledger,
                    token_budget: ctx.token_budget,
                    token_counter: ctx.token_counter,
                    tool_registry: ctx.tool_registry,
                    tools: ctx.tools,
                    cached_tools: ctx.cached_tools,
                    ctrl_c_state: ctx.ctrl_c_state,
                    ctrl_c_notify: ctx.ctrl_c_notify,
                    context_manager: ctx.context_manager,
                    last_forced_redraw: ctx.last_forced_redraw,
                    input_status_state: ctx.input_status_state,
                    lifecycle_hooks: ctx.lifecycle_hooks,
                    default_placeholder: ctx.default_placeholder,
                    tool_permission_cache: ctx.tool_permission_cache,
                    safety_validator: ctx.safety_validator,
                },
                tool_call.id.clone(),
                tool_name,
                &args_val,
                &tool_result,
                ctx.working_history,
                turn_modified_files,
                ctx.vt_cfg,
                ctx.token_budget,
                &traj,
            )
            .await?;
        }
        Ok(ToolPermissionFlow::Denied) => {
            let denial = ToolExecutionError::new(
                tool_name.clone(),
                ToolErrorType::PolicyViolation,
                format!("Tool '{}' execution denied by policy", tool_name),
            )
            .to_json_value();

            ctx.working_history
                .push(uni::Message::tool_response_with_origin(
                    tool_call.id.clone(),
                    serde_json::to_string(&denial).unwrap_or_else(|_| "{}".to_string()),
                    tool_name.clone(),
                ));
        }
        Ok(ToolPermissionFlow::Exit) => {
            return Ok(Some(TurnHandlerOutcome::Break(TurnLoopResult::Cancelled)));
        }
        Ok(ToolPermissionFlow::Interrupted) => {
            return Ok(Some(TurnHandlerOutcome::Break(TurnLoopResult::Cancelled)));
        }
        Err(err) => {
            let err_json = serde_json::json!({
                "error": format!("Failed to evaluate policy for tool '{}': {}", tool_name, err)
            });
            ctx.working_history
                .push(uni::Message::tool_response_with_origin(
                    tool_call.id.clone(),
                    err_json.to_string(),
                    tool_name.clone(),
                ));
        }
    }

    Ok(None)
}

pub(crate) async fn handle_tool_execution_result(
    ctx: &mut crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext<'_>,
    tool_call_id: String,
    tool_name: &str,
    args_val: &serde_json::Value,
    tool_result: &ToolExecutionStatus,
    working_history: &mut Vec<uni::Message>,
    turn_modified_files: &mut std::collections::BTreeSet<std::path::PathBuf>,
    vt_cfg: Option<&VTCodeConfig>,
    local_token_budget: &Arc<vtcode_core::core::token_budget::TokenBudgetManager>,
    traj: &vtcode_core::core::trajectory::TrajectoryLogger,
) -> Result<()> {
    match tool_result {
        ToolExecutionStatus::Success {
            output,
            stdout: _,
            modified_files,
            command_success,
            has_more,
        } => {
            // Add successful tool result to history (token-aware aggregation)
            // Determine per-call token budget (from args) or fall back to config
            let mut call_max_tokens = None;
            if let Some(mt) = args_val.get("max_tokens").and_then(|v| v.as_u64()) {
                call_max_tokens = Some(mt as usize);
            } else if let Some(ml) = args_val.get("max_lines").and_then(|v| v.as_u64()) {
                // Map legacy max_lines to tokens
                let est = (ml as usize)
                    .saturating_mul(vtcode_core::core::token_constants::TOKENS_PER_LINE);
                tracing::warn!(
                    "`max_lines` is deprecated at tool call; mapping {} lines -> ~{} tokens for backward compatibility",
                    ml,
                    est
                );
                call_max_tokens = Some(est);
            }

            let (max_tokens, byte_fuse) = if let Some(cfg) = vt_cfg {
                (
                    cfg.context.model_input_token_budget,
                    cfg.context.model_input_byte_fuse,
                )
            } else {
                (
                    vtcode_core::config::constants::context::DEFAULT_MODEL_INPUT_TOKEN_BUDGET,
                    vtcode_core::config::constants::context::DEFAULT_MODEL_INPUT_BYTE_FUSE,
                )
            };

            // If call supplied a per-call max_tokens, prefer that (but clamp to config max)
            let applied_max_tokens = call_max_tokens.map(|call| std::cmp::min(call, max_tokens));

            let content_for_model =
                crate::agent::runloop::token_trunc::aggregate_tool_output_for_model(
                    tool_name,
                    output,
                    applied_max_tokens.unwrap_or(max_tokens),
                    byte_fuse,
                    local_token_budget,
                )
                .await;

            working_history.push(uni::Message::tool_response_with_origin(
                tool_call_id,
                content_for_model,
                tool_name.to_string(),
            ));

            // Build a ToolPipelineOutcome to leverage centralized handling
            let pipeline_outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
                output: output.clone(),
                stdout: None,
                modified_files: modified_files.clone(),
                command_success: *command_success,
                has_more: *has_more,
            });

            // Build a small RunLoopContext to reuse the generic `handle_pipeline_output`
            let (_any_write, mod_files, last_stdout) = crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output_from_turn_ctx(
                ctx,
                tool_name,
                args_val,
                &pipeline_outcome,
                vt_cfg,
                local_token_budget,
                traj,
            )
            .await?;

            for f in mod_files {
                turn_modified_files.insert(f);
            }
            let _ = last_stdout;

            // Handle lifecycle hooks
            if let Some(hooks) = ctx.lifecycle_hooks {
                match hooks
                    .run_post_tool_use(tool_name, Some(args_val), output)
                    .await
                {
                    Ok(outcome) => {
                        crate::agent::runloop::unified::turn::utils::render_hook_messages(
                            ctx.renderer,
                            &outcome.messages,
                        )?;
                        for context in outcome.additional_context {
                            if !context.trim().is_empty() {
                                working_history.push(uni::Message::system(context));
                            }
                        }
                    }
                    Err(err) => {
                        ctx.renderer.line(
                            MessageStyle::Error,
                            &format!("Failed to run post-tool hooks: {}", err),
                        )?;
                    }
                }
            }
        }
        ToolExecutionStatus::Failure { error } => {
            // Add error result to history
            let error_msg = format!("Tool '{}' execution failed: {}", tool_name, error);
            ctx.renderer.line(MessageStyle::Error, &error_msg)?;

            let error_content = serde_json::json!({"error": error_msg});
            working_history.push(uni::Message::tool_response_with_origin(
                tool_call_id,
                error_content.to_string(),
                tool_name.to_string(),
            ));
        }
        ToolExecutionStatus::Timeout { error } => {
            // Add timeout result to history
            let error_msg = format!("Tool '{}' timed out: {}", tool_name, error.message);
            ctx.renderer.line(MessageStyle::Error, &error_msg)?;

            let error_content = serde_json::json!({"error": error_msg});
            working_history.push(uni::Message::tool_response_with_origin(
                tool_call_id,
                error_content.to_string(),
                tool_name.to_string(),
            ));
        }
        ToolExecutionStatus::Cancelled => {
            // Add cancellation result to history
            let error_msg = format!("Tool '{}' execution cancelled", tool_name);
            ctx.renderer.line(MessageStyle::Info, &error_msg)?;

            let error_content = serde_json::json!({"error": error_msg});
            working_history.push(uni::Message::tool_response_with_origin(
                tool_call_id,
                error_content.to_string(),
                tool_name.to_string(),
            ));
        }
        ToolExecutionStatus::Progress(_) => {
            // Progress events are handled internally by the tool execution system
            // Just continue without adding to the conversation history
        }
    }

    // Handle MCP events
    if tool_name.starts_with("mcp_") {
        match tool_result {
            ToolExecutionStatus::Success { output, .. } => {
                let mut mcp_event = mcp_events::McpEvent::new(
                    "mcp".to_string(),
                    tool_name.to_string(),
                    Some(serde_json::to_string(output).unwrap_or_else(|_| "{}".to_string())),
                );
                mcp_event.success(None);
                ctx.mcp_panel_state.add_event(mcp_event);
            }
            ToolExecutionStatus::Failure { error } => {
                let mut mcp_event = mcp_events::McpEvent::new(
                    "mcp".to_string(),
                    tool_name.to_string(),
                    Some(serde_json::json!({"error": error.to_string()}).to_string()),
                );
                mcp_event.failure(Some(error.to_string()));
                ctx.mcp_panel_state.add_event(mcp_event);
            }
            ToolExecutionStatus::Timeout { error } => {
                let error_str = &error.message;
                let mut mcp_event = mcp_events::McpEvent::new(
                    "mcp".to_string(),
                    tool_name.to_string(),
                    Some(serde_json::json!({"error": error_str}).to_string()),
                );
                mcp_event.failure(Some(error_str.clone()));
                ctx.mcp_panel_state.add_event(mcp_event);
            }
            ToolExecutionStatus::Cancelled => {
                let mut mcp_event = mcp_events::McpEvent::new(
                    "mcp".to_string(),
                    tool_name.to_string(),
                    Some(serde_json::json!({"error": "Cancelled"}).to_string()),
                );
                mcp_event.failure(Some("Cancelled".to_string()));
                ctx.mcp_panel_state.add_event(mcp_event);
            }
            ToolExecutionStatus::Progress(_) => {
                // Progress events are handled internally, no MCP event needed
            }
        }
    }

    Ok(())
}

pub(crate) fn handle_assistant_response(
    ctx: &mut TurnProcessingContext<'_>,
    assistant_text: String,
    reasoning: Option<String>,
    response_streamed: bool,
) -> Result<()> {
    use vtcode_core::utils::ansi::MessageStyle;

    if !response_streamed {
        if !assistant_text.trim().is_empty() {
            ctx.renderer.line(MessageStyle::Response, &assistant_text)?;
        }
        if let Some(reasoning_text) = reasoning.as_ref()
            && !reasoning_text.trim().is_empty()
        {
            ctx.renderer
                .line(MessageStyle::Info, &format!(" {}", reasoning_text))?;
        }
    }

    if !assistant_text.trim().is_empty() {
        let msg = uni::Message::assistant(assistant_text);
        let msg_with_reasoning = if let Some(reasoning_text) = reasoning {
            msg.with_reasoning(Some(reasoning_text))
        } else {
            msg
        };
        ctx.working_history.push(msg_with_reasoning);
    } else if let Some(reasoning_text) = reasoning {
        ctx.working_history
            .push(uni::Message::assistant(String::new()).with_reasoning(Some(reasoning_text)));
    }

    Ok(())
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
    session_stats: &mut SessionStats,
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
    let _ = handle_pipeline_output_renderer(
        renderer,
        session_stats,
        mcp_panel_state,
        Some(tool_result_cache),
        Some(decision_ledger),
        name,
        &serde_json::json!({}),
        &ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
            output: output.clone(),
            stdout: stdout.clone(),
            modified_files: modified_files.clone(),
            command_success,
            has_more,
        }),
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
    session_stats: &mut SessionStats,
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
    session_stats: &mut SessionStats,
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
    session_stats: &mut SessionStats,
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

pub(crate) async fn handle_text_response(
    ctx: &mut TurnProcessingContext<'_>,
    text: String,
    reasoning: Option<String>,
    response_streamed: bool,
    step_count: usize,
    repeated_tool_attempts: &mut std::collections::HashMap<String, usize>,
    turn_modified_files: &mut std::collections::BTreeSet<std::path::PathBuf>,
    traj: &vtcode_core::core::trajectory::TrajectoryLogger,
    session_end_reason: &mut crate::hooks::lifecycle::SessionEndReason,
) -> Result<TurnHandlerOutcome> {
    use vtcode_core::utils::ansi::MessageStyle;

    if !response_streamed {
        if !text.trim().is_empty() {
            ctx.renderer.line(MessageStyle::Response, &text)?;
        }
        if let Some(reasoning_text) = reasoning.as_ref()
            && !reasoning_text.trim().is_empty()
        {
            ctx.renderer
                .line(MessageStyle::Info, &format!(" {}", reasoning_text))?;
        }
    }

    if let Some((tool_name, args)) =
        crate::agent::runloop::text_tools::detect_textual_tool_call(&text)
    {
        let args_json = serde_json::json!(&args);
        let tool_call_str = format!("call_textual_{}", ctx.working_history.len());
        let tool_call = uni::ToolCall::function(
            tool_call_str,
            tool_name.clone(),
            serde_json::to_string(&args_json).unwrap_or_else(|_| "{}".to_string()),
        );

        let function = tool_call
            .function
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Tool call has no function definition"))?;
        let call_tool_name = &function.name;
        let call_args_val = tool_call
            .parsed_arguments()
            .unwrap_or_else(|_| serde_json::json!({}));

        use crate::agent::runloop::unified::tool_summary::{
            describe_tool_action, humanize_tool_name,
        };
        let (headline, _) = describe_tool_action(call_tool_name, &call_args_val);
        let notice = if headline.is_empty() {
            format!("Detected {} request", humanize_tool_name(call_tool_name))
        } else {
            format!("Detected {headline}")
        };
        ctx.renderer.line(MessageStyle::Info, &notice)?;

        // HP-4: Validate tool call safety before execution
        {
            let mut validator = ctx.safety_validator.write().await;
            if let Err(err) = validator.validate_call(call_tool_name) {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Safety validation failed: {}", err),
                )?;
                ctx.working_history
                    .push(uni::Message::tool_response_with_origin(
                    tool_call.id.clone(),
                    serde_json::to_string(
                        &serde_json::json!({"error": format!("Safety validation failed: {}", err)}),
                    )
                    .unwrap(),
                    call_tool_name.clone(),
                ));
                return Ok(handle_turn_balancer(ctx, step_count, repeated_tool_attempts).await);
            }
        }

        match ensure_tool_permission(
            ctx.tool_registry,
            call_tool_name,
            Some(&call_args_val),
            ctx.renderer,
            ctx.handle,
            ctx.session,
            ctx.default_placeholder.clone(),
            ctx.ctrl_c_state,
            ctx.ctrl_c_notify,
            ctx.lifecycle_hooks,
            None, // justification
            Some(ctx.approval_recorder.as_ref()),
            Some(ctx.decision_ledger),
            Some(ctx.tool_permission_cache),
            ctx.vt_cfg
                .map(|cfg| cfg.security.hitl_notification_bell)
                .unwrap_or(true),
        )
        .await
        {
            Ok(ToolPermissionFlow::Approved) => {
                let tool_result = execute_tool_with_timeout_ref(
                    ctx.tool_registry,
                    call_tool_name,
                    &call_args_val,
                    ctx.ctrl_c_state,
                    ctx.ctrl_c_notify,
                    None, // progress_reporter
                )
                .await;

                handle_tool_execution_result(
                    &mut crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext {
                        renderer: ctx.renderer,
                        handle: ctx.handle,
                        session: ctx.session,
                        session_stats: ctx.session_stats,
                        mcp_panel_state: ctx.mcp_panel_state,
                        tool_result_cache: ctx.tool_result_cache,
                        approval_recorder: ctx.approval_recorder,
                        decision_ledger: ctx.decision_ledger,
                        pruning_ledger: ctx.pruning_ledger,
                        token_budget: ctx.token_budget,
                        token_counter: ctx.token_counter,
                        tool_registry: ctx.tool_registry,
                        tools: ctx.tools,
                        cached_tools: ctx.cached_tools,
                        ctrl_c_state: ctx.ctrl_c_state,
                        ctrl_c_notify: ctx.ctrl_c_notify,
                        context_manager: ctx.context_manager,
                        last_forced_redraw: ctx.last_forced_redraw,
                        input_status_state: ctx.input_status_state,
                        lifecycle_hooks: ctx.lifecycle_hooks,
                        default_placeholder: ctx.default_placeholder,
                        tool_permission_cache: ctx.tool_permission_cache,
                        safety_validator: ctx.safety_validator,
                    },
                    tool_call.id.clone(),
                    call_tool_name,
                    &call_args_val,
                    &tool_result,
                    ctx.working_history,
                    turn_modified_files,
                    ctx.vt_cfg,
                    ctx.token_budget,
                    traj,
                )
                .await?;
            }
            Ok(ToolPermissionFlow::Denied) => {
                let denial = ToolExecutionError::new(
                    call_tool_name.clone(),
                    ToolErrorType::PolicyViolation,
                    format!(
                        "Detected tool '{}' execution denied by policy",
                        call_tool_name
                    ),
                )
                .to_json_value();

                ctx.working_history
                    .push(uni::Message::tool_response_with_origin(
                        tool_call.id.clone(),
                        serde_json::to_string(&denial).unwrap_or_else(|_| "{}".to_string()),
                        call_tool_name.clone(),
                    ));
            }
            Ok(ToolPermissionFlow::Exit) => {
                *session_end_reason = crate::hooks::lifecycle::SessionEndReason::Exit;
                return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Cancelled));
            }
            Ok(ToolPermissionFlow::Interrupted) => {
                return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Cancelled));
            }
            Err(err) => {
                let err_json = serde_json::json!({
                    "error": format!("Failed to evaluate policy for detected tool '{}': {}", call_tool_name, err)
                });
                ctx.working_history
                    .push(uni::Message::tool_response_with_origin(
                        tool_call.id.clone(),
                        err_json.to_string(),
                        call_tool_name.clone(),
                    ));
            }
        }
        Ok(handle_turn_balancer(ctx, step_count, repeated_tool_attempts).await)
    } else {
        let msg = uni::Message::assistant(text.clone());
        let msg_with_reasoning = if let Some(reasoning_text) = reasoning {
            msg.with_reasoning(Some(reasoning_text))
        } else {
            msg
        };

        if !text.is_empty() || msg_with_reasoning.reasoning.is_some() {
            ctx.working_history.push(msg_with_reasoning);
        }

        Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed))
    }
}
