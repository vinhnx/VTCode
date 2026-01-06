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
use futures::future::join_all;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Notify;
use vtcode_core::utils::session_archive::SessionMessage;

use vtcode_core::config::constants::tools;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::decision_tracker::DecisionOutcome;
use vtcode_core::llm::provider as uni;
use vtcode_core::tool_policy::ToolPolicy;
use vtcode_core::tools::parallel_executor::ParallelExecutionPlanner;
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
            ctx.ctrl_c_state.clear_cancel();
            Ok(())
        }
        TurnLoopResult::Blocked { reason: _ } => {
            *ctx.conversation_history = outcome.working_history.clone();
            ctx.handle.clear_input();
            ctx.handle.set_placeholder(ctx.default_placeholder.clone());
            ctx.ctrl_c_state.clear_cancel();
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
            ctx.ctrl_c_state.clear_cancel();
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

pub(crate) async fn handle_tool_calls(
    ctx: &mut TurnProcessingContext<'_>,
    tool_calls: &[uni::ToolCall],
    repeated_tool_attempts: &mut std::collections::HashMap<String, usize>,
    turn_modified_files: &mut std::collections::BTreeSet<std::path::PathBuf>,
    traj: &vtcode_core::core::trajectory::TrajectoryLogger,
) -> Result<Option<TurnHandlerOutcome>> {
    if tool_calls.is_empty() {
        return Ok(None);
    }

    // HP-4: Use ParallelExecutionPlanner to group independent tool calls
    let planner = ParallelExecutionPlanner::new();
    let mut planner_calls = Vec::with_capacity(tool_calls.len());
    for tc in tool_calls {
        let name = tc.function.as_ref().map(|f| f.name.as_str()).unwrap_or("");
        let args = Arc::new(tc.parsed_arguments().unwrap_or(serde_json::json!({})));
        planner_calls.push((name.to_string(), args, tc.id.clone()));
    }

    let groups = planner.plan(&planner_calls);

    for group in groups {
        if group.len() > 1 && ctx.full_auto {
            // HP-5: Implement true parallel execution for non-conflicting groups in full-auto mode
            // Optimization: Pre-allocate with group size to avoid reallocations
            let mut group_tool_calls = Vec::with_capacity(group.len());
            for (_, _, call_id) in &group.tool_calls {
                if let Some(tc) = tool_calls.iter().find(|tc| &tc.id == call_id) {
                    group_tool_calls.push(tc);
                }
            }

            // Check if all tools in group are safe and approved
            let mut can_run_parallel = true;
            // Optimization: Pre-allocate execution_items with expected capacity
            let mut execution_items = Vec::with_capacity(group_tool_calls.len());

            for tc in &group_tool_calls {
                let func = match tc.function.as_ref() {
                    Some(f) => f,
                    None => {
                        can_run_parallel = false;
                        break;
                    }
                };
                let tool_name = &func.name;
                let args_val = tc
                    .parsed_arguments()
                    .unwrap_or_else(|_| serde_json::json!({}));

                // Quick safety check
                {
                    let mut validator = ctx.safety_validator.write().await;
                    if validator.validate_call(tool_name).is_err() {
                        can_run_parallel = false;
                        break;
                    }
                }

                // Check policy - only parallelize if allowed
                let is_allowed = matches!(
                    ctx.tool_registry.get_tool_policy(tool_name).await,
                    ToolPolicy::Allow
                );

                if !is_allowed {
                    can_run_parallel = false;
                    break;
                }

                execution_items.push((tc.id.clone(), tool_name.clone(), args_val));
            }

            if can_run_parallel && !execution_items.is_empty() {
                let tool_names: Vec<_> = execution_items
                    .iter()
                    .map(|(_, name, _)| name.as_str())
                    .collect();
                let batch_msg = format!("Executing batch: [{}]", tool_names.join(", "));

                let progress_reporter = ProgressReporter::new();
                let _spinner = crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner::with_progress(
                    ctx.handle,
                    ctx.input_status_state.left.clone(),
                    ctx.input_status_state.right.clone(),
                    batch_msg,
                    Some(&progress_reporter),
                );

                // Execute all in parallel
                let registry = ctx.tool_registry.clone();
                let ctrl_c_state = Arc::clone(ctx.ctrl_c_state);
                let ctrl_c_notify = Arc::clone(ctx.ctrl_c_notify);

                let futures: Vec<_> = execution_items
                    .iter()
                    .map(|(call_id, name, args)| {
                        let registry = registry.clone();
                        let ctrl_c_state = Arc::clone(&ctrl_c_state);
                        let ctrl_c_notify = Arc::clone(&ctrl_c_notify);
                        let name = name.clone();
                        let args = args.clone();
                        let reporter = progress_reporter.clone();

                        async move {
                            let result = execute_tool_with_timeout_ref(
                                &registry,
                                &name,
                                &args,
                                &ctrl_c_state,
                                &ctrl_c_notify,
                                Some(&reporter),
                            )
                            .await;
                            (call_id.clone(), name, args, result)
                        }
                    })
                    .collect();

                let results = join_all(futures).await;

                // Handle results sequentially
                for (call_id, name, args, status) in results {
                    let outcome = ToolPipelineOutcome::from_status(status);

                    // Update attempts
                    let signature_key = format!("{}:{}", name, args);
                    let current_count = repeated_tool_attempts.entry(signature_key).or_insert(0);
                    *current_count += 1;

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
                        call_id,
                        &name,
                        &args,
                        &outcome,
                        ctx.working_history,
                        turn_modified_files,
                        ctx.vt_cfg,
                        traj,
                    )
                    .await?;
                }
                continue; // Move to next group
            }

            // Fallback to sequential for the group if parallel not possible
            for tc in group_tool_calls {
                if let Some(outcome) =
                    handle_tool_call(ctx, tc, repeated_tool_attempts, turn_modified_files, traj)
                        .await?
                {
                    return Ok(Some(outcome));
                }
            }
        } else {
            // Single tool group
            let call_id = &group.tool_calls[0].2;
            if let Some(tc) = tool_calls.iter().find(|tc| &tc.id == call_id)
                && let Some(outcome) =
                    handle_tool_call(ctx, tc, repeated_tool_attempts, turn_modified_files, traj)
                        .await?
            {
                return Ok(Some(outcome));
            }
        }
    }

    Ok(None)
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
        crate::agent::runloop::unified::tool_routing::ToolPermissionsContext {
            tool_registry: ctx.tool_registry,
            renderer: ctx.renderer,
            handle: ctx.handle,
            session: ctx.session,
            default_placeholder: ctx.default_placeholder.clone(),
            ctrl_c_state: ctx.ctrl_c_state,
            ctrl_c_notify: ctx.ctrl_c_notify,
            hooks: ctx.lifecycle_hooks,
            justification: None,
            approval_recorder: Some(ctx.approval_recorder.as_ref()),
            decision_ledger: Some(ctx.decision_ledger),
            tool_permission_cache: Some(ctx.tool_permission_cache),
            hitl_notification_bell: ctx
                .vt_cfg
                .map(|cfg| cfg.security.hitl_notification_bell)
                .unwrap_or(true),
        },
        tool_name,
        Some(&args_val),
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

            let pipeline_outcome = ToolPipelineOutcome::from_status(tool_result);

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
                &pipeline_outcome,
                ctx.working_history,
                turn_modified_files,
                ctx.vt_cfg,
                traj,
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

#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_tool_execution_result(
    ctx: &mut crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext<'_>,
    tool_call_id: String,
    tool_name: &str,
    args_val: &serde_json::Value,
    pipeline_outcome: &ToolPipelineOutcome,
    working_history: &mut Vec<uni::Message>,
    turn_modified_files: &mut std::collections::BTreeSet<std::path::PathBuf>,
    vt_cfg: Option<&VTCodeConfig>,
    traj: &vtcode_core::core::trajectory::TrajectoryLogger,
) -> Result<()> {
    match &pipeline_outcome.status {
        ToolExecutionStatus::Success {
            output,
            stdout: _,
            modified_files: _,
            command_success: _,
            has_more: _,
        } => {
            // Convert output to string for model
            let content_for_model = if let Some(s) = output.as_str() {
                s.to_string()
            } else {
                serde_json::to_string(output).unwrap_or_else(|_| "{}".to_string())
            };

            working_history.push(uni::Message::tool_response_with_origin(
                tool_call_id,
                content_for_model,
                tool_name.to_string(),
            ));

            // Build a small RunLoopContext to reuse the generic `handle_pipeline_output`
            let (_any_write, mod_files, last_stdout) = crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output_from_turn_ctx(
                ctx,
                tool_name,
                args_val,
                pipeline_outcome,
                vt_cfg,
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
        match &pipeline_outcome.status {
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

pub(crate) struct RunTurnExecuteToolParams<'a> {
    pub tool_registry: &'a mut vtcode_core::tools::registry::ToolRegistry,
    pub name: &'a str,
    pub args_val: &'a serde_json::Value,
    pub is_read_only_tool: bool,
    pub tool_result_cache: &'a Arc<tokio::sync::RwLock<vtcode_core::tools::ToolResultCache>>,
    pub ctrl_c_state: &'a Arc<CtrlCState>,
    pub ctrl_c_notify: &'a Arc<Notify>,
    pub progress_reporter: Option<&'a ProgressReporter>,
    pub handle: &'a vtcode_core::ui::tui::InlineHandle,
    pub last_forced_redraw: &'a mut Instant,
}

#[allow(dead_code)]
pub(crate) async fn run_turn_execute_tool(
    params: RunTurnExecuteToolParams<'_>,
) -> ToolExecutionStatus {
    use vtcode_core::tools::result_cache::ToolCacheKey;

    // Try to get from cache first for read-only tools
    if params.is_read_only_tool {
        let _params_str = serde_json::to_string(params.args_val).unwrap_or_default();
        let cache_key = ToolCacheKey::from_json(params.name, params.args_val, "");
        {
            let mut tool_cache = params.tool_result_cache.write().await;
            if let Some(cached_output) = tool_cache.get(&cache_key) {
                #[cfg(debug_assertions)]
                tracing::debug!("Cache hit for tool: {}", params.name);

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
        safe_force_redraw(params.handle, params.last_forced_redraw);

        let result = execute_tool_with_timeout_ref(
            params.tool_registry,
            params.name,
            params.args_val,
            params.ctrl_c_state,
            params.ctrl_c_notify,
            params.progress_reporter,
        )
        .await;

        // Cache successful read-only results
        if let ToolExecutionStatus::Success { ref output, .. } = result {
            let output_json = serde_json::to_string(output).unwrap_or_else(|_| "{}".to_string());
            let mut cache = params.tool_result_cache.write().await;
            cache.insert_arc(cache_key, Arc::new(output_json));
        }

        return result;
    }

    // Non-cached path for write tools
    safe_force_redraw(params.handle, params.last_forced_redraw);

    execute_tool_with_timeout_ref(
        params.tool_registry,
        params.name,
        params.args_val,
        params.ctrl_c_state,
        params.ctrl_c_notify,
        params.progress_reporter,
    )
    .await
}

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
    // repeated_tool_attempts is managed by the caller; not required here
    pub traj: &'a vtcode_core::core::trajectory::TrajectoryLogger,
    pub mcp_panel_state: &'a mut mcp_events::McpPanelState,
    pub tool_result_cache: &'a Arc<tokio::sync::RwLock<vtcode_core::tools::ToolResultCache>>,
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
    // Mirror original success handling but return Some(TurnLoopResult) when we need to break the outer loop.
    safe_force_redraw(params.handle, params.last_forced_redraw);
    redraw_with_sync(params.handle).await?;

    params.session_stats.record_tool(params.name);
    // repeated_tool_attempts is mutated by caller; remove signature elsewhere as original code did before calling helper
    // Note: caller must manage repeated_tool_attempts removal.
    params.traj.log_tool_call(
        params.working_history.len(),
        params.name,
        &serde_json::to_value(&params.output).unwrap_or(serde_json::json!({})),
        true,
    );

    // Handle MCP events
    if let Some(tool_name) = params.name.strip_prefix("mcp_") {
        let mut mcp_event = mcp_events::McpEvent::new(
            "mcp".to_string(),
            tool_name.to_string(),
            Some(params.output.to_string()),
        );
        mcp_event.success(None);
        params.mcp_panel_state.add_event(mcp_event);
    } else {
        // Render tool summary with status
        let exit_code = params.output.get("exit_code").and_then(|v| v.as_i64());
        let status_icon = if params.command_success { "✓" } else { "✗" };
        render_tool_call_summary_with_status(
            params.renderer,
            params.name,
            &serde_json::to_value(&params.output).unwrap_or(serde_json::json!({})),
            status_icon,
            exit_code,
        )?;
    }

    // Render unified tool output via generic minimal adapter, to ensure consistent handling
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
            // Invalidate cache for modified files
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

    // Optimization: Pre-allocate with estimated capacity to avoid reallocation
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

    params
        .working_history
        .push(uni::Message::tool_response_with_origin(
            params.call_id.to_string(),
            content,
            params.name.to_string(),
        ));

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
        // Signal session end and break outer loop
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
        params.working_history.push(uni::Message::assistant(reply));
        let _ = params.last_tool_stdout.take();
        return Ok(Some(TurnLoopResult::Completed));
    }

    Ok(None)
}

#[allow(dead_code)]
pub(crate) async fn run_turn_handle_tool_failure(
    params: RunTurnHandleToolFailureParams<'_>,
) -> Result<()> {
    // Finish spinner / ensure redraw is caller's responsibility
    safe_force_redraw(params.handle, &mut Instant::now());
    redraw_with_sync(params.handle).await?;

    params.session_stats.record_tool(params.name);

    // Display a simple failure message and log
    let failure_msg = format!("Tool '{}' failed: {}", params.name, params.error);
    params.renderer.line(MessageStyle::Error, &failure_msg)?;
    // Provide simple recovery hint to reduce repeated failures
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
    // Render via the renderer adapter so all cache invalidation and MCP events are handled
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

    params
        .working_history
        .push(uni::Message::tool_response_with_origin(
            params.call_id.to_string(),
            serde_json::to_string(&error_json).unwrap_or_default(),
            params.name.to_string(),
        ));

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
    pub tool_result_cache:
        Option<&'a Arc<tokio::sync::RwLock<vtcode_core::tools::ToolResultCache>>>,
    pub vt_cfg: Option<&'a VTCodeConfig>,
}

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
    // Timeout handling mirrors original behavior
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

    params
        .working_history
        .push(uni::Message::tool_response_with_origin(
            params.call_id.to_string(),
            timeout_content,
            params.name.to_string(),
        ));

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

    params
        .working_history
        .push(uni::Message::tool_response_with_origin(
            params.call_id.to_string(),
            serde_json::to_string(&err_json).unwrap_or_else(|_| "{}".to_string()),
            params.name.to_string(),
        ));

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

pub(crate) struct HandleTextResponseParams<'a> {
    pub ctx: &'a mut TurnProcessingContext<'a>,
    pub text: String,
    pub reasoning: Option<String>,
    pub response_streamed: bool,
    pub step_count: usize,
    pub repeated_tool_attempts: &'a mut std::collections::HashMap<String, usize>,
    pub turn_modified_files: &'a mut std::collections::BTreeSet<std::path::PathBuf>,
    pub traj: &'a vtcode_core::core::trajectory::TrajectoryLogger,
    pub session_end_reason: &'a mut crate::hooks::lifecycle::SessionEndReason,
    /// Pre-computed max tool loops limit for efficiency
    pub max_tool_loops: usize,
    /// Pre-computed tool repeat limit for efficiency
    pub tool_repeat_limit: usize,
}

pub(crate) async fn handle_text_response(
    params: HandleTextResponseParams<'_>,
) -> Result<TurnHandlerOutcome> {
    use vtcode_core::utils::ansi::MessageStyle;

    if !params.response_streamed {
        if !params.text.trim().is_empty() {
            params
                .ctx
                .renderer
                .line(MessageStyle::Response, &params.text)?;
        }
        if let Some(reasoning_text) = params.reasoning.as_ref()
            && !reasoning_text.trim().is_empty()
        {
            params
                .ctx
                .renderer
                .line(MessageStyle::Info, &format!(" {}", reasoning_text))?;
        }
    }

    if let Some((tool_name, args)) =
        crate::agent::runloop::text_tools::detect_textual_tool_call(&params.text)
    {
        let args_json = serde_json::json!(&args);
        let tool_call_str = format!("call_textual_{}", params.ctx.working_history.len());
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
        params.ctx.renderer.line(MessageStyle::Info, &notice)?;

        // HP-4: Validate tool call safety before execution
        {
            let mut validator = params.ctx.safety_validator.write().await;
            if let Err(err) = validator.validate_call(call_tool_name) {
                params.ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Safety validation failed: {}", err),
                )?;
                params
                    .ctx
                    .working_history
                    .push(uni::Message::tool_response_with_origin(
                    tool_call.id.clone(),
                    serde_json::to_string(
                        &serde_json::json!({"error": format!("Safety validation failed: {}", err)}),
                    )
                    .unwrap(),
                    call_tool_name.clone(),
                ));
                return Ok(handle_turn_balancer(
                    params.ctx,
                    params.step_count,
                    params.repeated_tool_attempts,
                    params.max_tool_loops,
                    params.tool_repeat_limit,
                )
                .await);
            }
        }

        match ensure_tool_permission(
            crate::agent::runloop::unified::tool_routing::ToolPermissionsContext {
                tool_registry: params.ctx.tool_registry,
                renderer: params.ctx.renderer,
                handle: params.ctx.handle,
                session: params.ctx.session,
                default_placeholder: params.ctx.default_placeholder.clone(),
                ctrl_c_state: params.ctx.ctrl_c_state,
                ctrl_c_notify: params.ctx.ctrl_c_notify,
                hooks: params.ctx.lifecycle_hooks,
                justification: None,
                approval_recorder: Some(params.ctx.approval_recorder.as_ref()),
                decision_ledger: Some(params.ctx.decision_ledger),
                tool_permission_cache: Some(params.ctx.tool_permission_cache),
                hitl_notification_bell: params
                    .ctx
                    .vt_cfg
                    .map(|cfg| cfg.security.hitl_notification_bell)
                    .unwrap_or(true),
            },
            call_tool_name,
            Some(&call_args_val),
        )
        .await
        {
            Ok(ToolPermissionFlow::Approved) => {
                let tool_result = execute_tool_with_timeout_ref(
                    params.ctx.tool_registry,
                    call_tool_name,
                    &call_args_val,
                    params.ctx.ctrl_c_state,
                    params.ctx.ctrl_c_notify,
                    None, // progress_reporter
                )
                .await;

                let pipeline_outcome = ToolPipelineOutcome::from_status(tool_result);

                handle_tool_execution_result(
                    &mut crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext {
                        renderer: params.ctx.renderer,
                        handle: params.ctx.handle,
                        session: params.ctx.session,
                        session_stats: params.ctx.session_stats,
                        mcp_panel_state: params.ctx.mcp_panel_state,
                        tool_result_cache: params.ctx.tool_result_cache,
                        approval_recorder: params.ctx.approval_recorder,
                        decision_ledger: params.ctx.decision_ledger,
                        tool_registry: params.ctx.tool_registry,
                        tools: params.ctx.tools,
                        cached_tools: params.ctx.cached_tools,
                        ctrl_c_state: params.ctx.ctrl_c_state,
                        ctrl_c_notify: params.ctx.ctrl_c_notify,
                        context_manager: params.ctx.context_manager,
                        last_forced_redraw: params.ctx.last_forced_redraw,
                        input_status_state: params.ctx.input_status_state,
                        lifecycle_hooks: params.ctx.lifecycle_hooks,
                        default_placeholder: params.ctx.default_placeholder,
                        tool_permission_cache: params.ctx.tool_permission_cache,
                        safety_validator: params.ctx.safety_validator,
                    },
                    tool_call.id.clone(),
                    call_tool_name,
                    &call_args_val,
                    &pipeline_outcome,
                    params.ctx.working_history,
                    params.turn_modified_files,
                    params.ctx.vt_cfg,
                    params.traj,
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

                params
                    .ctx
                    .working_history
                    .push(uni::Message::tool_response_with_origin(
                        tool_call.id.clone(),
                        serde_json::to_string(&denial).unwrap_or_else(|_| "{}".to_string()),
                        call_tool_name.clone(),
                    ));
            }
            Ok(ToolPermissionFlow::Exit) => {
                *params.session_end_reason = crate::hooks::lifecycle::SessionEndReason::Exit;
                return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Cancelled));
            }
            Ok(ToolPermissionFlow::Interrupted) => {
                return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Cancelled));
            }
            Err(err) => {
                let err_json = serde_json::json!({
                    "error": format!("Failed to evaluate policy for detected tool '{}': {}", call_tool_name, err)
                });
                params
                    .ctx
                    .working_history
                    .push(uni::Message::tool_response_with_origin(
                        tool_call.id.clone(),
                        err_json.to_string(),
                        call_tool_name.clone(),
                    ));
            }
        }
        Ok(handle_turn_balancer(
            params.ctx,
            params.step_count,
            params.repeated_tool_attempts,
            params.max_tool_loops,
            params.tool_repeat_limit,
        )
        .await)
    } else {
        let msg = uni::Message::assistant(params.text.clone());
        let msg_with_reasoning = if let Some(reasoning_text) = params.reasoning {
            msg.with_reasoning(Some(reasoning_text))
        } else {
            msg
        };

        if !params.text.is_empty() || msg_with_reasoning.reasoning.is_some() {
            params.ctx.working_history.push(msg_with_reasoning);
        }

        Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed))
    }
}
