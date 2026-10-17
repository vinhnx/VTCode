use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use anyhow::Result;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use tokio::sync::{Notify, mpsc};

use crate::agent::runloop::unified::plan_blocks::{
    ProposedPlanStreamParser, extract_any_plan, strip_plan_persistence_policy_line,
};
use crate::agent::runloop::unified::stream_sanitization::StreamSanitizer;
use vtcode_core::copilot::CopilotRuntimeRequest;
use vtcode_core::llm::error_display;
use vtcode_core::llm::provider::{self as uni, LLMStreamEvent, NormalizedStreamEvent};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::state::CtrlCState;
use super::stop_requests::wait_for_stop;
use super::ui_interaction::{PlaceholderSpinner, StreamProgressEvent, StreamSpinnerOptions};
use super::ui_interaction_stream_helpers::{common_prefix_len, map_render_error, reasoning_matches_content};
mod streaming_reasoning;
use streaming_reasoning::{StreamingReasoningState, flush_pending_reasoning, flush_pending_reasoning_delta};

#[derive(Clone, Copy)]
pub(crate) struct FirstProgressTimeout {
    pub(crate) deadline: tokio::time::Instant,
    budget: Duration,
}

impl FirstProgressTimeout {
    pub(crate) fn starting_now(budget: Duration) -> Self {
        Self {
            deadline: tokio::time::Instant::now() + budget,
            budget,
        }
    }
}

fn first_progress_timeout_error(provider_name: &str, budget: Duration) -> uni::LLMError {
    uni::LLMError::Provider {
        message: error_display::format_llm_error(
            provider_name,
            &format!("LLM first token timed out after {} seconds", budget.as_secs()),
        ),
        metadata: None,
    }
}

fn stream_markdown_with_provider_error(
    provider_name: &str,
    renderer: &mut AnsiRenderer,
    text: &str,
    previous_line_count: usize,
) -> Result<usize, uni::LLMError> {
    renderer
        .stream_markdown_response(text, previous_line_count)
        .map_err(|err| map_render_error(provider_name, err))
}

// Provider-noise and harmony-control-token sanitization has been extracted to
// `stream_sanitization::StreamSanitizer`. The constants and helper functions
// that previously lived here (`HARMONY_MARKERS`, `contains_harmony_marker`,
// `sanitize_harmony_stream_text`, `sanitize_harmony_final_text`, etc.) now
// reside in that module, alongside MiniMax flat-noise stripping.

#[async_trait]
pub(crate) trait CopilotRuntimeRequestHandler: Send {
    async fn handle_runtime_request(
        &mut self,
        renderer: &mut AnsiRenderer,
        request: CopilotRuntimeRequest,
    ) -> Result<(), uni::LLMError>;
}

fn normalized_to_legacy_stream(
    mut stream: uni::LLMNormalizedStream,
) -> (uni::LLMStream, mpsc::Receiver<StreamProgressEvent>) {
    let (progress_tx, progress_rx) = mpsc::channel(256);
    let stream = try_stream! {
        let mut pending_usage = None;
        let mut provider_activity_reported = false;

        while let Some(event) = stream.next().await {
            match event? {
                NormalizedStreamEvent::TextDelta { delta } => {
                    yield LLMStreamEvent::Token { delta };
                }
                NormalizedStreamEvent::ReasoningDelta { delta, source } if source.is_public_summary() => {
                    yield LLMStreamEvent::Reasoning { delta };
                }
                NormalizedStreamEvent::ReasoningDelta { .. } => {
                    if !provider_activity_reported {
                        provider_activity_reported = true;
                        let _ = progress_tx.send(StreamProgressEvent::ProviderActivity).await;
                    }
                }
                NormalizedStreamEvent::ReasoningStage { stage } => {
                    yield LLMStreamEvent::ReasoningStage { stage };
                }
                NormalizedStreamEvent::ToolCallStart { call_id, name } => {
                    let _ = progress_tx.send(StreamProgressEvent::ToolCallStarted { call_id, name }).await;
                }
                NormalizedStreamEvent::ToolCallDelta { call_id, delta } => {
                    let _ = progress_tx.send(StreamProgressEvent::ToolCallDelta { call_id, delta }).await;
                }
                NormalizedStreamEvent::Usage { usage } => {
                    pending_usage = Some(usage);
                }
                NormalizedStreamEvent::Done { response } => {
                    let mut response = *response;
                    if response.usage.is_none() {
                        response.usage = pending_usage.take();
                    }
                    yield LLMStreamEvent::Completed {
                        response: Box::new(response),
                    };
                }
            }
        }
    };

    (Box::pin(stream), progress_rx)
}

#[cfg(test)]
#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub(crate) async fn stream_and_render_response_with_options_impl(
    provider: &dyn uni::LLMProvider,
    request: uni::LLMRequest,
    spinner: &PlaceholderSpinner,
    renderer: &mut AnsiRenderer,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    options: StreamSpinnerOptions,
    on_progress: Option<&mut (dyn FnMut(StreamProgressEvent) + Send)>,
) -> Result<(uni::LLMResponse, bool), uni::LLMError> {
    stream_and_render_response_with_options_impl_first_progress_timeout(
        provider,
        request,
        None,
        spinner,
        renderer,
        ctrl_c_state,
        ctrl_c_notify,
        options,
        on_progress,
    )
    .await
}

#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub(crate) async fn stream_and_render_response_with_options_impl_first_progress_timeout(
    provider: &dyn uni::LLMProvider,
    request: uni::LLMRequest,
    first_progress_timeout: Option<Duration>,
    spinner: &PlaceholderSpinner,
    renderer: &mut AnsiRenderer,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    options: StreamSpinnerOptions,
    on_progress: Option<&mut (dyn FnMut(StreamProgressEvent) + Send)>,
) -> Result<(uni::LLMResponse, bool), uni::LLMError> {
    let provider_name = provider.name();

    if ctrl_c_state.is_cancel_requested() {
        spinner.finish_with_restore(true);
        return Err(uni::LLMError::Provider {
            message: error_display::format_llm_error(provider_name, "Interrupted by user"),
            metadata: None,
        });
    }

    let stream_future = provider.stream_normalized(request);
    tokio::pin!(stream_future);
    let first_progress_timeout = first_progress_timeout.map(FirstProgressTimeout::starting_now);

    if ctrl_c_state.is_cancel_requested() || ctrl_c_state.is_exit_requested() {
        spinner.finish_with_restore(true);
        return Err(uni::LLMError::Provider {
            message: error_display::format_llm_error(provider_name, "Interrupted by user"),
            metadata: None,
        });
    }

    let normalized_stream = tokio::select! {
        biased;
        _ = wait_for_stop(ctrl_c_state, ctrl_c_notify) => {
            spinner.finish_with_restore(true);
            return Err(uni::LLMError::Provider { message: error_display::format_llm_error(provider_name, "Interrupted by user"), metadata: None });
        }
        _ = async {
            match first_progress_timeout {
                Some(timeout) => tokio::time::sleep_until(timeout.deadline).await,
                None => std::future::pending().await,
            }
        } => {
            spinner.finish_with_restore(true);
            return Err(first_progress_timeout_error(
                provider_name,
                first_progress_timeout.map_or(Duration::ZERO, |timeout| timeout.budget),
            ));
        }
        result = stream_future => result?,
    };
    let (mut stream, mut progress_events) = normalized_to_legacy_stream(normalized_stream);

    render_stream_with_options_and_progress_impl(
        provider_name,
        &mut stream,
        Some(&mut progress_events),
        first_progress_timeout,
        spinner,
        renderer,
        ctrl_c_state,
        ctrl_c_notify,
        options,
        on_progress,
    )
    .await
}

#[allow(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]
pub(crate) async fn render_stream_with_options_and_progress_impl(
    provider_name: &str,
    stream: &mut uni::BorrowedLLMStream<'_>,
    progress_events: Option<&mut mpsc::Receiver<StreamProgressEvent>>,
    first_progress_timeout: Option<FirstProgressTimeout>,
    spinner: &PlaceholderSpinner,
    renderer: &mut AnsiRenderer,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    options: StreamSpinnerOptions,
    on_progress: Option<&mut (dyn FnMut(StreamProgressEvent) + Send)>,
) -> Result<(uni::LLMResponse, bool), uni::LLMError> {
    render_stream_with_options_and_copilot_runtime_impl(
        provider_name,
        stream,
        progress_events,
        None,
        None,
        first_progress_timeout,
        spinner,
        renderer,
        ctrl_c_state,
        ctrl_c_notify,
        options,
        on_progress,
    )
    .await
}

fn is_output_suppressed(options: &StreamSpinnerOptions) -> bool {
    options.suppress_output
        || options
            .suppress_output_signal
            .as_ref()
            .is_some_and(|signal| signal.load(Ordering::Acquire))
}

/// Forward a plan collected from the stream into the completed response so
/// response processing can validate and persist it through the normal planning
/// workflow. Streaming removes plan markup before rendering, while providers
/// may omit the same text from their completion payload; the semantic handoff
/// must therefore happen before any rendering-suppressed return.
fn merge_streamed_plan_into_response(
    response: &mut uni::LLMResponse,
    streamed_plan_text: Option<String>,
    streamed_visible_content: &str,
) {
    let Some(plan_text) = streamed_plan_text.filter(|text| !text.trim().is_empty()) else {
        return;
    };

    let mut content = response
        .content
        .take()
        .filter(|text| !text.trim().is_empty())
        .or_else(|| (!streamed_visible_content.trim().is_empty()).then(|| streamed_visible_content.to_string()))
        .unwrap_or_default();

    if extract_any_plan(&content).plan_text.is_some() {
        response.content = Some(content);
        return;
    }

    if !content.trim().is_empty() {
        content.push_str("\n\n");
    }
    content.push_str("<proposed_plan>\n");
    content.push_str(plan_text.trim());
    content.push_str("\n</proposed_plan>");
    response.content = Some(content);
}

#[expect(
    clippy::too_many_arguments,
    reason = "Intentional compatibility, platform, test, or API-shape suppression."
)]
pub(crate) async fn render_stream_with_options_and_copilot_runtime_impl(
    provider_name: &str,
    stream: &mut uni::BorrowedLLMStream<'_>,
    progress_events: Option<&mut mpsc::Receiver<StreamProgressEvent>>,
    runtime_requests: Option<&mut mpsc::UnboundedReceiver<CopilotRuntimeRequest>>,
    mut runtime_handler: Option<&mut dyn CopilotRuntimeRequestHandler>,
    first_progress_timeout: Option<FirstProgressTimeout>,
    spinner: &PlaceholderSpinner,
    renderer: &mut AnsiRenderer,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    options: StreamSpinnerOptions,
    mut on_progress: Option<&mut (dyn FnMut(StreamProgressEvent) + Send)>,
) -> Result<(uni::LLMResponse, bool), uni::LLMError> {
    if ctrl_c_state.is_cancel_requested() {
        spinner.finish_with_restore(true);
        return Err(uni::LLMError::Provider {
            message: error_display::format_llm_error(provider_name, "Interrupted by user"),
            metadata: None,
        });
    }

    let supports_streaming_markdown = renderer.supports_streaming_markdown();
    let stream_reasoning_deltas = supports_streaming_markdown && renderer.reasoning_visible();
    let mut final_response: Option<uni::LLMResponse> = None;
    let mut aggregated = String::new();
    let mut spinner_active = true;
    let mut progress_events = progress_events;
    let mut runtime_requests = runtime_requests;
    let mut rendered_line_count = 0usize;
    let finish_spinner = |active: &mut bool, force: bool| {
        if *active {
            if force {
                spinner.finish_with_restore(true);
                *active = false;
            } else if !options.defer_finish {
                spinner.finish();
                *active = false;
            }
        }
    };
    let mut emitted_tokens = false;
    let mut visible_output_observed = false;
    let mut reasoning_state = StreamingReasoningState::new(stream_reasoning_deltas);
    let mut spinner_message_updated = false;
    let mut reasoning_accumulated = String::new();
    let mut pending_content = String::new();
    let mut content_suppressed = false;
    const MAX_PENDING_CONTENT_BYTES: usize = 4_096;
    const STREAM_RENDER_MIN_INTERVAL: Duration = Duration::from_millis(16);
    const STREAM_RENDER_MAX_BYTES: usize = 384;
    const REASONING_RENDER_MAX_BYTES: usize = 256;
    let mut pending_render_bytes = 0usize;
    let mut last_render_at = Instant::now();
    let mut pending_reasoning_delta = String::new();
    let mut pending_reasoning_render_bytes = 0usize;
    let mut last_reasoning_render_at = Instant::now();

    let mut suppress_reasoning_due_to_duplication = false;
    let mut plan_parser = options.strip_proposed_plan_blocks.then(ProposedPlanStreamParser::new);
    let mut streamed_plan_text = None;

    let mut token_count = 0;
    let mut reasoning_token_count = 0;
    let mut last_progress_update = Instant::now();
    let mut reasoning_emitted = false;
    let mut stream_sanitizer = StreamSanitizer::new();
    let mut first_progress_timeout = first_progress_timeout;
    let mut first_wire_event_reported = false;

    loop {
        if ctrl_c_state.is_cancel_requested() || ctrl_c_state.is_exit_requested() {
            finish_spinner(&mut spinner_active, true);
            reasoning_state
                .handle_stream_failure(renderer)
                .map_err(|err| map_render_error(provider_name, err))?;
            return Err(uni::LLMError::Provider {
                message: error_display::format_llm_error(provider_name, "Interrupted by user"),
                metadata: None,
            });
        }

        let maybe_event = tokio::select! {
            biased;
            _ = wait_for_stop(ctrl_c_state, ctrl_c_notify) => {
                finish_spinner(&mut spinner_active, true);
                reasoning_state
                    .handle_stream_failure(renderer)
                    .map_err(|err| map_render_error(provider_name, err))?;
                return Err(uni::LLMError::Provider { message: error_display::format_llm_error(provider_name, "Interrupted by user"), metadata: None });
            }
            _ = async {
                match first_progress_timeout {
                    Some(timeout) => tokio::time::sleep_until(timeout.deadline).await,
                    None => std::future::pending().await,
                }
            } => {
                finish_spinner(&mut spinner_active, true);
                reasoning_state
                    .handle_stream_failure(renderer)
                    .map_err(|err| map_render_error(provider_name, err))?;
                return Err(first_progress_timeout_error(
                    provider_name,
                    first_progress_timeout.map_or(Duration::ZERO, |timeout| timeout.budget),
                ));
            }
            request = async {
                match runtime_requests.as_deref_mut() {
                    Some(receiver) => receiver.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                match request {
                    Some(request) => {
                        first_progress_timeout = None;
                        finish_spinner(&mut spinner_active, true);
                        let Some(handler) = runtime_handler.as_deref_mut() else {
                            return Err(uni::LLMError::Provider {
                                message: error_display::format_llm_error(
                                    provider_name,
                                    "Copilot runtime request arrived without a VT Code handler",
                                ),
                                metadata: None,
                            });
                        };
                        if let Some(callback) = on_progress.as_deref_mut() {
                            callback(StreamProgressEvent::ProviderActivity);
                        }
                        handler.handle_runtime_request(renderer, request).await?;
                        // Return status mapping to the caller after tool/approval work.
                        if let Some(callback) = on_progress.as_deref_mut() {
                            callback(StreamProgressEvent::ProviderActivity);
                        }
                        continue;
                    }
                    None => {
                        runtime_requests = None;
                        continue;
                    }
                }
            }
            progress_event = async {
                match progress_events.as_deref_mut() {
                    Some(receiver) => receiver.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                match progress_event {
                    Some(StreamProgressEvent::ToolCallStarted { call_id, name }) => {
                        first_progress_timeout = None;
                        finish_spinner(&mut spinner_active, false);
                        if let Some(tool_name) = name.as_deref().filter(|value| !value.is_empty()) {
                            spinner.update_message(format!("Preparing tool call: {tool_name}"));
                            spinner_message_updated = true;
                        }
                        if let Some(callback) = on_progress.as_deref_mut() {
                            callback(StreamProgressEvent::ToolCallStarted { call_id, name });
                        }
                        continue;
                    }
                    Some(StreamProgressEvent::ToolCallDelta { call_id, delta }) => {
                        first_progress_timeout = None;
                        if let Some(callback) = on_progress.as_deref_mut() {
                            callback(StreamProgressEvent::ToolCallDelta { call_id, delta });
                        }
                        continue;
                    }
                    Some(
                        StreamProgressEvent::OutputDelta(_)
                        | StreamProgressEvent::ReasoningDelta(_)
                        | StreamProgressEvent::ReasoningStage(_),
                    ) => {
                        first_progress_timeout = None;
                        continue;
                    }
                    Some(StreamProgressEvent::ProviderActivity) => {
                        first_progress_timeout = None;
                        if let Some(callback) = on_progress.as_deref_mut() {
                            callback(StreamProgressEvent::ProviderActivity);
                        }
                        continue;
                    }
                    None => {
                        progress_events = None;
                        continue;
                    }
                }
            }
            event = stream.next() => event,
        };

        let Some(event_result) = maybe_event else {
            break;
        };

        if event_result.is_ok() && !first_wire_event_reported {
            first_wire_event_reported = true;
            if let Some(callback) = on_progress.as_deref_mut() {
                callback(StreamProgressEvent::ProviderActivity);
            }
        }
        match event_result {
            Ok(LLMStreamEvent::Token { delta }) => {
                first_progress_timeout = None;
                token_count += 1;
                let output_suppressed = is_output_suppressed(&options);
                let mut visible_delta = if let Some(parser) = plan_parser.as_mut() {
                    parser.consume(&delta)
                } else {
                    delta
                };

                let sanitized = stream_sanitizer.process_delta(&visible_delta);
                visible_delta = sanitized.visible_delta;
                let has_aggregated_override = sanitized.aggregated_override.is_some();
                if let Some(cleaned_aggregated) = sanitized.aggregated_override {
                    aggregated = cleaned_aggregated;
                }

                if stream_reasoning_deltas && !output_suppressed && !pending_reasoning_delta.is_empty() {
                    flush_pending_reasoning(
                        provider_name,
                        renderer,
                        &mut reasoning_state,
                        &mut on_progress,
                        &mut pending_reasoning_delta,
                        &mut pending_reasoning_render_bytes,
                        &mut last_reasoning_render_at,
                        &mut reasoning_emitted,
                    )?;
                }
                if !output_suppressed
                    && !reasoning_emitted
                    && reasoning_token_count > 0
                    && !reasoning_state.is_deferred()
                {
                    let rendered = reasoning_state
                        .flush_pending(renderer)
                        .map_err(|err| map_render_error(provider_name, err))?;
                    if rendered {
                        reasoning_emitted = true;
                    }
                }

                let is_drafting_plan = plan_parser.as_ref().is_some_and(|parser| parser.is_drafting_plan());

                if !spinner_message_updated {
                    if is_drafting_plan {
                        let len = plan_parser.as_ref().map(|parser| parser.drafted_plan_len()).unwrap_or(0);
                        spinner.update_message(format!("Drafting plan... ({len} chars)"));
                    } else {
                        spinner.update_message("Receiving response...");
                    }
                    spinner_message_updated = true;
                } else if is_drafting_plan {
                    if last_progress_update.elapsed() >= Duration::from_millis(200) {
                        let len = plan_parser.as_ref().map(|parser| parser.drafted_plan_len()).unwrap_or(0);
                        spinner.update_message(format!("Drafting plan... ({len} chars)"));
                        last_progress_update = Instant::now();
                    }
                } else if last_progress_update.elapsed() >= Duration::from_millis(500) {
                    spinner.update_message(format!("Receiving response... ({token_count} tokens)"));
                    last_progress_update = Instant::now();
                }
                finish_spinner(&mut spinner_active, false);
                if visible_delta.is_empty() {
                    continue;
                }
                if output_suppressed {
                    if !has_aggregated_override {
                        aggregated.push_str(&visible_delta);
                    }
                    continue;
                }
                if let Some(callback) = on_progress.as_deref_mut() {
                    callback(StreamProgressEvent::OutputDelta(visible_delta.clone()));
                    visible_output_observed |= !visible_delta.trim().is_empty();
                }
                if !supports_streaming_markdown && !reasoning_accumulated.trim().is_empty() && !emitted_tokens {
                    pending_content.push_str(&visible_delta);
                    if pending_content.len() >= MAX_PENDING_CONTENT_BYTES {
                        aggregated.push_str(&pending_content);
                        pending_content.clear();
                    }
                    continue;
                }

                if !has_aggregated_override {
                    aggregated.push_str(&visible_delta);
                }
                if supports_streaming_markdown {
                    pending_render_bytes = pending_render_bytes.saturating_add(visible_delta.len());
                    let should_render_now = !emitted_tokens
                        || visible_delta.contains('\n')
                        || pending_render_bytes >= STREAM_RENDER_MAX_BYTES
                        || last_render_at.elapsed() >= STREAM_RENDER_MIN_INTERVAL;
                    if should_render_now {
                        rendered_line_count = stream_markdown_with_provider_error(
                            provider_name,
                            renderer,
                            &aggregated,
                            rendered_line_count,
                        )?;
                        emitted_tokens = true;
                        pending_render_bytes = 0;
                        last_render_at = Instant::now();
                    }
                }
            }
            Ok(LLMStreamEvent::Reasoning { delta }) => {
                first_progress_timeout = None;
                reasoning_token_count += 1;
                let output_suppressed = is_output_suppressed(&options);
                if !spinner_message_updated {
                    spinner.update_message("Processing reasoning...");
                    spinner_message_updated = true;
                } else if last_progress_update.elapsed() >= Duration::from_millis(500) {
                    spinner.update_message(format!("Processing reasoning... ({reasoning_token_count} tokens)"));
                    last_progress_update = Instant::now();
                }
                finish_spinner(&mut spinner_active, false);
                reasoning_accumulated.push_str(&delta);
                if stream_reasoning_deltas && !output_suppressed {
                    pending_reasoning_delta.push_str(&delta);
                    pending_reasoning_render_bytes = pending_reasoning_render_bytes.saturating_add(delta.len());
                    let should_render_now = !reasoning_emitted
                        || delta.contains('\n')
                        || pending_reasoning_render_bytes >= REASONING_RENDER_MAX_BYTES
                        || last_reasoning_render_at.elapsed() >= STREAM_RENDER_MIN_INTERVAL;
                    if should_render_now {
                        flush_pending_reasoning(
                            provider_name,
                            renderer,
                            &mut reasoning_state,
                            &mut on_progress,
                            &mut pending_reasoning_delta,
                            &mut pending_reasoning_render_bytes,
                            &mut last_reasoning_render_at,
                            &mut reasoning_emitted,
                        )?;
                    }
                }
            }
            Ok(LLMStreamEvent::ReasoningStage { stage }) => {
                first_progress_timeout = None;
                if stream_reasoning_deltas && !is_output_suppressed(&options) && !pending_reasoning_delta.is_empty() {
                    flush_pending_reasoning(
                        provider_name,
                        renderer,
                        &mut reasoning_state,
                        &mut on_progress,
                        &mut pending_reasoning_delta,
                        &mut pending_reasoning_render_bytes,
                        &mut last_reasoning_render_at,
                        &mut reasoning_emitted,
                    )?;
                }
                if stream_reasoning_deltas {
                    if !is_output_suppressed(&options) {
                        if let Some(callback) = on_progress.as_deref_mut() {
                            callback(StreamProgressEvent::ReasoningStage(stage.clone()));
                        }
                        spinner.set_reasoning_stage(Some(stage));
                    }
                }
            }
            Ok(LLMStreamEvent::ReasoningSignature { .. }) => {
                // Signature field not currently processed in UI stream
            }
            Ok(LLMStreamEvent::Completed { response }) => {
                final_response = Some(*response);
            }
            Err(err) => {
                finish_spinner(&mut spinner_active, true);
                reasoning_state
                    .handle_stream_failure(renderer)
                    .map_err(|render_err| map_render_error(provider_name, render_err))?;
                return Err(err);
            }
        }
    }

    finish_spinner(&mut spinner_active, false);

    if !is_output_suppressed(&options) && stream_reasoning_deltas && !pending_reasoning_delta.is_empty() {
        let rendered = flush_pending_reasoning_delta(
            provider_name,
            renderer,
            &mut reasoning_state,
            &mut on_progress,
            &mut pending_reasoning_delta,
        )?;
        if rendered {
            reasoning_emitted = true;
        }
    }

    if let Some(parser) = plan_parser.as_mut() {
        let trailing_plan_parse = parser.finish();
        streamed_plan_text = trailing_plan_parse.plan_text;
        if !is_output_suppressed(&options) && !trailing_plan_parse.stripped_text.is_empty() {
            if let Some(callback) = on_progress.as_deref_mut() {
                callback(StreamProgressEvent::OutputDelta(trailing_plan_parse.stripped_text.clone()));
                visible_output_observed |= !trailing_plan_parse.stripped_text.trim().is_empty();
            }
            if !supports_streaming_markdown && !reasoning_accumulated.trim().is_empty() && !emitted_tokens {
                pending_content.push_str(&trailing_plan_parse.stripped_text);
                if pending_content.len() >= MAX_PENDING_CONTENT_BYTES {
                    aggregated.push_str(&pending_content);
                    pending_content.clear();
                }
            } else {
                aggregated.push_str(&trailing_plan_parse.stripped_text);
                if supports_streaming_markdown {
                    rendered_line_count =
                        stream_markdown_with_provider_error(provider_name, renderer, &aggregated, rendered_line_count)?;
                    emitted_tokens = true;
                }
            }
        }
    }

    if !is_output_suppressed(&options) && supports_streaming_markdown && pending_render_bytes > 0 {
        rendered_line_count =
            stream_markdown_with_provider_error(provider_name, renderer, &aggregated, rendered_line_count)?;
        emitted_tokens = true;
    }

    let mut response = match final_response {
        Some(response) => response,
        None => {
            reasoning_state
                .handle_stream_failure(renderer)
                .map_err(|err| map_render_error(provider_name, err))?;
            finish_spinner(&mut spinner_active, true);
            let formatted_error =
                error_display::format_llm_error(provider_name, "Stream ended without a completion event");
            return Err(uni::LLMError::Provider { message: formatted_error, metadata: None });
        }
    };

    let streamed_visible_content = if pending_content.is_empty() {
        Cow::Borrowed(aggregated.as_str())
    } else {
        let mut visible_content = String::with_capacity(aggregated.len() + pending_content.len());
        visible_content.push_str(&aggregated);
        visible_content.push_str(&pending_content);
        Cow::Owned(visible_content)
    };
    merge_streamed_plan_into_response(&mut response, streamed_plan_text, streamed_visible_content.as_ref());

    if is_output_suppressed(&options) {
        return Ok((response, false));
    }

    if !pending_content.is_empty() && !content_suppressed {
        let reasoning_for_compare = reasoning_accumulated.as_str();
        if !reasoning_for_compare.trim().is_empty()
            && reasoning_matches_content(reasoning_for_compare, &pending_content)
        {
            suppress_reasoning_due_to_duplication = true;
        }
    }

    if !pending_content.is_empty() && !content_suppressed {
        let prefix_len = common_prefix_len(&reasoning_accumulated, &pending_content);
        let reasoning_prefix = !reasoning_accumulated.is_empty() && prefix_len == reasoning_accumulated.len();
        let pending = std::mem::take(&mut pending_content);
        let render_text = if reasoning_prefix {
            pending.get(prefix_len..).unwrap_or("").to_string()
        } else {
            pending
        };

        if reasoning_prefix && render_text.is_empty() && (reasoning_state.rendered_reasoning() || reasoning_emitted) {
            content_suppressed = true;
        } else {
            aggregated.push_str(&render_text);
            if supports_streaming_markdown {
                let prev_count = if aggregated.trim().is_empty() {
                    0
                } else {
                    rendered_line_count
                };
                let _ = stream_markdown_with_provider_error(provider_name, renderer, &aggregated, prev_count)?;
                emitted_tokens = true;
            }
            if reasoning_prefix && (reasoning_state.rendered_reasoning() || reasoning_emitted) {
                content_suppressed = true;
            }
        }
    }

    let content_for_render = if options.strip_proposed_plan_blocks {
        response
            .content
            .as_deref()
            .map(extract_any_plan)
            .map(|extraction| strip_plan_persistence_policy_line(&extraction.stripped_text))
    } else {
        response.content.clone()
    };
    let content_for_render = content_for_render.map(|text| stream_sanitizer.finalize(&text));
    let has_renderable_content = content_for_render
        .as_deref()
        .map(|content| !content.trim().is_empty())
        .unwrap_or(false);

    if !content_suppressed && let Some(content) = content_for_render.as_deref() {
        let content_trimmed = content.trim();
        if !content_trimmed.is_empty() {
            let reasoning_dupes_content = reasoning_matches_content(reasoning_accumulated.as_str(), content);

            if reasoning_dupes_content {
                suppress_reasoning_due_to_duplication = true;
            }

            let already_rendered = supports_streaming_markdown
                && emitted_tokens
                && !aggregated.trim().is_empty()
                && aggregated.trim() == content_trimmed;

            reasoning_state
                .finalize(
                    renderer,
                    Some(reasoning_accumulated.as_str()),
                    reasoning_emitted,
                    suppress_reasoning_due_to_duplication,
                )
                .map_err(|err| map_render_error(provider_name, err))?;

            if !already_rendered {
                if supports_streaming_markdown {
                    let prev_count = if aggregated.trim().is_empty() {
                        0
                    } else {
                        rendered_line_count
                    };
                    let _ = stream_markdown_with_provider_error(provider_name, renderer, content, prev_count)?;
                } else {
                    renderer
                        .line(MessageStyle::Response, content)
                        .map_err(|err| map_render_error(provider_name, err))?;
                }
                if !visible_output_observed && let Some(callback) = on_progress {
                    callback(StreamProgressEvent::OutputDelta(content.to_owned()));
                }
                emitted_tokens = true;
                aggregated = content.to_string();
            }
        }
    }

    if !has_renderable_content || aggregated.trim().is_empty() || suppress_reasoning_due_to_duplication {
        let suppress_reasoning = suppress_reasoning_due_to_duplication;
        reasoning_state
            .finalize(renderer, Some(reasoning_accumulated.as_str()), reasoning_emitted, suppress_reasoning)
            .map_err(|err| map_render_error(provider_name, err))?;
    }

    let response_rendered = emitted_tokens || reasoning_emitted || reasoning_state.rendered_reasoning();

    Ok((response, response_rendered))
}

#[cfg(test)]
mod tests;
