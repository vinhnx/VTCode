use anyhow::Result;
use std::time::Instant;
use tokio::task;
use tracing::debug;

use crate::agent::runloop::unified::extract_action_from_messages;
use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext, TurnProcessingResult,
};
use crate::agent::runloop::unified::turn::guards::{
    handle_turn_balancer, validate_required_tool_args,
};
use crate::agent::runloop::unified::ui_interaction::stream_and_render_response;
use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use vtcode_core::core::trajectory::TrajectoryLogger;
use vtcode_core::llm::provider::{self as uni, ParallelToolConfig};
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_core::utils::ansi::MessageStyle;

/// Execute an LLM request and return the response
pub(crate) async fn execute_llm_request(
    ctx: &mut TurnProcessingContext<'_>,
    step_count: usize,
    active_model: &str,
    max_tokens_opt: Option<u32>,
    parallel_cfg_opt: Option<ParallelToolConfig>,
) -> Result<(uni::LLMResponse, bool)> {
    let provider_client = ctx.provider_client.as_ref();
    // Apply semantic pruning with decision tracking if configured
    if ctx.context_manager.trim_config().semantic_compression {
        let mut pruning_ledger_mut = ctx.pruning_ledger.write().await;
        ctx.context_manager.prune_with_semantic_priority(
            ctx.working_history,
            Some(&mut *pruning_ledger_mut),
            step_count,
        );
    }

    // HP-9: Lazy context window enforcement - only when needed
    if ctx
        .context_manager
        .should_enforce_context(ctx.working_history)
    {
        let _ = ctx
            .context_manager
            .enforce_context_window(ctx.working_history);
    }
    // HP-1: Eliminate unnecessary clone - work directly on working_history

    let system_prompt = ctx
        .context_manager
        .build_system_prompt(ctx.working_history, step_count, ctx.full_auto)
        .await?;

    let use_streaming = provider_client.supports_streaming();
    let reasoning_effort = ctx.vt_cfg.as_ref().and_then(|cfg| {
        if provider_client.supports_reasoning_effort(active_model) {
            Some(cfg.agent.reasoning_effort)
        } else {
            None
        }
    });

    // HP-3: Use cached tools instead of acquiring lock and cloning
    let current_tools = ctx.cached_tools.as_ref().map(|arc| (**arc).clone());
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
        max_tokens: max_tokens_opt.or(Some(2000)),
        temperature: Some(0.7),
        stream: use_streaming,
        tool_choice,
        parallel_tool_config: parallel_config,
        reasoning_effort,
        ..Default::default()
    };

    let action_suggestion = extract_action_from_messages(ctx.working_history);
    use crate::agent::runloop::unified::ui_interaction::PlaceholderSpinner;
    let _spinner = PlaceholderSpinner::new(
        ctx.handle,
        ctx.input_status_state.left.clone(),
        ctx.input_status_state.right.clone(),
        action_suggestion,
    );
    task::yield_now().await;

    #[cfg(debug_assertions)]
    let request_timer = Instant::now();
    #[cfg(debug_assertions)]
    {
        let tool_count = request.tools.as_ref().map_or(0, |tools| tools.len());
        debug!(
            target = "vtcode::agent::llm",
            model = %request.model,
            streaming = use_streaming,
            step = step_count,
            messages = request.messages.len(),
            tools = tool_count,
            "Dispatching provider request"
        );
    }

    if let Err(err) = provider_client.validate_request(&request) {
        _spinner.finish();
        return Err(anyhow::Error::new(err));
    }

    let llm_result = if use_streaming {
        stream_and_render_response(
            provider_client,
            request,
            &_spinner,
            ctx.renderer,
            ctx.ctrl_c_state,
            ctx.ctrl_c_notify,
        )
        .await
    } else {
        let provider_name = provider_client.name().to_string();

        if ctx.ctrl_c_state.is_cancel_requested() || ctx.ctrl_c_state.is_exit_requested() {
            _spinner.finish();
            Err(uni::LLMError::Provider {
                message: vtcode_core::llm::error_display::format_llm_error(
                    &provider_name,
                    "Interrupted by user",
                ),
                metadata: None,
            })
        } else {
            let generate_future = provider_client.generate(request);
            tokio::pin!(generate_future);
            let cancel_notifier = ctx.ctrl_c_notify.notified();
            tokio::pin!(cancel_notifier);
            let outcome = tokio::select! {
                res = &mut generate_future => {
                    _spinner.finish();
                    res.map(|resp| (resp, false))
                }
                _ = &mut cancel_notifier => {
                    _spinner.finish();
                    Err(uni::LLMError::Provider {
                        message: vtcode_core::llm::error_display::format_llm_error(
                            &provider_name,
                            "Interrupted by user",
                        ),
                        metadata: None,
                    })
                }
            };
            outcome
        }
    };

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

    let (response, response_streamed) = match llm_result {
        Ok(result) => result,
        Err(error) => {
            // Finish spinner before returning error to remove it from transcript
            _spinner.finish();
            return Err(anyhow::Error::new(error));
        }
    };
    // HP-1: No restoration needed - working_history was never modified
    Ok((response, response_streamed))
}

// Use `strip_harmony_syntax` and `derive_recent_tool_output` helpers from other modules

// NOTE: is_giving_up_reasoning, get_constructive_reasoning, and is_thinking_only_content
// are now imported from crate::agent::runloop::unified::reasoning

/// Result of processing a single turn
/// Dispatch the appropriate response handler based on the processing result.
pub(crate) async fn handle_turn_processing_result(
    ctx: &mut TurnProcessingContext<'_>,
    processing_result: TurnProcessingResult,
    response_streamed: bool,
    step_count: usize,
    repeated_tool_attempts: &mut HashMap<String, usize>,
    turn_modified_files: &mut BTreeSet<PathBuf>,
    traj: &TrajectoryLogger,
    session_end_reason: &mut crate::hooks::lifecycle::SessionEndReason,
) -> Result<TurnHandlerOutcome> {
    match processing_result {
        TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text,
            reasoning,
        } => {
            crate::agent::runloop::unified::turn::tool_outcomes::handle_assistant_response(
                ctx,
                assistant_text,
                reasoning,
                response_streamed,
            )?;

            for tool_call in &tool_calls {
                if let Some(outcome) =
                    crate::agent::runloop::unified::turn::tool_outcomes::handle_tool_call(
                        ctx,
                        tool_call,
                        repeated_tool_attempts,
                        turn_modified_files,
                        traj,
                    )
                    .await?
                {
                    return Ok(outcome);
                }
            }
            // Fall through to balancer
        }
        TurnProcessingResult::TextResponse { text, reasoning } => {
            return crate::agent::runloop::unified::turn::tool_outcomes::handle_text_response(
                ctx,
                text,
                reasoning,
                response_streamed,
                step_count,
                repeated_tool_attempts,
                turn_modified_files,
                traj,
                session_end_reason,
            )
            .await;
        }
        TurnProcessingResult::Empty | TurnProcessingResult::Completed => {
            return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed));
        }
        TurnProcessingResult::Cancelled => {
            *session_end_reason = crate::hooks::lifecycle::SessionEndReason::Cancelled;
            return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Cancelled));
        }
        TurnProcessingResult::Aborted => {
            return Ok(TurnHandlerOutcome::Break(TurnLoopResult::Aborted));
        }
    };

    Ok(handle_turn_balancer(ctx, step_count, repeated_tool_attempts).await)
}

/// Process an LLM response and return a `TurnProcessingResult` describing whether
/// there are tool calls to run, a textual assistant response, or nothing.
pub(crate) fn process_llm_response(
    response: &vtcode_core::llm::provider::LLMResponse,
    renderer: &mut AnsiRenderer,
    conversation_len: usize,
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
        // Validate required arguments before adding the tool call.
        // This prevents executing tools with empty args that will fail and trigger loop detection.
        if let Some(missing_params) = validate_required_tool_args(&name, &args) {
            // Show warning about missing parameters but don't add the tool call.
            // This allows the model to continue naturally instead of failing execution.
            let tool_display =
                crate::agent::runloop::unified::tool_summary::humanize_tool_name(&name);
            let missing_list = missing_params.join(", ");
            renderer.line(
                MessageStyle::Info,
                &format!(
                    "Detected {} but missing required params: {}",
                    tool_display, missing_list
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
            renderer.line(MessageStyle::Info, &notice)?;
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
        let assistant_text = if interpreted_textual_call {
            String::new()
        } else {
            final_text.clone().unwrap_or_default()
        };
        return Ok(TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text,
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
