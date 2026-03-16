mod metrics;
mod request_builder;
mod retry;
mod streaming;

use anyhow::Result;
use std::time::{Duration, Instant};
use tokio::task;
#[cfg(debug_assertions)]
use tracing::debug;
#[cfg(test)]
use vtcode_core::config::{OpenAIPromptCacheKeyMode, PromptCachingConfig};
use vtcode_core::llm::provider::{self as uni, ParallelToolConfig};

use crate::agent::runloop::unified::extract_action_from_messages;
#[cfg(test)]
use crate::agent::runloop::unified::incremental_system_prompt::PromptCacheShapingMode;
use crate::agent::runloop::unified::reasoning::resolve_reasoning_visibility;
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;
use crate::agent::runloop::unified::turn::turn_helpers::supports_responses_chaining;
use crate::agent::runloop::unified::ui_interaction::{
    StreamProgressEvent, StreamSpinnerOptions, stream_and_render_response_with_options_and_progress,
};
use crate::agent::runloop::unified::wait_feedback::{
    WAIT_KEEPALIVE_INITIAL, WAIT_KEEPALIVE_INTERVAL, resolve_fractional_warning_delay,
    wait_keepalive_message, wait_timeout_warning_message,
};
use metrics::emit_llm_retry_metrics;
#[cfg(test)]
use request_builder::{
    build_openai_prompt_cache_key, is_openai_prompt_cache_enabled,
    resolve_prompt_cache_shaping_mode,
};
use request_builder::{
    build_turn_request, capture_turn_request_snapshot, interrupted_provider_error,
    update_previous_response_chain_after_success,
};
#[cfg(test)]
use retry::is_retryable_llm_error;
#[cfg(test)]
use retry::{DEFAULT_LLM_RETRY_ATTEMPTS, MAX_LLM_RETRY_ATTEMPTS};
use retry::{
    PostToolRetryAction, classify_llm_error, compact_error_message,
    compact_tool_messages_for_retry, has_recent_tool_responses, is_stream_timeout_error,
    llm_attempt_timeout_secs, llm_retry_attempts, next_post_tool_retry_action,
    supports_streaming_timeout_fallback, switch_to_non_streaming_retry_mode,
};
use streaming::HarnessStreamingBridge;

const WAIT_TIMEOUT_WARNING_HEADROOM: Duration = Duration::from_secs(15);
const WAIT_TIMEOUT_WARNING_FRACTION: f32 = 0.75;

fn llm_timeout_warning_delay(timeout_budget: Duration) -> Option<Duration> {
    resolve_fractional_warning_delay(
        timeout_budget,
        WAIT_TIMEOUT_WARNING_FRACTION,
        WAIT_TIMEOUT_WARNING_HEADROOM,
    )
}

/// Execute an LLM request and return the response.
pub(crate) async fn execute_llm_request(
    ctx: &mut TurnProcessingContext<'_>,
    step_count: usize,
    active_model: &str,
    max_tokens_opt: Option<u32>,
    tool_free_recovery: bool,
    parallel_cfg_opt: Option<Box<ParallelToolConfig>>,
) -> Result<(uni::LLMResponse, bool)> {
    let turn_snapshot = capture_turn_request_snapshot(ctx, active_model, tool_free_recovery);
    let request_timeout_secs = llm_attempt_timeout_secs(
        turn_snapshot.turn_timeout_secs,
        turn_snapshot.plan_mode,
        &turn_snapshot.provider_name,
    );

    ctx.renderer
        .set_reasoning_visible(resolve_reasoning_visibility(
            ctx.vt_cfg,
            turn_snapshot.capabilities.reasoning,
        ));
    let mut use_streaming = turn_snapshot.capabilities.streaming;
    let initial_request = build_turn_request(
        ctx,
        step_count,
        active_model,
        &turn_snapshot,
        max_tokens_opt,
        parallel_cfg_opt,
        use_streaming,
    )
    .await?;
    let mut request = initial_request.request;
    let has_tools = initial_request.has_tools;
    if let Err(err) = ctx.provider_client.as_ref().validate_request(&request) {
        return Err(anyhow::Error::new(err));
    }

    let action_suggestion = extract_action_from_messages(ctx.working_history);

    let max_retries = llm_retry_attempts(ctx.vt_cfg.map(|cfg| cfg.agent.max_task_retries));
    let supports_non_streaming = ctx.provider_client.supports_non_streaming(active_model);
    let mut llm_result = Err(anyhow::anyhow!("LLM request failed to execute"));
    let mut attempts_made = 0usize;
    let mut stream_fallback_used = false;
    let mut compacted_tool_retry_used = false;
    let mut dropped_previous_response_id_for_retry = false;
    let mut last_error_retryable: Option<bool> = None;
    let mut last_error_preview: Option<String> = None;
    let mut last_error_category: Option<vtcode_commons::ErrorCategory> = None;

    #[cfg(debug_assertions)]
    let mut request_timer = Instant::now();

    for attempt in 0..max_retries {
        attempts_made = attempt + 1;
        if attempt > 0 {
            use crate::agent::runloop::unified::turn::turn_helpers::calculate_backoff;
            // Use category-aware backoff: rate limits get longer base delays,
            // timeouts get moderate delays, network errors use standard exponential.
            let (base_ms, max_ms) = match last_error_category {
                Some(vtcode_commons::ErrorCategory::RateLimit) => (1000, 30_000),
                Some(vtcode_commons::ErrorCategory::Timeout) => (1000, 15_000),
                _ => (500, 10_000),
            };
            let delay = calculate_backoff(attempt - 1, base_ms, max_ms);
            let delay_secs = delay.as_secs_f64();
            let reason_hint = last_error_category
                .as_ref()
                .map(|cat| cat.user_label())
                .unwrap_or("unknown error");
            crate::agent::runloop::unified::turn::turn_helpers::display_status(
                ctx.renderer,
                &format!(
                    "LLM request failed ({}), retrying in {:.1}s... (attempt {}/{})",
                    reason_hint,
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
                        llm_result = Err(interrupted_provider_error(&turn_snapshot.provider_name));
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
        let attempt_started_at = Instant::now();

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
        let has_post_tool_context = has_recent_tool_responses(&request.messages);
        let preserve_structured_post_tool_context =
            supports_responses_chaining(&turn_snapshot.provider_name);

        let step_result = if use_streaming {
            let mut stream_bridge = HarnessStreamingBridge::new(
                ctx.harness_emitter,
                &ctx.harness_state.turn_id.0,
                step_count,
                attempt + 1,
            );
            let stream_options = StreamSpinnerOptions {
                defer_finish: has_tools,
                strip_proposed_plan_blocks: turn_snapshot.plan_mode,
            };
            let mut progress = |event: StreamProgressEvent| stream_bridge.on_progress(event);
            let stream_future = stream_and_render_response_with_options_and_progress(
                &**ctx.provider_client,
                request.clone(),
                &_spinner,
                ctx.renderer,
                ctx.ctrl_c_state,
                ctx.ctrl_c_notify,
                stream_options,
                Some(&mut progress),
            );
            let res =
                tokio::time::timeout(Duration::from_secs(request_timeout_secs), stream_future)
                    .await;

            match res {
                Ok(Ok((response, emitted_tokens))) => {
                    stream_bridge.complete_open_items();
                    Ok((response, emitted_tokens))
                }
                Ok(Err(err)) => {
                    stream_bridge.abort();
                    Err(anyhow::Error::new(err))
                }
                Err(_) => {
                    stream_bridge.abort();
                    Err(anyhow::anyhow!(
                        "LLM request timed out after {} seconds",
                        request_timeout_secs
                    ))
                }
            }
        } else if ctx.ctrl_c_state.is_cancel_requested() || ctx.ctrl_c_state.is_exit_requested() {
            Err(interrupted_provider_error(&turn_snapshot.provider_name))
        } else {
            let generate_future = tokio::time::timeout(
                Duration::from_secs(request_timeout_secs),
                ctx.provider_client.generate(request.clone()),
            );
            tokio::pin!(generate_future);
            let keepalive_started_at = tokio::time::Instant::now();
            let mut next_keepalive_at = keepalive_started_at + WAIT_KEEPALIVE_INITIAL;
            let timeout_budget = Duration::from_secs(request_timeout_secs);
            let warning_delay = llm_timeout_warning_delay(timeout_budget);
            let mut timeout_warning_emitted = false;
            let wait_subject = format!("LLM request for model '{}'", active_model);

            loop {
                let cancel_notifier = ctx.ctrl_c_notify.notified();
                tokio::pin!(cancel_notifier);
                let keepalive_sleep = tokio::time::sleep_until(next_keepalive_at);
                tokio::pin!(keepalive_sleep);

                let outcome = tokio::select! {
                    res = &mut generate_future => Some(match res {
                        Ok(inner) => inner.map_err(anyhow::Error::from),
                        Err(_) => Err(anyhow::anyhow!(
                            "LLM request timed out after {} seconds",
                            request_timeout_secs
                        )),
                    }),
                    _ = &mut cancel_notifier => {
                        Some(Err(interrupted_provider_error(&turn_snapshot.provider_name)))
                    }
                    _ = &mut keepalive_sleep => None,
                };

                if let Some(outcome) = outcome {
                    match outcome {
                        Ok(response) => break Ok((response, false)),
                        Err(err) => break Err(err),
                    }
                }

                let elapsed = keepalive_started_at.elapsed();
                let keepalive_message = wait_keepalive_message(&wait_subject, elapsed);
                _spinner.update_message(keepalive_message.clone());
                crate::agent::runloop::unified::turn::turn_helpers::display_status(
                    ctx.renderer,
                    &keepalive_message,
                )?;

                let remaining = timeout_budget.saturating_sub(elapsed);
                if !timeout_warning_emitted && warning_delay.is_some_and(|delay| elapsed >= delay) {
                    timeout_warning_emitted = true;
                    let warning =
                        wait_timeout_warning_message(&wait_subject, timeout_budget, remaining);
                    _spinner.update_message(warning.clone());
                    crate::agent::runloop::unified::turn::turn_helpers::display_status(
                        ctx.renderer,
                        &warning,
                    )?;
                }

                next_keepalive_at += WAIT_KEEPALIVE_INTERVAL;
            }
        };
        let attempt_elapsed = attempt_started_at.elapsed();
        match &step_result {
            Ok((response, _)) => {
                ctx.telemetry.record_llm_request(
                    active_model,
                    attempt_elapsed,
                    response.usage.as_ref(),
                );
            }
            Err(_) => {
                ctx.telemetry
                    .record_llm_request(active_model, attempt_elapsed, None);
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
                succeeded = step_result.is_ok(),
                attempt = attempt + 1,
                "Provider request finished"
            );
        }

        match step_result {
            Ok((response, response_streamed)) => {
                update_previous_response_chain_after_success(
                    ctx.session_stats,
                    &turn_snapshot.provider_name,
                    active_model,
                    response.request_id.as_deref(),
                );
                llm_result = Ok((response, response_streamed));
                _spinner.finish();
                break;
            }
            Err(err) => {
                let msg = err.to_string();
                let category = classify_llm_error(&msg);
                let is_retryable = category.is_retryable();
                last_error_retryable = Some(is_retryable);
                last_error_preview = Some(compact_error_message(&msg, 180));
                last_error_category = Some(category);

                tracing::warn!(
                    target: "vtcode.llm.retry",
                    error = %msg,
                    category = %category.user_label(),
                    retryable = is_retryable,
                    attempt = attempt + 1,
                    max_retries,
                    "LLM request attempt failed"
                );

                if !crate::agent::runloop::unified::turn::turn_helpers::should_continue_operation(
                    ctx.ctrl_c_state,
                ) {
                    llm_result = Err(err);
                    _spinner.finish();
                    break;
                }

                // Fail-fast for permanent errors: don't waste retry budget
                // on authentication failures, resource exhaustion, or policy violations.
                if category.is_permanent() {
                    tracing::info!(
                        target: "vtcode.llm.retry",
                        category = %category.user_label(),
                        "Permanent error detected; skipping remaining retries"
                    );
                    llm_result = Err(err);
                    _spinner.finish();
                    break;
                }

                if is_retryable && attempt < max_retries - 1 {
                    if request.previous_response_id.is_some()
                        && !dropped_previous_response_id_for_retry
                    {
                        request.previous_response_id = None;
                        dropped_previous_response_id_for_retry = true;
                        ctx.session_stats.clear_previous_response_chain();
                        crate::agent::runloop::unified::turn::turn_helpers::display_status(
                            ctx.renderer,
                            "Retrying without previous response chain after provider error.",
                        )?;
                    }
                    if use_streaming
                        && supports_streaming_timeout_fallback(&turn_snapshot.provider_name)
                        && is_stream_timeout_error(&msg)
                    {
                        switch_to_non_streaming_retry_mode(
                            &mut use_streaming,
                            &mut stream_fallback_used,
                        );
                        crate::agent::runloop::unified::turn::turn_helpers::display_status(
                            ctx.renderer,
                            "Streaming timed out; retrying with non-streaming for this provider.",
                        )?;
                    }
                    _spinner.finish();
                    continue;
                }

                // Universal post-tool recovery: when a provider fails after
                // receiving tool results, prefer non-streaming when supported,
                // otherwise keep streaming and compact the tool messages.
                if has_post_tool_context && attempt < max_retries - 1 {
                    match next_post_tool_retry_action(
                        use_streaming,
                        supports_non_streaming,
                        compacted_tool_retry_used,
                        preserve_structured_post_tool_context,
                    ) {
                        Some(PostToolRetryAction::SwitchToNonStreaming) => {
                            switch_to_non_streaming_retry_mode(
                                &mut use_streaming,
                                &mut stream_fallback_used,
                            );
                            crate::agent::runloop::unified::turn::turn_helpers::display_status(
                                ctx.renderer,
                                &format!(
                                    "{} post-tool follow-up failed; retrying with non-streaming.",
                                    turn_snapshot.provider_name
                                ),
                            )?;
                            _spinner.finish();
                            continue;
                        }
                        Some(PostToolRetryAction::CompactToolContext) => {
                            let status = if use_streaming {
                                format!(
                                    "{} post-tool follow-up failed; retrying with compacted tool context.",
                                    turn_snapshot.provider_name
                                )
                            } else {
                                format!(
                                    "{} follow-up still failed; retrying with compacted tool context.",
                                    turn_snapshot.provider_name
                                )
                            };
                            let compacted = compact_tool_messages_for_retry(&request.messages);
                            request.messages = compacted;
                            compacted_tool_retry_used = true;
                            crate::agent::runloop::unified::turn::turn_helpers::display_status(
                                ctx.renderer,
                                &status,
                            )?;
                            _spinner.finish();
                            continue;
                        }
                        None => {}
                    }
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
        turn_snapshot.plan_mode,
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
            cache_read_tokens: u32,
            cache_creation_tokens: u32,
            cache_hit_ratio: f64,
            ts: i64,
        }

        let cached_prompt_tokens = usage.cached_prompt_tokens.unwrap_or(0);
        let cache_read_tokens = usage.cache_read_tokens_or_fallback();
        let cache_creation_tokens = usage.cache_creation_tokens_or_zero();
        let cache_hit_ratio = usage.cache_hit_rate().unwrap_or(0.0) / 100.0;
        let record = PromptCacheMetricsRecord {
            kind: "prompt_cache_metrics",
            turn: step_count,
            model: active_model,
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total_tokens,
            cached_prompt_tokens,
            cache_read_tokens,
            cache_creation_tokens,
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
    use tempfile::TempDir;
    use vtcode_core::prompts::upsert_harness_limits_section;

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
        assert!(supports_streaming_timeout_fallback("minimax"));
        assert!(supports_streaming_timeout_fallback("HUGGINGFACE"));
        assert!(!supports_streaming_timeout_fallback("openai"));
    }

    #[test]
    fn post_tool_retry_uses_non_streaming_before_compaction_when_supported() {
        assert_eq!(
            next_post_tool_retry_action(true, true, false, false),
            Some(PostToolRetryAction::SwitchToNonStreaming)
        );
    }

    #[test]
    fn post_tool_retry_skips_non_streaming_when_unsupported() {
        assert_eq!(
            next_post_tool_retry_action(true, false, false, false),
            Some(PostToolRetryAction::CompactToolContext)
        );
    }

    #[test]
    fn post_tool_retry_preserves_structured_context_for_responses_chaining_providers() {
        assert_eq!(next_post_tool_retry_action(false, true, false, true), None);
    }

    #[test]
    fn compact_tool_messages_for_retry_keeps_recent_tool_outputs_only() {
        let messages = vec![
            uni::Message::user("u1".to_string()),
            uni::Message::tool_response("call_1".to_string(), "old tool".to_string()),
            uni::Message::assistant("a1".to_string()),
            uni::Message::tool_response("call_2".to_string(), "new tool".to_string()),
        ];

        let compacted = compact_tool_messages_for_retry(&messages);
        assert_eq!(
            compacted
                .iter()
                .filter(|message| message.role == uni::MessageRole::Tool)
                .count(),
            2
        );
        assert_eq!(compacted.len(), 4);
    }

    #[test]
    fn compact_tool_messages_for_retry_keeps_all_tool_call_ids() {
        let messages = vec![
            uni::Message::tool_response("call_1".to_string(), "first".to_string()),
            uni::Message::assistant("a1".to_string()),
            uni::Message::tool_response("call_2".to_string(), "second".to_string()),
            uni::Message::assistant("a2".to_string()),
            uni::Message::tool_response("call_3".to_string(), "third".to_string()),
        ];

        let compacted = compact_tool_messages_for_retry(&messages);
        let tool_ids = compacted
            .iter()
            .filter(|message| message.role == uni::MessageRole::Tool)
            .filter_map(|message| message.tool_call_id.clone())
            .collect::<Vec<_>>();

        assert_eq!(tool_ids, vec!["call_1", "call_2", "call_3"]);
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
    fn llm_timeout_warning_delay_targets_three_quarters_of_budget() {
        assert_eq!(
            llm_timeout_warning_delay(Duration::from_secs(60)),
            Some(Duration::from_secs(45))
        );
    }

    #[test]
    fn llm_attempt_timeout_respects_plan_mode_cap() {
        assert_eq!(llm_attempt_timeout_secs(1_200, true, "huggingface"), 120);
    }

    #[test]
    fn openai_prompt_cache_enablement_requires_provider_and_flags() {
        assert!(is_openai_prompt_cache_enabled("openai", true, true));
        assert!(!is_openai_prompt_cache_enabled("openai", false, true));
        assert!(!is_openai_prompt_cache_enabled("openai", true, false));
        assert!(!is_openai_prompt_cache_enabled("anthropic", true, true));
    }

    #[test]
    fn prompt_cache_shaping_mode_requires_global_opt_in_and_provider_cache() {
        let mut cfg = PromptCachingConfig {
            enabled: true,
            cache_friendly_prompt_shaping: true,
            ..PromptCachingConfig::default()
        };
        cfg.providers.openai.enabled = true;

        assert_eq!(
            resolve_prompt_cache_shaping_mode("openai", &cfg),
            PromptCacheShapingMode::TrailingRuntimeContext
        );

        cfg.cache_friendly_prompt_shaping = false;
        assert_eq!(
            resolve_prompt_cache_shaping_mode("openai", &cfg),
            PromptCacheShapingMode::Disabled
        );
    }

    #[test]
    fn prompt_cache_shaping_mode_uses_block_mode_for_anthropic_family() {
        let mut cfg = PromptCachingConfig {
            enabled: true,
            cache_friendly_prompt_shaping: true,
            ..PromptCachingConfig::default()
        };
        cfg.providers.anthropic.enabled = true;

        assert_eq!(
            resolve_prompt_cache_shaping_mode("anthropic", &cfg),
            PromptCacheShapingMode::AnthropicBlockRuntimeContext
        );
        assert_eq!(
            resolve_prompt_cache_shaping_mode("minimax", &cfg),
            PromptCacheShapingMode::AnthropicBlockRuntimeContext
        );
    }

    #[test]
    fn prompt_cache_shaping_mode_respects_gemini_mode_off() {
        let mut cfg = PromptCachingConfig {
            enabled: true,
            cache_friendly_prompt_shaping: true,
            ..PromptCachingConfig::default()
        };
        cfg.providers.gemini.enabled = true;
        cfg.providers.gemini.mode = vtcode_core::config::core::GeminiPromptCacheMode::Off;

        assert_eq!(
            resolve_prompt_cache_shaping_mode("gemini", &cfg),
            PromptCacheShapingMode::Disabled
        );
    }

    #[test]
    fn openai_prompt_cache_key_uses_stable_session_identifier() {
        let run_id = "run-abc-123";
        let first = build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Session, run_id);
        let second =
            build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Session, run_id);

        assert_eq!(first, Some("vtcode:openai:run-abc-123".to_string()));
        assert_eq!(first, second);
    }

    #[test]
    fn openai_prompt_cache_key_honors_off_mode_or_disabled_cache() {
        assert_eq!(
            build_openai_prompt_cache_key(true, &OpenAIPromptCacheKeyMode::Off, "run-1"),
            None
        );
        assert_eq!(
            build_openai_prompt_cache_key(false, &OpenAIPromptCacheKeyMode::Session, "run-1"),
            None
        );
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

    #[test]
    fn harness_streaming_bridge_emits_incremental_agent_and_reasoning_items() {
        let tmp = TempDir::new().expect("temp dir");
        let path = tmp.path().join("harness.jsonl");
        let emitter =
            crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter::new(path)
                .expect("harness emitter");

        let mut bridge = HarnessStreamingBridge::new(Some(&emitter), "turn_123", 1, 1);
        bridge.on_progress(StreamProgressEvent::ReasoningStage("analysis".to_string()));
        bridge.on_progress(StreamProgressEvent::ReasoningDelta("think".to_string()));
        bridge.on_progress(StreamProgressEvent::OutputDelta("hello".to_string()));
        bridge.on_progress(StreamProgressEvent::OutputDelta(" world".to_string()));
        bridge.complete_open_items();

        let payload = std::fs::read_to_string(tmp.path().join("harness.jsonl")).expect("log");
        let mut saw_assistant_started = false;
        let mut saw_assistant_updated = false;
        let mut saw_assistant_completed = false;
        let mut saw_reasoning_started = false;
        let mut saw_reasoning_completed = false;

        for line in payload.lines() {
            let value: serde_json::Value = serde_json::from_str(line).expect("json");
            let event = value.get("event").expect("event");
            let event_type = event
                .get("type")
                .and_then(|kind| kind.as_str())
                .unwrap_or_default();
            let item_type = event
                .get("item")
                .and_then(|item| item.get("type"))
                .and_then(|kind| kind.as_str())
                .unwrap_or_default();
            let item_text = event
                .get("item")
                .and_then(|item| item.get("text"))
                .and_then(|text| text.as_str())
                .unwrap_or_default();

            if event_type == "item.started" && item_type == "agent_message" {
                saw_assistant_started = item_text == "hello";
            }
            if event_type == "item.updated" && item_type == "agent_message" {
                saw_assistant_updated = item_text == "hello world";
            }
            if event_type == "item.completed" && item_type == "agent_message" {
                saw_assistant_completed = item_text == "hello world";
            }
            if event_type == "item.started" && item_type == "reasoning" {
                saw_reasoning_started = item_text == "think";
            }
            if event_type == "item.completed" && item_type == "reasoning" {
                let stage = event
                    .get("item")
                    .and_then(|item| item.get("stage"))
                    .and_then(|stage| stage.as_str())
                    .unwrap_or_default();
                saw_reasoning_completed = item_text == "think" && stage == "analysis";
            }
        }

        assert!(saw_assistant_started);
        assert!(saw_assistant_updated);
        assert!(saw_assistant_completed);
        assert!(saw_reasoning_started);
        assert!(saw_reasoning_completed);
    }

    #[test]
    fn harness_streaming_bridge_throttles_reasoning_update_events() {
        let tmp = TempDir::new().expect("temp dir");
        let path = tmp.path().join("harness.jsonl");
        let emitter =
            crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter::new(path)
                .expect("harness emitter");

        let mut bridge = HarnessStreamingBridge::new(Some(&emitter), "turn_789", 2, 1);
        bridge.on_progress(StreamProgressEvent::ReasoningStage("analysis".to_string()));
        bridge.on_progress(StreamProgressEvent::ReasoningDelta("seed".to_string()));
        for _ in 0..12 {
            bridge.on_progress(StreamProgressEvent::ReasoningDelta("tiny".to_string()));
        }
        bridge.on_progress(StreamProgressEvent::ReasoningStage(
            "diagnosing".to_string(),
        ));
        bridge.on_progress(StreamProgressEvent::ReasoningDelta("x".repeat(200)));
        bridge.on_progress(StreamProgressEvent::ReasoningStage("final".to_string()));
        bridge.complete_open_items();

        let payload = std::fs::read_to_string(tmp.path().join("harness.jsonl")).expect("log");
        let reasoning_updates = payload
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|value| {
                value
                    .get("event")
                    .and_then(|event| event.get("type"))
                    .and_then(|kind| kind.as_str())
                    == Some("item.updated")
                    && value
                        .get("event")
                        .and_then(|event| event.get("item"))
                        .and_then(|item| item.get("type"))
                        .and_then(|kind| kind.as_str())
                        == Some("reasoning")
            })
            .count();

        assert!(
            reasoning_updates <= 2,
            "expected throttled reasoning updates, got {reasoning_updates}"
        );
        assert!(
            reasoning_updates >= 1,
            "expected at least one meaningful reasoning update, got {reasoning_updates}"
        );
    }

    #[test]
    fn harness_streaming_bridge_abort_closes_open_items() {
        let tmp = TempDir::new().expect("temp dir");
        let path = tmp.path().join("harness.jsonl");
        let emitter =
            crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter::new(path)
                .expect("harness emitter");

        let mut bridge = HarnessStreamingBridge::new(Some(&emitter), "turn_456", 3, 2);
        bridge.on_progress(StreamProgressEvent::OutputDelta("partial".to_string()));
        bridge.abort();

        let payload = std::fs::read_to_string(tmp.path().join("harness.jsonl")).expect("log");
        let completed_count = payload
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|value| {
                value
                    .get("event")
                    .and_then(|event| event.get("type"))
                    .and_then(|kind| kind.as_str())
                    == Some("item.completed")
            })
            .count();
        assert_eq!(completed_count, 1, "abort should close active stream item");
    }
}
