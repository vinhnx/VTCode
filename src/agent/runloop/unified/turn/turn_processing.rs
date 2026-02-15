use anyhow::Result;
use std::fmt::Write as _;
#[cfg(debug_assertions)]
use std::time::Instant;
use tokio::task;
#[cfg(debug_assertions)]
use tracing::debug;

use crate::agent::runloop::unified::extract_action_from_messages;
use crate::agent::runloop::unified::plan_blocks::extract_proposed_plan;
use crate::agent::runloop::unified::turn::context::{
    TurnHandlerOutcome, TurnLoopResult, TurnProcessingContext, TurnProcessingResult,
};
use crate::agent::runloop::unified::turn::guards::{
    handle_turn_balancer, validate_tool_args_security,
};
use crate::agent::runloop::unified::turn::tool_outcomes::ToolOutcomeContext;
use std::collections::BTreeSet;
use std::path::PathBuf;
use vtcode_config::constants::defaults::{DEFAULT_MAX_CONVERSATION_TURNS, DEFAULT_MAX_TOOL_LOOPS};
use vtcode_config::context::default_max_context_tokens;
use vtcode_core::llm::provider::{self as uni, ParallelToolConfig};
use vtcode_core::llm::providers::split_reasoning_from_text;
use vtcode_core::prompts::upsert_harness_limits_section;
use vtcode_core::turn_metadata;

use std::sync::Arc;
use std::sync::Mutex;
use vtcode_core::utils::ansi::AnsiRenderer;
use vtcode_core::utils::ansi::MessageStyle;

struct UnsafeSendContext {
    renderer: usize,
    handle: usize,
}
unsafe impl Send for UnsafeSendContext {}
unsafe impl Sync for UnsafeSendContext {}

fn is_retryable_llm_error(message: &str) -> bool {
    let msg = message.to_ascii_lowercase();
    [
        "rate limit",
        "timeout",
        "internal server error",
        "500",
        "502",
        "503",
        "service unavailable",
        "connection",
        "network",
    ]
    .iter()
    .any(|needle| msg.contains(needle))
}

/// Execute an LLM request and return the response
pub(crate) async fn execute_llm_request(
    ctx: &mut TurnProcessingContext<'_>,
    step_count: usize,
    active_model: &str,
    _max_tokens_opt: Option<u32>,
    parallel_cfg_opt: Option<Box<ParallelToolConfig>>,
) -> Result<(uni::LLMResponse, bool)> {
    let provider_name = ctx.provider_client.name().to_string();
    // Context trim and compaction has been removed - no pruning or enforcement needed
    // HP-1: Eliminate unnecessary clone - work directly on working_history

    let plan_mode = ctx.session_stats.is_plan_mode();
    let context_window_size = ctx.provider_client.effective_context_size(active_model);

    // Get active agent info for system prompt injection
    let active_agent_name = ctx.session_stats.active_agent();
    // Fetch the active agent's system prompt body from built-in definitions
    let active_agent_prompt_body = vtcode_core::subagents::get_agent_prompt_body(active_agent_name);

    let mut system_prompt = ctx
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

    // Keep prompt guidance aligned with runtime harness enforcement limits.
    upsert_harness_limits_section(
        &mut system_prompt,
        ctx.harness_state.max_tool_calls,
        ctx.harness_state.max_tool_wall_clock.as_secs(),
        ctx.harness_state.max_tool_retries,
    );

    // HP-6: Cache provider capabilities to avoid repeated trait method calls (2-3 calls → 1 per turn)
    let capabilities = uni::get_cached_capabilities(&**ctx.provider_client, active_model);

    let use_streaming = capabilities.streaming;
    let reasoning_effort = ctx.vt_cfg.as_ref().and_then(|cfg| {
        if capabilities.reasoning_effort {
            Some(cfg.agent.reasoning_effort)
        } else {
            None
        }
    });
    let temperature = if reasoning_effort.is_some()
        && matches!(provider_name.as_str(), "anthropic" | "minimax")
    {
        None
    } else {
        Some(0.7)
    };

    // HP-3: Use a versioned tool-catalog snapshot to avoid stale tool definitions.
    let current_tools = ctx.tool_catalog.sorted_snapshot(ctx.tools).await;
    let has_tools = current_tools.is_some();
    if let Some(defs) = current_tools.as_ref() {
        let _ = writeln!(
            system_prompt,
            "\n[Runtime Tool Catalog]\n- version: {}\n- available_tools: {}",
            ctx.tool_catalog.current_version(),
            defs.len()
        );
    }
    let parallel_config = if has_tools && capabilities.parallel_tool_config {
        parallel_cfg_opt.clone()
    } else {
        None
    };
    let tool_choice = if has_tools {
        Some(uni::ToolChoice::auto())
    } else {
        None
    };

    // Build turn metadata with git context for the LLM request
    let metadata = turn_metadata::build_turn_metadata_value(&ctx.config.workspace).ok();

    let request = uni::LLMRequest {
        // HP-1: Single clone only when building LLMRequest
        messages: ctx.working_history.to_vec(),
        system_prompt: Some(std::sync::Arc::new(system_prompt)),
        tools: current_tools,
        model: active_model.to_string(),
        temperature,
        stream: use_streaming,
        tool_choice,
        parallel_tool_config: parallel_config,
        reasoning_effort,
        metadata,
        ..Default::default()
    };

    // Extract action suggestion once outside the loop
    let action_suggestion = extract_action_from_messages(ctx.working_history);

    // Validate request once before entering loop
    if let Err(err) = ctx.provider_client.as_ref().validate_request(&request) {
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
        // If tools are available, set defer_restore so status bar isn't restored immediately.
        // The turn loop will restore it after processing the response if there are no tool calls.
        if has_tools {
            _spinner.set_defer_restore(true);
        }
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
            // Defer spinner finish if tools are available - keeps loading indicator active

            let state = ::vtcode_core::core::agent::session::AgentSessionState::new(
                "chat_session".to_string(),
                DEFAULT_MAX_CONVERSATION_TURNS,
                DEFAULT_MAX_TOOL_LOOPS,
                default_max_context_tokens(),
            );

            let mut controller =
                ::vtcode_core::core::agent::session::controller::AgentSessionController::new(
                    state, None, None,
                );

            // Bridge events to renderer
            // Use a Send-safe pointer wrapper since we're using unsafe anyway
            let send_ctx = UnsafeSendContext {
                renderer: ctx.renderer as *mut ::vtcode_core::utils::ansi::AnsiRenderer as usize,
                handle: ctx.handle as *const ::vtcode_core::ui::tui::InlineHandle
                    as *mut ::vtcode_core::ui::tui::InlineHandle as usize,
            };

            let event_sink = Arc::new(Mutex::new(Box::new(
                move |event: ::vtcode_core::core::agent::events::AgentEvent| unsafe {
                    use ::vtcode_core::core::agent::events::AgentEvent;
                    match event {
                        AgentEvent::OutputDelta { delta } => {
                            let r: &mut ::vtcode_core::utils::ansi::AnsiRenderer = &mut *(send_ctx
                                .renderer
                                as *mut ::vtcode_core::utils::ansi::AnsiRenderer);
                            let _ = r.render_token_delta(&delta);
                        }
                        AgentEvent::ThinkingDelta { delta } => {
                            let r: &mut ::vtcode_core::utils::ansi::AnsiRenderer = &mut *(send_ctx
                                .renderer
                                as *mut ::vtcode_core::utils::ansi::AnsiRenderer);
                            let _ = r.render_reasoning_delta(&delta);
                        }
                        AgentEvent::ThinkingStage { stage } => {
                            let h: &mut ::vtcode_core::ui::tui::InlineHandle = &mut *(send_ctx
                                .handle
                                as *mut ::vtcode_core::ui::tui::InlineHandle);
                            h.set_input_status(Some(stage), None);
                        }
                        _ => {}
                    }
                },
            )
                as Box<dyn FnMut(::vtcode_core::core::agent::events::AgentEvent) + Send>));

            controller.set_event_handler(event_sink);

            let mut steering = None;
            let res: Result<(uni::LLMResponse, String, Option<String>)> = controller
                .run_turn(
                    ctx.provider_client,
                    request.clone(),
                    &mut steering,
                    Some(std::time::Duration::from_secs(60)),
                )
                .await;

            match res {
                Ok((response, _content, _reasoning)) => Ok((response, true)),
                Err(err) => Err(anyhow::anyhow!(err.to_string())),
            }
        } else {
            if ctx.ctrl_c_state.is_cancel_requested() || ctx.ctrl_c_state.is_exit_requested() {
                Err(anyhow::Error::new(uni::LLMError::Provider {
                    message: vtcode_core::llm::error_display::format_llm_error(
                        &provider_name,
                        "Interrupted by user",
                    ),
                    metadata: None,
                }))
            } else {
                let generate_future = ctx.provider_client.generate(request.clone());
                tokio::pin!(generate_future);
                let cancel_notifier = ctx.ctrl_c_notify.notified();
                tokio::pin!(cancel_notifier);

                let outcome = tokio::select! {
                    res = &mut generate_future => res.map_err(anyhow::Error::from),
                    _ = &mut cancel_notifier => {
                        Err(anyhow::Error::from(uni::LLMError::Provider {
                            message: vtcode_core::llm::error_display::format_llm_error(
                                &provider_name,
                                "Interrupted by user",
                            ),
                            metadata: None,
                        }))
                    }
                };

                match outcome {
                    Ok(response) => Ok((response, false)),
                    Err(err) => Err(err),
                }
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
                let is_retryable = is_retryable_llm_error(&msg);

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
    pub repeated_tool_attempts:
        &'a mut crate::agent::runloop::unified::turn::tool_outcomes::helpers::LoopTracker,
    pub turn_modified_files: &'a mut BTreeSet<PathBuf>,
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
            params.ctx.handle_assistant_response(
                assistant_text,
                reasoning,
                params.response_streamed,
            )?;

            let outcome = {
                let mut t_ctx_inner = ToolOutcomeContext {
                    ctx: &mut *params.ctx,
                    repeated_tool_attempts: &mut *params.repeated_tool_attempts,
                    turn_modified_files: &mut *params.turn_modified_files,
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
        TurnProcessingResult::TextResponse {
            text,
            reasoning,
            proposed_plan,
        } => {
            params
                .ctx
                .handle_text_response(
                    text.clone(),
                    reasoning.clone(),
                    proposed_plan.clone(),
                    params.response_streamed,
                )
                .await
        }
        TurnProcessingResult::Empty | TurnProcessingResult::Completed => {
            Ok(TurnHandlerOutcome::Break(TurnLoopResult::Completed))
        }
        TurnProcessingResult::Cancelled => {
            *params.session_end_reason = crate::hooks::lifecycle::SessionEndReason::Cancelled;
            Ok(TurnHandlerOutcome::Break(TurnLoopResult::Cancelled))
        }
        TurnProcessingResult::Aborted => Ok(TurnHandlerOutcome::Break(TurnLoopResult::Aborted)),
    }
}

/// Process an LLM response and return a `TurnProcessingResult` describing whether
/// there are tool calls to run, a textual assistant response, or nothing.
pub(crate) fn process_llm_response(
    response: &vtcode_core::llm::provider::LLMResponse,
    renderer: &mut AnsiRenderer,
    conversation_len: usize,
    plan_mode_active: bool,
    allow_plan_interview: bool,
    ask_questions_enabled: bool,
    validation_cache: Option<
        &std::sync::Arc<vtcode_core::tools::validation_cache::ValidationCache>,
    >,
    tool_registry: Option<&vtcode_core::tools::ToolRegistry>,
) -> Result<TurnProcessingResult> {
    use crate::agent::runloop::unified::turn::harmony::strip_harmony_syntax;
    use vtcode_core::config::constants::tools;
    use vtcode_core::llm::provider as uni;

    let mut final_text = response.content.clone();
    let mut proposed_plan: Option<String> = None;
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

    if plan_mode_active
        && tool_calls.is_empty()
        && let Some(ref text) = final_text
    {
        let extraction = extract_proposed_plan(text);
        final_text = Some(extraction.stripped_text);
        proposed_plan = extraction.plan_text;
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
            validate_tool_args_security(&name, &args, validation_cache, tool_registry)
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

    if !interpreted_textual_call
        && allow_plan_interview
        && ask_questions_enabled
        && tool_calls.is_empty()
        && let Some(text) = final_text.clone()
        && let Some(args) = build_interview_args_from_text(&text)
    {
        let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
        let call_id = format!("call_interview_{}", conversation_len);
        tool_calls.push(uni::ToolCall::function(
            call_id.clone(),
            tools::ASK_QUESTIONS.to_string(),
            args_json,
        ));
        interpreted_textual_call = true;
        final_text = None;
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
            reasoning: split_reasoning_from_text(response.reasoning.as_deref().unwrap_or("")).0,
        });
    }

    if let Some(text) = final_text
        && (!text.trim().is_empty() || is_harmony || proposed_plan.is_some())
    {
        return Ok(TurnProcessingResult::TextResponse {
            text,
            reasoning: split_reasoning_from_text(response.reasoning.as_deref().unwrap_or("")).0,
            proposed_plan,
        });
    }

    Ok(TurnProcessingResult::Empty)
}

fn build_interview_args_from_text(text: &str) -> Option<serde_json::Value> {
    let questions = extract_interview_questions(text);
    if questions.is_empty() {
        return None;
    }

    let payload = questions
        .iter()
        .enumerate()
        .map(|(index, question)| {
            serde_json::json!({
                "id": format!("question_{}", index + 1),
                "header": format!("Q{}", index + 1),
                "question": question,
            })
        })
        .collect::<Vec<_>>();

    Some(serde_json::json!({ "questions": payload }))
}

pub(crate) fn extract_interview_questions(text: &str) -> Vec<String> {
    let mut questions = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(question) = parse_numbered_question(trimmed) {
            questions.push(question);
            continue;
        }
        if let Some(question) = parse_bullet_question(trimmed) {
            questions.push(question);
        }
    }

    if questions.is_empty() {
        let trimmed = text.trim();
        let normalized = normalize_question_line(trimmed);
        if !normalized.is_empty() && normalized.contains('?') && normalized.len() <= 200 {
            questions.push(normalized);
        }
    }

    questions.truncate(3);
    questions
}

fn parse_numbered_question(line: &str) -> Option<String> {
    let mut digits_len = 0usize;
    for ch in line.chars() {
        if ch.is_ascii_digit() {
            digits_len += ch.len_utf8();
        } else {
            break;
        }
    }
    if digits_len == 0 {
        return None;
    }

    let rest = line[digits_len..].trim_start();
    let mut chars = rest.chars();
    let punct = chars.next()?;
    if punct != '.' && punct != ')' {
        return None;
    }
    let remainder = chars.as_str().trim_start();
    let normalized = normalize_question_line(remainder);
    if normalized.contains('?') {
        Some(normalized)
    } else {
        None
    }
}

fn parse_bullet_question(line: &str) -> Option<String> {
    for prefix in ["- ", "* ", "• "] {
        if let Some(stripped) = line.strip_prefix(prefix) {
            let candidate = normalize_question_line(stripped.trim());
            if candidate.contains('?') {
                return Some(candidate);
            }
        }
    }
    None
}

fn normalize_question_line(line: &str) -> String {
    let mut current = line.trim();

    if let Some(stripped) = current.strip_prefix('>') {
        current = stripped.trim_start();
    }

    let mut changed = true;
    while changed {
        changed = false;
        if let Some(stripped) = strip_wrapping(current, "**", "**") {
            current = stripped;
            changed = true;
        } else if let Some(stripped) = strip_wrapping(current, "__", "__") {
            current = stripped;
            changed = true;
        } else if let Some(stripped) = strip_wrapping(current, "`", "`") {
            current = stripped;
            changed = true;
        } else if let Some(stripped) = strip_wrapping(current, "*", "*") {
            current = stripped;
            changed = true;
        } else if let Some(stripped) = strip_wrapping(current, "_", "_") {
            current = stripped;
            changed = true;
        } else if let Some(stripped) = strip_wrapping(current, "\"", "\"") {
            current = stripped;
            changed = true;
        } else if let Some(stripped) = strip_wrapping(current, "'", "'") {
            current = stripped;
            changed = true;
        }
    }

    current.trim().to_string()
}

fn strip_wrapping<'a>(line: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    if line.len() <= prefix.len() + suffix.len() {
        return None;
    }
    if !line.starts_with(prefix) || !line.ends_with(suffix) {
        return None;
    }
    Some(line[prefix.len()..line.len() - suffix.len()].trim())
}

const MIN_PLAN_MODE_TURNS_BEFORE_INTERVIEW: usize = 1;
const PLAN_MODE_REMINDER: &str = vtcode_core::prompts::system::PLAN_MODE_IMPLEMENT_REMINDER;

fn has_discovery_tool(session_stats: &crate::agent::runloop::unified::state::SessionStats) -> bool {
    use vtcode_core::config::constants::tools;

    [
        tools::READ_FILE,
        tools::LIST_FILES,
        tools::GREP_FILE,
        tools::UNIFIED_SEARCH,
        tools::CODE_INTELLIGENCE,
        tools::SPAWN_SUBAGENT,
    ]
    .iter()
    .any(|tool| session_stats.has_tool(tool))
}

pub(crate) fn plan_mode_interview_ready(
    session_stats: &crate::agent::runloop::unified::state::SessionStats,
) -> bool {
    has_discovery_tool(session_stats)
        && session_stats.plan_mode_turns() >= MIN_PLAN_MODE_TURNS_BEFORE_INTERVIEW
}

fn strip_assistant_text(processing_result: TurnProcessingResult) -> TurnProcessingResult {
    match processing_result {
        TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text: _,
            reasoning,
        } => TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text: String::new(),
            reasoning,
        },
        TurnProcessingResult::TextResponse { .. } => TurnProcessingResult::Empty,
        TurnProcessingResult::Empty
        | TurnProcessingResult::Completed
        | TurnProcessingResult::Cancelled
        | TurnProcessingResult::Aborted => processing_result,
    }
}

fn append_plan_mode_reminder_text(text: &str) -> String {
    if text.contains(PLAN_MODE_REMINDER) || text.trim().is_empty() {
        return text.to_string();
    }

    let separator = if text.ends_with('\n') { "\n" } else { "\n\n" };
    format!("{text}{separator}{PLAN_MODE_REMINDER}")
}

fn maybe_append_plan_mode_reminder(
    processing_result: TurnProcessingResult,
) -> TurnProcessingResult {
    match processing_result {
        TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text,
            reasoning,
        } => TurnProcessingResult::ToolCalls {
            tool_calls,
            assistant_text: append_plan_mode_reminder_text(&assistant_text),
            reasoning,
        },
        TurnProcessingResult::TextResponse {
            text,
            reasoning,
            proposed_plan,
        } => {
            let reminder_text = if text.trim().is_empty() && proposed_plan.is_some() {
                PLAN_MODE_REMINDER.to_string()
            } else {
                append_plan_mode_reminder_text(&text)
            };
            TurnProcessingResult::TextResponse {
                text: reminder_text,
                reasoning,
                proposed_plan,
            }
        }
        TurnProcessingResult::Empty
        | TurnProcessingResult::Completed
        | TurnProcessingResult::Cancelled
        | TurnProcessingResult::Aborted => processing_result,
    }
}

pub(crate) fn maybe_force_plan_mode_interview(
    processing_result: TurnProcessingResult,
    response_text: Option<&str>,
    session_stats: &mut crate::agent::runloop::unified::state::SessionStats,
    conversation_len: usize,
) -> TurnProcessingResult {
    let allow_interview = plan_mode_interview_ready(session_stats);

    let response_has_plan = response_text
        .map(|text| text.contains("<proposed_plan>"))
        .unwrap_or(false);

    if response_has_plan {
        if !session_stats.plan_mode_interview_shown() && allow_interview {
            let stripped = strip_assistant_text(processing_result);
            return inject_plan_mode_interview(stripped, session_stats, conversation_len);
        }

        return maybe_append_plan_mode_reminder(processing_result);
    }

    let filter_outcome = filter_interview_tool_calls(
        processing_result,
        session_stats,
        allow_interview,
        response_has_plan,
    );
    let processing_result = filter_outcome.processing_result;
    let has_interview_tool_calls = filter_outcome.had_interview_tool_calls;
    let has_non_interview_tool_calls = filter_outcome.had_non_interview_tool_calls;

    if session_stats.plan_mode_interview_shown() {
        if has_interview_tool_calls {
            session_stats.mark_plan_mode_interview_shown();
        }
        return processing_result;
    }

    if session_stats.plan_mode_interview_pending() {
        if has_interview_tool_calls && allow_interview {
            session_stats.mark_plan_mode_interview_shown();
            return processing_result;
        }

        if has_non_interview_tool_calls {
            return processing_result;
        }

        if !allow_interview {
            return processing_result;
        }

        return inject_plan_mode_interview(processing_result, session_stats, conversation_len);
    }

    let explicit_questions = response_text
        .map(|text| !extract_interview_questions(text).is_empty())
        .unwrap_or(false);

    if explicit_questions {
        if allow_interview {
            session_stats.mark_plan_mode_interview_shown();
        }
        return processing_result;
    }

    if has_interview_tool_calls {
        if allow_interview {
            session_stats.mark_plan_mode_interview_shown();
        } else {
            session_stats.mark_plan_mode_interview_pending();
        }
        return processing_result;
    }

    if has_non_interview_tool_calls {
        session_stats.mark_plan_mode_interview_pending();
        return processing_result;
    }

    if !allow_interview {
        return processing_result;
    }

    inject_plan_mode_interview(processing_result, session_stats, conversation_len)
}

fn default_plan_mode_interview_args() -> serde_json::Value {
    serde_json::json!({
        "questions": [
            {
                "id": "goal",
                "header": "Goal",
                "question": "What outcome should this change deliver?"
            },
            {
                "id": "constraints",
                "header": "Constraints",
                "question": "Any constraints, preferences, or non-goals I should follow?"
            },
            {
                "id": "verification",
                "header": "Verification",
                "question": "How should we verify the result (tests or manual checks)?"
            }
        ]
    })
}

fn inject_plan_mode_interview(
    processing_result: TurnProcessingResult,
    session_stats: &mut crate::agent::runloop::unified::state::SessionStats,
    conversation_len: usize,
) -> TurnProcessingResult {
    use vtcode_core::config::constants::tools;

    let args = default_plan_mode_interview_args();
    let args_json = serde_json::to_string(&args).unwrap_or_else(|_| "{}".to_string());
    let call_id = format!("call_plan_interview_{}", conversation_len);
    let call = uni::ToolCall::function(call_id, tools::ASK_QUESTIONS.to_string(), args_json);

    session_stats.mark_plan_mode_interview_shown();

    match processing_result {
        TurnProcessingResult::ToolCalls {
            mut tool_calls,
            assistant_text,
            reasoning,
        } => {
            tool_calls.push(call);
            TurnProcessingResult::ToolCalls {
                tool_calls,
                assistant_text,
                reasoning,
            }
        }
        TurnProcessingResult::TextResponse {
            text,
            reasoning,
            proposed_plan: _,
        } => TurnProcessingResult::ToolCalls {
            tool_calls: vec![call],
            assistant_text: text,
            reasoning,
        },
        TurnProcessingResult::Empty | TurnProcessingResult::Completed => {
            TurnProcessingResult::ToolCalls {
                tool_calls: vec![call],
                assistant_text: String::new(),
                reasoning: Vec::new(),
            }
        }
        TurnProcessingResult::Cancelled | TurnProcessingResult::Aborted => processing_result,
    }
}

struct InterviewToolCallFilter {
    processing_result: TurnProcessingResult,
    had_interview_tool_calls: bool,
    had_non_interview_tool_calls: bool,
}

fn filter_interview_tool_calls(
    processing_result: TurnProcessingResult,
    session_stats: &mut crate::agent::runloop::unified::state::SessionStats,
    allow_interview: bool,
    response_has_plan: bool,
) -> InterviewToolCallFilter {
    use vtcode_core::config::constants::tools;

    let TurnProcessingResult::ToolCalls {
        tool_calls,
        assistant_text,
        reasoning,
    } = processing_result
    else {
        return InterviewToolCallFilter {
            processing_result,
            had_interview_tool_calls: false,
            had_non_interview_tool_calls: false,
        };
    };

    let mut had_interview = false;
    let mut had_non_interview = false;
    let mut filtered = Vec::with_capacity(tool_calls.len());

    for call in tool_calls {
        let is_interview = call
            .function
            .as_ref()
            .map(|func| {
                matches!(
                    func.name.as_str(),
                    tools::ASK_QUESTIONS | tools::REQUEST_USER_INPUT
                )
            })
            .unwrap_or(false);

        if is_interview {
            had_interview = true;
            if allow_interview && !response_has_plan {
                filtered.push(call);
            }
        } else {
            had_non_interview = true;
            filtered.push(call);
        }
    }

    if had_interview && (had_non_interview || !allow_interview) && !response_has_plan {
        session_stats.mark_plan_mode_interview_pending();
    }

    let processing_result = if filtered.is_empty() {
        if assistant_text.trim().is_empty() {
            TurnProcessingResult::ToolCalls {
                tool_calls: Vec::new(),
                assistant_text,
                reasoning,
            }
        } else {
            TurnProcessingResult::TextResponse {
                text: assistant_text,
                reasoning,
                proposed_plan: None,
            }
        }
    } else {
        TurnProcessingResult::ToolCalls {
            tool_calls: filtered,
            assistant_text,
            reasoning,
        }
    };

    InterviewToolCallFilter {
        processing_result,
        had_interview_tool_calls: had_interview,
        had_non_interview_tool_calls: had_non_interview,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::runloop::unified::state::SessionStats;
    use vtcode_core::config::constants::tools;
    use vtcode_core::llm::provider::{FinishReason, LLMResponse};

    #[test]
    fn retryable_llm_error_includes_internal_server_error_message() {
        assert!(is_retryable_llm_error(
            "Provider error: Internal Server Error"
        ));
    }

    #[test]
    fn retryable_llm_error_excludes_non_transient_messages() {
        assert!(!is_retryable_llm_error("Provider error: Invalid API key"));
    }

    #[test]
    fn extract_interview_questions_from_numbered_lines() {
        let text = "1. First question?\n2) Second question?\n3. Third question?";
        let questions = extract_interview_questions(text);
        assert_eq!(questions.len(), 3);
        assert_eq!(questions[0], "First question?");
        assert_eq!(questions[1], "Second question?");
        assert_eq!(questions[2], "Third question?");
    }

    #[test]
    fn extract_interview_questions_from_bullets() {
        let text = "- Should we do X?\n- Should we do Y?";
        let questions = extract_interview_questions(text);
        assert_eq!(questions.len(), 2);
        assert_eq!(questions[0], "Should we do X?");
    }

    #[test]
    fn process_llm_response_turns_questions_into_tool_call() {
        let response = LLMResponse {
            content: Some("1. First question?\n2. Second question?".to_string()),
            tool_calls: None,
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result =
            process_llm_response(&response, &mut renderer, 0, false, true, true, None, None)
                .expect("processing should succeed");

        match result {
            TurnProcessingResult::ToolCalls { tool_calls, .. } => {
                assert_eq!(tool_calls.len(), 1);
            }
            _ => panic!("Expected tool calls"),
        }
    }

    #[test]
    fn process_llm_response_skips_questions_when_interview_not_ready() {
        let response = LLMResponse {
            content: Some("1. First question?\n2. Second question?".to_string()),
            tool_calls: None,
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result =
            process_llm_response(&response, &mut renderer, 0, false, false, true, None, None)
                .expect("processing should succeed");

        match result {
            TurnProcessingResult::TextResponse { text, .. } => {
                assert!(text.contains("First question"));
            }
            _ => panic!("Expected text response without tool calls"),
        }
    }

    #[test]
    fn process_llm_response_strips_proposed_plan_in_plan_mode() {
        let response = LLMResponse {
            content: Some("Intro\n<proposed_plan>\n- Step 1\n</proposed_plan>\nOutro".to_string()),
            tool_calls: None,
            model: "test".to_string(),
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            tool_references: Vec::new(),
            request_id: None,
            organization_id: None,
        };

        let mut renderer = AnsiRenderer::stdout();
        let result =
            process_llm_response(&response, &mut renderer, 0, true, false, true, None, None)
                .expect("processing should succeed");

        match result {
            TurnProcessingResult::TextResponse {
                text,
                proposed_plan,
                ..
            } => {
                assert_eq!(text, "Intro\n\nOutro");
                assert_eq!(proposed_plan.as_deref(), Some("- Step 1"));
            }
            _ => panic!("Expected stripped text response with proposed plan"),
        }
    }

    #[test]
    fn extract_interview_questions_strips_markdown_wrapping() {
        let text = "**How should we proceed?**";
        let questions = extract_interview_questions(text);
        assert_eq!(questions, vec!["How should we proceed?".to_string()]);
    }

    #[test]
    fn extract_interview_questions_handles_bold_bullets() {
        let text = "- **Should we do X?**";
        let questions = extract_interview_questions(text);
        assert_eq!(questions, vec!["Should we do X?".to_string()]);
    }

    #[test]
    fn maybe_force_plan_mode_interview_inserts_tool_call() {
        let mut stats = SessionStats::default();
        let processing_result = TurnProcessingResult::TextResponse {
            text: "Proceeding without explicit questions.".to_string(),
            reasoning: None,
            proposed_plan: None,
        };

        stats.record_tool(tools::READ_FILE);
        stats.increment_plan_mode_turns();

        let result = maybe_force_plan_mode_interview(
            processing_result,
            Some("Proceeding without explicit questions."),
            &mut stats,
            1,
        );

        match result {
            TurnProcessingResult::ToolCalls {
                tool_calls,
                assistant_text,
                ..
            } => {
                assert_eq!(assistant_text, "Proceeding without explicit questions.");
                assert!(!tool_calls.is_empty());
                let name = tool_calls
                    .last()
                    .and_then(|call| call.function.as_ref())
                    .map(|func| func.name.as_str())
                    .unwrap_or("");
                assert_eq!(name, tools::ASK_QUESTIONS);
            }
            _ => panic!("Expected tool calls with forced interview"),
        }
    }

    #[test]
    fn maybe_force_plan_mode_interview_skips_when_questions_present() {
        let mut stats = SessionStats::default();
        let processing_result = TurnProcessingResult::TextResponse {
            text: "What should I do next?".to_string(),
            reasoning: None,
            proposed_plan: None,
        };

        stats.increment_plan_mode_turns();

        let result = maybe_force_plan_mode_interview(
            processing_result,
            Some("What should I do next?"),
            &mut stats,
            2,
        );

        match result {
            TurnProcessingResult::TextResponse { text, .. } => {
                assert_eq!(text, "What should I do next?");
                assert!(!stats.plan_mode_interview_shown());
                assert!(!stats.plan_mode_interview_pending());
            }
            _ => panic!("Expected text response without forced interview"),
        }
    }

    #[test]
    fn maybe_force_plan_mode_interview_marks_shown_when_plan_present() {
        let mut stats = SessionStats::default();
        let processing_result = TurnProcessingResult::TextResponse {
            text: "<proposed_plan>\nPlan content\n</proposed_plan>".to_string(),
            reasoning: None,
            proposed_plan: None,
        };

        stats.record_tool(tools::READ_FILE);
        stats.increment_plan_mode_turns();

        let result = maybe_force_plan_mode_interview(
            processing_result,
            Some("<proposed_plan>\nPlan content\n</proposed_plan>"),
            &mut stats,
            1,
        );

        match result {
            TurnProcessingResult::ToolCalls { tool_calls, .. } => {
                let name = tool_calls
                    .last()
                    .and_then(|call| call.function.as_ref())
                    .map(|func| func.name.as_str())
                    .unwrap_or("");
                assert_eq!(name, tools::ASK_QUESTIONS);
            }
            _ => panic!("Expected tool calls for plan interview"),
        }
    }

    #[test]
    fn maybe_force_plan_mode_interview_appends_reminder_when_plan_ready() {
        let mut stats = SessionStats::default();
        let processing_result = TurnProcessingResult::TextResponse {
            text: "<proposed_plan>\nPlan content\n</proposed_plan>".to_string(),
            reasoning: None,
            proposed_plan: None,
        };

        stats.record_tool(tools::READ_FILE);
        stats.increment_plan_mode_turns();
        stats.mark_plan_mode_interview_shown();

        let result = maybe_force_plan_mode_interview(
            processing_result,
            Some("<proposed_plan>\nPlan content\n</proposed_plan>"),
            &mut stats,
            2,
        );

        match result {
            TurnProcessingResult::TextResponse { text, .. } => {
                assert!(text.contains(super::PLAN_MODE_REMINDER));
            }
            _ => panic!("Expected text response with plan reminder"),
        }
    }

    #[test]
    fn maybe_force_plan_mode_interview_does_not_duplicate_reminder() {
        let mut stats = SessionStats::default();
        let text = format!(
            "<proposed_plan>\nPlan content\n</proposed_plan>\n\n{}",
            super::PLAN_MODE_REMINDER
        );
        let processing_result = TurnProcessingResult::TextResponse {
            text: text.clone(),
            reasoning: None,
            proposed_plan: None,
        };

        stats.record_tool(tools::READ_FILE);
        stats.increment_plan_mode_turns();
        stats.mark_plan_mode_interview_shown();

        let result = maybe_force_plan_mode_interview(processing_result, Some(&text), &mut stats, 3);

        match result {
            TurnProcessingResult::TextResponse { text, .. } => {
                assert_eq!(text.matches(super::PLAN_MODE_REMINDER).count(), 1);
            }
            _ => panic!("Expected text response with single reminder"),
        }
    }

    #[test]
    fn maybe_force_plan_mode_interview_defers_when_tool_calls_present() {
        let mut stats = SessionStats::default();
        stats.increment_plan_mode_turns();
        stats.increment_plan_mode_turns();

        let processing_result = TurnProcessingResult::ToolCalls {
            tool_calls: vec![uni::ToolCall::function(
                "call_read".to_string(),
                tools::READ_FILE.to_string(),
                "{}".to_string(),
            )],
            assistant_text: String::new(),
            reasoning: None,
        };

        let result = maybe_force_plan_mode_interview(
            processing_result,
            Some("Going to read files."),
            &mut stats,
            3,
        );

        match result {
            TurnProcessingResult::ToolCalls { tool_calls, .. } => {
                assert_eq!(tool_calls.len(), 1);
                assert!(stats.plan_mode_interview_pending());
            }
            _ => panic!("Expected tool calls to continue without interview"),
        }
    }

    #[test]
    fn maybe_force_plan_mode_interview_strips_interview_from_mixed_tool_calls() {
        let mut stats = SessionStats::default();
        stats.increment_plan_mode_turns();
        stats.increment_plan_mode_turns();
        stats.increment_plan_mode_turns();

        let processing_result = TurnProcessingResult::ToolCalls {
            tool_calls: vec![
                uni::ToolCall::function(
                    "call_read".to_string(),
                    tools::READ_FILE.to_string(),
                    "{}".to_string(),
                ),
                uni::ToolCall::function(
                    "call_interview".to_string(),
                    tools::ASK_QUESTIONS.to_string(),
                    "{}".to_string(),
                ),
            ],
            assistant_text: String::new(),
            reasoning: None,
        };

        let result = maybe_force_plan_mode_interview(
            processing_result,
            Some("Going to read files."),
            &mut stats,
            3,
        );

        match result {
            TurnProcessingResult::ToolCalls { tool_calls, .. } => {
                assert_eq!(tool_calls.len(), 1);
                let name = tool_calls
                    .first()
                    .and_then(|call| call.function.as_ref())
                    .map(|func| func.name.as_str())
                    .unwrap_or("");
                assert_eq!(name, tools::READ_FILE);
                assert!(stats.plan_mode_interview_pending());
            }
            _ => panic!("Expected tool calls with interview stripped"),
        }
    }

    #[test]
    fn upsert_harness_limits_adds_single_section() {
        let mut prompt = "Base prompt".to_string();

        upsert_harness_limits_section(&mut prompt, 12, 180, 2);

        assert_eq!(prompt.matches("[Harness Limits]").count(), 1);
        assert!(prompt.contains("- max_tool_calls_per_turn: 12"));
        assert!(prompt.contains("- max_tool_wall_clock_secs: 180"));
        assert!(prompt.contains("- max_tool_retries: 2"));
    }

    #[test]
    fn upsert_harness_limits_replaces_existing_values() {
        let mut prompt = "Base prompt\n[Harness Limits]\n- max_tool_calls_per_turn: 3\n- max_tool_wall_clock_secs: 60\n- max_tool_retries: 1\n".to_string();

        upsert_harness_limits_section(&mut prompt, 9, 240, 4);

        assert_eq!(prompt.matches("[Harness Limits]").count(), 1);
        assert!(prompt.contains("- max_tool_calls_per_turn: 9"));
        assert!(prompt.contains("- max_tool_wall_clock_secs: 240"));
        assert!(prompt.contains("- max_tool_retries: 4"));
        assert!(!prompt.contains("- max_tool_calls_per_turn: 3"));
    }

    #[test]
    fn upsert_harness_limits_preserves_trailing_prompt_sections() {
        let mut prompt = "Base prompt\n[Harness Limits]\n- max_tool_calls_per_turn: 3\n- max_tool_wall_clock_secs: 60\n- max_tool_retries: 1\n[Additional Context]\nKeep this section".to_string();

        upsert_harness_limits_section(&mut prompt, 11, 90, 3);

        assert_eq!(prompt.matches("[Harness Limits]").count(), 1);
        assert!(prompt.contains("[Additional Context]\nKeep this section"));
        assert!(prompt.ends_with("- max_tool_retries: 3\n"));
    }

    #[test]
    fn upsert_harness_limits_replaces_indented_section_header() {
        let mut prompt = "Base prompt\n  [Harness Limits]\n- max_tool_calls_per_turn: 1\n- max_tool_wall_clock_secs: 1\n- max_tool_retries: 1\n".to_string();

        upsert_harness_limits_section(&mut prompt, 5, 30, 2);

        assert_eq!(prompt.matches("[Harness Limits]").count(), 1);
        assert!(prompt.contains("- max_tool_calls_per_turn: 5"));
        assert!(!prompt.contains("- max_tool_calls_per_turn: 1"));
    }

    #[test]
    fn upsert_harness_limits_removes_duplicate_sections() {
        let mut prompt = "Base prompt\n[Harness Limits]\n- max_tool_calls_per_turn: 2\n- max_tool_wall_clock_secs: 10\n- max_tool_retries: 1\n[Other]\nkeep\n[Harness Limits]\n- max_tool_calls_per_turn: 3\n- max_tool_wall_clock_secs: 20\n- max_tool_retries: 2\n".to_string();

        upsert_harness_limits_section(&mut prompt, 7, 70, 3);

        assert_eq!(prompt.matches("[Harness Limits]").count(), 1);
        assert!(prompt.contains("- max_tool_calls_per_turn: 7"));
        assert!(prompt.contains("[Other]\nkeep"));
    }
}
