use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::StreamExt;
use tokio::sync::{Notify, mpsc};

use crate::agent::runloop::unified::plan_blocks::{
    ProposedPlanStreamParser, extract_proposed_plan,
};
use crate::agent::runloop::unified::turn::harmony::strip_harmony_syntax;
use vtcode_core::copilot::CopilotRuntimeRequest;
use vtcode_core::llm::error_display;
use vtcode_core::llm::provider::{self as uni, LLMStreamEvent, NormalizedStreamEvent};
use vtcode_core::llm::providers::clean_reasoning_text;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::state::CtrlCState;
use super::ui_interaction::{PlaceholderSpinner, StreamProgressEvent, StreamSpinnerOptions};
use super::ui_interaction_stream_helpers::{
    common_prefix_len, map_render_error, reasoning_matches_content,
};
#[derive(Default)]
struct StreamingReasoningState {
    buffered: String,
    render_inline: bool,
    render_output: bool,
    defer_rendering: bool,
    started: bool,
    rendered_any: bool,
}

impl StreamingReasoningState {
    fn new(inline_enabled: bool) -> Self {
        Self {
            buffered: String::new(),
            render_inline: inline_enabled,
            render_output: true,
            defer_rendering: !inline_enabled,
            started: false,
            rendered_any: false,
        }
    }

    fn handle_delta(&mut self, renderer: &mut AnsiRenderer, delta: &str) -> Result<bool> {
        if !self.render_output || !self.render_inline || self.defer_rendering {
            self.buffered.push_str(delta);
            return Ok(false);
        }

        self.started = true;
        renderer.inline_with_style(MessageStyle::Reasoning, delta)?;
        self.rendered_any = true;
        Ok(true)
    }

    fn flush_pending(&mut self, renderer: &mut AnsiRenderer) -> Result<bool> {
        if !self.render_output {
            self.buffered.clear();
            return Ok(false);
        }
        if !self.buffered.is_empty() {
            let cleaned = clean_reasoning_text(&self.buffered);
            if !cleaned.is_empty() {
                if self.render_inline && self.started {
                    renderer.inline_with_style(MessageStyle::Reasoning, "\n")?;
                }
                renderer.line(MessageStyle::Reasoning, &cleaned)?;
                self.rendered_any = true;
            }
            self.buffered.clear();
            return Ok(self.rendered_any);
        }
        Ok(false)
    }

    fn finalize(
        &mut self,
        renderer: &mut AnsiRenderer,
        final_reasoning: Option<&str>,
        reasoning_already_emitted: bool,
        suppress_reasoning: bool,
    ) -> Result<()> {
        if !self.render_output {
            self.buffered.clear();
            return Ok(());
        }
        if suppress_reasoning {
            self.buffered.clear();
            return Ok(());
        }

        self.flush_pending(renderer)?;
        if self.rendered_any {
            return Ok(());
        }

        if !reasoning_already_emitted
            && let Some(reasoning_text) = final_reasoning
            && !reasoning_text.trim().is_empty()
        {
            let cleaned_reasoning = clean_reasoning_text(reasoning_text);
            if !cleaned_reasoning.trim().is_empty() {
                renderer.line(MessageStyle::Reasoning, &cleaned_reasoning)?;
                self.rendered_any = true;

                use super::reasoning::analyze_reasoning;
                let analysis = analyze_reasoning(&cleaned_reasoning);
                if analysis.has_concerns() {
                    tracing::debug!(
                        concern = ?analysis.priority_concern(),
                        "Reasoning concern detected in CoT output"
                    );
                }
            }
        }
        Ok(())
    }

    fn handle_stream_failure(&mut self, _renderer: &mut AnsiRenderer) -> Result<()> {
        self.buffered.clear();
        Ok(())
    }

    fn rendered_reasoning(&self) -> bool {
        self.rendered_any
    }

    fn is_deferred(&self) -> bool {
        self.defer_rendering
    }
}

fn flush_pending_reasoning_delta(
    provider_name: &str,
    renderer: &mut AnsiRenderer,
    reasoning_state: &mut StreamingReasoningState,
    on_progress: &mut Option<&mut (dyn FnMut(StreamProgressEvent) + Send)>,
    pending_delta: &mut String,
) -> Result<bool, uni::LLMError> {
    if pending_delta.is_empty() {
        return Ok(false);
    }

    let delta = std::mem::take(pending_delta);
    if let Some(callback) = on_progress.as_deref_mut() {
        callback(StreamProgressEvent::ReasoningDelta(delta.clone()));
    }

    reasoning_state
        .handle_delta(renderer, &delta)
        .map_err(|err| map_render_error(provider_name, err))
}

fn flush_pending_reasoning(
    provider_name: &str,
    renderer: &mut AnsiRenderer,
    reasoning_state: &mut StreamingReasoningState,
    on_progress: &mut Option<&mut (dyn FnMut(StreamProgressEvent) + Send)>,
    pending_delta: &mut String,
    pending_render_bytes: &mut usize,
    last_render_at: &mut Instant,
    reasoning_emitted: &mut bool,
) -> Result<(), uni::LLMError> {
    let rendered = flush_pending_reasoning_delta(
        provider_name,
        renderer,
        reasoning_state,
        on_progress,
        pending_delta,
    )?;
    if rendered {
        *reasoning_emitted = true;
    }
    *pending_render_bytes = 0;
    *last_render_at = Instant::now();
    Ok(())
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

const HARMONY_MARKERS: &[&str] = &[
    "<|start|>",
    "<|channel|>",
    "<|message|>",
    "<|call|>",
    "<|return|>",
    "<|end|>",
];

const HARMONY_TERMINATORS: &[&str] = &["<|call|>", "<|return|>", "<|end|>"];

fn contains_harmony_marker(text: &str) -> bool {
    text.contains("<|") || HARMONY_MARKERS.iter().any(|marker| text.contains(marker))
}

fn incomplete_harmony_block_start(raw: &str) -> Option<usize> {
    let start_pos = raw.rfind("<|start|>")?;
    let tail = &raw[start_pos..];
    let has_terminator = HARMONY_TERMINATORS
        .iter()
        .any(|terminator| tail.contains(terminator));
    if has_terminator {
        None
    } else {
        Some(start_pos)
    }
}

fn sanitize_harmony_stream_text(raw: &str) -> String {
    let stable_raw = if let Some(start_pos) = incomplete_harmony_block_start(raw) {
        &raw[..start_pos]
    } else {
        raw
    };
    strip_harmony_syntax(stable_raw)
}

fn sanitize_harmony_final_text(text: String) -> String {
    if contains_harmony_marker(&text) {
        strip_harmony_syntax(&text)
    } else {
        text
    }
}

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
) -> (uni::LLMStream, mpsc::UnboundedReceiver<StreamProgressEvent>) {
    let (progress_tx, progress_rx) = mpsc::unbounded_channel();
    let stream = try_stream! {
        let mut pending_usage = None;

        while let Some(event) = stream.next().await {
            match event? {
                NormalizedStreamEvent::TextDelta { delta } => {
                    yield LLMStreamEvent::Token { delta };
                }
                NormalizedStreamEvent::ReasoningDelta { delta } => {
                    yield LLMStreamEvent::Reasoning { delta };
                }
                NormalizedStreamEvent::ToolCallStart { call_id, name } => {
                    let _ = progress_tx.send(StreamProgressEvent::ToolCallStarted { call_id, name });
                }
                NormalizedStreamEvent::ToolCallDelta { call_id, delta } => {
                    let _ = progress_tx.send(StreamProgressEvent::ToolCallDelta { call_id, delta });
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

    if ctrl_c_state.is_cancel_requested() || ctrl_c_state.is_exit_requested() {
        spinner.finish_with_restore(true);
        return Err(uni::LLMError::Provider {
            message: error_display::format_llm_error(provider_name, "Interrupted by user"),
            metadata: None,
        });
    }

    let normalized_stream = tokio::select! {
        biased;
        _ = ctrl_c_notify.notified() => {
            spinner.finish_with_restore(true);
            return Err(uni::LLMError::Provider { message: error_display::format_llm_error(provider_name, "Interrupted by user"), metadata: None });
        }
        result = stream_future => result?,
    };
    let (mut stream, mut progress_events) = normalized_to_legacy_stream(normalized_stream);

    render_stream_with_options_and_progress_impl(
        provider_name,
        &mut stream,
        Some(&mut progress_events),
        spinner,
        renderer,
        ctrl_c_state,
        ctrl_c_notify,
        options,
        on_progress,
    )
    .await
}

pub(crate) async fn render_stream_with_options_and_progress_impl(
    provider_name: &str,
    stream: &mut uni::BorrowedLLMStream<'_>,
    progress_events: Option<&mut mpsc::UnboundedReceiver<StreamProgressEvent>>,
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

#[allow(clippy::too_many_arguments)]
pub(crate) async fn render_stream_with_options_and_copilot_runtime_impl(
    provider_name: &str,
    stream: &mut uni::BorrowedLLMStream<'_>,
    progress_events: Option<&mut mpsc::UnboundedReceiver<StreamProgressEvent>>,
    runtime_requests: Option<&mut mpsc::UnboundedReceiver<CopilotRuntimeRequest>>,
    mut runtime_handler: Option<&mut dyn CopilotRuntimeRequestHandler>,
    timeout_budget: Option<Duration>,
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
    let mut plan_parser = options
        .strip_proposed_plan_blocks
        .then(ProposedPlanStreamParser::new);

    let mut token_count = 0;
    let mut reasoning_token_count = 0;
    let mut last_progress_update = std::time::Instant::now();
    let mut reasoning_emitted = false;
    let mut harmony_stream_mode = false;
    let mut harmony_raw_stream = String::new();
    let mut harmony_visible_stream = String::new();
    let mut timeout_deadline = timeout_budget.map(|budget| tokio::time::Instant::now() + budget);

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
            _ = ctrl_c_notify.notified() => {
                finish_spinner(&mut spinner_active, true);
                reasoning_state
                    .handle_stream_failure(renderer)
                    .map_err(|err| map_render_error(provider_name, err))?;
                return Err(uni::LLMError::Provider { message: error_display::format_llm_error(provider_name, "Interrupted by user"), metadata: None });
            }
            _ = async {
                match timeout_deadline {
                    Some(deadline) => tokio::time::sleep_until(deadline).await,
                    None => std::future::pending().await,
                }
            } => {
                finish_spinner(&mut spinner_active, true);
                reasoning_state
                    .handle_stream_failure(renderer)
                    .map_err(|err| map_render_error(provider_name, err))?;
                return Err(uni::LLMError::Provider {
                    message: error_display::format_llm_error(
                        provider_name,
                        &format!(
                            "LLM request timed out after {} seconds",
                            timeout_budget.unwrap_or_default().as_secs()
                        ),
                    ),
                    metadata: None,
                });
            }
            request = async {
                match runtime_requests.as_deref_mut() {
                    Some(receiver) => receiver.recv().await,
                    None => std::future::pending().await,
                }
            } => {
                match request {
                    Some(request) => {
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
                        let blocked_started_at = tokio::time::Instant::now();
                        handler.handle_runtime_request(renderer, request).await?;
                        if let Some(deadline) = timeout_deadline.as_mut() {
                            *deadline += blocked_started_at.elapsed();
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
                        if let Some(callback) = on_progress.as_deref_mut() {
                            callback(StreamProgressEvent::ToolCallDelta { call_id, delta });
                        }
                        continue;
                    }
                    Some(
                        StreamProgressEvent::OutputDelta(_)
                        | StreamProgressEvent::ReasoningDelta(_)
                        | StreamProgressEvent::ReasoningStage(_),
                    ) => continue,
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

        match event_result {
            Ok(LLMStreamEvent::Token { delta }) => {
                token_count += 1;
                let mut visible_delta = if let Some(parser) = plan_parser.as_mut() {
                    parser.consume(&delta)
                } else {
                    delta
                };

                if !harmony_stream_mode && contains_harmony_marker(&visible_delta) {
                    harmony_stream_mode = true;
                }
                if harmony_stream_mode {
                    harmony_raw_stream.push_str(&visible_delta);
                    let sanitized = sanitize_harmony_stream_text(&harmony_raw_stream);
                    let prefix_len = common_prefix_len(&harmony_visible_stream, &sanitized);
                    visible_delta = sanitized.get(prefix_len..).unwrap_or_default().to_string();
                    harmony_visible_stream = sanitized;
                    aggregated = harmony_visible_stream.clone();
                }

                if stream_reasoning_deltas && !pending_reasoning_delta.is_empty() {
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
                if !reasoning_emitted && reasoning_token_count > 0 && !reasoning_state.is_deferred()
                {
                    let rendered = reasoning_state
                        .flush_pending(renderer)
                        .map_err(|err| map_render_error(provider_name, err))?;
                    if rendered {
                        reasoning_emitted = true;
                    }
                }

                if !spinner_message_updated {
                    spinner.update_message("Receiving response...");
                    spinner_message_updated = true;
                } else if last_progress_update.elapsed() >= std::time::Duration::from_millis(500) {
                    spinner
                        .update_message(format!("Receiving response... ({} tokens)", token_count));
                    last_progress_update = std::time::Instant::now();
                }
                finish_spinner(&mut spinner_active, false);
                if visible_delta.is_empty() {
                    continue;
                }
                if let Some(callback) = on_progress.as_deref_mut() {
                    callback(StreamProgressEvent::OutputDelta(visible_delta.clone()));
                }
                if !supports_streaming_markdown
                    && !reasoning_accumulated.trim().is_empty()
                    && !emitted_tokens
                {
                    pending_content.push_str(&visible_delta);
                    if pending_content.len() >= MAX_PENDING_CONTENT_BYTES {
                        aggregated.push_str(&pending_content);
                        pending_content.clear();
                    }
                    continue;
                }

                if !harmony_stream_mode {
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
                reasoning_token_count += 1;
                if !spinner_message_updated {
                    spinner.update_message("Processing reasoning...");
                    spinner_message_updated = true;
                } else if last_progress_update.elapsed() >= std::time::Duration::from_millis(500) {
                    spinner.update_message(format!(
                        "Processing reasoning... ({} tokens)",
                        reasoning_token_count
                    ));
                    last_progress_update = std::time::Instant::now();
                }
                finish_spinner(&mut spinner_active, false);
                reasoning_accumulated.push_str(&delta);
                if stream_reasoning_deltas {
                    pending_reasoning_delta.push_str(&delta);
                    pending_reasoning_render_bytes =
                        pending_reasoning_render_bytes.saturating_add(delta.len());
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
                if stream_reasoning_deltas && !pending_reasoning_delta.is_empty() {
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
                    if let Some(callback) = on_progress.as_deref_mut() {
                        callback(StreamProgressEvent::ReasoningStage(stage.clone()));
                    }
                    spinner.set_reasoning_stage(Some(stage));
                }
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

    if stream_reasoning_deltas && !pending_reasoning_delta.is_empty() {
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
        if !trailing_plan_parse.stripped_text.is_empty() {
            if let Some(callback) = on_progress {
                callback(StreamProgressEvent::OutputDelta(
                    trailing_plan_parse.stripped_text.clone(),
                ));
            }
            if !supports_streaming_markdown
                && !reasoning_accumulated.trim().is_empty()
                && !emitted_tokens
            {
                pending_content.push_str(&trailing_plan_parse.stripped_text);
                if pending_content.len() >= MAX_PENDING_CONTENT_BYTES {
                    aggregated.push_str(&pending_content);
                    pending_content.clear();
                }
            } else {
                aggregated.push_str(&trailing_plan_parse.stripped_text);
                if supports_streaming_markdown {
                    rendered_line_count = stream_markdown_with_provider_error(
                        provider_name,
                        renderer,
                        &aggregated,
                        rendered_line_count,
                    )?;
                    emitted_tokens = true;
                }
            }
        }
    }

    if supports_streaming_markdown && pending_render_bytes > 0 {
        rendered_line_count = stream_markdown_with_provider_error(
            provider_name,
            renderer,
            &aggregated,
            rendered_line_count,
        )?;
        emitted_tokens = true;
    }

    let response = match final_response {
        Some(response) => response,
        None => {
            reasoning_state
                .handle_stream_failure(renderer)
                .map_err(|err| map_render_error(provider_name, err))?;
            finish_spinner(&mut spinner_active, true);
            let formatted_error = error_display::format_llm_error(
                provider_name,
                "Stream ended without a completion event",
            );
            return Err(uni::LLMError::Provider {
                message: formatted_error,
                metadata: None,
            });
        }
    };

    if !pending_content.is_empty() && !content_suppressed {
        let reasoning_for_compare = response
            .reasoning
            .as_deref()
            .unwrap_or(reasoning_accumulated.as_str());
        if !reasoning_for_compare.trim().is_empty()
            && reasoning_matches_content(reasoning_for_compare, &pending_content)
        {
            suppress_reasoning_due_to_duplication = true;
        }
    }

    if !pending_content.is_empty() && !content_suppressed {
        let prefix_len = common_prefix_len(&reasoning_accumulated, &pending_content);
        let reasoning_prefix =
            !reasoning_accumulated.is_empty() && prefix_len == reasoning_accumulated.len();
        let pending = std::mem::take(&mut pending_content);
        let render_text = if reasoning_prefix {
            pending.get(prefix_len..).unwrap_or("").to_string()
        } else {
            pending
        };

        if reasoning_prefix
            && render_text.is_empty()
            && (reasoning_state.rendered_reasoning() || reasoning_emitted)
        {
            content_suppressed = true;
        } else {
            aggregated.push_str(&render_text);
            if supports_streaming_markdown {
                let prev_count = if aggregated.trim().is_empty() {
                    0
                } else {
                    rendered_line_count
                };
                let _ = stream_markdown_with_provider_error(
                    provider_name,
                    renderer,
                    &aggregated,
                    prev_count,
                )?;
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
            .map(extract_proposed_plan)
            .map(|extraction| extraction.stripped_text)
    } else {
        response.content.clone()
    };
    let content_for_render = content_for_render.map(sanitize_harmony_final_text);
    let has_renderable_content = content_for_render
        .as_deref()
        .map(|content| !content.trim().is_empty())
        .unwrap_or(false);

    if !content_suppressed && let Some(content) = content_for_render.as_deref() {
        let content_trimmed = content.trim();
        if !content_trimmed.is_empty() {
            let reasoning_dupes_content = response
                .reasoning
                .as_deref()
                .map(|reasoning| reasoning_matches_content(reasoning, content))
                .unwrap_or(false);

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
                    response.reasoning.as_deref(),
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
                    let _ = stream_markdown_with_provider_error(
                        provider_name,
                        renderer,
                        content,
                        prev_count,
                    )?;
                } else {
                    renderer
                        .line(MessageStyle::Response, content)
                        .map_err(|err| map_render_error(provider_name, err))?;
                }
                emitted_tokens = true;
                aggregated = content.to_string();
            }
        }
    }

    let rendered_reasoning_before = reasoning_state.rendered_reasoning();
    if !has_renderable_content
        || aggregated.trim().is_empty()
        || suppress_reasoning_due_to_duplication
    {
        let suppress_reasoning = suppress_reasoning_due_to_duplication;
        reasoning_state
            .finalize(
                renderer,
                response.reasoning.as_deref(),
                reasoning_emitted,
                suppress_reasoning,
            )
            .map_err(|err| map_render_error(provider_name, err))?;
    }

    if !emitted_tokens
        && aggregated.trim().is_empty()
        && !has_renderable_content
        && !rendered_reasoning_before
        && renderer.reasoning_visible()
        && let Some(reasoning) = response.reasoning.as_deref()
    {
        let reasoning_trimmed = clean_reasoning_text(reasoning.trim());
        if !reasoning_trimmed.is_empty() {
            if supports_streaming_markdown {
                let _ = stream_markdown_with_provider_error(
                    provider_name,
                    renderer,
                    &reasoning_trimmed,
                    0,
                )?;
            } else {
                renderer
                    .line(MessageStyle::Response, &reasoning_trimmed)
                    .map_err(|err| map_render_error(provider_name, err))?;
            }
            emitted_tokens = true;
        }
    }

    let response_rendered =
        emitted_tokens || reasoning_emitted || reasoning_state.rendered_reasoning();

    Ok((response, response_rendered))
}

#[cfg(test)]
mod tests {
    use super::{
        CopilotRuntimeRequestHandler, render_stream_with_options_and_copilot_runtime_impl,
    };
    use crate::agent::runloop::unified::state::CtrlCState;
    use crate::agent::runloop::unified::ui_interaction::{
        PlaceholderSpinner, StreamSpinnerOptions,
    };
    use async_trait::async_trait;
    use futures::stream;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::{Notify, mpsc};
    use vtcode_core::copilot::{
        CopilotObservedToolCall, CopilotObservedToolCallStatus, CopilotRuntimeRequest,
    };
    use vtcode_core::llm::provider::{self as uni, FinishReason, LLMResponse, LLMStreamEvent};
    use vtcode_core::utils::ansi::AnsiRenderer;
    use vtcode_tui::app::{InlineCommand, InlineHandle};

    struct SleepingRuntimeHandler {
        sleep_for: Duration,
    }

    #[async_trait]
    impl CopilotRuntimeRequestHandler for SleepingRuntimeHandler {
        async fn handle_runtime_request(
            &mut self,
            _renderer: &mut AnsiRenderer,
            _request: CopilotRuntimeRequest,
        ) -> Result<(), uni::LLMError> {
            tokio::time::sleep(self.sleep_for).await;
            Ok(())
        }
    }

    fn build_spinner() -> PlaceholderSpinner {
        let (tx, _rx) = mpsc::unbounded_channel::<InlineCommand>();
        let handle = InlineHandle::new_for_tests(tx);
        PlaceholderSpinner::new(&handle, None, None, "")
    }

    fn completed_response(content: &str) -> LLMResponse {
        LLMResponse {
            content: Some(content.to_string()),
            model: "mock-model".to_string(),
            tool_calls: None,
            usage: None,
            finish_reason: FinishReason::Stop,
            reasoning: None,
            reasoning_details: None,
            organization_id: None,
            request_id: None,
            tool_references: vec![],
        }
    }

    #[tokio::test]
    async fn copilot_runtime_prompt_time_does_not_consume_timeout_budget() {
        let spinner = build_spinner();
        let mut renderer = AnsiRenderer::stdout();
        let ctrl_c_state = Arc::new(CtrlCState::new());
        let ctrl_c_notify = Arc::new(Notify::new());

        let mut stream: uni::LLMStream = Box::pin(stream::once(async {
            tokio::time::sleep(Duration::from_millis(5)).await;
            Ok(LLMStreamEvent::Completed {
                response: Box::new(completed_response("ok")),
            })
        }));

        let (runtime_tx, mut runtime_rx) = mpsc::unbounded_channel();
        runtime_tx
            .send(CopilotRuntimeRequest::ObservedToolCall(
                CopilotObservedToolCall {
                    tool_call_id: "call_1".to_string(),
                    tool_name: "copilot_tool".to_string(),
                    status: CopilotObservedToolCallStatus::Pending,
                    arguments: None,
                    output: None,
                    terminal_id: None,
                },
            ))
            .expect("send runtime request");
        drop(runtime_tx);

        let mut handler = SleepingRuntimeHandler {
            sleep_for: Duration::from_millis(40),
        };

        let result = render_stream_with_options_and_copilot_runtime_impl(
            "copilot",
            &mut stream,
            None,
            Some(&mut runtime_rx),
            Some(&mut handler),
            Some(Duration::from_millis(20)),
            &spinner,
            &mut renderer,
            &ctrl_c_state,
            &ctrl_c_notify,
            StreamSpinnerOptions::default(),
            None,
        )
        .await;

        assert!(
            result.is_ok(),
            "runtime prompt handling should pause timeout"
        );
    }
}
