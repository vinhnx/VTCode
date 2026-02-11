use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Error, anyhow};
use serde_json::Value;
use tokio::sync::Notify;
use tokio::time;
use tracing::{debug, warn};

use crate::agent::runloop::tool_output::resolve_stdout_tail_limit;
use crate::agent::runloop::unified::plan_confirmation::{
    PlanConfirmationOutcome, execute_plan_confirmation, plan_confirmation_outcome_to_json,
};
use crate::agent::runloop::unified::plan_mode_state::{
    transition_to_edit_mode, transition_to_plan_mode,
};
use crate::agent::runloop::unified::plan_mode_switch::{
    maybe_disable_plan_mode_for_tool, restore_plan_mode_after_tool,
};
use crate::agent::runloop::unified::progress::ProgressReporter;
use crate::agent::runloop::unified::run_loop_context::RunLoopContext;
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::tool_routing::ToolPermissionFlow;
use crate::agent::runloop::unified::turn::guards::validate_tool_args_security;
use crate::agent::runloop::unified::turn::plan_content::parse_plan_content_from_json;
use vtcode_core::config::constants::tools;
use vtcode_core::exec::cancellation;
use vtcode_core::tools::registry::ToolErrorType;
use vtcode_core::tools::registry::{ToolRegistry, classify_error};
use vtcode_core::ui::tui::PlanContent;

use crate::agent::runloop::git::confirm_changes_with_git_diff;
use crate::agent::runloop::unified::inline_events::harness::{
    HarnessEventEmitter, tool_completed_event, tool_started_event,
};
use crate::agent::runloop::unified::tool_routing::ensure_tool_permission;
use crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner;
use crate::hooks::lifecycle::LifecycleHookEngine;
use tokio_util::sync::CancellationToken;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::exec::events::CommandExecutionStatus;

use super::cache::{cache_target_path, create_enhanced_cache_key, is_tool_cacheable};
use super::execute_hitl_tool;
use super::pty_stream::PtyStreamRuntime;
use super::status::{ToolExecutionStatus, ToolPipelineOutcome};
use super::timeout::{TimeoutWarningGuard, create_timeout_error};
use super::{DEFAULT_TOOL_TIMEOUT, MAX_RETRY_BACKOFF, RETRY_BACKOFF_BASE};
use vtcode_core::tools::registry::ToolTimeoutCategory;

pub(crate) async fn run_tool_call(
    ctx: &mut RunLoopContext<'_>,
    call: &vtcode_core::llm::provider::ToolCall,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    default_placeholder: Option<String>,
    lifecycle_hooks: Option<&LifecycleHookEngine>,
    skip_confirmations: bool,
    vt_cfg: Option<&VTCodeConfig>,
    turn_index: usize,
    prevalidated: bool,
) -> Result<ToolPipelineOutcome, anyhow::Error> {
    let function = match call.function.as_ref() {
        Some(func) => func,
        None => {
            return Ok(ToolPipelineOutcome::from_status(
                ToolExecutionStatus::Failure {
                    error: anyhow!("Tool call missing function"),
                },
            ));
        }
    };

    let name = function.name.as_str();
    let args_val = match call.parsed_arguments() {
        Ok(args) => args,
        Err(err) => {
            return Ok(ToolPipelineOutcome::from_status(
                ToolExecutionStatus::Failure {
                    error: anyhow!(err),
                },
            ));
        }
    };
    let tool_item_id = call.id.clone();

    if !prevalidated {
        if let Some(validation_failures) =
            validate_tool_args_security(name, &args_val, None, Some(ctx.tool_registry))
        {
            return Ok(ToolPipelineOutcome::from_status(
                ToolExecutionStatus::Failure {
                    error: anyhow!(
                        "Tool argument validation failed: {}",
                        validation_failures.join("; ")
                    ),
                },
            ));
        }

        if let Some(safety_validator) = ctx.safety_validator {
            let validation = {
                let mut validator = safety_validator.write().await;
                validator.validate_call(name).await
            };
            if let Err(err) = validation {
                return Ok(ToolPipelineOutcome::from_status(
                    ToolExecutionStatus::Failure {
                        error: anyhow!("Safety validation failed: {}", err),
                    },
                ));
            }
        }
    }

    let harness_emitter = ctx.harness_emitter;
    let mut tool_started_emitted = false;
    if let Some(emitter) = harness_emitter {
        let _ = emitter.emit(tool_started_event(tool_item_id.clone(), name));
        tool_started_emitted = true;
    }
    let max_tool_retries = ctx.harness_state.max_tool_retries as usize;
    // Resolve tool name through registry (handles aliases and normalization)
    let resolved_name = ctx
        .tool_registry
        .get_tool(name)
        .map(|tool| tool.name())
        .unwrap_or(name);

    if !prevalidated {
        // Pre-flight permission check
        match ensure_tool_permission(
            crate::agent::runloop::unified::tool_routing::ToolPermissionsContext {
                tool_registry: ctx.tool_registry,
                renderer: ctx.renderer,
                handle: ctx.handle,
                session: ctx.session,
                default_placeholder: default_placeholder.clone(),
                ctrl_c_state,
                ctrl_c_notify,
                hooks: lifecycle_hooks,
                justification: None,
                approval_recorder: Some(ctx.approval_recorder),
                decision_ledger: Some(ctx.decision_ledger),
                tool_permission_cache: Some(ctx.tool_permission_cache),
                hitl_notification_bell: vt_cfg
                    .map(|cfg| cfg.security.hitl_notification_bell)
                    .unwrap_or(true),
                autonomous_mode: ctx.session_stats.is_autonomous_mode(),
                human_in_the_loop: vt_cfg
                    .map(|cfg| cfg.security.human_in_the_loop)
                    .unwrap_or(true),
                delegate_mode: ctx.session_stats.is_delegate_mode(),
            },
            name,
            Some(&args_val),
        )
        .await
        {
            Ok(ToolPermissionFlow::Approved) => {}
            Ok(ToolPermissionFlow::Denied) => {
                let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure {
                    error: anyhow::anyhow!("Tool permission denied"),
                });
                emit_tool_completion_for_status(
                    harness_emitter,
                    tool_started_emitted,
                    &tool_item_id,
                    name,
                    &outcome.status,
                );
                return Ok(outcome);
            }
            Ok(ToolPermissionFlow::Interrupted) => {
                let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Cancelled);
                emit_tool_completion_for_status(
                    harness_emitter,
                    tool_started_emitted,
                    &tool_item_id,
                    name,
                    &outcome.status,
                );
                return Ok(outcome);
            }
            Ok(ToolPermissionFlow::Exit) => {
                let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Cancelled);
                emit_tool_completion_for_status(
                    harness_emitter,
                    tool_started_emitted,
                    &tool_item_id,
                    name,
                    &outcome.status,
                );
                return Ok(outcome);
            }
            Err(e) => {
                let outcome =
                    ToolPipelineOutcome::from_status(ToolExecutionStatus::Failure { error: e });
                emit_tool_completion_for_status(
                    harness_emitter,
                    tool_started_emitted,
                    &tool_item_id,
                    name,
                    &outcome.status,
                );
                return Ok(outcome);
            }
        }
    }

    if let Some(hitl_result) = execute_hitl_tool(
        resolved_name,
        ctx.handle,
        ctx.session,
        &args_val,
        ctrl_c_state,
        ctrl_c_notify,
        ctx.session_stats.editing_mode,
    )
    .await
    {
        let outcome = ToolPipelineOutcome::from_status(match hitl_result {
            Ok(value) => ToolExecutionStatus::Success {
                output: value,
                stdout: None,
                modified_files: vec![],
                command_success: true,
                has_more: false,
            },
            Err(error) => ToolExecutionStatus::Failure { error },
        });
        emit_tool_completion_for_status(
            harness_emitter,
            tool_started_emitted,
            &tool_item_id,
            name,
            &outcome.status,
        );
        return Ok(outcome);
    }

    let restore_plan_mode = match maybe_disable_plan_mode_for_tool(
        ctx.session_stats,
        ctx.tool_registry,
        ctx.renderer,
        ctx.handle,
        resolved_name,
        &args_val,
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            emit_tool_completion_status(
                harness_emitter,
                tool_started_emitted,
                &tool_item_id,
                name,
                CommandExecutionStatus::Failed,
            );
            return Err(error);
        }
    };

    // Special-case enter_plan_mode: execute tool and enable plan mode in registry.
    // This ensures the registry's plan_read_only_mode flag is set when agent enters plan mode.
    if name == tools::ENTER_PLAN_MODE {
        let tool_result = execute_tool_with_timeout_ref(
            ctx.tool_registry,
            name,
            &args_val,
            ctrl_c_state,
            ctrl_c_notify,
            None,
            max_tool_retries,
        )
        .await;

        // If tool execution succeeded, enable plan mode in the registry
        if let ToolExecutionStatus::Success { ref output, .. } = tool_result {
            let status = output.get("status").and_then(|s| s.as_str());
            // Enable plan mode unless we were already in plan mode
            if status == Some("success") {
                transition_to_plan_mode(
                    ctx.tool_registry,
                    ctx.session_stats,
                    ctx.handle,
                    false,
                    false,
                )
                .await;
                tracing::info!(
                    target: "vtcode.plan_mode",
                    "Agent entered Plan Mode with planner profile (read-only, mutating tools blocked)"
                );
            }
        }

        let outcome = ToolPipelineOutcome::from_status(tool_result);
        emit_tool_completion_for_status(
            harness_emitter,
            tool_started_emitted,
            &tool_item_id,
            name,
            &outcome.status,
        );
        return Ok(outcome);
    }

    // Special-case exit_plan_mode: execute tool first, then show confirmation modal if needed.
    // This implements the "Execute After Confirmation (HITL)" pattern from Claude Code.
    if name == tools::EXIT_PLAN_MODE {
        // Check if plan confirmation is enabled via config
        let require_confirmation = vt_cfg
            .map(|cfg| cfg.agent.require_plan_confirmation)
            .unwrap_or(true);

        // Execute the exit_plan_mode tool to get the plan summary
        let tool_result = execute_tool_with_timeout_ref(
            ctx.tool_registry,
            name,
            &args_val,
            ctrl_c_state,
            ctrl_c_notify,
            None,
            max_tool_retries,
        )
        .await;

        // If tool execution succeeded, check if confirmation is required
        if let ToolExecutionStatus::Success { ref output, .. } = tool_result {
            // Check if the result indicates pending confirmation
            let status = output.get("status").and_then(|s| s.as_str());
            let requires_confirmation_from_result = output
                .get("requires_confirmation")
                .and_then(|r| r.as_bool())
                .unwrap_or(false);

            if status == Some("pending_confirmation")
                && requires_confirmation_from_result
                && require_confirmation
            {
                // Parse the plan content for the modal
                // Prefer using raw markdown content with PlanContent::from_markdown for better parsing
                // Fall back to JSON summary if raw content not available
                let plan_content = if let Some(raw_content) =
                    output.get("plan_content").and_then(|v| v.as_str())
                {
                    let title = output
                        .get("plan_summary")
                        .and_then(|s| s.get("title"))
                        .and_then(|t| t.as_str())
                        .unwrap_or("Implementation Plan")
                        .to_string();
                    let file_path = output
                        .get("plan_file")
                        .and_then(|p| p.as_str())
                        .map(|s| s.to_string());
                    PlanContent::from_markdown(title, raw_content, file_path)
                } else {
                    let plan_summary_json = output.get("plan_summary").cloned().unwrap_or_default();
                    parse_plan_content_from_json(&plan_summary_json)
                };

                // Show the confirmation modal and wait for user response
                let confirmation_outcome = execute_plan_confirmation(
                    ctx.handle,
                    ctx.session,
                    plan_content,
                    ctrl_c_state,
                    ctrl_c_notify,
                )
                .await;

                let final_output = match confirmation_outcome {
                    Ok(outcome) => {
                        // If user approved, we need to actually disable plan mode
                        if matches!(
                            outcome,
                            PlanConfirmationOutcome::Execute | PlanConfirmationOutcome::AutoAccept
                        ) {
                            transition_to_edit_mode(
                                ctx.tool_registry,
                                ctx.session_stats,
                                ctx.handle,
                                true,
                            )
                            .await;
                            tracing::info!(
                                target: "vtcode.plan_mode",
                                "User approved plan execution, transitioning to coder profile (mutating tools enabled)"
                            );
                        } else if matches!(outcome, PlanConfirmationOutcome::EditPlan) {
                            // User wants to edit the plan - ensure plan mode stays active
                            tracing::info!(
                                target: "vtcode.plan_mode",
                                "User requested plan edit, remaining in Plan mode"
                            );
                        }
                        plan_confirmation_outcome_to_json(&outcome)
                    }
                    Err(e) => serde_json::json!({
                        "status": "error",
                        "error": format!("Plan confirmation failed: {}", e)
                    }),
                };

                let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
                    output: final_output,
                    stdout: None,
                    modified_files: vec![],
                    command_success: true,
                    has_more: false,
                });
                emit_tool_completion_for_status(
                    harness_emitter,
                    tool_started_emitted,
                    &tool_item_id,
                    name,
                    &outcome.status,
                );
                return Ok(outcome);
            } else if !require_confirmation {
                // Confirmation disabled via config, auto-approve
                transition_to_edit_mode(ctx.tool_registry, ctx.session_stats, ctx.handle, true)
                    .await;
                tracing::info!(
                    target: "vtcode.plan_mode",
                    "Plan confirmation disabled via config, auto-approving with coder profile (mutating tools enabled)"
                );
                let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
                    output: serde_json::json!({
                        "status": "approved",
                        "action": "execute",
                        "auto_accept": true,
                        "message": "Plan confirmation disabled. Proceeding with implementation."
                    }),
                    stdout: None,
                    modified_files: vec![],
                    command_success: true,
                    has_more: false,
                });
                emit_tool_completion_for_status(
                    harness_emitter,
                    tool_started_emitted,
                    &tool_item_id,
                    name,
                    &outcome.status,
                );
                return Ok(outcome);
            }
        }

        // Fall through: return the original tool result if no special handling needed
        let outcome = ToolPipelineOutcome::from_status(tool_result);
        emit_tool_completion_for_status(
            harness_emitter,
            tool_started_emitted,
            &tool_item_id,
            name,
            &outcome.status,
        );
        return Ok(outcome);
    }

    // Determine read-only tools for caching
    // Enhanced caching: Determine if tool is cacheable based on tool type and arguments
    let is_cacheable_tool = is_tool_cacheable(name, &args_val);
    let cache_target = cache_target_path(name, &args_val);

    // Attempt cache retrieval for cacheable tools
    if is_cacheable_tool {
        let workspace_path = ctx
            .tool_registry
            .workspace_root()
            .to_string_lossy()
            .to_string();
        let cache_key = create_enhanced_cache_key(name, &args_val, &cache_target, &workspace_path);

        let mut cache = ctx.tool_result_cache.write().await;
        if let Some(cached_output) = cache.get(&cache_key) {
            match parse_cached_output(&cached_output) {
                Ok(cached_json) => {
                    // Telemetry: Log cache hit
                    tracing::debug!(
                        target: "vtcode.performance.cache",
                        "Cache hit for tool: {} (workspace: {})",
                        name,
                        workspace_path
                    );

                    let outcome = ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
                        output: cached_json,
                        stdout: None,
                        modified_files: vec![],
                        command_success: true,
                        has_more: false,
                    });
                    emit_tool_completion_for_status(
                        harness_emitter,
                        tool_started_emitted,
                        &tool_item_id,
                        name,
                        &outcome.status,
                    );
                    return Ok(outcome);
                }
                Err(error) => {
                    warn!(
                        target: "vtcode.performance.cache",
                        tool = name,
                        error = %error,
                        "Discarding malformed cached output"
                    );
                    cache.invalidate_for_path(&cache_target);
                }
            }
        } else {
            // Telemetry: Log cache miss
            tracing::debug!(
                target: "vtcode.performance.cache",
                "Cache miss for tool: {} (workspace: {})",
                name,
                workspace_path
            );
        }
    }

    // Force TUI redraw to ensure stable UI without added delay
    // Note: In the enhanced version, this would use the UI redraw batcher
    // For now, we keep the direct call for compatibility
    ctx.handle.force_redraw();

    // Execute with progress reporter
    let progress_reporter = ProgressReporter::new();
    progress_reporter.set_total(100).await;
    progress_reporter.set_progress(0).await;
    progress_reporter
        .set_message(format!("Starting {}...", name))
        .await;

    let status_message = build_tool_status_message(name, &args_val);
    let tool_spinner = PlaceholderSpinner::with_progress(
        ctx.handle,
        Some("".to_string()),
        Some("".to_string()),
        status_message,
        Some(&progress_reporter),
    );

    let should_stream_pty = matches!(
        resolved_name,
        tools::RUN_PTY_CMD | tools::UNIFIED_EXEC | tools::SEND_PTY_INPUT
    );
    let mut pty_stream_runtime: Option<PtyStreamRuntime> = None;
    let previous_progress_callback = if should_stream_pty {
        let tail_limit = resolve_stdout_tail_limit(vt_cfg);
        let (runtime, callback) =
            PtyStreamRuntime::start(ctx.handle.clone(), progress_reporter.clone(), tail_limit);
        pty_stream_runtime = Some(runtime);
        Some(ctx.tool_registry.replace_progress_callback(Some(callback)))
    } else {
        None
    };

    let outcome = execute_tool_with_timeout_ref(
        ctx.tool_registry,
        name,
        &args_val,
        ctrl_c_state,
        ctrl_c_notify,
        Some(&progress_reporter),
        max_tool_retries,
    )
    .await;

    if let Some(previous) = previous_progress_callback {
        let _ = ctx.tool_registry.replace_progress_callback(previous);
    }
    if let Some(runtime) = pty_stream_runtime {
        runtime.shutdown().await;
    }

    // Handle loop detection for read-only tools: if blocked, try to return cached result
    let outcome = if is_cacheable_tool && is_loop_detection_status(&outcome) {
        // Tool was blocked due to loop detection - try to get cached result
        let workspace_path = ctx
            .tool_registry
            .workspace_root()
            .to_string_lossy()
            .to_string();
        let cache_key = create_enhanced_cache_key(name, &args_val, &cache_target, &workspace_path);
        let mut cache = ctx.tool_result_cache.write().await;
        if let Some(cached_output) = cache.get(&cache_key) {
            match parse_cached_output(&cached_output) {
                Ok(cached_json) => {
                    drop(cache);
                    tool_spinner.finish();
                    let cached_outcome =
                        ToolPipelineOutcome::from_status(ToolExecutionStatus::Success {
                            output: cached_json,
                            stdout: None,
                            modified_files: vec![],
                            command_success: true,
                            has_more: false,
                        });
                    emit_tool_completion_for_status(
                        harness_emitter,
                        tool_started_emitted,
                        &tool_item_id,
                        name,
                        &cached_outcome.status,
                    );
                    return Ok(cached_outcome);
                }
                Err(error) => {
                    warn!(
                        target: "vtcode.performance.cache",
                        tool = name,
                        error = %error,
                        "Discarding malformed cached output after loop detection"
                    );
                    cache.invalidate_for_path(&cache_target);
                }
            }
        }
        outcome
    } else {
        outcome
    };

    if let ToolExecutionStatus::Success {
        output,
        stdout: _stdout,
        modified_files: _modified_files,
        command_success,
        has_more: _has_more,
    } = &outcome
    {
        tool_spinner.finish();
        // Cache successful cacheable results
        if is_cacheable_tool && *command_success {
            let workspace_path = ctx
                .tool_registry
                .workspace_root()
                .to_string_lossy()
                .to_string();
            let cache_key =
                create_enhanced_cache_key(name, &args_val, &cache_target, &workspace_path);
            let mut cache = ctx.tool_result_cache.write().await;
            let output_json = serde_json::to_string(&output).unwrap_or_else(|_| "{}".to_string());
            cache.insert_arc(cache_key, Arc::new(output_json));
        }
    }

    if let Err(error) = restore_plan_mode_after_tool(
        ctx.session_stats,
        ctx.tool_registry,
        ctx.renderer,
        ctx.handle,
        restore_plan_mode,
    )
    .await
    {
        emit_tool_completion_status(
            harness_emitter,
            tool_started_emitted,
            &tool_item_id,
            name,
            CommandExecutionStatus::Failed,
        );
        return Err(error);
    }

    let mut pipeline_outcome = ToolPipelineOutcome::from_status(outcome);

    // If tool made file modifications, optionally confirm with git diff and either keep or revert
    if !pipeline_outcome.modified_files.is_empty() {
        let modified_files = pipeline_outcome.modified_files.clone();
        let keep_changes =
            match confirm_changes_with_git_diff(&modified_files, skip_confirmations).await {
                Ok(value) => value,
                Err(error) => {
                    emit_tool_completion_status(
                        harness_emitter,
                        tool_started_emitted,
                        &tool_item_id,
                        name,
                        CommandExecutionStatus::Failed,
                    );
                    return Err(error);
                }
            };
        if keep_changes {
            // record confirmed changes in trajectory inside ctx.traj
            ctx.traj.log_tool_call(
                turn_index,
                name,
                &args_val,
                pipeline_outcome.command_success,
            );
            if pipeline_outcome.command_success {
                let mut cache = ctx.tool_result_cache.write().await;
                for path in &pipeline_outcome.modified_files {
                    cache.invalidate_for_path(path);
                }
            }
            // modified_files are kept as-is
        } else {
            // Reverted by confirm function; clear modified files
            pipeline_outcome.modified_files.clear();
            pipeline_outcome.command_success = false;
        }
    } else {
        // Log that the tool was invoked but made no file modifications
        ctx.traj.log_tool_call(
            turn_index,
            name,
            &args_val,
            pipeline_outcome.command_success,
        );
    }

    emit_tool_completion_for_status(
        harness_emitter,
        tool_started_emitted,
        &tool_item_id,
        name,
        &pipeline_outcome.status,
    );

    // Ledger recording is left to the run loop where a decision id is available. Return the pipeline outcome only.
    Ok(pipeline_outcome)
}

fn emit_tool_completion_status(
    harness_emitter: Option<&HarnessEventEmitter>,
    tool_started_emitted: bool,
    tool_item_id: &str,
    tool_name: &str,
    status: CommandExecutionStatus,
) {
    if !tool_started_emitted {
        return;
    }

    if let Some(emitter) = harness_emitter {
        let _ = emitter.emit(tool_completed_event(
            tool_item_id.to_string(),
            tool_name,
            status,
            None,
        ));
    }
}

fn emit_tool_completion_for_status(
    harness_emitter: Option<&HarnessEventEmitter>,
    tool_started_emitted: bool,
    tool_item_id: &str,
    tool_name: &str,
    tool_status: &ToolExecutionStatus,
) {
    let status = if matches!(tool_status, ToolExecutionStatus::Success { .. }) {
        CommandExecutionStatus::Completed
    } else {
        CommandExecutionStatus::Failed
    };
    emit_tool_completion_status(
        harness_emitter,
        tool_started_emitted,
        tool_item_id,
        tool_name,
        status,
    );
}

fn parse_cached_output(cached_output: &str) -> serde_json::Result<Value> {
    serde_json::from_str(cached_output)
}

fn is_loop_detection_status(status: &ToolExecutionStatus) -> bool {
    match status {
        ToolExecutionStatus::Success { output, .. } => output
            .get("loop_detected")
            .and_then(|value| value.as_bool())
            .unwrap_or(false),
        ToolExecutionStatus::Failure { error } => error.to_string().contains("LOOP DETECTION"),
        _ => false,
    }
}

fn is_retry_safe_tool(registry: &ToolRegistry, name: &str, args: &Value) -> bool {
    registry
        .preflight_validate_call(name, args)
        .map(|preflight| preflight.readonly_classification)
        .unwrap_or_else(|_| !registry.is_mutating_tool(name))
}

fn build_tool_status_message(tool_name: &str, args: &Value) -> String {
    if is_command_tool(tool_name) {
        let command = args
            .get("command")
            .and_then(|value| value.as_str())
            .unwrap_or(tool_name);
        format!("Running command: {}", command)
    } else {
        format!("Running tool: {}", tool_name)
    }
}

fn is_command_tool(tool_name: &str) -> bool {
    matches!(
        tool_name,
        tools::RUN_PTY_CMD
            | tools::SHELL
            | tools::UNIFIED_EXEC
            | tools::EXECUTE_CODE
            | tools::EXEC_PTY_CMD
            | tools::EXEC
    )
}

/// Execute a tool with a timeout and progress reporting
///
/// This is a convenience wrapper around `execute_tool_with_timeout_ref` that takes
/// ownership of the args Value. Primarily used in tests and legacy code.
/// Production code should prefer `execute_tool_with_timeout_ref` to avoid cloning.
#[allow(dead_code)]
pub(crate) async fn execute_tool_with_timeout(
    registry: &ToolRegistry,
    name: &str,
    args: Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    progress_reporter: Option<&ProgressReporter>,
    max_tool_retries: usize,
) -> ToolExecutionStatus {
    execute_tool_with_timeout_ref(
        registry,
        name,
        &args,
        ctrl_c_state,
        ctrl_c_notify,
        progress_reporter,
        max_tool_retries,
    )
    .await
}

/// Execute a tool with a timeout and progress reporting (reference-based to avoid cloning args)
pub(crate) async fn execute_tool_with_timeout_ref(
    registry: &ToolRegistry,
    name: &str,
    args: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    progress_reporter: Option<&ProgressReporter>,
    max_tool_retries: usize,
) -> ToolExecutionStatus {
    // Use provided progress reporter or create a new one
    let mut local_progress_reporter = None;
    let progress_reporter = if let Some(reporter) = progress_reporter {
        reporter
    } else {
        local_progress_reporter = Some(ProgressReporter::new());
        local_progress_reporter.as_ref().unwrap()
    };

    // Determine the timeout category for this tool
    let timeout_category = registry.timeout_category_for(name).await;
    let timeout_ceiling = registry
        .timeout_policy()
        .ceiling_for(timeout_category)
        .unwrap_or(DEFAULT_TOOL_TIMEOUT);
    let retry_allowed = is_retry_safe_tool(registry, name, args);

    if !retry_allowed && max_tool_retries > 0 {
        debug!(
            target: "vtcode.tool.exec",
            tool = name,
            "tool classified as non-idempotent; retries disabled"
        );
    }

    // Execute with progress tracking
    let result = execute_tool_with_progress(
        registry,
        name,
        args,
        ctrl_c_state,
        ctrl_c_notify,
        progress_reporter,
        timeout_ceiling,
        timeout_category,
        retry_allowed,
        max_tool_retries,
    )
    .await;

    // Ensure progress is marked as complete only if we created the reporter
    if let Some(ref local_reporter) = local_progress_reporter {
        local_reporter.complete().await;
    }
    result
}

/// Execute a tool with progress reporting
///
/// # Timeout Semantics
/// The `tool_timeout` represents a **total ceiling** across all attempts including retries.
/// Each attempt receives the *remaining* time, ensuring the total wall time does not
/// exceed the ceiling (plus small overhead for backoff delays).
async fn execute_tool_with_progress(
    registry: &ToolRegistry,
    name: &str,
    args: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    progress_reporter: &ProgressReporter,
    tool_timeout: Duration,
    timeout_category: ToolTimeoutCategory,
    retry_allowed: bool,
    max_tool_retries: usize,
) -> ToolExecutionStatus {
    // Track total deadline to enforce ceiling across all attempts
    let deadline = Instant::now() + tool_timeout;

    // Execute first attempt
    let mut attempt = 0usize;
    let mut status = {
        let attempt_start = Instant::now();
        let remaining_timeout = deadline.saturating_duration_since(attempt_start);
        let status = run_single_tool_attempt(
            registry,
            name,
            args,
            ctrl_c_state,
            ctrl_c_notify,
            progress_reporter,
            remaining_timeout,
        )
        .await;

        debug!(
            target: "vtcode.tool.exec",
            tool = name,
            attempt = attempt + 1,
            status = status_label(&status),
            elapsed_ms = attempt_start.elapsed().as_millis(),
            "tool attempt finished"
        );

        status
    };

    // Retry on recoverable errors with bounded backoff
    while let Some(delay) =
        retry_delay_for_status(&status, attempt, max_tool_retries, retry_allowed)
    {
        attempt += 1;
        progress_reporter
            .set_message(format!(
                "Retrying {} (attempt {}/{}) after {}ms...",
                name,
                attempt + 1,
                max_tool_retries + 1,
                delay.as_millis()
            ))
            .await;

        // Cancellable backoff: listen for Ctrl+C/exit signals during retry delay
        tokio::select! {
            _ = tokio::time::sleep(delay) => {},
            _ = ctrl_c_notify.notified() => {
                return ToolExecutionStatus::Cancelled;
            }
        }

        // Check if we've exceeded the total deadline before attempting
        let now = Instant::now();
        let remaining_timeout = deadline.saturating_duration_since(now);
        if remaining_timeout < Duration::from_secs(1) {
            return create_timeout_error(name, timeout_category, Some(tool_timeout));
        }

        let attempt_start = Instant::now();
        status = run_single_tool_attempt(
            registry,
            name,
            args,
            ctrl_c_state,
            ctrl_c_notify,
            progress_reporter,
            remaining_timeout,
        )
        .await;

        debug!(
            target: "vtcode.tool.exec",
            tool = name,
            attempt = attempt + 1,
            status = status_label(&status),
            elapsed_ms = attempt_start.elapsed().as_millis(),
            retry_delay_ms = delay.as_millis(),
            "tool attempt finished"
        );
    }

    status
}

async fn run_single_tool_attempt(
    registry: &ToolRegistry,
    name: &str,
    args: &Value,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    progress_reporter: &ProgressReporter,
    tool_timeout: Duration,
) -> ToolExecutionStatus {
    let start_time = Instant::now();
    let warning_fraction = registry.timeout_policy().warning_fraction();
    let mut warning_guard =
        TimeoutWarningGuard::new(name, start_time, tool_timeout, warning_fraction);

    progress_reporter
        .set_message(format!("Preparing {}...", name))
        .await;
    progress_reporter.set_progress(5).await;

    if let Err(_e) = ctrl_c_state.check_cancellation() {
        progress_reporter
            .set_message(format!("{} cancelled", name))
            .await;
        progress_reporter.set_progress(100).await;
        warning_guard.cancel().await;
        return ToolExecutionStatus::Cancelled;
    }

    progress_reporter
        .set_message(format!("Setting up {} execution...", name))
        .await;
    progress_reporter.set_progress(20).await;

    // Spawn a background task to update progress periodically with elapsed time
    // This is created once per attempt, outside the execution loop
    let _progress_update_guard = {
        use crate::agent::runloop::unified::progress::{
            ProgressUpdateGuard, spawn_elapsed_time_updater,
        };
        let handle = spawn_elapsed_time_updater(progress_reporter.clone(), name.to_string(), 500);
        ProgressUpdateGuard::new(handle)
    };

    let status = loop {
        if let Err(_e) = ctrl_c_state.check_cancellation() {
            progress_reporter
                .set_message(format!("{} cancelled", name))
                .await;
            progress_reporter.set_progress(100).await;
            warning_guard.cancel().await;
            break ToolExecutionStatus::Cancelled;
        }

        progress_reporter
            .set_message(format!("Executing {}...", name))
            .await;

        let token = CancellationToken::new();
        let exec_future = cancellation::with_tool_cancellation(token.clone(), async {
            progress_reporter.set_progress(40).await;

            let result = registry.execute_tool_ref(name, args).await;

            progress_reporter
                .set_message(format!("Processing {} results...", name))
                .await;
            progress_reporter.set_progress(90).await;

            result
        });

        if ctrl_c_state.is_cancel_requested() || ctrl_c_state.is_exit_requested() {
            token.cancel();
            return ToolExecutionStatus::Cancelled;
        }

        enum ExecutionControl {
            Continue,
            Completed(Result<Result<Value, Error>, time::error::Elapsed>),
            Cancelled,
        }

        let control = tokio::select! {
            biased;

            _ = ctrl_c_notify.notified() => {
                if let Err(_e) = ctrl_c_state.check_cancellation() {
                    token.cancel();
                    ExecutionControl::Cancelled
                } else {
                    token.cancel();
                    ExecutionControl::Continue
                }
            }

            result = vtcode_core::utils::async_utils::with_timeout(exec_future, tool_timeout, "tool execution") => {
                match result {
                    Ok(val) => ExecutionControl::Completed(Ok(val)),
                    Err(e) => {
                        // Check if it's a timeout or other error
                        if e.to_string().contains("timed out") {
                            // We can't easily construct tokio::time::error::Elapsed,
                            // but we can return it via a timeout call that always fails
                            let elapsed = time::timeout(Duration::from_nanos(0), async {}).await.unwrap_err();
                            ExecutionControl::Completed(Err(elapsed))
                        } else {
                            ExecutionControl::Completed(Ok(Err(e)))
                        }
                    }
                }
            },
        };

        // Stop the background update task (handled by guard drop)

        match control {
            ExecutionControl::Continue => continue,
            ExecutionControl::Cancelled => {
                progress_reporter
                    .set_message(format!("{} cancelled", name))
                    .await;
                progress_reporter.set_progress(100).await;
                break ToolExecutionStatus::Cancelled;
            }
            ExecutionControl::Completed(result) => {
                break match result {
                    Ok(Ok(output)) => {
                        progress_reporter
                            .set_message(format!("{} completed", name))
                            .await;
                        progress_reporter.set_progress(100).await;
                        process_llm_tool_output(output)
                    }
                    Ok(Err(error)) => {
                        progress_reporter
                            .set_message(format!("{} failed", name))
                            .await;
                        ToolExecutionStatus::Failure { error }
                    }
                    Err(_) => {
                        token.cancel();
                        progress_reporter
                            .set_message(format!("{} timed out", name))
                            .await;
                        let timeout_category = registry.timeout_category_for(name).await;
                        create_timeout_error(name, timeout_category, Some(tool_timeout))
                    }
                };
            }
        }
    };

    warning_guard.cancel().await;

    status
}

fn retry_delay_for_status(
    status: &ToolExecutionStatus,
    attempt: usize,
    max_tool_retries: usize,
    retry_allowed: bool,
) -> Option<Duration> {
    if !retry_allowed || attempt >= max_tool_retries {
        return None;
    }

    match status {
        ToolExecutionStatus::Timeout { error } => {
            if error.is_recoverable {
                Some(backoff_for_attempt(attempt))
            } else {
                None
            }
        }
        ToolExecutionStatus::Failure { error } => {
            let error_type = classify_error(error);
            if matches!(
                error_type,
                ToolErrorType::Timeout | ToolErrorType::NetworkError
            ) {
                Some(backoff_for_attempt(attempt))
            } else {
                None
            }
        }
        _ => None,
    }
}

fn backoff_for_attempt(attempt: usize) -> Duration {
    let retry_number = attempt.saturating_add(1);
    let exp = 2_u64.saturating_pow(retry_number.saturating_sub(1).min(4) as u32); // cap exponent growth
    let jitter = Duration::from_millis(((retry_number as u64 * 53) % 150) + 75);
    let backoff = RETRY_BACKOFF_BASE
        .saturating_mul(exp as u32)
        .saturating_add(jitter);
    std::cmp::max(backoff, Duration::from_millis(350)).min(MAX_RETRY_BACKOFF)
}

fn status_label(status: &ToolExecutionStatus) -> &'static str {
    match status {
        ToolExecutionStatus::Success { .. } => "success",
        ToolExecutionStatus::Failure { .. } => "failure",
        ToolExecutionStatus::Timeout { .. } => "timeout",
        ToolExecutionStatus::Cancelled => "cancelled",
        ToolExecutionStatus::Progress(_) => "progress",
    }
}

/// Process the output from a tool execution and convert it to a ToolExecutionStatus
pub(crate) fn process_llm_tool_output(output: Value) -> ToolExecutionStatus {
    // Check for loop detection first - this is a critical signal to stop retrying
    if let Some(loop_detected) = output.get("loop_detected").and_then(|v| v.as_bool())
        && loop_detected
    {
        let tool_name = output
            .get("tool")
            .and_then(|v| v.as_str())
            .unwrap_or("tool");
        let repeat_count = output
            .get("repeat_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let base_error_msg = output
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("Tool blocked due to repeated identical invocations");

        // Create a structured, explicit error message that clearly instructs the LLM to stop
        // Format: Use clear directives and structured information for better LLM understanding
        let clear_error_msg = format!(
            "LOOP DETECTION: Tool '{}' has been called {} times with identical parameters and is now blocked.\n\n\
                ACTION REQUIRED: DO NOT retry this tool call. The tool execution has been prevented to avoid infinite loops.\n\n\
                If you need the result from this tool:\n\
                1. Check if you already have the result from a previous successful call in your conversation history\n\
                2. If not available, use a different approach or modify your request\n\n\
                Original error: {}",
            tool_name, repeat_count, base_error_msg
        );
        return ToolExecutionStatus::Failure {
            error: anyhow::anyhow!(clear_error_msg),
        };
    }

    // Check if the output contains an error object
    if let Some(error_value) = output.get("error") {
        let error_msg = if let Some(message) = error_value.get("message").and_then(|m| m.as_str()) {
            // Error is an object with message field
            message.to_string()
        } else if let Some(error_str) = error_value.as_str() {
            // Error is a direct string
            error_str.to_string()
        } else {
            // Fallback for unknown error format
            "Unknown tool execution error".to_string()
        };
        return ToolExecutionStatus::Failure {
            error: anyhow::anyhow!(error_msg),
        };
    }

    let exit_code = output
        .get("exit_code")
        .and_then(|value| value.as_i64())
        .unwrap_or(0);
    let command_success = exit_code == 0;

    // Extract stdout if available
    let stdout = output
        .get("stdout")
        .and_then(|value| value.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    // Extract modified files if available
    let modified_files = output
        .get("modified_files")
        .and_then(|value| value.as_array())
        .map(|files| {
            files
                .iter()
                .filter_map(|entry| entry.as_str().map(|s| s.to_string()))
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();

    // Check if there are more results
    let has_more = output
        .get("has_more")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    ToolExecutionStatus::Success {
        output,
        stdout,
        modified_files,
        command_success,
        has_more,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    #[test]
    fn first_retry_backoff_is_non_zero_and_meaningful() {
        let delay = backoff_for_attempt(0);
        assert!(delay >= Duration::from_millis(350));
        assert!(delay <= MAX_RETRY_BACKOFF);
    }

    #[test]
    fn retry_delay_honors_retry_safety_gate() {
        let timeout_status = create_timeout_error(
            "read_file",
            ToolTimeoutCategory::Default,
            Some(Duration::from_secs(1)),
        );

        assert!(retry_delay_for_status(&timeout_status, 0, 2, true).is_some());
        assert!(retry_delay_for_status(&timeout_status, 0, 2, false).is_none());
    }

    #[tokio::test]
    async fn retry_safety_allows_read_only_and_blocks_mutating() {
        let temp_dir = TempDir::new().expect("temp dir");
        let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        assert!(is_retry_safe_tool(
            &registry,
            tools::READ_FILE,
            &json!({"path": "Cargo.toml"})
        ));
        assert!(!is_retry_safe_tool(
            &registry,
            tools::WRITE_FILE,
            &json!({"path": "scratch.txt", "content": "x"})
        ));
    }

    #[test]
    fn loop_detection_status_detects_failure_marker() {
        let status = process_llm_tool_output(json!({
            "error": {
                "message": "Tool blocked after repeated invocations"
            },
            "loop_detected": true,
            "repeat_count": 3,
            "tool": "read_file"
        }));

        assert!(is_loop_detection_status(&status));
    }

    #[test]
    fn malformed_cached_json_is_rejected() {
        assert!(parse_cached_output("{not-valid-json").is_err());
    }
}
