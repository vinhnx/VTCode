use anyhow::Result;
use std::fmt::Write as _;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;
#[cfg(debug_assertions)]
use std::time::Instant;
use tokio::task;
#[cfg(debug_assertions)]
use tracing::debug;
use vtcode_config::constants::defaults::{DEFAULT_MAX_CONVERSATION_TURNS, DEFAULT_MAX_TOOL_LOOPS};
use vtcode_config::context::default_max_context_tokens;
use vtcode_core::config::OpenAIPromptCacheKeyMode;
use vtcode_core::llm::provider::{self as uni, ParallelToolConfig};
use vtcode_core::prompts::upsert_harness_limits_section;
use vtcode_core::turn_metadata;

use crate::agent::runloop::unified::extract_action_from_messages;
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;

struct UnsafeSendContext {
    handle: usize,
}
unsafe impl Send for UnsafeSendContext {}
unsafe impl Sync for UnsafeSendContext {}

fn contains_any(message: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| message.contains(marker))
}

fn is_retryable_llm_error(message: &str) -> bool {
    let msg = message.to_ascii_lowercase();
    let non_retryable = [
        "invalid api key",
        "authentication failed",
        "unauthorized",
        "forbidden",
        "permission denied",
        "usage limit",
        "weekly usage limit",
        "daily usage limit",
        "monthly spending limit",
        "insufficient credits",
        "quota exceeded",
        "billing",
        "payment required",
        "bad request",
        "context length exceeded",
        "maximum context length",
        "token limit exceeded",
        "invalid request",
        "400",
        "401",
        "403",
        "404",
        "model not found",
        "endpoint not found",
    ];
    if contains_any(&msg, &non_retryable) {
        return false;
    }

    let retryable = [
        "rate limit",
        "too many requests",
        "timeout",
        "timed out",
        "429",
        "internal server error",
        "500",
        "502",
        "503",
        "504",
        "bad gateway",
        "gateway timeout",
        "service unavailable",
        "temporarily unavailable",
        "overloaded",
        "try again",
        "retry later",
        "connection reset",
        "connection refused",
        "connection",
        "socket hang up",
        "econnreset",
        "etimedout",
        "deadline exceeded",
        "network",
    ];
    contains_any(&msg, &retryable)
}

fn supports_streaming_timeout_fallback(provider_name: &str) -> bool {
    provider_name.eq_ignore_ascii_case("huggingface")
        || provider_name.eq_ignore_ascii_case("ollama")
}

fn is_stream_timeout_error(message: &str) -> bool {
    let msg = message.to_ascii_lowercase();
    msg.contains("stream request timed out")
        || msg.contains("streaming request timed out")
        || msg.contains("llm request timed out after")
}

fn llm_attempt_timeout_secs(turn_timeout_secs: u64, plan_mode: bool, provider_name: &str) -> u64 {
    let baseline = (turn_timeout_secs / 5).clamp(30, 120);
    if !plan_mode {
        return baseline;
    }

    // Plan Mode requests usually include heavier context and can need
    // extra first-token latency budget before retries are useful.
    let plan_mode_floor = if supports_streaming_timeout_fallback(provider_name) {
        90
    } else {
        60
    };
    let plan_mode_budget = (turn_timeout_secs / 2).clamp(plan_mode_floor, 120);
    baseline.max(plan_mode_budget)
}

const DEFAULT_LLM_RETRY_ATTEMPTS: usize = 3;
const MAX_LLM_RETRY_ATTEMPTS: usize = 6;

fn llm_retry_attempts(configured_task_retries: Option<u32>) -> usize {
    configured_task_retries
        .and_then(|value| usize::try_from(value).ok())
        .map(|value| value.saturating_add(1))
        .unwrap_or(DEFAULT_LLM_RETRY_ATTEMPTS)
        .clamp(1, MAX_LLM_RETRY_ATTEMPTS)
}

fn compact_error_message(message: &str, max_chars: usize) -> String {
    if message.chars().count() <= max_chars {
        return message.to_string();
    }
    let mut preview = message.chars().take(max_chars).collect::<String>();
    preview.push_str("... [truncated]");
    preview
}

fn emit_tool_catalog_cache_metrics(
    ctx: &TurnProcessingContext<'_>,
    step_count: usize,
    model: &str,
    cache_hit: bool,
    plan_mode: bool,
    request_user_input_enabled: bool,
    available_tools: usize,
) {
    tracing::info!(
        target: "vtcode.turn.metrics",
        metric = "tool_catalog_cache",
        run_id = %ctx.harness_state.run_id.0,
        turn_id = %ctx.harness_state.turn_id.0,
        turn = step_count,
        model,
        cache_hit,
        plan_mode,
        request_user_input_enabled,
        available_tools,
        "turn metric"
    );

    #[derive(serde::Serialize)]
    struct ToolCatalogCacheRecord<'a> {
        kind: &'static str,
        turn: usize,
        model: &'a str,
        cache_hit: bool,
        plan_mode: bool,
        request_user_input_enabled: bool,
        available_tools: usize,
        ts: i64,
    }

    ctx.traj.log(&ToolCatalogCacheRecord {
        kind: "tool_catalog_cache_metrics",
        turn: step_count,
        model,
        cache_hit,
        plan_mode,
        request_user_input_enabled,
        available_tools,
        ts: chrono::Utc::now().timestamp(),
    });
}

#[allow(clippy::too_many_arguments)]
fn emit_llm_retry_metrics(
    ctx: &TurnProcessingContext<'_>,
    step_count: usize,
    model: &str,
    plan_mode: bool,
    attempts_made: usize,
    max_retries: usize,
    success: bool,
    stream_fallback_used: bool,
    last_error_retryable: Option<bool>,
    last_error_preview: Option<&str>,
) {
    let retries_used = attempts_made.saturating_sub(1);
    let exhausted_retry_budget = !success && attempts_made >= max_retries;
    tracing::info!(
        target: "vtcode.turn.metrics",
        metric = "llm_retry_outcome",
        run_id = %ctx.harness_state.run_id.0,
        turn_id = %ctx.harness_state.turn_id.0,
        turn = step_count,
        model,
        plan_mode,
        attempts_made,
        retries_used,
        max_retries,
        success,
        exhausted_retry_budget,
        stream_fallback_used,
        last_error_retryable = last_error_retryable.unwrap_or(false),
        "turn metric"
    );

    #[derive(serde::Serialize)]
    struct LlmRetryMetricsRecord<'a> {
        kind: &'static str,
        turn: usize,
        model: &'a str,
        plan_mode: bool,
        attempts_made: usize,
        retries_used: usize,
        max_retries: usize,
        success: bool,
        exhausted_retry_budget: bool,
        stream_fallback_used: bool,
        last_error_retryable: Option<bool>,
        last_error: Option<&'a str>,
        ts: i64,
    }

    ctx.traj.log(&LlmRetryMetricsRecord {
        kind: "llm_retry_metrics",
        turn: step_count,
        model,
        plan_mode,
        attempts_made,
        retries_used,
        max_retries,
        success,
        exhausted_retry_budget,
        stream_fallback_used,
        last_error_retryable,
        last_error: last_error_preview,
        ts: chrono::Utc::now().timestamp(),
    });
}

/// Execute an LLM request and return the response.
pub(crate) async fn execute_llm_request(
    ctx: &mut TurnProcessingContext<'_>,
    step_count: usize,
    active_model: &str,
    _max_tokens_opt: Option<u32>,
    parallel_cfg_opt: Option<Box<ParallelToolConfig>>,
) -> Result<(uni::LLMResponse, bool)> {
    let provider_name = ctx.provider_client.name().to_string();
    let plan_mode = ctx.session_stats.is_plan_mode();
    let request_user_input_enabled = ctx
        .vt_cfg
        .as_ref()
        .map(|cfg| cfg.chat.ask_questions.enabled)
        .unwrap_or(true);
    let context_window_size = ctx.provider_client.effective_context_size(active_model);
    let turn_timeout_secs = ctx
        .vt_cfg
        .as_ref()
        .map(|cfg| cfg.optimization.agent_execution.max_execution_time_secs)
        .unwrap_or(300);
    let request_timeout_secs =
        llm_attempt_timeout_secs(turn_timeout_secs, plan_mode, &provider_name);

    let active_agent_name = ctx.session_stats.active_agent();
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

    upsert_harness_limits_section(
        &mut system_prompt,
        ctx.harness_state.max_tool_calls,
        ctx.harness_state.max_tool_wall_clock.as_secs(),
        ctx.harness_state.max_tool_retries,
    );

    let capabilities = uni::get_cached_capabilities(&**ctx.provider_client, active_model);
    let mut use_streaming = capabilities.streaming;
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

    let tool_snapshot = ctx
        .tool_catalog
        .filtered_snapshot_with_stats(ctx.tools, plan_mode, request_user_input_enabled)
        .await;
    let current_tools = tool_snapshot.snapshot;
    let openai_prompt_cache_enabled = provider_name.eq_ignore_ascii_case("openai")
        && ctx.config.prompt_cache.enabled
        && ctx.config.prompt_cache.providers.openai.enabled;
    let has_tools = current_tools.is_some();
    emit_tool_catalog_cache_metrics(
        ctx,
        step_count,
        active_model,
        tool_snapshot.cache_hit,
        plan_mode,
        request_user_input_enabled,
        current_tools.as_ref().map_or(0, |defs| defs.len()),
    );
    if let Some(defs) = current_tools.as_ref()
        && !openai_prompt_cache_enabled
    {
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
    let metadata = match turn_metadata::build_turn_metadata_value_with_timeout(
        &ctx.config.workspace,
        std::time::Duration::from_millis(250),
    )
    .await
    {
        Ok(value) => value,
        Err(err) => {
            tracing::warn!(error = %err, "Turn metadata collection failed");
            None
        }
    };
    let prompt_cache_key = if openai_prompt_cache_enabled {
        match ctx
            .config
            .prompt_cache
            .providers
            .openai
            .prompt_cache_key_mode
        {
            OpenAIPromptCacheKeyMode::Session => {
                Some(format!("vtcode:openai:{}", ctx.harness_state.run_id.0))
            }
            OpenAIPromptCacheKeyMode::Off => None,
        }
    } else {
        None
    };

    let mut request = uni::LLMRequest {
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
        prompt_cache_key,
        ..Default::default()
    };
    if let Err(err) = ctx.provider_client.as_ref().validate_request(&request) {
        return Err(anyhow::Error::new(err));
    }

    let action_suggestion = extract_action_from_messages(ctx.working_history);

    let max_retries = llm_retry_attempts(ctx.vt_cfg.map(|cfg| cfg.agent.max_task_retries));
    let mut llm_result = Err(anyhow::anyhow!("LLM request failed to execute"));
    let mut attempts_made = 0usize;
    let mut stream_fallback_used = false;
    let mut last_error_retryable: Option<bool> = None;
    let mut last_error_preview: Option<String> = None;

    #[cfg(debug_assertions)]
    let mut request_timer = Instant::now();

    for attempt in 0..max_retries {
        attempts_made = attempt + 1;
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
                    max_retries
                ),
            )?;
            let cancel_notifier = ctx.ctrl_c_notify.notified();
            tokio::pin!(cancel_notifier);
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = &mut cancel_notifier => {
                    if ctx.ctrl_c_state.is_cancel_requested() || ctx.ctrl_c_state.is_exit_requested() {
                        llm_result = Err(anyhow::Error::new(uni::LLMError::Provider {
                            message: vtcode_core::llm::error_display::format_llm_error(
                                &provider_name,
                                "Interrupted by user",
                            ),
                            metadata: None,
                        }));
                        break;
                    }
                }
            }
        }

        let spinner_msg = if attempt > 0 {
            let action = action_suggestion.clone();
            if action.is_empty() {
                format!("Retrying request (attempt {}/{})", attempt + 1, max_retries)
            } else {
                format!("{} (Retry {}/{})", action, attempt + 1, max_retries)
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
        if has_tools {
            _spinner.set_defer_restore(true);
        }
        task::yield_now().await;

        #[cfg(debug_assertions)]
        {
            request_timer = Instant::now();
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

        request.stream = use_streaming;

        let step_result = if use_streaming {
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

            let send_ctx = UnsafeSendContext {
                handle: ctx.handle as *const ::vtcode_tui::InlineHandle
                    as *mut ::vtcode_tui::InlineHandle as usize,
            };

            let event_sink = Arc::new(Mutex::new(Box::new(
                move |event: ::vtcode_core::core::agent::events::AgentEvent| unsafe {
                    use ::vtcode_core::core::agent::events::AgentEvent;
                    match event {
                        AgentEvent::OutputDelta { .. } => {}
                        AgentEvent::ThinkingDelta { .. } => {}
                        AgentEvent::ThinkingStage { stage } => {
                            let h: &mut ::vtcode_tui::InlineHandle =
                                &mut *(send_ctx.handle as *mut ::vtcode_tui::InlineHandle);
                            h.set_input_status(Some(stage), None);
                        }
                        _ => {}
                    }
                },
            )
                as Box<dyn FnMut(::vtcode_core::core::agent::events::AgentEvent) + Send>));

            controller.set_event_handler(event_sink);

            let mut steering = None;
            let res = tokio::time::timeout(
                Duration::from_secs(request_timeout_secs),
                controller.run_turn(
                    ctx.provider_client,
                    request.clone(),
                    &mut steering,
                    Some(Duration::from_secs(request_timeout_secs)),
                ),
            )
            .await;

            match res {
                Ok(Ok((response, _content, _reasoning))) => Ok((response, false)),
                Ok(Err(err)) => Err(anyhow::anyhow!(err.to_string())),
                Err(_) => Err(anyhow::anyhow!(
                    "LLM request timed out after {} seconds",
                    request_timeout_secs
                )),
            }
        } else if ctx.ctrl_c_state.is_cancel_requested() || ctx.ctrl_c_state.is_exit_requested() {
            Err(anyhow::Error::new(uni::LLMError::Provider {
                message: vtcode_core::llm::error_display::format_llm_error(
                    &provider_name,
                    "Interrupted by user",
                ),
                metadata: None,
            }))
        } else {
            let generate_future = tokio::time::timeout(
                Duration::from_secs(request_timeout_secs),
                ctx.provider_client.generate(request.clone()),
            );
            tokio::pin!(generate_future);
            let cancel_notifier = ctx.ctrl_c_notify.notified();
            tokio::pin!(cancel_notifier);

            let outcome = tokio::select! {
                res = &mut generate_future => match res {
                    Ok(inner) => inner.map_err(anyhow::Error::from),
                    Err(_) => Err(anyhow::anyhow!(
                        "LLM request timed out after {} seconds",
                        request_timeout_secs
                    )),
                },
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
        };

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
                last_error_retryable = Some(is_retryable);
                last_error_preview = Some(compact_error_message(&msg, 180));

                if !crate::agent::runloop::unified::turn::turn_helpers::should_continue_operation(
                    ctx.ctrl_c_state,
                ) {
                    llm_result = Err(err);
                    _spinner.finish();
                    break;
                }

                if is_retryable && attempt < max_retries - 1 {
                    if use_streaming
                        && supports_streaming_timeout_fallback(&provider_name)
                        && is_stream_timeout_error(&msg)
                    {
                        use_streaming = false;
                        stream_fallback_used = true;
                        crate::agent::runloop::unified::turn::turn_helpers::display_status(
                            ctx.renderer,
                            "Streaming timed out; retrying with non-streaming for this provider.",
                        )?;
                    }
                    _spinner.finish();
                    continue;
                }

                llm_result = Err(err);
                _spinner.finish();
                break;
            }
        }
    }

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

    if attempts_made == 0 {
        attempts_made = 1;
    }
    if last_error_preview.is_none()
        && let Err(err) = &llm_result
    {
        last_error_preview = Some(compact_error_message(&err.to_string(), 180));
    }
    emit_llm_retry_metrics(
        ctx,
        step_count,
        active_model,
        plan_mode,
        attempts_made,
        max_retries,
        llm_result.is_ok(),
        stream_fallback_used,
        last_error_retryable,
        last_error_preview.as_deref(),
    );

    let (response, response_streamed) = match llm_result {
        Ok(result) => result,
        Err(error) => {
            return Err(error);
        }
    };
    if let Some(usage) = response.usage.as_ref() {
        #[derive(serde::Serialize)]
        struct PromptCacheMetricsRecord<'a> {
            kind: &'static str,
            turn: usize,
            model: &'a str,
            prompt_tokens: u32,
            completion_tokens: u32,
            total_tokens: u32,
            cached_prompt_tokens: u32,
            cache_hit_ratio: f64,
            ts: i64,
        }

        let cached_prompt_tokens = usage.cached_prompt_tokens.unwrap_or(0);
        let cache_hit_ratio = if usage.prompt_tokens == 0 {
            0.0
        } else {
            cached_prompt_tokens as f64 / usage.prompt_tokens as f64
        };
        let record = PromptCacheMetricsRecord {
            kind: "prompt_cache_metrics",
            turn: step_count,
            model: active_model,
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
            cached_prompt_tokens,
            cache_hit_ratio,
            ts: chrono::Utc::now().timestamp(),
        };
        ctx.traj.log(&record);
    }
    Ok((response, response_streamed))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn retryable_llm_error_excludes_forbidden_quota_failures() {
        assert!(!is_retryable_llm_error(
            "Provider error: HuggingFace API error (403 Forbidden): {\"error\":\"You have exceeded your monthly spending limit.\"}"
        ));
    }

    #[test]
    fn retryable_llm_error_includes_rate_limit_429() {
        assert!(is_retryable_llm_error(
            "Provider error: 429 Too Many Requests"
        ));
    }

    #[test]
    fn retryable_llm_error_includes_service_unavailable_class() {
        assert!(is_retryable_llm_error(
            "Provider error: 503 Service Unavailable"
        ));
        assert!(is_retryable_llm_error(
            "Provider error: 504 Gateway Timeout"
        ));
    }

    #[test]
    fn retryable_llm_error_excludes_usage_limit_messages() {
        assert!(!is_retryable_llm_error(
            "Provider error: you have reached your weekly usage limit"
        ));
    }

    #[test]
    fn supports_streaming_timeout_fallback_covers_supported_providers() {
        assert!(supports_streaming_timeout_fallback("huggingface"));
        assert!(supports_streaming_timeout_fallback("ollama"));
        assert!(supports_streaming_timeout_fallback("HUGGINGFACE"));
        assert!(!supports_streaming_timeout_fallback("openai"));
    }

    #[test]
    fn llm_retry_attempts_uses_default_when_unset() {
        assert_eq!(llm_retry_attempts(None), DEFAULT_LLM_RETRY_ATTEMPTS);
    }

    #[test]
    fn llm_retry_attempts_uses_configured_retries_plus_initial_attempt() {
        assert_eq!(llm_retry_attempts(Some(2)), 3);
    }

    #[test]
    fn llm_retry_attempts_respects_upper_bound() {
        assert_eq!(llm_retry_attempts(Some(16)), MAX_LLM_RETRY_ATTEMPTS);
    }

    #[test]
    fn stream_timeout_error_detection_matches_common_messages() {
        assert!(is_stream_timeout_error(
            "Stream request timed out after 75s"
        ));
        assert!(is_stream_timeout_error(
            "Streaming request timed out after configured timeout"
        ));
        assert!(is_stream_timeout_error(
            "LLM request timed out after 120 seconds"
        ));
    }

    #[test]
    fn llm_attempt_timeout_defaults_to_fifth_of_turn_budget() {
        assert_eq!(llm_attempt_timeout_secs(300, false, "openai"), 60);
    }

    #[test]
    fn llm_attempt_timeout_expands_for_plan_mode() {
        assert_eq!(llm_attempt_timeout_secs(300, true, "openai"), 120);
    }

    #[test]
    fn llm_attempt_timeout_plan_mode_respects_smaller_turn_budget() {
        assert_eq!(llm_attempt_timeout_secs(180, true, "openai"), 90);
    }

    #[test]
    fn llm_attempt_timeout_plan_mode_huggingface_uses_higher_floor() {
        assert_eq!(llm_attempt_timeout_secs(150, true, "huggingface"), 90);
    }

    #[test]
    fn llm_attempt_timeout_respects_plan_mode_cap() {
        assert_eq!(llm_attempt_timeout_secs(1_200, true, "huggingface"), 120);
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
