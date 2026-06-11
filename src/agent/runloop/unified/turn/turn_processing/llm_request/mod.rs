//! Agent Legibility:
//! - Entrypoint: `execute_llm_request` builds a provider request, runs streaming or non-streaming execution, and applies retry policy.
//! - Common changes:
//!   - Request assembly lives in `request_builder.rs`.
//!   - Retry policy helpers live in `retry.rs`.
//!   - Streaming bridge helpers live in `streaming.rs`.
//!   - Copilot runtime integration lives in `copilot_runtime.rs`.
//! - Constraints: Preserve retry semantics, previous-response-chain recovery, and prompt-cache telemetry behavior.
//! - Verify: `cargo check -p vtcode && cargo test -p vtcode --bin vtcode llm_request`

mod copilot_runtime;
mod metrics;
mod request_builder;
mod retry;
mod streaming;
#[cfg(test)]
mod tests;

use anyhow::Result;
use std::time::{Duration, Instant};
use tokio::task;
#[cfg(debug_assertions)]
use tracing::debug;
use vtcode_core::config::types::ReasoningEffortLevel;
#[cfg(test)]
use vtcode_core::config::{OpenAIPromptCacheKeyMode, PromptCachingConfig};
use vtcode_core::llm::provider::{self as uni, ParallelToolConfig, supports_responses_chaining};

use crate::agent::runloop::unified::extract_action_from_messages;
#[cfg(test)]
use crate::agent::runloop::unified::incremental_system_prompt::PromptCacheShapingMode;
use crate::agent::runloop::unified::reasoning::resolve_reasoning_visibility;
use crate::agent::runloop::unified::turn::context::TurnProcessingContext;
use crate::agent::runloop::unified::ui_interaction::{
    PlaceholderSpinner, StreamProgressEvent, StreamSpinnerOptions,
    stream_and_render_response_with_options_and_progress,
};
use crate::agent::runloop::unified::ui_interaction_stream::render_stream_with_options_and_copilot_runtime_impl;
use crate::agent::runloop::unified::wait_feedback::{
    WAIT_KEEPALIVE_INITIAL, WAIT_KEEPALIVE_INTERVAL, resolve_fractional_warning_delay,
    wait_keepalive_message, wait_timeout_warning_message,
};
use copilot_runtime::{CopilotRuntimeHost, prompt_session_to_stream};
use metrics::emit_llm_retry_metrics;
use request_builder::{
    build_turn_request, capture_turn_request_snapshot, interrupted_provider_error,
    update_previous_response_chain_after_success,
};
#[cfg(test)]
use request_builder::{is_openai_prompt_cache_enabled, resolve_prompt_cache_shaping_mode};
#[cfg(test)]
use retry::is_retryable_llm_error;
#[cfg(test)]
use retry::{DEFAULT_LLM_RETRY_ATTEMPTS, MAX_LLM_RETRY_ATTEMPTS};
use retry::{
    PostToolRetryAction, classify_llm_error, compact_error_message,
    compact_tool_messages_for_retry, has_recent_tool_responses, is_previous_response_chain_error,
    is_stream_timeout_error, llm_retry_attempts, next_post_tool_retry_action,
    supports_streaming_timeout_fallback, switch_to_non_streaming_retry_mode,
};
use streaming::HarnessStreamingBridge;
#[cfg(test)]
use vtcode_core::config::build_openai_prompt_cache_key;

pub(crate) use retry::llm_attempt_timeout_secs;

const WAIT_TIMEOUT_WARNING_HEADROOM: Duration = Duration::from_secs(15);
const WAIT_TIMEOUT_WARNING_FRACTION: f32 = 0.75;

fn llm_timeout_warning_delay(timeout_budget: Duration) -> Option<Duration> {
    resolve_fractional_warning_delay(
        timeout_budget,
        WAIT_TIMEOUT_WARNING_FRACTION,
        WAIT_TIMEOUT_WARNING_HEADROOM,
    )
}

async fn run_standard_stream_attempt(
    ctx: &mut TurnProcessingContext<'_>,
    request: uni::LLMRequest,
    request_timeout_secs: u64,
    spinner: &PlaceholderSpinner,
    stream_options: StreamSpinnerOptions,
    progress: &mut (dyn FnMut(StreamProgressEvent) + Send),
) -> Result<(uni::LLMResponse, bool)> {
    let stream_future = stream_and_render_response_with_options_and_progress(
        &**ctx.provider_client,
        request,
        spinner,
        ctx.renderer,
        ctx.ctrl_c_state,
        ctx.ctrl_c_notify,
        stream_options,
        Some(progress),
    );
    let res = tokio::time::timeout(Duration::from_secs(request_timeout_secs), stream_future).await;

    match res {
        Ok(Ok((response, emitted_tokens))) => Ok((response, emitted_tokens)),
        Ok(Err(err)) => Err(anyhow::Error::new(err)),
        Err(_) => Err(anyhow::anyhow!(
            "LLM request timed out after {} seconds",
            request_timeout_secs
        )),
    }
}

fn finish_streaming_bridge_success(
    ctx: &mut TurnProcessingContext<'_>,
    stream_bridge: &mut HarnessStreamingBridge,
) {
    ctx.harness_state
        .remember_streamed_tool_call_items(stream_bridge.take_streamed_tool_call_items());
    stream_bridge.complete_open_items();
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
    let active_model = turn_snapshot.active_model.clone();
    let request_timeout_secs = llm_attempt_timeout_secs(
        turn_snapshot.turn_timeout_secs,
        turn_snapshot.planning_active,
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
        &active_model,
        &turn_snapshot,
        max_tokens_opt,
        parallel_cfg_opt,
        use_streaming,
    )
    .await?;
    let mut request = initial_request.request;
    let has_tools = initial_request.has_tools;
    let runtime_tools = initial_request.runtime_tools;
    let continuation_messages = initial_request.continuation_messages;
    if let Err(err) = ctx.provider_client.as_ref().validate_request(&request) {
        return Err(anyhow::Error::new(err));
    }

    let action_suggestion = extract_action_from_messages(ctx.working_history);

    let max_retries = llm_retry_attempts(ctx.vt_cfg.map(|cfg| cfg.agent.max_task_retries));
    let supports_non_streaming = ctx.provider_client.supports_non_streaming(&active_model);
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

    let mut attempt = 0usize;
    while attempt < max_retries {
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
            tracing::debug!(
                category = reason_hint,
                delay_secs,
                attempt = attempt + 1,
                max_retries,
                "LLM request failed; retrying after backoff"
            );
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
        let preserve_structured_post_tool_context = supports_responses_chaining(
            &turn_snapshot.provider_name,
            turn_snapshot.capabilities.responses_compaction,
        );

        // DeepSeek: preemptively disable thinking for post-tool follow-ups.
        // Thinking mode + tool messages causes API errors because DeepSeek
        // expects `reasoning_content` in all assistant messages when thinking
        // is enabled, but serialized tool results don't include it.
        if has_post_tool_context
            && turn_snapshot.provider_name == "DeepSeek"
            && request
                .reasoning_effort
                .is_some_and(|e| e != ReasoningEffortLevel::None)
        {
            request.reasoning_effort = Some(ReasoningEffortLevel::None);
        }

        let step_result = if use_streaming {
            let mut stream_bridge = HarnessStreamingBridge::new(
                ctx.harness_emitter,
                &ctx.harness_state.turn_id.0,
                step_count,
                attempt + 1,
            );
            let stream_options = StreamSpinnerOptions {
                defer_finish: has_tools,
                strip_proposed_plan_blocks: turn_snapshot.planning_active,
            };
            let mut progress = |event: StreamProgressEvent| stream_bridge.on_progress(event);
            let stream_result =
                if turn_snapshot.provider_name == vtcode_core::copilot::COPILOT_PROVIDER_KEY {
                    let mut runtime_host = CopilotRuntimeHost::new(
                        ctx.tool_registry,
                        ctx.tool_result_cache,
                        ctx.session,
                        ctx.session_stats,
                        ctx.plan_session,
                        ctx.mcp_panel_state,
                        ctx.handle,
                        ctx.ctrl_c_state,
                        ctx.ctrl_c_notify,
                        ctx.default_placeholder.clone(),
                        ctx.approval_recorder,
                        ctx.decision_ledger,
                        ctx.tool_permission_cache,
                        ctx.permissions_state,
                        ctx.vt_cfg
                            .and_then(|cfg| cfg.runtime_agent_permissions.as_ref())
                            .or(Some(&turn_snapshot.active_primary_agent.permissions)),
                        ctx.safety_validator,
                        ctx.lifecycle_hooks,
                        ctx.vt_cfg,
                        ctx.traj,
                        ctx.harness_state,
                        runtime_tools.as_ref(),
                        ctx.skip_confirmations,
                        ctx.harness_emitter,
                        format!("{}-step-{}", ctx.harness_state.turn_id.0, step_count),
                    );
                    let exposed_tools = runtime_host.exposed_tools().to_vec();

                    if let Some(start_prompt_session) = ctx
                        .provider_client
                        .as_ref()
                        .start_copilot_prompt_session(request.clone(), &exposed_tools)
                    {
                        let prompt_session = start_prompt_session.await?;
                        let (mut stream, mut runtime_requests) =
                            prompt_session_to_stream(request.model.clone(), prompt_session);
                        render_stream_with_options_and_copilot_runtime_impl(
                            &turn_snapshot.provider_name,
                            &mut stream,
                            None,
                            Some(&mut runtime_requests),
                            Some(&mut runtime_host),
                            Some(Duration::from_secs(request_timeout_secs)),
                            &_spinner,
                            ctx.renderer,
                            ctx.ctrl_c_state,
                            ctx.ctrl_c_notify,
                            stream_options,
                            Some(&mut progress),
                        )
                        .await
                        .map_err(anyhow::Error::new)
                    } else {
                        drop(runtime_host);
                        run_standard_stream_attempt(
                            ctx,
                            request.clone(),
                            request_timeout_secs,
                            &_spinner,
                            stream_options,
                            &mut progress,
                        )
                        .await
                    }
                } else {
                    run_standard_stream_attempt(
                        ctx,
                        request.clone(),
                        request_timeout_secs,
                        &_spinner,
                        stream_options,
                        &mut progress,
                    )
                    .await
                };

            match stream_result {
                Ok((response, emitted_tokens)) => {
                    finish_streaming_bridge_success(ctx, &mut stream_bridge);
                    Ok((response, emitted_tokens))
                }
                Err(err) => {
                    stream_bridge.abort();
                    Err(err)
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
                    &active_model,
                    attempt_elapsed,
                    response.usage.as_ref(),
                );
            }
            Err(_) => {
                ctx.telemetry
                    .record_llm_request(&active_model, attempt_elapsed, None);
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
                    turn_snapshot.capabilities.responses_compaction,
                    &active_model,
                    response.request_id.as_deref(),
                    &continuation_messages,
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

                if request.previous_response_id.is_some()
                    && !dropped_previous_response_id_for_retry
                    && is_previous_response_chain_error(&msg)
                {
                    request.previous_response_id = None;
                    dropped_previous_response_id_for_retry = true;
                    last_error_retryable = Some(true);
                    last_error_category = None;
                    ctx.session_stats.clear_previous_response_chain_for(
                        &turn_snapshot.provider_name,
                        &request.model,
                    );
                    // Retry immediately on the same logical attempt. A stale
                    // continuation chain is bookkeeping drift, not a provider
                    // availability failure that should consume retry budget.
                    crate::agent::runloop::unified::turn::turn_helpers::display_status(
                        ctx.renderer,
                        "Previous response chain expired; retrying with a fresh provider chain.",
                    )?;
                    _spinner.finish();
                    continue;
                }

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
                    attempt += 1;
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
                            tracing::debug!(
                                provider = %turn_snapshot.provider_name,
                                "post-tool follow-up failed; retrying with non-streaming.",
                            );
                            _spinner.finish();
                            attempt += 1;
                            continue;
                        }
                        Some(PostToolRetryAction::CompactToolContext) => {
                            let status_msg = if use_streaming {
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
                            // Strip reasoning_effort on retry — DeepSeek's
                            // `thinking` parameter causes ExecutionError on
                            // post-tool follow-ups where tool messages are
                            // already in context. Reasoning isn't needed for
                            // a synthesis retry. Use Some(None) instead of None
                            // to explicitly disable thinking mode via the
                            // `thinking: {"type": "disabled"}` payload field.
                            request.reasoning_effort = Some(ReasoningEffortLevel::None);
                            compacted_tool_retry_used = true;
                            tracing::debug!(provider = %turn_snapshot.provider_name, "{status_msg}",);
                            _spinner.finish();
                            attempt += 1;
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
        &active_model,
        turn_snapshot.planning_active,
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
            model: &active_model,
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
