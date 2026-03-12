use anyhow::Result;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;
use crate::agent::runloop::unified::inline_events::harness::{
    turn_completed_event, turn_failed_event, turn_started_event,
};
use crate::agent::runloop::unified::run_loop_context::HarnessTurnState;
use crate::agent::runloop::unified::run_loop_context::TurnPhase;
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
use crate::agent::runloop::unified::turn::turn_loop_helpers::{
    ToolLoopLimitAction, extract_turn_config, handle_steering_messages,
    maybe_handle_plan_mode_exit_trigger, maybe_handle_tool_loop_limit,
};
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::agent::steering::SteeringMessage;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::hooks::LifecycleHookEngine;
use vtcode_core::llm::provider as uni;
use vtcode_core::notifications::{CompletionStatus, NotificationEvent, send_global_notification};
use vtcode_core::tools::ToolResultCache;
use vtcode_core::tools::{ApprovalRecorder, ToolRegistry};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};
use vtcode_tui::{InlineHandle, InlineSession};

// Using `tool_output_handler::handle_pipeline_output_from_turn_ctx` adapter where needed

use crate::agent::runloop::mcp_events;
use crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker;
use crate::agent::runloop::unified::turn::turn_helpers::display_error;
use vtcode_core::config::types::AgentConfig;
use vtcode_core::core::agent::error_recovery::ErrorType;

pub(crate) struct TurnLoopOutcome {
    pub result: TurnLoopResult,
    pub turn_modified_files: BTreeSet<PathBuf>,
}

pub(crate) struct TurnLoopContext<'a> {
    pub renderer: &'a mut AnsiRenderer,
    pub handle: &'a InlineHandle,
    pub session: &'a mut InlineSession,
    pub session_stats: &'a mut crate::agent::runloop::unified::state::SessionStats,
    pub auto_exit_plan_mode_attempted: &'a mut bool,
    pub mcp_panel_state: &'a mut mcp_events::McpPanelState,
    pub tool_result_cache: &'a Arc<RwLock<ToolResultCache>>,
    pub approval_recorder: &'a Arc<ApprovalRecorder>,
    pub decision_ledger: &'a Arc<RwLock<DecisionTracker>>,
    pub tool_registry: &'a mut ToolRegistry,
    pub tools: &'a Arc<RwLock<Vec<uni::ToolDefinition>>>,
    pub tool_catalog: &'a Arc<crate::agent::runloop::unified::tool_catalog::ToolCatalogState>,
    pub ctrl_c_state: &'a Arc<crate::agent::runloop::unified::state::CtrlCState>,
    pub ctrl_c_notify: &'a Arc<tokio::sync::Notify>,
    pub context_manager: &'a mut crate::agent::runloop::unified::context_manager::ContextManager,
    pub last_forced_redraw: &'a mut Instant,
    pub input_status_state: &'a mut crate::agent::runloop::unified::status_line::InputStatusState,
    pub lifecycle_hooks: Option<&'a LifecycleHookEngine>,
    pub default_placeholder: &'a Option<String>,
    pub tool_permission_cache: &'a Arc<RwLock<ToolPermissionCache>>,
    pub safety_validator: &'a Arc<RwLock<ToolCallSafetyValidator>>,
    pub circuit_breaker: &'a Arc<vtcode_core::tools::circuit_breaker::CircuitBreaker>,
    pub tool_health_tracker: &'a Arc<vtcode_core::tools::health::ToolHealthTracker>,
    pub rate_limiter: &'a Arc<vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter>,
    pub telemetry: &'a Arc<vtcode_core::core::telemetry::TelemetryManager>,
    pub autonomous_executor: &'a Arc<vtcode_core::tools::autonomous_executor::AutonomousExecutor>,
    pub error_recovery:
        &'a Arc<RwLock<vtcode_core::core::agent::error_recovery::ErrorRecoveryState>>,
    pub harness_state: &'a mut HarnessTurnState,
    pub harness_emitter: Option<&'a HarnessEventEmitter>,
    pub config: &'a mut AgentConfig,
    pub vt_cfg: Option<&'a VTCodeConfig>,
    pub turn_metadata_cache: &'a mut Option<Option<serde_json::Value>>,
    pub provider_client: &'a mut Box<dyn uni::LLMProvider>,
    pub traj: &'a TrajectoryLogger,
    pub full_auto: bool,
    pub steering_receiver: &'a mut Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
}

impl<'a> TurnLoopContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        renderer: &'a mut AnsiRenderer,
        handle: &'a InlineHandle,
        session: &'a mut InlineSession,
        session_stats: &'a mut crate::agent::runloop::unified::state::SessionStats,
        auto_exit_plan_mode_attempted: &'a mut bool,
        mcp_panel_state: &'a mut mcp_events::McpPanelState,
        tool_result_cache: &'a Arc<RwLock<ToolResultCache>>,
        approval_recorder: &'a Arc<ApprovalRecorder>,
        decision_ledger: &'a Arc<RwLock<DecisionTracker>>,
        tool_registry: &'a mut ToolRegistry,
        tools: &'a Arc<RwLock<Vec<uni::ToolDefinition>>>,
        tool_catalog: &'a Arc<crate::agent::runloop::unified::tool_catalog::ToolCatalogState>,
        ctrl_c_state: &'a Arc<crate::agent::runloop::unified::state::CtrlCState>,
        ctrl_c_notify: &'a Arc<tokio::sync::Notify>,
        context_manager: &'a mut crate::agent::runloop::unified::context_manager::ContextManager,
        last_forced_redraw: &'a mut Instant,
        input_status_state: &'a mut crate::agent::runloop::unified::status_line::InputStatusState,
        lifecycle_hooks: Option<&'a LifecycleHookEngine>,
        default_placeholder: &'a Option<String>,
        tool_permission_cache: &'a Arc<RwLock<ToolPermissionCache>>,
        safety_validator: &'a Arc<RwLock<ToolCallSafetyValidator>>,
        circuit_breaker: &'a Arc<vtcode_core::tools::circuit_breaker::CircuitBreaker>,
        tool_health_tracker: &'a Arc<vtcode_core::tools::health::ToolHealthTracker>,
        rate_limiter: &'a Arc<vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter>,
        telemetry: &'a Arc<vtcode_core::core::telemetry::TelemetryManager>,
        autonomous_executor: &'a Arc<vtcode_core::tools::autonomous_executor::AutonomousExecutor>,
        error_recovery: &'a Arc<
            RwLock<vtcode_core::core::agent::error_recovery::ErrorRecoveryState>,
        >,
        harness_state: &'a mut HarnessTurnState,
        harness_emitter: Option<&'a HarnessEventEmitter>,
        config: &'a mut AgentConfig,
        vt_cfg: Option<&'a VTCodeConfig>,
        turn_metadata_cache: &'a mut Option<Option<serde_json::Value>>,
        provider_client: &'a mut Box<dyn uni::LLMProvider>,
        traj: &'a TrajectoryLogger,
        full_auto: bool,
        steering_receiver: &'a mut Option<tokio::sync::mpsc::UnboundedReceiver<SteeringMessage>>,
    ) -> Self {
        Self {
            renderer,
            handle,
            session,
            session_stats,
            auto_exit_plan_mode_attempted,
            mcp_panel_state,
            tool_result_cache,
            approval_recorder,
            decision_ledger,
            tool_registry,
            tools,
            tool_catalog,
            ctrl_c_state,
            ctrl_c_notify,
            context_manager,
            last_forced_redraw,
            input_status_state,
            lifecycle_hooks,
            default_placeholder,
            tool_permission_cache,
            safety_validator,
            circuit_breaker,
            tool_health_tracker,
            rate_limiter,
            telemetry,
            autonomous_executor,
            error_recovery,
            harness_state,
            harness_emitter,
            config,
            vt_cfg,
            turn_metadata_cache,
            provider_client,
            traj,
            full_auto,
            steering_receiver,
        }
    }

    pub(crate) fn as_run_loop_context(
        &mut self,
    ) -> crate::agent::runloop::unified::run_loop_context::RunLoopContext<'_> {
        crate::agent::runloop::unified::run_loop_context::RunLoopContext::new(
            self.renderer,
            self.handle,
            self.tool_registry,
            self.tools,
            self.tool_result_cache,
            self.tool_permission_cache,
            self.decision_ledger,
            self.session_stats,
            self.mcp_panel_state,
            self.approval_recorder,
            self.session,
            Some(self.safety_validator),
            self.traj,
            self.harness_state,
            self.harness_emitter,
        )
    }

    pub(crate) fn as_turn_processing_context<'b>(
        &'b mut self,
        working_history: &'b mut Vec<uni::Message>,
    ) -> crate::agent::runloop::unified::turn::context::TurnProcessingContext<'b> {
        let tool = crate::agent::runloop::unified::turn::context::ToolContext {
            tool_result_cache: self.tool_result_cache,
            approval_recorder: self.approval_recorder,
            tool_registry: self.tool_registry,
            tools: self.tools,
            tool_catalog: self.tool_catalog,
            tool_permission_cache: self.tool_permission_cache,
            safety_validator: self.safety_validator,
            circuit_breaker: self.circuit_breaker,
            tool_health_tracker: self.tool_health_tracker,
            rate_limiter: self.rate_limiter,
            telemetry: self.telemetry,
            autonomous_executor: self.autonomous_executor,
            error_recovery: self.error_recovery,
        };
        let llm = crate::agent::runloop::unified::turn::context::LLMContext {
            provider_client: self.provider_client,
            config: self.config,
            vt_cfg: self.vt_cfg,
            context_manager: self.context_manager,
            decision_ledger: self.decision_ledger,
            traj: self.traj,
        };
        let ui = crate::agent::runloop::unified::turn::context::UIContext {
            renderer: self.renderer,
            handle: self.handle,
            session: self.session,
            ctrl_c_state: self.ctrl_c_state,
            ctrl_c_notify: self.ctrl_c_notify,
            lifecycle_hooks: self.lifecycle_hooks,
            default_placeholder: self.default_placeholder,
            last_forced_redraw: self.last_forced_redraw,
            input_status_state: self.input_status_state,
        };
        let state = crate::agent::runloop::unified::turn::context::TurnProcessingState {
            session_stats: self.session_stats,
            auto_exit_plan_mode_attempted: self.auto_exit_plan_mode_attempted,
            mcp_panel_state: self.mcp_panel_state,
            working_history,
            turn_metadata_cache: self.turn_metadata_cache,
            full_auto: self.full_auto,
            harness_state: self.harness_state,
            harness_emitter: self.harness_emitter,
            steering_receiver: self.steering_receiver,
        };

        crate::agent::runloop::unified::turn::context::TurnProcessingContext::from_parts(
            crate::agent::runloop::unified::turn::context::TurnProcessingContextParts {
                tool,
                llm,
                ui,
                state,
            },
        )
    }

    pub(crate) fn is_plan_mode(&self) -> bool {
        self.session_stats.is_plan_mode()
    }

    pub(crate) fn set_phase(&mut self, phase: TurnPhase) {
        self.harness_state.set_phase(phase);
    }
}

fn has_tool_response_since(messages: &[uni::Message], baseline_len: usize) -> bool {
    messages
        .get(baseline_len..)
        .is_some_and(|recent| recent.iter().any(|msg| msg.role == uni::MessageRole::Tool))
}

fn maybe_recover_after_post_tool_llm_failure(
    renderer: &mut AnsiRenderer,
    working_history: &[uni::Message],
    err: &anyhow::Error,
    step_count: usize,
    turn_history_start_len: usize,
    failure_stage: &'static str,
) -> Result<bool> {
    let has_partial_tool_progress =
        has_tool_response_since(working_history, turn_history_start_len);
    if !has_partial_tool_progress {
        return Ok(false);
    }

    let err_cat = vtcode_commons::classify_anyhow_error(err);
    let transient_hint = if err_cat.is_retryable() {
        " (transient — may resolve on retry)"
    } else {
        ""
    };
    let summary = format!(
        "Tool execution completed, but the model follow-up failed{}. Output above is valid.",
        transient_hint,
    );
    renderer.line(MessageStyle::Info, &summary)?;
    renderer.line(
        MessageStyle::Info,
        &format!("Follow-up error category: {}", err_cat.user_label()),
    )?;
    if !err_cat.is_retryable() {
        renderer.line(
            MessageStyle::Info,
            "Tip: rerun with a narrower prompt or switch provider/model for the follow-up.",
        )?;
    }

    tracing::warn!(
        error = %err,
        step = step_count,
        stage = failure_stage,
        category = ?err_cat,
        retryable = err_cat.is_retryable(),
        "Recovered turn after post-tool LLM phase failure"
    );
    Ok(true)
}

// For `TurnLoopContext`, we will reuse the generic `handle_pipeline_output` via an adapter below.

pub(crate) async fn run_turn_loop(
    working_history: &mut Vec<uni::Message>,
    mut ctx: TurnLoopContext<'_>,
) -> Result<TurnLoopOutcome> {
    use crate::agent::runloop::unified::turn::context::{TurnHandlerOutcome, TurnProcessingResult};
    use crate::agent::runloop::unified::turn::guards::run_proactive_guards;
    use crate::agent::runloop::unified::turn::turn_processing::{
        HandleTurnProcessingResultParams, execute_llm_request, handle_turn_processing_result,
        maybe_force_plan_mode_interview, process_llm_response,
        should_attempt_dynamic_interview_generation, synthesize_plan_mode_interview_args,
    };

    // Initialize the outcome result
    let mut result = TurnLoopResult::Completed;
    let mut turn_modified_files = BTreeSet::new();
    *ctx.auto_exit_plan_mode_attempted = false;

    ctx.set_phase(TurnPhase::Preparing);
    if let Some(Err(e)) = ctx.harness_emitter.map(|e| e.emit(turn_started_event())) {
        tracing::debug!(error = %e, "harness turn_started event emission failed");
    }

    // Optimization: Extract all frequently accessed config values once
    let turn_config = extract_turn_config(ctx.vt_cfg, ctx.is_plan_mode());

    let mut step_count = 0;
    let mut current_max_tool_loops = turn_config.max_tool_loops;
    let turn_history_start_len = working_history.len();
    // Optimization: Interned signatures with exponential backoff for loop detection
    let mut repeated_tool_attempts = LoopTracker::new();

    // Reset safety validator for a new turn
    {
        let max_session_turns = if ctx.is_plan_mode() {
            usize::MAX
        } else {
            turn_config.max_session_turns
        };
        let mut validator = ctx.safety_validator.write().await;
        validator.set_limits(current_max_tool_loops, max_session_turns);
        validator.start_turn().await;
    }

    // Loop detection warnings are scoped per turn; clear streak state here.
    ctx.autonomous_executor.reset_turn_loop_detection();

    loop {
        if handle_steering_messages(&mut ctx, working_history, &mut result).await? {
            break;
        }

        step_count += 1;
        ctx.telemetry.record_turn();

        if maybe_handle_plan_mode_exit_trigger(&mut ctx, working_history, step_count, &mut result)
            .await?
        {
            break;
        }

        match maybe_handle_tool_loop_limit(&mut ctx, step_count, &mut current_max_tool_loops)
            .await?
        {
            ToolLoopLimitAction::Proceed => {}
            ToolLoopLimitAction::ContinueLoop => continue,
            ToolLoopLimitAction::BreakLoop => break,
        }

        // Clone validation cache arc to avoid borrow conflict
        let validation_cache = ctx.session_stats.validation_cache.clone();

        // Capture input status state for potential restoration after LLM response
        // (needed because turn_processing_ctx will mutably borrow input_status_state)
        let restore_status_left = ctx.input_status_state.left.clone();
        let restore_status_right = ctx.input_status_state.right.clone();

        // Prepare turn processing context
        let mut turn_processing_ctx = ctx.as_turn_processing_context(working_history);

        // === PROACTIVE GUARDS (HP-2: Pre-request checks) ===
        run_proactive_guards(&mut turn_processing_ctx, step_count).await?;

        // Execute the LLM request
        turn_processing_ctx.set_phase(TurnPhase::Requesting);
        let active_model = turn_processing_ctx.config.model.clone();
        let (response, response_streamed) = match execute_llm_request(
            &mut turn_processing_ctx,
            step_count,
            &active_model,
            None, // max_tokens_opt
            None, // parallel_cfg_opt
        )
        .await
        {
            Ok(val) => val,
            Err(err) => {
                // Record the error in the recovery state for diagnostics
                turn_processing_ctx
                    .record_recovery_error("llm_request", &err, ErrorType::Other)
                    .await;

                // execute_llm_request already performs retry/backoff for retryable provider errors.
                // Avoid a second retry layer here, which can consume turn budget and cause timeouts.
                // Restore input status on request failure to clear loading/shimmer state.
                turn_processing_ctx.restore_input_status(
                    restore_status_left.clone(),
                    restore_status_right.clone(),
                );

                let recovered = maybe_recover_after_post_tool_llm_failure(
                    turn_processing_ctx.renderer,
                    turn_processing_ctx.working_history,
                    &err,
                    step_count,
                    turn_history_start_len,
                    "execute_llm_request",
                )?;
                if recovered {
                    result = TurnLoopResult::Completed;
                    break;
                }

                display_error(turn_processing_ctx.renderer, "LLM request failed", &err)?;
                // Show recovery hints derived from the canonical error category
                {
                    let err_cat = vtcode_commons::classify_anyhow_error(&err);
                    let suggestions = err_cat.recovery_suggestions();
                    if !suggestions.is_empty() {
                        let hint = suggestions.join("; ");
                        turn_processing_ctx.renderer.line(
                            vtcode_core::utils::ansi::MessageStyle::Info,
                            &format!("Hint: {}", hint),
                        )?;
                    }
                }
                // Log error via tracing instead of polluting conversation history
                // Adding error messages as assistant content can poison future turns
                tracing::error!(error = %err, step = step_count, "LLM request failed");
                // Do NOT add error message to working_history - this prevents the model
                // from learning spurious error patterns and keeps the conversation clean
                result = TurnLoopResult::Aborted;
                break;
            }
        };

        // Track token usage for context awareness before any borrows occur
        let response_usage = response.usage.clone();
        if !response.tool_references.is_empty() {
            turn_processing_ctx
                .tool_catalog
                .note_tool_references(turn_processing_ctx.tools, &response.tool_references)
                .await;
        }

        {
            if turn_processing_ctx.is_plan_mode() {
                turn_processing_ctx
                    .session_stats
                    .increment_plan_mode_turns();
            }
        }

        // Process the LLM response
        let processing_result_outcome = {
            let allow_plan_interview = turn_processing_ctx.session_stats.is_plan_mode()
                && turn_config.request_user_input_enabled
                && crate::agent::runloop::unified::turn::turn_processing::plan_mode_interview_ready(
                    turn_processing_ctx.session_stats,
                );
            process_llm_response(
                &response,
                turn_processing_ctx.renderer,
                turn_processing_ctx.working_history.len(),
                turn_processing_ctx.session_stats.is_plan_mode(),
                allow_plan_interview,
                turn_config.request_user_input_enabled,
                Some(&validation_cache),
                Some(turn_processing_ctx.tool_registry),
            )
        };
        let mut processing_result = match processing_result_outcome {
            Ok(result) => result,
            Err(err) => {
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
                    recovery.record_error(
                        "llm_response_parse",
                        format!("{:#}", err),
                        ErrorType::Other,
                    );
                }
                let recovered = maybe_recover_after_post_tool_llm_failure(
                    turn_processing_ctx.renderer,
                    turn_processing_ctx.working_history,
                    &err,
                    step_count,
                    turn_history_start_len,
                    "process_llm_response",
                )?;
                if recovered {
                    result = TurnLoopResult::Completed;
                    break;
                }
                return Err(err);
            }
        };
        if turn_config.request_user_input_enabled {
            let should_attempt_synthesis = {
                turn_processing_ctx.is_plan_mode()
                    && should_attempt_dynamic_interview_generation(
                        &processing_result,
                        response.content.as_deref(),
                        turn_processing_ctx.session_stats,
                    )
            };
            let synthesized_interview_args = if should_attempt_synthesis {
                synthesize_plan_mode_interview_args(
                    turn_processing_ctx.provider_client,
                    &active_model,
                    turn_processing_ctx.working_history,
                    response.content.as_deref(),
                    turn_processing_ctx.session_stats,
                )
                .await
            } else {
                None
            };

            if turn_processing_ctx.is_plan_mode() {
                processing_result = maybe_force_plan_mode_interview(
                    processing_result,
                    response.content.as_deref(),
                    turn_processing_ctx.session_stats,
                    turn_processing_ctx.working_history.len(),
                    synthesized_interview_args,
                );
            }
        }

        // Restore input status if there are no tool calls (turn is completing)
        // This handles the case where defer_restore was set but no tool spinners will take over
        let has_tool_calls = matches!(processing_result, TurnProcessingResult::ToolCalls { .. });
        if !has_tool_calls {
            turn_processing_ctx.restore_input_status(restore_status_left, restore_status_right);
        }

        if has_tool_calls {
            turn_processing_ctx.set_phase(TurnPhase::ExecutingTools);
        } else {
            turn_processing_ctx.set_phase(TurnPhase::Finalizing);
        }

        // Handle the turn processing result (dispatch tool calls or finish turn)
        let turn_outcome_result = handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut turn_processing_ctx,
            processing_result,
            response_streamed,
            step_count,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            max_tool_loops: current_max_tool_loops,
            tool_repeat_limit: turn_config.tool_repeat_limit,
        })
        .await;
        let turn_outcome = match turn_outcome_result {
            Ok(outcome) => outcome,
            Err(err) => {
                // Record result-handler errors for diagnostics (mirrors llm_request recording)
                ctx.error_recovery.write().await.record_error(
                    "turn_result_handler",
                    format!("{:#}", err),
                    ErrorType::ToolExecution,
                );
                if maybe_recover_after_post_tool_llm_failure(
                    ctx.renderer,
                    working_history,
                    &err,
                    step_count,
                    turn_history_start_len,
                    "handle_turn_processing_result",
                )? {
                    result = TurnLoopResult::Completed;
                    break;
                }
                return Err(err);
            }
        };
        match turn_outcome {
            TurnHandlerOutcome::Continue => {
                // Update token usage before continuing loop
                ctx.context_manager.update_token_usage(&response_usage);
                #[cfg(debug_assertions)]
                ctx.context_manager.validate_token_tracking(&response_usage);
                continue;
            }
            TurnHandlerOutcome::Break(outcome_result) => {
                // Update token usage before breaking
                ctx.context_manager.update_token_usage(&response_usage);
                #[cfg(debug_assertions)]
                ctx.context_manager.validate_token_tracking(&response_usage);
                result = outcome_result;
                break;
            }
        }
    }

    ctx.set_phase(TurnPhase::Finalizing);
    if let Some(emitter) = ctx.harness_emitter {
        // Exit is a graceful user-initiated action, not a failure
        let event = match result {
            TurnLoopResult::Completed | TurnLoopResult::Exit => turn_completed_event(),
            TurnLoopResult::Aborted => turn_failed_event("turn aborted"),
            TurnLoopResult::Cancelled => turn_failed_event("turn cancelled"),
            TurnLoopResult::Blocked { .. } => turn_failed_event("turn blocked"),
        };
        if let Err(e) = emitter.emit(event) {
            tracing::debug!(error = %e, "harness turn outcome event emission failed");
        }
    }
    emit_turn_outcome_notification(&result).await;

    // Final outcome with the correct result status
    Ok(TurnLoopOutcome {
        result,
        turn_modified_files,
    })
}

async fn emit_turn_outcome_notification(result: &TurnLoopResult) {
    let event = match result {
        TurnLoopResult::Completed => Some(NotificationEvent::Completion {
            task: "turn".to_string(),
            status: CompletionStatus::Success,
            details: None,
        }),
        TurnLoopResult::Blocked { reason } => Some(NotificationEvent::Completion {
            task: "turn".to_string(),
            status: CompletionStatus::PartialSuccess,
            details: reason.clone(),
        }),
        TurnLoopResult::Aborted => Some(NotificationEvent::Completion {
            task: "turn".to_string(),
            status: CompletionStatus::Failure,
            details: Some("Turn aborted".to_string()),
        }),
        TurnLoopResult::Cancelled => Some(NotificationEvent::Completion {
            task: "turn".to_string(),
            status: CompletionStatus::Cancelled,
            details: Some("Turn cancelled".to_string()),
        }),
        TurnLoopResult::Exit => None,
    };

    if let Some(notification) = event
        && let Err(err) = send_global_notification(notification).await
    {
        tracing::debug!(error = %err, "Failed to emit turn outcome notification");
    }
}

#[cfg(test)]
mod tests {
    use super::has_tool_response_since;
    use vtcode_core::llm::provider as uni;

    #[test]
    fn has_tool_response_since_detects_new_tool_message() {
        let messages = vec![
            uni::Message::user("run script".to_string()),
            uni::Message::assistant("".to_string()),
            uni::Message::tool_response("call_1".to_string(), "ok".to_string()),
        ];

        assert!(has_tool_response_since(&messages, 1));
    }

    #[test]
    fn has_tool_response_since_ignores_non_tool_messages() {
        let messages = vec![
            uni::Message::user("hello".to_string()),
            uni::Message::assistant("done".to_string()),
        ];

        assert!(!has_tool_response_since(&messages, 0));
    }

    #[test]
    fn has_tool_response_since_handles_baseline_past_end() {
        let messages = vec![uni::Message::tool_response(
            "call_1".to_string(),
            "ok".to_string(),
        )];

        assert!(!has_tool_response_since(&messages, 10));
    }
}
