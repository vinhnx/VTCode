//! Tool execution result handling for turn flow.

use anyhow::Result;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::tool_pipeline::ToolExecutionStatus;
use crate::agent::runloop::unified::tool_pipeline::ToolPipelineOutcome;
use crate::agent::runloop::unified::tool_pipeline::run_tool_call;
use crate::agent::runloop::unified::tool_output_handler::handle_pipeline_output_from_turn_ctx;
use crate::agent::runloop::unified::turn::turn_helpers::display_status;

use super::helpers::push_tool_response;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_tool_execution_result(
    ctx: &mut crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext<'_>,
    tool_call_id: String,
    tool_name: &str,
    args_val: &serde_json::Value,
    pipeline_outcome: &ToolPipelineOutcome,
    working_history: &mut Vec<uni::Message>,
    turn_modified_files: &mut std::collections::BTreeSet<std::path::PathBuf>,
    vt_cfg: Option<&vtcode_core::config::loader::VTCodeConfig>,
    traj: &vtcode_core::core::trajectory::TrajectoryLogger,
    tool_start_time: std::time::Instant,
) -> Result<()> {
    match &pipeline_outcome.status {
        ToolExecutionStatus::Success {
            output,
            stdout: _,
            modified_files: _,
            command_success: _,
            has_more: _,
        } => {
            let duration = tool_start_time.elapsed();
            ctx.circuit_breaker.record_success_for_tool(tool_name);
            ctx.tool_health_tracker
                .record_execution(tool_name, true, duration);
            ctx.telemetry.record_tool_usage(tool_name, true);

            let content_for_model = if let Some(s) = output.as_str() {
                s.to_string()
            } else {
                serde_json::to_string(output).unwrap_or_else(|_| "{}".to_string())
            };

            push_tool_response(
                working_history,
                tool_call_id,
                content_for_model,
                tool_name,
            );

            let (_any_write, mod_files, last_stdout) = handle_pipeline_output_from_turn_ctx(
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
            let duration = tool_start_time.elapsed();

            let error_str = error.to_string();
            let is_plan_mode_denial = error_str.contains("tool denied by plan mode");
            let is_argument_error = error_str.contains("Missing required")
                || error_str.contains("Invalid arguments")
                || error_str.contains("required path parameter")
                || error_str.contains("expected ")
                || error_str.contains("Expected:");

            if !is_plan_mode_denial {
                ctx.circuit_breaker
                    .record_failure_for_tool(tool_name, is_argument_error);
                ctx.tool_health_tracker
                    .record_execution(tool_name, false, duration);
                ctx.telemetry.record_tool_usage(tool_name, false);
            } else {
                tracing::debug!(
                    tool = %tool_name,
                    "Plan mode denial - not recording as circuit breaker failure"
                );
            }

            let error_msg = format!("Tool '{}' execution failed: {}", tool_name, error);
            tracing::debug!(tool = %tool_name, error = %error, "Tool execution failed");

            let error_content = serde_json::json!({"error": error_msg});
            push_tool_response(
                working_history,
                tool_call_id,
                error_content.to_string(),
                tool_name,
            );

            if is_plan_mode_denial && ctx.session_stats.is_plan_mode() {
                if *ctx.auto_exit_plan_mode_attempted {
                    display_status(
                        ctx.renderer,
                        "Plan Mode still active. Call `exit_plan_mode` to review the plan or refine the plan before retrying.",
                    )?;
                    return Ok(());
                }
                *ctx.auto_exit_plan_mode_attempted = true;

                let exit_args = serde_json::json!({
                    "reason": "auto_trigger_on_plan_denial"
                });
                let exit_args_json =
                    serde_json::to_string(&exit_args).unwrap_or_else(|_| "{}".to_string());
                let exit_call_id = format!(
                    "call_auto_exit_plan_mode_{}",
                    tool_start_time.elapsed().as_millis()
                );
                let exit_call = uni::ToolCall::function(
                    exit_call_id.clone(),
                    tool_names::EXIT_PLAN_MODE.to_string(),
                    exit_args_json.clone(),
                );

                let exit_start = std::time::Instant::now();
                let exit_outcome = run_tool_call(
                    &mut crate::agent::runloop::unified::run_loop_context::RunLoopContext {
                        renderer: ctx.renderer,
                        handle: ctx.handle,
                        tool_registry: ctx.tool_registry,
                        tools: ctx.tools,
                        tool_result_cache: ctx.tool_result_cache,
                        tool_permission_cache: ctx.tool_permission_cache,
                        decision_ledger: ctx.decision_ledger,
                        session_stats: ctx.session_stats,
                        mcp_panel_state: ctx.mcp_panel_state,
                        approval_recorder: ctx.approval_recorder,
                        session: ctx.session,
                        traj,
                        harness_state: ctx.harness_state,
                        harness_emitter: ctx.harness_emitter,
                    },
                    &exit_call,
                    ctx.ctrl_c_state,
                    ctx.ctrl_c_notify,
                    ctx.default_placeholder.clone(),
                    ctx.lifecycle_hooks,
                    true,
                    vt_cfg,
                    0,
                )
                .await;

                if let Ok(exit_pipeline_outcome) = exit_outcome {
                    match &exit_pipeline_outcome.status {
                        ToolExecutionStatus::Success { output, .. } => {
                            let duration = exit_start.elapsed();
                            ctx.circuit_breaker
                                .record_success_for_tool(tool_names::EXIT_PLAN_MODE);
                            ctx.tool_health_tracker.record_execution(
                                tool_names::EXIT_PLAN_MODE,
                                true,
                                duration,
                            );

                            let content_for_model = if let Some(s) = output.as_str() {
                                s.to_string()
                            } else {
                                serde_json::to_string(output)
                                    .unwrap_or_else(|_| "{}".to_string())
                            };
                            push_tool_response(
                                working_history,
                                exit_call_id.clone(),
                                content_for_model,
                                tool_names::EXIT_PLAN_MODE,
                            );

                            let (_any_write, mod_files, _last_stdout) =
                                handle_pipeline_output_from_turn_ctx(
                                    ctx,
                                    tool_names::EXIT_PLAN_MODE,
                                    &exit_args,
                                    &exit_pipeline_outcome,
                                    vt_cfg,
                                    traj,
                                )
                                .await?;
                            for f in mod_files {
                                turn_modified_files.insert(f);
                            }
                        }
                        ToolExecutionStatus::Failure { error } => {
                            let duration = exit_start.elapsed();
                            ctx.circuit_breaker
                                .record_failure_for_tool(tool_names::EXIT_PLAN_MODE, false);
                            ctx.tool_health_tracker.record_execution(
                                tool_names::EXIT_PLAN_MODE,
                                false,
                                duration,
                            );

                            let err_msg = format!(
                                "Tool '{}' execution failed: {}",
                                tool_names::EXIT_PLAN_MODE,
                                error
                            );
                            tracing::debug!(
                                tool = tool_names::EXIT_PLAN_MODE,
                                error = %error,
                                "exit_plan_mode failed"
                            );
                            let error_content = serde_json::json!({"error": err_msg});
                            push_tool_response(
                                working_history,
                                exit_call_id.clone(),
                                error_content.to_string(),
                                tool_names::EXIT_PLAN_MODE,
                            );
                        }
                        ToolExecutionStatus::Timeout { error } => {
                            let duration = exit_start.elapsed();
                            ctx.circuit_breaker
                                .record_failure_for_tool(tool_names::EXIT_PLAN_MODE, false);
                            ctx.tool_health_tracker.record_execution(
                                tool_names::EXIT_PLAN_MODE,
                                false,
                                duration,
                            );
                            let err_msg = format!(
                                "Tool '{}' timed out: {}",
                                tool_names::EXIT_PLAN_MODE,
                                error.message
                            );
                            tracing::debug!(
                                tool = tool_names::EXIT_PLAN_MODE,
                                error = %error.message,
                                "exit_plan_mode timed out"
                            );
                            let error_content = serde_json::json!({"error": err_msg});
                            push_tool_response(
                                working_history,
                                exit_call_id.clone(),
                                error_content.to_string(),
                                tool_names::EXIT_PLAN_MODE,
                            );
                        }
                        ToolExecutionStatus::Cancelled | ToolExecutionStatus::Progress(_) => {}
                    }
                }
            }
        }
        ToolExecutionStatus::Timeout { error } => {
            let duration = tool_start_time.elapsed();
            ctx.circuit_breaker
                .record_failure_for_tool(tool_name, false);
            ctx.tool_health_tracker
                .record_execution(tool_name, false, duration);

            let error_msg = format!("Tool '{}' timed out: {}", tool_name, error.message);
            tracing::debug!(tool = %tool_name, error = %error.message, "Tool timed out");

            let error_content = serde_json::json!({"error": error_msg});
            push_tool_response(
                working_history,
                tool_call_id,
                error_content.to_string(),
                tool_name,
            );
        }
        ToolExecutionStatus::Cancelled => {
            let error_msg = format!("Tool '{}' execution cancelled", tool_name);
            ctx.renderer.line(MessageStyle::Info, &error_msg)?;

            let error_content = serde_json::json!({"error": error_msg});
            push_tool_response(
                working_history,
                tool_call_id,
                error_content.to_string(),
                tool_name,
            );
        }
        ToolExecutionStatus::Progress(_) => {}
    }

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
            ToolExecutionStatus::Progress(_) => {}
        }
    }

    Ok(())
}
