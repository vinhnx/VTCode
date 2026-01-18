use anyhow::Result;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::RwLock;

#[allow(unused_imports)]
use crate::agent::runloop::unified::progress::ProgressReporter;
#[allow(unused_imports)]
use crate::agent::runloop::unified::state::CtrlCState;
use crate::agent::runloop::unified::tool_call_safety::ToolCallSafetyValidator;
use crate::agent::runloop::unified::turn::context::TurnLoopResult;
#[allow(unused_imports)]
use crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner;
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

#[allow(dead_code)]
pub enum LlmHandleOutcome {
    Success,
    Failure,
    Cancelled,
}

#[allow(dead_code)]
pub enum TurnResultKind {
    Completed,
    Cancelled,
    Failed,
}

// Note: the module references are kept similar to original file; compiler will resolve them.

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
    pub mcp_panel_state: &'a mut mcp_events::McpPanelState,
    pub tool_result_cache: &'a Arc<RwLock<ToolResultCache>>,
    pub approval_recorder: &'a Arc<ApprovalRecorder>,
    pub decision_ledger: &'a Arc<RwLock<DecisionTracker>>,
    pub tool_registry: &'a mut ToolRegistry,
    pub tools: &'a Arc<RwLock<Vec<uni::ToolDefinition>>>,
    /// Cached tool definitions for efficient reuse (HP-3 optimization)
    pub cached_tools: &'a Option<Arc<Vec<uni::ToolDefinition>>>,
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
}

// For `TurnLoopContext`, we will reuse the generic `handle_pipeline_output` via an adapter below.

#[allow(clippy::too_many_arguments)]
pub async fn run_turn_loop(
    _input: &str,
    mut working_history: Vec<uni::Message>,
    mut ctx: TurnLoopContext<'_>,
    config: &AgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
    provider_client: &mut Box<dyn uni::LLMProvider>,
    traj: &TrajectoryLogger,
    _skip_confirmations: bool,
    full_auto: bool,
    session_end_reason: &mut crate::hooks::lifecycle::SessionEndReason,
) -> Result<TurnLoopOutcome> {
    use crate::agent::runloop::unified::turn::context::{
        TurnHandlerOutcome, TurnProcessingContext,
    };
    use crate::agent::runloop::unified::turn::guards::run_proactive_guards;
    use crate::agent::runloop::unified::turn::turn_processing::{
        HandleTurnProcessingResultParams, execute_llm_request, handle_turn_processing_result,
        process_llm_response,
    };
    use vtcode_core::llm::provider as uni;
    use crate::agent::runloop::unified::context_manager::PreRequestAction;

    // Initialize the outcome result
    let mut result = TurnLoopResult::Completed;
    let mut turn_modified_files = BTreeSet::new();

    // NOTE: The user input is already in working_history from the caller (session_loop or run_loop)
    // Do NOT add it again here, as it will cause duplicate messages in the conversation

    // Process up to max_tool_loops iterations to handle tool calls
    let max_tool_loops = vt_cfg
        .map(|cfg| cfg.tools.max_tool_loops)
        .filter(|&value| value > 0)
        .unwrap_or(vtcode_core::config::constants::defaults::DEFAULT_MAX_TOOL_LOOPS);

    let mut step_count = 0;
    let mut current_max_tool_loops = max_tool_loops;
    // Optimization: Pre-allocate HashMap with expected capacity to reduce rehashing
    let mut repeated_tool_attempts: HashMap<String, usize> = HashMap::with_capacity(16);

    // Reset safety validator for a new turn
    {
        let max_session_turns = vt_cfg
            .map(|c| c.agent.max_conversation_turns)
            .unwrap_or(150);
        let mut validator = ctx.safety_validator.write().await;
        validator.set_limits(max_tool_loops, max_session_turns);
        validator.start_turn();
    }

    // Optimization: Pre-compute tool repeat limit once
    let tool_repeat_limit = vt_cfg
        .map(|cfg| cfg.tools.max_repeated_tool_calls)
        .filter(|&value| value > 0)
        .unwrap_or(vtcode_core::config::constants::defaults::DEFAULT_MAX_REPEATED_TOOL_CALLS);

    loop {
        step_count += 1;
        ctx.telemetry.record_turn();

        // Check session boundaries
        match ctx.context_manager.pre_request_check(&working_history) {
            PreRequestAction::Stop(msg) => {
                 crate::agent::runloop::unified::turn::turn_helpers::display_error(
                    ctx.renderer,
                    "Session Limit Reached",
                    &anyhow::anyhow!("{}", msg)
                 )?;
                 result = TurnLoopResult::Aborted; // Or completed?
                 *session_end_reason = crate::hooks::lifecycle::SessionEndReason::Error; 
                 break;
            }
            PreRequestAction::Warn(msg) => {
                 crate::agent::runloop::unified::turn::turn_helpers::display_status(
                    ctx.renderer,
                    &format!("Warning: {}", msg)
                 )?;
                 // Inject warning into history?
                 working_history.push(uni::Message::system(format!("SYSTEM ALERT: {}", msg)));
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
            let normalized = last_user_msg
                .content
                .as_text()
                .to_lowercase()
                .chars()
                .map(|c| {
                    if c.is_alphanumeric() || c.is_whitespace() {
                        c
                    } else {
                        ' '
                    }
                })
                .collect::<String>();

            let trigger_phrases = [
                "start implement",
                "start implementation",
                "start implementing",
                "begin implement",
                "begin implementation",
                "proceed to implement",
                "proceed with implementation",
                "let s implement",
                "lets implement",
                "go ahead and implement",
                "ready to implement",
                "exit plan mode",
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
                    traj,
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
                    vt_cfg,
                    step_count,
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
        if step_count > current_max_tool_loops {
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

        // Prepare turn processing context
        let mut turn_processing_ctx = TurnProcessingContext::new(
            &mut ctx,
            &mut working_history,
            provider_client,
            vt_cfg,
            full_auto,
        );

        // === PROACTIVE GUARDS (HP-2: Pre-request checks) ===
        // === PROACTIVE GUARDS (HP-2: Pre-request checks) ===
        run_proactive_guards(&mut turn_processing_ctx, step_count).await?;

        // Execute the LLM request
        let (response, response_streamed) = match execute_llm_request(
            &mut turn_processing_ctx,
            step_count,
            &config.model,
            None, // max_tokens_opt
            None, // parallel_cfg_opt
        )
        .await
        {
            Ok(val) => val,
            Err(err) => {
                crate::agent::runloop::unified::turn::turn_helpers::display_error(
                    ctx.renderer,
                    "LLM request failed",
                    &err,
                )?;
                working_history.push(uni::Message::assistant(format!("Request failed: {}", err)));
                result = TurnLoopResult::Aborted;
                break;
            }
        };

        // Process the LLM response
        let processing_result = process_llm_response(
            &response,
            turn_processing_ctx.renderer,
            turn_processing_ctx.working_history.len(),
            Some(&validation_cache),
        )?;

        // Handle the turn processing result (dispatch tool calls or finish turn)
        match handle_turn_processing_result(HandleTurnProcessingResultParams {
            ctx: &mut turn_processing_ctx,
            processing_result,
            response_streamed,
            step_count,
            repeated_tool_attempts: &mut repeated_tool_attempts,
            turn_modified_files: &mut turn_modified_files,
            traj,
            session_end_reason,
            max_tool_loops,
            tool_repeat_limit,
        })
        .await?
        {
            TurnHandlerOutcome::Continue => {
                continue;
            }
            TurnHandlerOutcome::Break(outcome_result) => {
                result = outcome_result;
                break;
            }
        }
    }

    // Final outcome with the correct result status
    Ok(TurnLoopOutcome {
        result,
        working_history,
        turn_modified_files,
    })
}
