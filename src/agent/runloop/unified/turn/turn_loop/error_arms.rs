//! Match-arm handlers for turn-loop fallible stages.

use std::ops::ControlFlow;

use anyhow::Result;
use vtcode_core::core::agent::error_recovery::ErrorType;
use vtcode_core::llm::provider as uni;
use vtcode_core::utils::ansi::MessageStyle;

use super::context::TurnLoopContext;
use super::post_tool_recovery::{PostToolFailureAction, PostToolRecoveryContext, dispatch_post_tool_failure};
use super::responses::provider_stop_outcome;
use crate::agent::runloop::unified::turn::context::TurnHandlerOutcome;
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;
use crate::agent::runloop::unified::turn::turn_helpers::{display_error, error_message_for_user};

/// Handle a `handle_turn_processing_result` failure: record diagnostics,
/// reset terminal state, offer post-tool recovery, else propagate the error.
pub(super) async fn handle_result_handler_error(
    ctx: &mut TurnLoopContext<'_>,
    working_history: &mut Vec<uni::Message>,
    err: anyhow::Error,
    step_count: usize,
    turn_history_start_len: usize,
) -> Result<ControlFlow<TurnLoopResult>> {
    // Record result-handler errors for diagnostics (mirrors llm_request recording)
    ctx.error_recovery
        .write()
        .await
        .record_error("turn_result_handler", format!("{err:#}"), ErrorType::ToolExecution);
    let tool_free_recovery = ctx.harness_state.recovery_pass_used() && ctx.harness_state.recovery_is_tool_free();
    let planning = ctx.is_planning_active();

    // Ensure the inline input and status line are not left showing a
    // loading or shimmer state when the result handler fails and the
    // turn may exit early. Keep the terminal state synchronized.
    crate::agent::runloop::unified::display::reset_inline_input(ctx.handle, ctx.default_placeholder.clone());
    crate::agent::runloop::unified::status_line::clear_input_status(ctx.handle, ctx.input_status_state);

    match dispatch_post_tool_failure(PostToolRecoveryContext {
        renderer: &mut *ctx.renderer,
        working_history: &mut *working_history,
        harness_state: &mut *ctx.harness_state,
        harness_emitter: ctx.harness_emitter,
        plan_session: planning.then_some(&mut *ctx.plan_session),
        plan_state: planning.then_some(&ctx.tool_registry.planning_workflow_state()),
        err: &err,
        step_count,
        turn_history_start_len,
        stage: "handle_turn_processing_result",
        tool_free_recovery,
    })
    .await?
    {
        PostToolFailureAction::Continue => return Ok(ControlFlow::Continue(())),
        PostToolFailureAction::Break(r) => {
            return Ok(ControlFlow::Break(r));
        }
        PostToolFailureAction::Fallthrough => {}
    }
    Err(err)
}

/// Handle a `process_llm_response` failure: record diagnostics, restore UI
/// state, offer post-tool recovery, else propagate the error.
pub(super) async fn handle_response_parse_error(
    turn_processing_ctx: &mut TurnProcessingContext<'_>,
    err: anyhow::Error,
    step_count: usize,
    turn_history_start_len: usize,
    restore_status_left: Option<String>,
    restore_status_right: Option<String>,
) -> Result<ControlFlow<TurnLoopResult>> {
    let err_cat = vtcode_commons::classify_anyhow_error(&err);
    if err_cat.is_retryable() {
        tracing::warn!(
            error = %err,
            step = step_count,
            category = ?err_cat,
            "Response parse failed with transient error; skipping extra request retry"
        );
    }

    {
        let mut recovery = turn_processing_ctx.error_recovery.write().await;
        recovery.record_error("llm_response_parse", format!("{err:#}"), ErrorType::Other);
    }
    let tool_free_recovery = turn_processing_ctx.recovery_pass_used() && turn_processing_ctx.recovery_is_tool_free();
    let planning = turn_processing_ctx.is_planning_active();

    // Restore the input status/UI before dispatching recovery handling
    // so the bottom-line is not left in a loading state if recovery
    // ultimately ends the turn or returns an error.
    turn_processing_ctx.restore_input_status(restore_status_left, restore_status_right);

    match dispatch_post_tool_failure(PostToolRecoveryContext {
        renderer: &mut *turn_processing_ctx.renderer,
        working_history: &mut *turn_processing_ctx.working_history,
        harness_state: &mut *turn_processing_ctx.harness_state,
        harness_emitter: turn_processing_ctx.harness_emitter,
        plan_session: planning.then_some(&mut *turn_processing_ctx.plan_session),
        plan_state: planning.then_some(&turn_processing_ctx.tool_registry.planning_workflow_state()),
        err: &err,
        step_count,
        turn_history_start_len,
        stage: "process_llm_response",
        tool_free_recovery,
    })
    .await?
    {
        PostToolFailureAction::Continue => return Ok(ControlFlow::Continue(())),
        PostToolFailureAction::Break(r) => {
            return Ok(ControlFlow::Break(r));
        }
        PostToolFailureAction::Fallthrough => {}
    }
    Err(err)
}

/// Handle an `execute_llm_request` failure: Ctrl-C stops, unsupported knobs,
/// planning recovery, post-tool recovery, error display, else abort the turn.
pub(super) async fn handle_llm_request_error(
    turn_processing_ctx: &mut TurnProcessingContext<'_>,
    err: &anyhow::Error,
    step_count: usize,
    turn_history_start_len: usize,
    restore_status_left: Option<String>,
    restore_status_right: Option<String>,
    tool_free_recovery: bool,
) -> Result<ControlFlow<TurnLoopResult>> {
    if let Some(stop) = provider_stop_outcome(turn_processing_ctx.ctrl_c_state, err) {
        turn_processing_ctx.restore_input_status(restore_status_left, restore_status_right);
        return Ok(ControlFlow::Break(stop));
    }
    if err
        .downcast_ref::<vtcode_core::llm::reasoning_effort::ReasoningEffortUnsupported>()
        .is_some()
    {
        turn_processing_ctx.restore_input_status(restore_status_left.clone(), restore_status_right.clone());
        return Ok(ControlFlow::Break(TurnLoopResult::Blocked { reason: Some(err.to_string()) }));
    }
    // Record the error in the recovery state for diagnostics
    turn_processing_ctx
        .record_recovery_error("llm_request", err, ErrorType::Other)
        .await;

    // execute_llm_request already performs retry/backoff for retryable provider errors.
    // Avoid a second retry layer here, which can consume turn budget and cause timeouts.
    // Restore input status on request failure to clear loading/shimmer state.
    turn_processing_ctx.restore_input_status(restore_status_left.clone(), restore_status_right.clone());

    let planning = turn_processing_ctx.is_planning_active();
    if planning && turn_processing_ctx.plan_session.recovery.research_closed {
        turn_processing_ctx
            .plan_session
            .recovery
            .record_response_class(vtcode_core::exec::events::planning::PlanningResponseClass::ProviderFailure);
        turn_processing_ctx.persist_planning_recovery().await?;
        match turn_processing_ctx
            .recover_planning_failure(&error_message_for_user(err), None)
            .await?
        {
            TurnHandlerOutcome::Continue => return Ok(ControlFlow::Continue(())),
            TurnHandlerOutcome::Break(outcome) => {
                return Ok(ControlFlow::Break(outcome));
            }
            _ => {}
        }
    }
    match dispatch_post_tool_failure(PostToolRecoveryContext {
        renderer: &mut *turn_processing_ctx.renderer,
        working_history: &mut *turn_processing_ctx.working_history,
        harness_state: &mut *turn_processing_ctx.harness_state,
        harness_emitter: turn_processing_ctx.harness_emitter,
        plan_session: planning.then_some(&mut *turn_processing_ctx.plan_session),
        plan_state: planning.then_some(&turn_processing_ctx.tool_registry.planning_workflow_state()),
        err,
        step_count,
        turn_history_start_len,
        stage: "execute_llm_request",
        tool_free_recovery,
    })
    .await?
    {
        PostToolFailureAction::Continue => return Ok(ControlFlow::Continue(())),
        PostToolFailureAction::Break(r) => {
            return Ok(ControlFlow::Break(r));
        }
        PostToolFailureAction::Fallthrough => {}
    }

    display_error(turn_processing_ctx.renderer, "LLM request failed", err)?;
    // Show recovery hints derived from the canonical error category
    {
        let err_cat = vtcode_commons::classify_anyhow_error(err);
        if matches!(err_cat, vtcode_commons::ErrorCategory::Authentication) {
            // For auth errors, show actionable provider-specific guidance
            // that distinguishes "no key stored" from "stored key rejected".
            let (provider_label, provider_key, is_managed_auth, has_stored_credential) = turn_processing_ctx
                .config
                .provider
                .parse::<vtcode_core::config::models::Provider>()
                .map(|p| {
                    let storage_mode = turn_processing_ctx
                        .vt_cfg
                        .map(|cfg| cfg.agent.credential_storage_mode)
                        .unwrap_or_default();
                    let has = vtcode_config::api_keys::provider_credential_detail_with_mode(p, storage_mode).is_some();
                    (p.label().to_string(), p.as_ref().to_string(), p.uses_managed_auth(), has)
                })
                .unwrap_or_else(|_| {
                    (
                        turn_processing_ctx.config.provider.clone(),
                        turn_processing_ctx.config.provider.clone(),
                        false,
                        false,
                    )
                });
            let guidance =
                err_cat.auth_recovery_guidance(&provider_label, &provider_key, is_managed_auth, has_stored_credential);
            for line in &guidance {
                turn_processing_ctx.renderer.line(MessageStyle::Info, line)?;
            }
        } else {
            let suggestions = err_cat.recovery_suggestions();
            if !suggestions.is_empty() {
                let hint = suggestions.join("; ");
                turn_processing_ctx
                    .renderer
                    .line(MessageStyle::Info, &format!("Hint: {hint}"))?;
            }
        }
    }
    // Log error via tracing instead of polluting conversation history
    // Adding error messages as assistant content can poison future turns
    let error_message = error_message_for_user(err);
    tracing::error!(error = %error_message, step = step_count, "LLM request failed");
    // Do NOT add error message to working_history - this prevents the model
    // from learning spurious error patterns and keeps the conversation clean
    Ok(ControlFlow::Break(TurnLoopResult::Aborted))
}
