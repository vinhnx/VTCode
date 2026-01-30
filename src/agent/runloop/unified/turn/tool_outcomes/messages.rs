//! Message handling helpers for tool outcomes.

use anyhow::Result;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::registry::{ToolErrorType, ToolExecutionError};
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::unified::tool_pipeline::{
    ToolPipelineOutcome, execute_tool_with_timeout_ref,
};
use crate::agent::runloop::unified::tool_routing::{ToolPermissionFlow, ensure_tool_permission};
use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext,
};
use crate::agent::runloop::unified::turn::guards::handle_turn_balancer;

use super::execution_result::handle_tool_execution_result;
use super::helpers::{push_assistant_message, push_tool_response, resolve_max_tool_retries};

pub(crate) fn handle_assistant_response(
    ctx: &mut TurnProcessingContext<'_>,
    assistant_text: String,
    reasoning: Option<String>,
    response_streamed: bool,
) -> Result<()> {
    fn reasoning_duplicates_content(cleaned_reasoning: &str, content: &str) -> bool {
        let cleaned_content = vtcode_core::llm::providers::clean_reasoning_text(content);
        !cleaned_reasoning.is_empty()
            && !cleaned_content.is_empty()
            && cleaned_reasoning == cleaned_content
    }

    if !response_streamed {
        if !assistant_text.trim().is_empty() {
            ctx.renderer.line(MessageStyle::Response, &assistant_text)?;
        }
        if let Some(reasoning_text) = reasoning.as_ref()
            && !reasoning_text.trim().is_empty()
        {
            let cleaned_reasoning =
                vtcode_core::llm::providers::clean_reasoning_text(reasoning_text);
            let duplicates_content = !assistant_text.trim().is_empty()
                && reasoning_duplicates_content(&cleaned_reasoning, &assistant_text);
            if !cleaned_reasoning.trim().is_empty() && !duplicates_content {
                ctx.renderer
                    .line(MessageStyle::Reasoning, &cleaned_reasoning)?;
            }
        }
    }

    if !assistant_text.trim().is_empty() {
        let msg = uni::Message::assistant(assistant_text.clone());
        let msg_with_reasoning = if let Some(reasoning_text) = reasoning {
            let cleaned_reasoning =
                vtcode_core::llm::providers::clean_reasoning_text(&reasoning_text);
            let duplicates_content =
                reasoning_duplicates_content(&cleaned_reasoning, &assistant_text);
            if duplicates_content {
                msg
            } else {
                msg.with_reasoning(Some(reasoning_text))
            }
        } else {
            msg
        };
        push_assistant_message(ctx.working_history, msg_with_reasoning);
    } else if let Some(reasoning_text) = reasoning {
        push_assistant_message(
            ctx.working_history,
            uni::Message::assistant(String::new()).with_reasoning(Some(reasoning_text)),
        );
    }

    Ok(())
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
    pub max_tool_loops: usize,
    pub tool_repeat_limit: usize,
}

pub(crate) async fn handle_text_response(
    params: HandleTextResponseParams<'_>,
) -> Result<TurnHandlerOutcome> {
    fn reasoning_duplicates_content(cleaned_reasoning: &str, content: &str) -> bool {
        let cleaned_content = vtcode_core::llm::providers::clean_reasoning_text(content);
        !cleaned_reasoning.is_empty()
            && !cleaned_content.is_empty()
            && cleaned_reasoning == cleaned_content
    }

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
            let cleaned_reasoning =
                vtcode_core::llm::providers::clean_reasoning_text(reasoning_text);
            let duplicates_content = !params.text.trim().is_empty()
                && reasoning_duplicates_content(&cleaned_reasoning, &params.text);
            if !cleaned_reasoning.trim().is_empty() && !duplicates_content {
                params
                    .ctx
                    .renderer
                    .line(MessageStyle::Reasoning, &cleaned_reasoning)?;
            }
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

        {
            let mut validator = params.ctx.safety_validator.write().await;
            if let Err(err) = validator.validate_call(call_tool_name) {
                params.ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Safety validation failed: {}", err),
                )?;
                push_tool_response(
                    params.ctx.working_history,
                    tool_call.id.clone(),
                    serde_json::to_string(
                        &serde_json::json!({"error": format!("Safety validation failed: {}", err)}),
                    )
                    .unwrap_or_else(|_| "{}".to_string()),
                    call_tool_name,
                );
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
                autonomous_mode: params.ctx.session_stats.is_autonomous_mode(),
                human_in_the_loop: params
                    .ctx
                    .vt_cfg
                    .map(|cfg| cfg.security.human_in_the_loop)
                    .unwrap_or(true),
            },
            call_tool_name,
            Some(&call_args_val),
        )
        .await
        {
            Ok(ToolPermissionFlow::Approved) => {
                let tool_execution_start = std::time::Instant::now();
                let tool_result = execute_tool_with_timeout_ref(
                    params.ctx.tool_registry,
                    call_tool_name,
                    &call_args_val,
                    params.ctx.ctrl_c_state,
                    params.ctx.ctrl_c_notify,
                    None,
                    resolve_max_tool_retries(params.ctx.vt_cfg),
                )
                .await;

                let pipeline_outcome = ToolPipelineOutcome::from_status(tool_result);

                handle_tool_execution_result(
                    &mut crate::agent::runloop::unified::turn::turn_loop::TurnLoopContext {
                        renderer: params.ctx.renderer,
                        handle: params.ctx.handle,
                        session: params.ctx.session,
                        session_stats: params.ctx.session_stats,
                        auto_exit_plan_mode_attempted: params.ctx.auto_exit_plan_mode_attempted,
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
                        circuit_breaker: params.ctx.circuit_breaker,
                        tool_health_tracker: params.ctx.tool_health_tracker,
                        rate_limiter: params.ctx.rate_limiter,
                        telemetry: params.ctx.telemetry,
                        autonomous_executor: params.ctx.autonomous_executor,
                        error_recovery: params.ctx.error_recovery,
                        harness_state: params.ctx.harness_state,
                        harness_emitter: params.ctx.harness_emitter,
                    },
                    tool_call.id.clone(),
                    call_tool_name,
                    &call_args_val,
                    &pipeline_outcome,
                    params.ctx.working_history,
                    params.turn_modified_files,
                    params.ctx.vt_cfg,
                    params.traj,
                    tool_execution_start,
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

                push_tool_response(
                    params.ctx.working_history,
                    tool_call.id.clone(),
                    serde_json::to_string(&denial).unwrap_or_else(|_| "{}".to_string()),
                    call_tool_name,
                );
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
                push_tool_response(
                    params.ctx.working_history,
                    tool_call.id.clone(),
                    err_json.to_string(),
                    call_tool_name,
                );
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
            let cleaned_reasoning =
                vtcode_core::llm::providers::clean_reasoning_text(&reasoning_text);
            let duplicates_content = reasoning_duplicates_content(&cleaned_reasoning, &params.text);
            if duplicates_content {
                msg
            } else {
                msg.with_reasoning(Some(reasoning_text))
            }
        } else {
            msg
        };

        if !params.text.is_empty() || msg_with_reasoning.reasoning.is_some() {
            push_assistant_message(params.ctx.working_history, msg_with_reasoning);
        }

        Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed))
    }
}
