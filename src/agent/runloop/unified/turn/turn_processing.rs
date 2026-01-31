use anyhow::Result;
#[cfg(debug_assertions)]
use std::time::Instant;
use tokio::task;
#[cfg(debug_assertions)]
use tracing::debug;

use crate::agent::runloop::unified::extract_action_from_messages;
use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext, TurnProcessingResult,
};
use crate::agent::runloop::unified::turn::guards::{
    handle_turn_balancer, validate_tool_args_security,
};
use crate::agent::runloop::unified::turn::tool_outcomes::{HandleTextResponseParams, ToolOutcomeContext};
use crate::agent::runloop::unified::ui_interaction::stream_and_render_response;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::llm::provider::{self as uni, ParallelToolConfig};
use vtcode_core::prompts::sort_tool_definitions;

use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_core::utils::ansi::MessageStyle;

/// Execute an LLM request and return the response
pub(crate) async fn execute_llm_request(
    ctx: &mut TurnProcessingContext<'_>,
    step_count: usize,
    active_model: &str,
    _max_tokens_opt: Option<u32>,
    parallel_cfg_opt: Option<ParallelToolConfig>,
) -> Result<(uni::LLMResponse, bool)> {
    let provider_client = ctx.provider_client.as_ref();
    // Context trim and compaction has been removed - no pruning or enforcement needed
    // HP-1: Eliminate unnecessary clone - work directly on working_history

    let plan_mode = ctx.session_stats.is_plan_mode();
    let context_window_size = provider_client.effective_context_size(active_model);

    // Get active agent info for system prompt injection
    let active_agent_name = ctx.session_stats.active_agent();
    // Fetch the active agent's system prompt body from built-in definitions
    let active_agent_prompt_body = vtcode_core::subagents::get_agent_prompt_body(active_agent_name);

    let system_prompt = ctx
        .context_manager
        .build_system_prompt(
            ctx.working_history,
            step_count,
            crate::agent::runloop::unified::context_manager::SystemPromptParams {
                full_auto: ctx.full_auto,
                plan_mode,
                context_window_size: Some(context_window_size),
                active_agent_name: Some(active_agent_name.to_string()),
                active_agent_prompt: active_agent_prompt_body,
            },
        )
        .await?;

    let use_streaming = provider_client.supports_streaming();
    let reasoning_effort = ctx.vt_cfg.as_ref().and_then(|cfg| {
        if provider_client.supports_reasoning_effort(active_model) {
            Some(cfg.agent.reasoning_effort)
        } else {
            None
        }
    });
    let provider_name = provider_client.name();
    let temperature =
        if reasoning_effort.is_some() && matches!(provider_name, "anthropic" | "minimax") {
            None
        } else {
            Some(0.7)
        };

    // HP-3: Use cached tools instead of acquiring lock and cloning
    let current_tools = ctx
        .cached_tools
        .as_ref()
        .map(|arc| sort_tool_definitions((**arc).clone()));
    let has_tools = current_tools.is_some();
    let parallel_config =
        if has_tools && provider_client.supports_parallel_tool_config(active_model) {
            parallel_cfg_opt.clone()
        } else {
            None
        };
    let tool_choice = if has_tools {
        Some(uni::ToolChoice::auto())
    } else {
        None
    };
    let request = uni::LLMRequest {
        // HP-1: Single clone only when building LLMRequest
        messages: ctx.working_history.to_vec(),
        system_prompt: Some(system_prompt),
        tools: current_tools,
        model: active_model.to_string(),
        temperature,
        stream: use_streaming,
        tool_choice,
        parallel_tool_config: parallel_config,
        reasoning_effort,
        ..Default::default()
    };

    // Extract action suggestion once outside the loop
    let action_suggestion = extract_action_from_messages(ctx.working_history);

    // Validate request once before entering loop
    if let Err(err) = provider_client.validate_request(&request) {
        return Err(anyhow::Error::new(err));
    }

    const MAX_RETRIES: usize = 3;

    let mut llm_result = Err(anyhow::anyhow!("LLM request failed to execute"));

    #[cfg(debug_assertions)]
    let mut request_timer = Instant::now(); // Declare outside loop

    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            use crate::agent::runloop::unified::turn::turn_helpers::calculate_backoff;
            let delay = calculate_backoff(attempt - 1, 500, 10_000);
            let delay_secs = delay.as_secs_f64();
            crate::agent::runloop::unified::turn::turn_helpers::display_status(
                ctx.renderer,
                &format!(
                    "LLM request failed, retrying in {:.1}s... (attempt {}/{})",
                    delay_secs,
                    attempt + 1,
                    MAX_RETRIES
                ),
            )?;
            tokio::time::sleep(delay).await;
        }

        // Create a new spinner for each attempt to ensuring it's active
        let spinner_msg = if attempt > 0 {
            let action = action_suggestion.clone();
            if action.is_empty() {
                format!("Retrying request (attempt {}/{})", attempt + 1, MAX_RETRIES)
            } else {
                format!("{} (Retry {}/{})", action, attempt + 1, MAX_RETRIES)
            }
        } else {
            action_suggestion.clone()
        };

        use crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner;
        let _spinner = PlaceholderSpinner::new(
            ctx.handle,
            ctx.input_status_state.left.clone(),
            ctx.input_status_state.right.clone(),
            spinner_msg,
        );
        task::yield_now().await;

        #[cfg(debug_assertions)]
        {
            request_timer = Instant::now(); // Re-assign inside loop for each attempt
            let tool_count = request.tools.as_ref().map_or(0, |tools| tools.len());
            debug!(
                target = "vtcode::agent::llm",
                model = %request.model,
                streaming = use_streaming,
                step = step_count,
                messages = request.messages.len(),
                tools = tool_count,
                attempt = attempt + 1,
                "Dispatching provider request"
            );
        }

        let step_result = if use_streaming {
            let stream_result = stream_and_render_response(
                provider_client,
                request.clone(),
                &_spinner,
                ctx.renderer,
                ctx.ctrl_c_state,
                ctx.ctrl_c_notify,
            )
            .await;

            match stream_result {
                Ok((response, emitted)) => Ok((response, emitted)),
                Err(ref err) => {
                    // Only retry streaming errors if we haven't emitted any tokens yet (best effort)
                    let msg = err.to_string();
                    let is_retryable = msg.contains("rate limit")
                        || msg.contains("timeout")
                        || msg.contains("500")
                        || msg.contains("502")
                        || msg.contains("503")
                        || msg.contains("service unavailable")
                        || msg.contains("connection")
                        || msg.contains("network");

                    if is_retryable {
                        Err(anyhow::anyhow!(msg)) // Return err to retry loop
                    } else {
                        Err(anyhow::anyhow!(msg))
                    }
                }
            }
        } else {
            let provider_name = provider_client.name().to_string();

            if ctx.ctrl_c_state.is_cancel_requested() || ctx.ctrl_c_state.is_exit_requested() {
                Err(anyhow::Error::new(uni::LLMError::Provider {
                    message: vtcode_core::llm::error_display::format_llm_error(
                        &provider_name,
                        "Interrupted by user",
                    ),
                    metadata: None,
                }))
            } else {
                let generate_future = provider_client.generate(request.clone());
                tokio::pin!(generate_future);
                let cancel_notifier = ctx.ctrl_c_notify.notified();
                tokio::pin!(cancel_notifier);

                let outcome = tokio::select! {
                    res = &mut generate_future => {
                        match res {
                            Ok(resp) => Ok((resp, false)),
                            Err(err) => Err(anyhow::Error::new(err))
                        }
                    }
                    _ = &mut cancel_notifier => {
                        Err(anyhow::Error::new(uni::LLMError::Provider {
                            message: vtcode_core::llm::error_display::format_llm_error(
                                &provider_name,
                                "Interrupted by user",
                            ),
                            metadata: None,
                        }))
                    }
                };
                outcome
            }
        };

        // Log outcome
        #[cfg(debug_assertions)]
        {
            debug!(
                target = "vtcode::agent::llm",
                model = %active_model,
                streaming = use_streaming,
                step = step_count,
                elapsed_ms = request_timer.elapsed().as_millis(),
                succeeded = step_result.is_ok(),
                attempt = attempt + 1,
                "Provider request finished"
            );
        }

        match step_result {
            Ok(val) => {
                llm_result = Ok(val);
                _spinner.finish();
                break;
            }
            Err(err) => {
                let msg = err.to_string();
                let is_retryable = msg.contains("rate limit")
                    || msg.contains("timeout")
                    || msg.contains("500")
                    || msg.contains("502")
                    || msg.contains("503")
                    || msg.contains("service unavailable")
                    || msg.contains("connection")
                    || msg.contains("network");

                // Special check for user interruption - never retry
                if !crate::agent::runloop::unified::turn::turn_helpers::should_continue_operation(
                    ctx.ctrl_c_state,
                ) {
                    llm_result = Err(err);
                    _spinner.finish();
                    break;
                }

                if is_retryable && attempt < MAX_RETRIES - 1 {
                    _spinner.finish(); // Clean up spinner before retrying
                    // Continue to next attempt
                    continue;
                }

                llm_result = Err(err);
                _spinner.finish();
                break;
            }
        }
    }

    // Finalize response

    #[cfg(debug_assertions)]
    {
        debug!(
            target = "vtcode::agent::llm",
            model = %active_model,
            streaming = use_streaming,
            step = step_count,
            elapsed_ms = request_timer.elapsed().as_millis(),
            succeeded = llm_result.is_ok(),
            "Provider request finished"
        );
    }

    // Check if result is error, if so return it. _spinner is already finished inside loop.
    let (response, response_streamed) = match llm_result {
        Ok(result) => result,
        Err(error) => {
            return Err(error);
        }
    };
    // HP-1: No restoration needed - working_history was never modified
    Ok((response, response_streamed))
}

// Use `strip_harmony_syntax` and `derive_recent_tool_output` helpers from other modules

/// Result of processing a single turn
pub(crate) struct HandleTurnProcessingResultParams<'a> {
    pub ctx: &'a mut TurnProcessingContext<'a>,
    pub processing_result: TurnProcessingResult,
    pub response_streamed: bool,
    pub step_count: usize,
    pub repeated_tool_attempts: &'a mut HashMap<String, usize>,
    pub turn_modified_files: &'a mut BTreeSet<PathBuf>,
    pub traj: &'a TrajectoryLogger,
    pub session_end_reason: &'a mut crate::hooks::lifecycle::SessionEndReason,
    /// Pre-computed max tool loops limit for efficiency
    pub max_tool_loops: usize,
    /// Pre-computed tool repeat limit for efficiency
    pub tool_repeat_limit: usize,
}

/// Dispatch the appropriate response handler based on the processing result.
pub(crate) async fn handle_turn_processing_result<'a>(
    params: HandleTurnProcessingResultParams<'a>,
) -> Result<TurnHandlerOutcome> {
    match params.processing_result {
        TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text,
            reasoning,
        } => {
            crate::agent::runloop::unified::turn::tool_outcomes::handle_assistant_response(
                &mut *params.ctx,
                assistant_text,
                reasoning,
                params.response_streamed,
            )?;

            let outcome = {
                let mut t_ctx_inner = ToolOutcomeContext {
                    ctx: &mut *params.ctx,
                    repeated_tool_attempts: &mut *params.repeated_tool_attempts,
                    turn_modified_files: &mut *params.turn_modified_files,
                    traj: params.traj,
                };

                crate::agent::runloop::unified::turn::tool_outcomes::handle_tool_calls(
                    &mut t_ctx_inner,
                    &tool_calls,
                )
                .await?
            };

            if let Some(res) = outcome {
                return Ok(res);
            }

            // Call balancer and return directly to release borrows
            return Ok(handle_turn_balancer(
                &mut *params.ctx,
                params.step_count,
                &mut *params.repeated_tool_attempts,
                params.max_tool_loops,
                params.tool_repeat_limit,
            )
            .await);
        }
        TurnProcessingResult::TextResponse { text, reasoning } => {
            return crate::agent::runloop::unified::turn::tool_outcomes::handle_text_response(
                HandleTextResponseParams {
                    ctx: &mut *params.ctx,
                    repeated_tool_attempts: &mut *params.repeated_tool_attempts,
                    turn_modified_files: &mut *params.turn_modified_files,
                    traj: params.traj,
                    text: text.clone(),
                    reasoning: reasoning.clone(),
                    response_streamed: params.response_streamed,
                    step_count: params.step_count,
                    session_end_reason: &mut *params.session_end_reason,
                    max_tool_loops: params.max_tool_loops,
                    tool_repeat_limit: params.tool_repeat_limit,
                },
            )
            .await;
        }
        TurnProcessingResult::Empty | TurnProcessingResult::Completed => {
            return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed));
        }
        TurnProcessingResult::Cancelled => {
            *params.session_end_reason = crate::hooks::lifecycle::SessionEndReason::Cancelled;
            return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Cancelled));
        }
        TurnProcessingResult::Aborted => {
            return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Aborted));
        }
    }
}

/// Process an LLM response and return a `TurnProcessingResult` describing whether
/// there are tool calls to run, a textual assistant response, or nothing.
pub(crate) fn process_llm_response(
    response: &vtcode_core::llm::provider::LLMResponse,
    renderer: &mut AnsiRenderer,
    conversation_len: usize,
    validation_cache: Option<
        &std::sync::Arc<vtcode_core::tools::validation_cache::ValidationCache>,
    >,
) -> Result<TurnProcessingResult> {
    use crate::agent::runloop::unified::turn::harmony::strip_harmony_syntax;
    use vtcode_core::llm::provider as uni;

    let mut final_text = response.content.clone();
    let mut tool_calls = response.tool_calls.clone().unwrap_or_default();
    let mut interpreted_textual_call = false;
    let mut is_harmony = false;

    // Strip harmony syntax from displayed content if present
    if let Some(ref text) = final_text
        && (text.contains("<|start|>") || text.contains("<|channel|>") || text.contains("<|call|>"))
    {
        is_harmony = true;
        let cleaned = strip_harmony_syntax(text);
        if !cleaned.trim().is_empty() {
            final_text = Some(cleaned);
        } else {
            final_text = Some("".to_string());
        }
    }

    if tool_calls.is_empty()
        && let Some(text) = final_text.clone()
        && !text.trim().is_empty()
        && let Some((name, args)) =
            crate::agent::runloop::text_tools::detect_textual_tool_call(&text)
    {
        // Validate required arguments and security before adding the tool call.
        // This prevents executing tools with empty args or security violations.
        if let Some(validation_failures) =
            validate_tool_args_security(&name, &args, validation_cache)
        {
            // Show warning about validation failures but don't add the tool call.
            // This allows the model to continue naturally instead of failing execution.
            let tool_display =
                crate::agent::runloop::unified::tool_summary::humanize_tool_name(&name);
            let failures_list = validation_failures.join("; ");
            crate::agent::runloop::unified::turn::turn_helpers::display_status(
                renderer,
                &format!(
                    "Detected {} but validation failed: {}",
                    tool_display, failures_list
                ),
            )?;
            // Don't set interpreted_textual_call = true, let it fall through to TextResponse
        } else {
            let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
            let code_blocks = crate::agent::runloop::text_tools::extract_code_fence_blocks(&text);
            if !code_blocks.is_empty() {
                crate::agent::runloop::tool_output::render_code_fence_blocks(
                    renderer,
                    &code_blocks,
                )?;
                renderer.line(MessageStyle::Output, "")?;
            }
            let (headline, _) =
                crate::agent::runloop::unified::tool_summary::describe_tool_action(&name, &args);
            let notice = if headline.is_empty() {
                format!(
                    "Detected {} request",
                    crate::agent::runloop::unified::tool_summary::humanize_tool_name(&name)
                )
            } else {
                format!("Detected {headline}")
            };
            crate::agent::runloop::unified::turn::turn_helpers::display_status(renderer, &notice)?;
            let call_id = format!("call_textual_{}", conversation_len);
            tool_calls.push(uni::ToolCall::function(
                call_id.clone(),
                name.clone(),
                args_json.clone(),
            ));
            interpreted_textual_call = true;
            final_text = None;
        }
    }

    // Build result
    if !tool_calls.is_empty() {
        return Ok(TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text: if interpreted_textual_call {
                String::new()
            } else {
                final_text.clone().unwrap_or_default()
            },
            reasoning: response.reasoning.clone(),
        });
    }

    if let Some(text) = final_text
        && (!text.trim().is_empty() || is_harmony)
    {
        return Ok(TurnProcessingResult::TextResponse {
            text,
            reasoning: response.reasoning.clone(),
        });
    }

    Ok(TurnProcessingResult::Empty)
}
