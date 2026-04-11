//! Tool outcome handling helpers for turn execution.

use anyhow::Result;

use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::exec_policy::AskForApproval;
use vtcode_core::tools::registry::ToolExecutionError;
use vtcode_core::tools::registry::labels::tool_action_label;
use vtcode_core::utils::ansi::MessageStyle;

use crate::agent::runloop::unified::async_mcp_manager::approval_policy_from_human_in_the_loop;
use crate::agent::runloop::unified::tool_call_safety::SafetyError;
use crate::agent::runloop::unified::tool_routing::{
    ensure_tool_permission_with_call_id, prompt_session_limit_increase,
};
use crate::agent::runloop::unified::turn::context::{
    PreparedAssistantToolCall, TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext,
};

use crate::agent::runloop::unified::tool_routing::ToolPermissionFlow;
mod budget;
mod fallbacks;
mod guards;
#[path = "../handlers_batch.rs"]
mod handlers_batch;
mod looping;
mod rate_limit;
mod recovery;
#[cfg(test)]
mod tests;
mod types;
use budget::{build_tool_budget_exhausted_reason, record_tool_call_budget_usage};
use fallbacks::{
    build_validation_error_content_with_fallback, preflight_validation_fallback,
    recovery_fallback_for_tool, try_recover_preflight_with_fallback,
};
pub(crate) use guards::max_consecutive_blocked_tool_calls_per_turn;
use guards::{
    enforce_blocked_tool_call_guard, enforce_duplicate_task_tracker_create_guard,
    enforce_repeated_read_only_call_guard, enforce_repeated_shell_run_guard,
    enforce_spool_chunk_read_guard,
};
pub(crate) use handlers_batch::{execute_and_handle_tool_call, handle_tool_call_batch_prepared};
pub(crate) use looping::low_signal_family_key;
use looping::maybe_apply_spool_read_offset_hint;
use rate_limit::acquire_adaptive_rate_limit_slot;
use recovery::try_interactive_circuit_recovery;
pub(crate) use types::{PreparedToolCall, ToolOutcomeContext, ValidationResult};

fn build_failure_error_content(error: String, failure_kind: &'static str) -> String {
    super::execution_result::build_error_content(error, None, None, failure_kind).to_string()
}

pub(super) fn apply_reused_read_only_loop_metadata(
    obj: &mut serde_json::Map<String, serde_json::Value>,
) {
    obj.remove("output");
    obj.remove("content");
    obj.remove("stdout");
    obj.remove("stderr");
    obj.remove("stderr_preview");
    obj.insert(
        "reused_recent_result".to_string(),
        serde_json::Value::Bool(true),
    );
    obj.insert("result_ref_only".to_string(), serde_json::Value::Bool(true));
    obj.insert("loop_detected".to_string(), serde_json::Value::Bool(true));
    obj.insert(
        "loop_detected_note".to_string(),
        serde_json::Value::String(
            "Loop detected on repeated read-only call; reusing recent output. Use unified_search (action='grep') or summarize before another read."
                .to_string(),
        ),
    );
    obj.insert(
        "next_action".to_string(),
        serde_json::Value::String(
            "Use unified_search (action='grep') or retry unified_file with a narrower offset/limit before reading again."
                .to_string(),
        ),
    );
}

pub(super) enum ValidationTransition {
    Proceed(PreparedToolCall),
    Return(Option<TurnHandlerOutcome>),
}

pub(super) fn finalize_validation_result(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    tool_name: &str,
    args_val: &serde_json::Value,
    validation_result: ValidationResult,
) -> ValidationTransition {
    match validation_result {
        ValidationResult::Outcome(outcome) => ValidationTransition::Return(Some(outcome)),
        ValidationResult::Handled => {
            ctx.reset_blocked_tool_call_streak();
            ValidationTransition::Return(None)
        }
        ValidationResult::Blocked => {
            let outcome = enforce_blocked_tool_call_guard(ctx, tool_call_id, tool_name, args_val);
            ValidationTransition::Return(outcome)
        }
        ValidationResult::Proceed(prepared) => {
            ctx.reset_blocked_tool_call_streak();
            ValidationTransition::Proceed(prepared)
        }
    }
}

async fn run_safety_validation_loop(
    ctx: &mut TurnProcessingContext<'_>,
    tool_call_id: &str,
    canonical_tool_name: &str,
    effective_args: &serde_json::Value,
) -> Result<Option<ValidationResult>> {
    loop {
        let validation_result = ctx
            .safety_validator
            .validate_call(canonical_tool_name, effective_args)
            .await;

        match validation_result {
            Ok(_) => return Ok(None),
            Err(SafetyError::SessionLimitReached { max }) => {
                match prompt_session_limit_increase(
                    ctx.handle,
                    ctx.session,
                    ctx.ctrl_c_state,
                    ctx.ctrl_c_notify,
                    max,
                )
                .await
                {
                    Ok(Some(increment)) => {
                        ctx.safety_validator.increase_session_limit(increment);
                    }
                    _ => {
                        ctx.push_tool_response(
                            tool_call_id,
                            build_failure_error_content(
                                "Session tool limit reached and not increased by user".to_string(),
                                "safety_limit",
                            ),
                        );
                        return Ok(Some(ValidationResult::Blocked));
                    }
                }
            }
            Err(err) => {
                ctx.renderer.line(
                    MessageStyle::Error,
                    &format!("Safety validation failed: {}", err),
                )?;
                ctx.push_tool_response(
                    tool_call_id,
                    build_failure_error_content(
                        format!("Safety validation failed: {}", err),
                        "safety_validation",
                    ),
                );
                return Ok(Some(ValidationResult::Blocked));
            }
        }
    }
}

fn build_tool_permissions_context<'ctx, 'a>(
    ctx: &'ctx mut TurnProcessingContext<'a>,
) -> crate::agent::runloop::unified::tool_routing::ToolPermissionsContext<
    'ctx,
    vtcode_tui::app::InlineSession,
> {
    crate::agent::runloop::unified::tool_routing::ToolPermissionsContext {
        tool_registry: ctx.tool_registry,
        renderer: ctx.renderer,
        handle: ctx.handle,
        session: ctx.session,
        active_thread_label: Some(ctx.active_thread_label),
        default_placeholder: ctx.default_placeholder.clone(),
        ctrl_c_state: ctx.ctrl_c_state,
        ctrl_c_notify: ctx.ctrl_c_notify,
        hooks: ctx.lifecycle_hooks,
        justification: None,
        approval_recorder: Some(ctx.approval_recorder.as_ref()),
        decision_ledger: Some(ctx.decision_ledger),
        tool_permission_cache: Some(ctx.tool_permission_cache),
        permissions_state: Some(ctx.permissions_state),
        hitl_notification_bell: ctx
            .vt_cfg
            .map(|cfg| cfg.security.hitl_notification_bell)
            .unwrap_or(true),
        approval_policy: ctx
            .vt_cfg
            .map(|cfg| cfg.security.human_in_the_loop)
            .map(approval_policy_from_human_in_the_loop)
            .unwrap_or(AskForApproval::OnRequest),
        skip_confirmations: ctx.skip_confirmations,
        permissions_config: ctx.vt_cfg.map(|cfg| &cfg.permissions),
        auto_mode_runtime: Some(
            crate::agent::runloop::unified::run_loop_context::AutoModeRuntimeContext {
                config: ctx.config,
                vt_cfg: ctx.vt_cfg,
                provider_client: ctx.provider_client.as_mut(),
                working_history: ctx.working_history.as_slice(),
            },
        ),
        session_stats: Some(ctx.session_stats),
    }
}

/// Unified handler for a single tool call (whether native or textual).
///
/// This handler applies the full pipeline of checks:
/// 1. Circuit Breaker
/// 2. Rate Limiting
/// 3. Loop Detection
/// 4. Safety Validation (with potential user interaction for limits)
/// 5. Permission Checking (Allow/Deny/Ask)
/// 6. Execution (with progress spinners and PTY streaming)
/// 7. Result Handling (recording metrics, history, UI output)
pub(crate) async fn handle_prepared_tool_call<'a, 'b>(
    t_ctx: &mut ToolOutcomeContext<'a, 'b>,
    tool_call: &PreparedAssistantToolCall,
) -> Result<Option<TurnHandlerOutcome>> {
    let Some(args_val) = tool_call.args() else {
        return Ok(None);
    };
    handle_tool_call_inner(t_ctx, tool_call.call_id(), tool_call.tool_name(), args_val).await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn handle_single_tool_call<'a, 'b, 'tool>(
    t_ctx: &mut ToolOutcomeContext<'a, 'b>,
    tool_call_id: &str,
    tool_name: &'tool str,
    args_val: serde_json::Value,
) -> Result<Option<TurnHandlerOutcome>> {
    handle_tool_call_inner(t_ctx, tool_call_id, tool_name, &args_val).await
}

async fn handle_tool_call_inner<'a, 'b, 'tool>(
    t_ctx: &mut ToolOutcomeContext<'a, 'b>,
    tool_call_id: &str,
    tool_name: &'tool str,
    args_val: &serde_json::Value,
) -> Result<Option<TurnHandlerOutcome>> {
    use crate::agent::runloop::unified::run_loop_context::TurnPhase;
    t_ctx.ctx.set_phase(TurnPhase::ExecutingTools);

    // 1. Validate (Circuit Breaker, Rate Limit, Loop Detection, Safety, Permission)
    let validation_result =
        validate_tool_call(t_ctx.ctx, tool_call_id, tool_name, args_val).await?;
    let prepared = match finalize_validation_result(
        t_ctx.ctx,
        tool_call_id,
        tool_name,
        args_val,
        validation_result,
    ) {
        ValidationTransition::Proceed(prepared) => prepared,
        ValidationTransition::Return(outcome) => return Ok(outcome),
    };

    // 3. Execute and Handle Result
    if let Some(outcome) = execute_and_handle_tool_call(
        t_ctx.ctx,
        t_ctx.repeated_tool_attempts,
        t_ctx.turn_modified_files,
        tool_call_id.to_string(),
        &prepared.canonical_name,
        prepared.effective_args,
        None,
    )
    .await?
    {
        return Ok(Some(outcome));
    }

    Ok(None)
}

/// Validates a tool call against all safety and permission checks.
/// Returns Some(TurnHandlerOutcome) if the turn loop should break/exit/cancel.
/// Returns None if execution should proceed (or if a local error was already handled/pushed).
pub(crate) async fn validate_tool_call<'a>(
    ctx: &mut TurnProcessingContext<'a>,
    tool_call_id: &str,
    tool_name: &str,
    args_val: &serde_json::Value,
) -> Result<ValidationResult> {
    if let Some(max_tool_calls) = ctx.harness_state.exhausted_tool_call_limit() {
        let tool_calls = ctx.harness_state.tool_calls;
        let exhausted_emitted = ctx.harness_state.tool_budget_exhausted_emitted;
        let error_msg = format!(
            "Policy violation: exceeded max tool calls per turn ({})",
            max_tool_calls
        );
        let block_reason = build_tool_budget_exhausted_reason(tool_calls, max_tool_calls);
        ctx.push_tool_response(
            tool_call_id,
            build_failure_error_content(error_msg, "policy"),
        );
        if !exhausted_emitted {
            ctx.push_system_message(block_reason.clone());
            ctx.harness_state.mark_tool_budget_exhausted_emitted();
        }
        return Ok(ValidationResult::Outcome(TurnHandlerOutcome::Break(
            TurnLoopResult::Blocked {
                reason: Some(block_reason),
            },
        )));
    }

    let wall_clock_exhausted = ctx.harness_state.wall_clock_exhausted();
    if wall_clock_exhausted {
        let max_tool_wall_clock_secs = ctx.harness_state.max_tool_wall_clock.as_secs();
        let error_msg = format!(
            "Policy violation: exceeded tool wall clock budget ({}s)",
            max_tool_wall_clock_secs
        );
        ctx.push_tool_response(
            tool_call_id,
            build_failure_error_content(error_msg, "policy"),
        );
        return Ok(ValidationResult::Blocked);
    }

    let mut prepared = match ctx
        .tool_registry
        .admit_public_tool_call(tool_name, args_val)
    {
        Ok(prepared) => prepared,
        Err(err) => {
            if let Some(recovered_prepared) =
                try_recover_preflight_with_fallback(ctx, tool_name, args_val, &err)
            {
                tracing::info!(
                    tool = tool_name,
                    recovered_tool = %recovered_prepared.canonical_name,
                    "Recovered tool preflight by applying fallback arguments"
                );
                recovered_prepared
            } else {
                let fallback = preflight_validation_fallback(tool_name, args_val, &err);
                let (fallback_tool, fallback_tool_args) = fallback
                    .map(|(tool, args)| (Some(tool), Some(args)))
                    .unwrap_or((None, None));
                ctx.push_tool_response(
                    tool_call_id,
                    build_validation_error_content_with_fallback(
                        format!("Tool preflight validation failed: {}", err),
                        "preflight",
                        fallback_tool,
                        fallback_tool_args,
                    ),
                );
                return Ok(ValidationResult::Blocked);
            }
        }
    };

    let canonical_tool_name = prepared.canonical_name.clone();
    prepared.effective_args = maybe_apply_spool_read_offset_hint(
        ctx.tool_registry,
        &canonical_tool_name,
        &prepared.effective_args,
    );
    if !prepared.readonly_classification {
        ctx.harness_state.reset_file_read_family_streak();
    }
    prepared.parallel_safe_after_preflight = vtcode_core::tools::tool_intent::is_parallel_safe_call(
        &canonical_tool_name,
        &prepared.effective_args,
    );
    let fallback_recommendation = recovery_fallback_for_tool(
        &canonical_tool_name,
        &prepared.effective_args,
    )
    .map(|(tool_name, args)| {
        vtcode_core::core::agent::harness_kernel::FallbackRecommendation { tool_name, args }
    });
    prepared = prepared.with_fallback_recommendation(fallback_recommendation);
    let effective_args = &prepared.effective_args;

    if let Some(outcome) = enforce_duplicate_task_tracker_create_guard(
        ctx,
        tool_call_id,
        &canonical_tool_name,
        effective_args,
    ) {
        return Ok(outcome);
    }

    if let Some(outcome) = enforce_repeated_read_only_call_guard(
        ctx,
        tool_call_id,
        &canonical_tool_name,
        effective_args,
        prepared.readonly_classification,
    ) {
        return Ok(outcome);
    }

    if let Some(outcome) =
        enforce_repeated_shell_run_guard(ctx, tool_call_id, &canonical_tool_name, effective_args)
    {
        return Ok(outcome);
    }

    if let Some(outcome) =
        enforce_spool_chunk_read_guard(ctx, tool_call_id, &canonical_tool_name, effective_args)
    {
        return Ok(outcome);
    }

    // Phase 4 Check: Per-tool Circuit Breaker
    let circuit_breaker_blocked = !ctx
        .circuit_breaker
        .allow_request_for_tool(&canonical_tool_name);
    if circuit_breaker_blocked {
        let display_tool = tool_action_label(&canonical_tool_name, args_val);
        let (fallback_tool, fallback_tool_args) = prepared
            .fallback_recommendation
            .as_ref()
            .map(|fallback| {
                (
                    Some(fallback.tool_name.clone()),
                    Some(fallback.args.clone()),
                )
            })
            .unwrap_or((None, None));
        let block_reason = format!(
            "Circuit breaker blocked '{}' due to high failure rate. Switching to autonomous fallback strategy.",
            display_tool
        );
        tracing::warn!(tool = %canonical_tool_name, "Circuit breaker open, tool disabled");

        // In interactive mode, attempt recovery prompt; None = user chose to proceed.
        if let Some(result) = try_interactive_circuit_recovery(
            ctx,
            tool_call_id,
            &canonical_tool_name,
            fallback_tool,
            fallback_tool_args,
        )
        .await?
        {
            ctx.push_system_message(block_reason);
            return Ok(result);
        }
    }

    // Phase 4 Check: Adaptive Rate Limiter
    if let Some(outcome) =
        acquire_adaptive_rate_limit_slot(ctx, tool_call_id, &canonical_tool_name).await?
    {
        return Ok(outcome);
    }

    // Unified interactive turns own loop/recovery policy via turn-local guards and
    // the turn balancer. The legacy core loop detector remains available for
    // non-unified autonomous execution paths only.

    if let Some(outcome) =
        run_safety_validation_loop(ctx, tool_call_id, &canonical_tool_name, effective_args).await?
    {
        return Ok(outcome);
    }

    // Ensure tool permission
    let permission_result = ensure_tool_permission_with_call_id(
        build_tool_permissions_context(ctx),
        &canonical_tool_name,
        Some(effective_args),
        Some(tool_call_id),
    )
    .await;

    match permission_result {
        Ok(ToolPermissionFlow::Approved { updated_args }) => {
            if let Some(updated_args) = updated_args {
                prepared.effective_args = updated_args;
            }
            if canonical_tool_name == tool_names::ENTER_PLAN_MODE {
                ctx.harness_state.clear_task_tracker_create_signatures();
            }
            // Count budget only for calls that pass all validation/permission gates.
            record_tool_call_budget_usage(ctx);
            Ok(ValidationResult::Proceed(prepared))
        }
        Ok(ToolPermissionFlow::Denied) => {
            let denial = if let Some(denial) = ctx.session_stats.last_auto_mode_denial() {
                serde_json::json!({
                    "error": format!("Auto mode blocked tool '{}': {}", prepared.canonical_name, denial.reason),
                    "reason": denial.reason,
                    "matched_rule": denial.matched_rule,
                    "matched_exception": denial.matched_exception,
                    "review_stage": denial.stage,
                    "next_action": "Choose a safer tool or narrower action that stays within the user's explicit request."
                })
            } else {
                ToolExecutionError::policy_violation(
                    canonical_tool_name,
                    format!(
                        "Tool '{}' execution denied by policy",
                        prepared.canonical_name
                    ),
                )
                .to_json_value()
            };
            ctx.push_tool_response(
                tool_call_id,
                serde_json::to_string(&denial).unwrap_or_else(|_| "{}".to_string()),
            );
            Ok(ValidationResult::Blocked)
        }
        Ok(ToolPermissionFlow::Blocked { reason }) => Ok(ValidationResult::Outcome(
            TurnHandlerOutcome::Break(TurnLoopResult::Blocked {
                reason: Some(reason),
            }),
        )),
        Ok(ToolPermissionFlow::Exit) => Ok(ValidationResult::Outcome(TurnHandlerOutcome::Break(
            TurnLoopResult::Exit,
        ))),
        Ok(ToolPermissionFlow::Interrupted) => Ok(ValidationResult::Outcome(
            TurnHandlerOutcome::Break(TurnLoopResult::Cancelled),
        )),
        Err(err) => {
            let err_json = serde_json::json!({
                "error": format!("Failed to evaluate policy for tool '{}': {}", tool_name, err)
            });
            ctx.push_tool_response(tool_call_id, err_json.to_string());
            Ok(ValidationResult::Blocked)
        }
    }
}
