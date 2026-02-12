use anyhow::Result;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::RwLock as StdRwLock;
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
use vtcode_core::acp::ToolPermissionCache;
use vtcode_core::config::constants::tools as tool_names;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::core::decision_tracker::DecisionTracker;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::llm::provider as uni;
use vtcode_core::tools::ToolResultCache;
use vtcode_core::tools::{ApprovalRecorder, ToolRegistry};
use vtcode_core::ui::tui::{InlineHandle, InlineSession};
use vtcode_core::utils::ansi::AnsiRenderer;

// Using `tool_output_handler::handle_pipeline_output_from_turn_ctx` adapter where needed

use crate::agent::runloop::mcp_events;
use vtcode_core::config::types::AgentConfig;

pub struct TurnLoopOutcome {
    pub result: TurnLoopResult,
    pub working_history: Vec<uni::Message>,
    pub turn_modified_files: BTreeSet<PathBuf>,
}

pub struct TurnLoopContext<'a> {
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
    pub lifecycle_hooks: Option<&'a crate::hooks::lifecycle::LifecycleHookEngine>,
    pub default_placeholder: &'a Option<String>,
    pub tool_permission_cache: &'a Arc<RwLock<ToolPermissionCache>>,
    pub safety_validator: &'a Arc<RwLock<ToolCallSafetyValidator>>,
    pub circuit_breaker: &'a Arc<vtcode_core::tools::circuit_breaker::CircuitBreaker>,
    pub tool_health_tracker: &'a Arc<vtcode_core::tools::health::ToolHealthTracker>,
    pub rate_limiter: &'a Arc<vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter>,
    pub telemetry: &'a Arc<vtcode_core::core::telemetry::TelemetryManager>,
    pub autonomous_executor: &'a Arc<vtcode_core::tools::autonomous_executor::AutonomousExecutor>,
    pub error_recovery:
        &'a Arc<StdRwLock<vtcode_core::core::agent::error_recovery::ErrorRecoveryState>>,
    pub harness_state: &'a mut HarnessTurnState,
    pub harness_emitter: Option<&'a HarnessEventEmitter>,
    pub config: &'a mut AgentConfig,
    pub vt_cfg: Option<&'a VTCodeConfig>,
    pub provider_client: &'a mut Box<dyn uni::LLMProvider>,
    pub traj: &'a TrajectoryLogger,
    pub full_auto: bool,
}

impl<'a> TurnLoopContext<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
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
        lifecycle_hooks: Option<&'a crate::hooks::lifecycle::LifecycleHookEngine>,
        default_placeholder: &'a Option<String>,
        tool_permission_cache: &'a Arc<RwLock<ToolPermissionCache>>,
        safety_validator: &'a Arc<RwLock<ToolCallSafetyValidator>>,
        circuit_breaker: &'a Arc<vtcode_core::tools::circuit_breaker::CircuitBreaker>,
        tool_health_tracker: &'a Arc<vtcode_core::tools::health::ToolHealthTracker>,
        rate_limiter: &'a Arc<vtcode_core::tools::adaptive_rate_limiter::AdaptiveRateLimiter>,
        telemetry: &'a Arc<vtcode_core::core::telemetry::TelemetryManager>,
        autonomous_executor: &'a Arc<vtcode_core::tools::autonomous_executor::AutonomousExecutor>,
        error_recovery: &'a Arc<
            StdRwLock<vtcode_core::core::agent::error_recovery::ErrorRecoveryState>,
        >,
        harness_state: &'a mut HarnessTurnState,
        harness_emitter: Option<&'a HarnessEventEmitter>,
        config: &'a mut AgentConfig,
        vt_cfg: Option<&'a VTCodeConfig>,
        provider_client: &'a mut Box<dyn uni::LLMProvider>,
        traj: &'a TrajectoryLogger,
        full_auto: bool,
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
            provider_client,
            traj,
            full_auto,
        }
    }

    pub fn as_run_loop_context(
        &mut self,
    ) -> crate::agent::runloop::unified::run_loop_context::RunLoopContext<'_> {
        crate::agent::runloop::unified::run_loop_context::RunLoopContext {
            renderer: self.renderer,
            handle: self.handle,
            tool_registry: self.tool_registry,
            tools: self.tools,
            tool_result_cache: self.tool_result_cache,
            tool_permission_cache: self.tool_permission_cache,
            decision_ledger: self.decision_ledger,
            session_stats: self.session_stats,
            mcp_panel_state: self.mcp_panel_state,
            approval_recorder: self.approval_recorder,
            session: self.session,
            safety_validator: Some(self.safety_validator),
            traj: self.traj,
            harness_state: self.harness_state,
            harness_emitter: self.harness_emitter,
        }
    }
}

// For `TurnLoopContext`, we will reuse the generic `handle_pipeline_output` via an adapter below.

/// Optimization: Pre-computed turn configuration to avoid repeated Option unwrapping
#[derive(Debug, Clone)]
struct PrecomputedTurnConfig {
    max_tool_loops: usize,
    tool_repeat_limit: usize,
    max_session_turns: usize,
    ask_questions_enabled: bool,
}

/// Extract frequently accessed config values once per turn to reduce overhead
#[inline]
fn extract_turn_config(vt_cfg: Option<&VTCodeConfig>) -> PrecomputedTurnConfig {
    vt_cfg
        .map(|cfg| PrecomputedTurnConfig {
            max_tool_loops: if cfg.tools.max_tool_loops > 0 {
                cfg.tools.max_tool_loops
            } else {
                vtcode_core::config::constants::defaults::DEFAULT_MAX_TOOL_LOOPS
            },
            tool_repeat_limit: if cfg.tools.max_repeated_tool_calls > 0 {
                cfg.tools.max_repeated_tool_calls
            } else {
                vtcode_core::config::constants::defaults::DEFAULT_MAX_REPEATED_TOOL_CALLS
            },
            max_session_turns: cfg.agent.max_conversation_turns,
            ask_questions_enabled: cfg.chat.ask_questions.enabled,
        })
        .unwrap_or(PrecomputedTurnConfig {
            max_tool_loops: vtcode_core::config::constants::defaults::DEFAULT_MAX_TOOL_LOOPS,
            tool_repeat_limit:
                vtcode_core::config::constants::defaults::DEFAULT_MAX_REPEATED_TOOL_CALLS,
            max_session_turns: 150,
            ask_questions_enabled: true,
        })
}

#[allow(clippy::too_many_arguments)]
pub async fn run_turn_loop(
    _input: &str,
    mut working_history: Vec<uni::Message>,
    ctx: TurnLoopContext<'_>,
    session_end_reason: &mut crate::hooks::lifecycle::SessionEndReason,
) -> Result<TurnLoopOutcome> {
    use crate::agent::runloop::unified::context_manager::PreRequestAction;
    use crate::agent::runloop::unified::turn::context::{
        TurnHandlerOutcome, TurnProcessingContext, TurnProcessingResult,
    };
    use crate::agent::runloop::unified::turn::guards::run_proactive_guards;
    use crate::agent::runloop::unified::turn::turn_processing::{
        HandleTurnProcessingResultParams, execute_llm_request, handle_turn_processing_result,
        maybe_force_plan_mode_interview, process_llm_response,
    };
    use vtcode_core::llm::provider as uni;

    // Initialize the outcome result
    let mut result = TurnLoopResult::Completed;
    let mut turn_modified_files = BTreeSet::new();
    *ctx.auto_exit_plan_mode_attempted = false;

    ctx.harness_state.set_phase(TurnPhase::Preparing);
    if let Some(emitter) = ctx.harness_emitter {
        let _ = emitter.emit(turn_started_event());
    }

    // NOTE: The user input is already in working_history from the caller (session_loop or run_loop)
    // Do NOT add it again here, as it will cause duplicate messages in the conversation

    // Optimization: Extract all frequently accessed config values once
    let turn_config = extract_turn_config(ctx.vt_cfg);

    let mut step_count = 0;
    let mut current_max_tool_loops = turn_config.max_tool_loops;
    // Optimization: Interned signatures with exponential backoff for loop detection
    let mut repeated_tool_attempts =
        crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker::new();

    // Reset safety validator for a new turn
    {
        let max_session_turns = if ctx.session_stats.is_plan_mode() {
            usize::MAX
        } else {
            turn_config.max_session_turns
        };
        let mut validator = ctx.safety_validator.write().await;
        validator.set_limits(current_max_tool_loops, max_session_turns);
        validator.start_turn().await;
    }

    loop {
        step_count += 1;
        ctx.telemetry.record_turn();

        // Check session boundaries
        let context_window_size = ctx
            .provider_client
            .effective_context_size(&ctx.config.model);
        match ctx
            .context_manager
            .pre_request_check(&working_history, context_window_size)
        {
            PreRequestAction::Stop(msg) => {
                crate::agent::runloop::unified::turn::turn_helpers::display_error(
                    ctx.renderer,
                    "Session Limit Reached",
                    &anyhow::anyhow!("{}", msg),
                )?;
                result = TurnLoopResult::Aborted; // Or completed?
                *session_end_reason = crate::hooks::lifecycle::SessionEndReason::Error;
                break;
            }
            PreRequestAction::Warn(msg) => {
                crate::agent::runloop::unified::turn::turn_helpers::display_status(
                    ctx.renderer,
                    &format!("Warning: {}", msg),
                )?;
                let alert = format!("SYSTEM ALERT: {}", msg);
                let duplicate_alert = working_history.last().is_some_and(|last| {
                    last.role == uni::MessageRole::System
                        && last.content.as_text_borrowed() == Some(alert.as_str())
                });
                if !duplicate_alert {
                    working_history.push(uni::Message::system(alert));
                }
            }
            PreRequestAction::Compact(msg) => {
                crate::agent::runloop::unified::turn::turn_helpers::display_status(
                    ctx.renderer,
                    &msg,
                )?;
                let compacted = ctx
                    .context_manager
                    .compact_history_if_needed(
                        &working_history,
                        ctx.provider_client.as_ref(),
                        &ctx.config.model,
                    )
                    .await?;
                working_history = compacted;
            }
            PreRequestAction::Proceed => {}
        }

        // Proactive: In Plan mode, if the last user message signals readiness (e.g., "start implement"),
        // trigger exit_plan_mode immediately to show the confirmation modal, bypassing LLM guesswork.
        if ctx.session_stats.is_plan_mode()
            && let Some(last_user_msg) = working_history
                .iter()
                .rev()
                .find(|msg| msg.role == uni::MessageRole::User)
        {
            // Normalize to lower, strip punctuation so paths or extra symbols don't block detection
            // Optimization: Limit to first 500 characters as trigger phrases are usually at the start/end
            let text = last_user_msg.content.as_text();
            let normalized = text
                .chars()
                .take(500)
                .map(|c| {
                    if c.is_alphanumeric() {
                        c.to_ascii_lowercase()
                    } else if c.is_whitespace() {
                        ' '
                    } else {
                        ' '
                    }
                })
                .collect::<String>();

            let trigger_phrases = [
                "start implement",
                "start implementation",
                "start implementing",
                "implement now",
                "begin implement",
                "begin implementation",
                "begin coding",
                "proceed to implement",
                "proceed with implementation",
                "proceed to coding",
                "proceed with coding",
                "let s implement",
                "lets implement",
                "go ahead and implement",
                "go ahead and code",
                "ready to implement",
                "start coding",
                "start building",
                "switch to agent mode",
                "exit plan mode",
                "exit plan mode and implement",
            ];
            let should_exit_plan = trigger_phrases
                .iter()
                .any(|phrase| normalized.contains(phrase));

            if should_exit_plan {
                use crate::agent::runloop::unified::run_loop_context::RunLoopContext;
                use crate::agent::runloop::unified::tool_pipeline::run_tool_call;
                use vtcode_core::llm::provider as uni;

                let mut run_ctx = RunLoopContext {
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
                    safety_validator: Some(ctx.safety_validator),
                    traj: ctx.traj,
                    harness_state: ctx.harness_state,
                    harness_emitter: ctx.harness_emitter,
                };

                // Build a synthetic tool call for exit_plan_mode
                let args = serde_json::json!({
                    "reason": "user_requested_implementation"
                });
                let call = uni::ToolCall::function(
                    format!("call_{}_exit_plan_mode", step_count),
                    tool_names::EXIT_PLAN_MODE.to_string(),
                    serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string()),
                );

                let outcome = run_tool_call(
                    &mut run_ctx,
                    &call,
                    ctx.ctrl_c_state,
                    ctx.ctrl_c_notify,
                    ctx.default_placeholder.clone(),
                    ctx.lifecycle_hooks,
                    true,
                    ctx.vt_cfg,
                    step_count,
                    false,
                )
                .await;

                match outcome {
                    Ok(_pipe_outcome) => {
                        // The tool pipeline handles showing the confirmation modal and
                        // toggling plan/edit modes based on user choice. End this turn.
                        result = TurnLoopResult::Completed;
                        break;
                    }
                    Err(err) => {
                        crate::agent::runloop::unified::turn::turn_helpers::display_error(
                            ctx.renderer,
                            "Failed to exit Plan Mode",
                            &err,
                        )?;
                        // Fall through to normal LLM processing if proactive exit failed
                    }
                }
            }
        }

        // Check if we've reached the maximum number of tool loops
        // Note: step_count starts at 1 (incremented at loop start), so use >= for correct limit enforcement
        if step_count >= current_max_tool_loops {
            crate::agent::runloop::unified::turn::turn_helpers::display_status(
                ctx.renderer,
                &format!("Reached maximum tool loops ({})", current_max_tool_loops),
            )?;

            // Prompt user to continue with more tool loops
            match crate::agent::runloop::unified::tool_routing::prompt_tool_loop_limit_increase(
                ctx.handle,
                ctx.session,
                ctx.ctrl_c_state,
                ctx.ctrl_c_notify,
                current_max_tool_loops,
            )
            .await
            {
                Ok(Some(increment)) => {
                    let previous_max_tool_loops = current_max_tool_loops;
                    current_max_tool_loops = current_max_tool_loops.saturating_add(increment);
                    // Update the safety validator with the new turn limit while preserving the session limit
                    {
                        let mut validator = ctx.safety_validator.write().await;
                        // Get the current session limit to preserve it
                        let current_session_limit = validator.get_session_limit();
                        validator.set_limits(current_max_tool_loops, current_session_limit);
                        tracing::info!(
                            "Updated safety validator limits: turn={} (was {}), session={}",
                            current_max_tool_loops,
                            previous_max_tool_loops,
                            current_session_limit
                        );
                    }
                    crate::agent::runloop::unified::turn::turn_helpers::display_status(
                        ctx.renderer,
                        &format!("Tool loop limit increased to {}", current_max_tool_loops),
                    )?;
                    continue; // Continue the loop with the new limit
                }
                _ => {
                    // User denied or cancelled - end the turn normally
                    break;
                }
            }
        }

        // Clone validation cache arc to avoid borrow conflict
        let validation_cache = ctx.session_stats.validation_cache.clone();

        // Capture input status state for potential restoration after LLM response
        // (needed because turn_processing_ctx will mutably borrow input_status_state)
        let restore_status_left = ctx.input_status_state.left.clone();
        let restore_status_right = ctx.input_status_state.right.clone();

        // Prepare turn processing context
        let mut turn_processing_ctx = TurnProcessingContext {
            renderer: ctx.renderer,
            handle: ctx.handle,
            session_stats: ctx.session_stats,
            auto_exit_plan_mode_attempted: ctx.auto_exit_plan_mode_attempted,
            mcp_panel_state: ctx.mcp_panel_state,
            tool_result_cache: ctx.tool_result_cache,
            approval_recorder: ctx.approval_recorder,
            decision_ledger: ctx.decision_ledger,
            working_history: &mut working_history,
            tool_registry: ctx.tool_registry,
            tools: ctx.tools,
            tool_catalog: ctx.tool_catalog,
            ctrl_c_state: ctx.ctrl_c_state,
            ctrl_c_notify: ctx.ctrl_c_notify,
            vt_cfg: ctx.vt_cfg,
            context_manager: ctx.context_manager,
            last_forced_redraw: ctx.last_forced_redraw,
            input_status_state: ctx.input_status_state,
            session: ctx.session,
            lifecycle_hooks: ctx.lifecycle_hooks,
            default_placeholder: ctx.default_placeholder,
            tool_permission_cache: ctx.tool_permission_cache,
            safety_validator: ctx.safety_validator,
            provider_client: ctx.provider_client,
            config: ctx.config,
            traj: ctx.traj,
            full_auto: ctx.full_auto,
            circuit_breaker: ctx.circuit_breaker,
            tool_health_tracker: ctx.tool_health_tracker,
            rate_limiter: ctx.rate_limiter,
            telemetry: ctx.telemetry,
            autonomous_executor: ctx.autonomous_executor,
            error_recovery: ctx.error_recovery,
            harness_state: ctx.harness_state,
            harness_emitter: ctx.harness_emitter,
        };

        // === PROACTIVE GUARDS (HP-2: Pre-request checks) ===
        run_proactive_guards(&mut turn_processing_ctx, step_count).await?;

        // Execute the LLM request
        turn_processing_ctx
            .harness_state
            .set_phase(TurnPhase::Requesting);
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
                // Restore input status on request failure to clear loading/shimmer state.
                ctx.handle
                    .set_input_status(restore_status_left.clone(), restore_status_right.clone());
                ctx.input_status_state.left = restore_status_left.clone();
                ctx.input_status_state.right = restore_status_right.clone();
                crate::agent::runloop::unified::turn::turn_helpers::display_error(
                    ctx.renderer,
                    "LLM request failed",
                    &err,
                )?;
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

        if turn_processing_ctx.session_stats.is_plan_mode() {
            turn_processing_ctx
                .session_stats
                .increment_plan_mode_turns();
        }

        // Process the LLM response
        let allow_plan_interview = turn_processing_ctx.session_stats.is_plan_mode()
            && turn_config.ask_questions_enabled
            && crate::agent::runloop::unified::turn::turn_processing::plan_mode_interview_ready(
                turn_processing_ctx.session_stats,
            );
        let mut processing_result = process_llm_response(
            &response,
            turn_processing_ctx.renderer,
            turn_processing_ctx.working_history.len(),
            allow_plan_interview,
            turn_config.ask_questions_enabled,
            Some(&validation_cache),
            Some(turn_processing_ctx.tool_registry),
        )?;
        if turn_processing_ctx.session_stats.is_plan_mode() && turn_config.ask_questions_enabled {
            processing_result = maybe_force_plan_mode_interview(
                processing_result,
                response.content.as_deref(),
                turn_processing_ctx.session_stats,
                turn_processing_ctx.working_history.len(),
            );
        }

        // Restore input status if there are no tool calls (turn is completing)
        // This handles the case where defer_restore was set but no tool spinners will take over
        let has_tool_calls = matches!(processing_result, TurnProcessingResult::ToolCalls { .. });
        if !has_tool_calls {
            // Restore the input status bar to its original state
            ctx.handle
                .set_input_status(restore_status_left.clone(), restore_status_right.clone());
        }

        if has_tool_calls {
            turn_processing_ctx
                .harness_state
                .set_phase(TurnPhase::ExecutingTools);
        } else {
            turn_processing_ctx
                .harness_state
                .set_phase(TurnPhase::Finalizing);
        }

        // Handle the turn processing result (dispatch tool calls or finish turn)
        match handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut turn_processing_ctx,
            processing_result,
            response_streamed,
            step_count,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            session_end_reason,
            max_tool_loops: current_max_tool_loops,
            tool_repeat_limit: turn_config.tool_repeat_limit,
        })
        .await?
        {
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

    ctx.harness_state.set_phase(TurnPhase::Finalizing);
    if let Some(emitter) = ctx.harness_emitter {
        let event = match result {
            TurnLoopResult::Completed => Some(turn_completed_event()),
            TurnLoopResult::Aborted => Some(turn_failed_event("turn aborted")),
            TurnLoopResult::Cancelled => Some(turn_failed_event("turn cancelled")),
            TurnLoopResult::Exit => Some(turn_failed_event("turn exit")),
            TurnLoopResult::Blocked { .. } => Some(turn_failed_event("turn blocked")),
        };
        if let Some(event) = event {
            let _ = emitter.emit(event);
        }
    }

    // Final outcome with the correct result status
    Ok(TurnLoopOutcome {
        result,
        working_history,
        turn_modified_files,
    })
}
