use std::sync::Arc;

use anyhow::Result;
use futures::StreamExt;
use tokio::sync::Notify;

use crate::agent::runloop::unified::plan_blocks::{
    ProposedPlanStreamParser, extract_proposed_plan,
};
use vtcode_core::llm::error_display;
use vtcode_core::llm::provider::{self as uni, LLMStreamEvent};
use vtcode_core::llm::providers::clean_reasoning_text;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

use super::state::CtrlCState;
use super::ui_interaction::{PlaceholderSpinner, StreamSpinnerOptions};
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
    fn new(_inline_enabled: bool) -> Self {
        Self {
            buffered: String::new(),
            render_inline: false,
            render_output: true,
            defer_rendering: true,
            started: false,
            rendered_any: false,
        }
    }

    fn handle_delta(&mut self, renderer: &mut AnsiRenderer, delta: &str) -> Result<bool> {
        if !self.render_output || !self.render_inline || self.defer_rendering {
            self.buffered.push_str(delta);
            return Ok(false);
        }

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

pub(crate) async fn stream_and_render_response_with_options_impl(
    provider: &dyn uni::LLMProvider,
    request: uni::LLMRequest,
    spinner: &PlaceholderSpinner,
    renderer: &mut AnsiRenderer,
    ctrl_c_state: &Arc<CtrlCState>,
    ctrl_c_notify: &Arc<Notify>,
    options: StreamSpinnerOptions,
) -> Result<(uni::LLMResponse, bool), uni::LLMError> {
    let provider_name = provider.name();

    if ctrl_c_state.is_cancel_requested() {
        spinner.finish_with_restore(true);
        return Err(uni::LLMError::Provider {
            message: error_display::format_llm_error(provider_name, "Interrupted by user"),
            metadata: None,
        });
    }

    let supports_streaming_markdown = renderer.supports_streaming_markdown();
    let stream_future = provider.stream(request);
    tokio::pin!(stream_future);

    if ctrl_c_state.is_cancel_requested() || ctrl_c_state.is_exit_requested() {
        spinner.finish_with_restore(true);
        return Err(uni::LLMError::Provider {
            message: error_display::format_llm_error(provider_name, "Interrupted by user"),
            metadata: None,
        });
    }

    let mut stream = tokio::select! {
        biased;
        _ = ctrl_c_notify.notified() => {
            spinner.finish_with_restore(true);
            return Err(uni::LLMError::Provider { message: error_display::format_llm_error(provider_name, "Interrupted by user"), metadata: None });
        }
        result = stream_future => result?,
    };

    let mut final_response: Option<uni::LLMResponse> = None;
    let mut aggregated = String::new();
    let mut spinner_active = true;
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
    let mut reasoning_state = StreamingReasoningState::new(supports_streaming_markdown);
    let mut spinner_message_updated = false;
    let mut reasoning_accumulated = String::new();
    let mut pending_content = String::new();
    let mut content_suppressed = false;
    const MAX_PENDING_CONTENT_BYTES: usize = 4_096;

    let mut suppress_reasoning_due_to_duplication = false;
    let mut plan_parser = options
        .strip_proposed_plan_blocks
        .then(ProposedPlanStreamParser::new);

    let mut token_count = 0;
    let mut reasoning_token_count = 0;
    let mut last_progress_update = std::time::Instant::now();
    let mut reasoning_emitted = false;

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
            event = stream.next() => event,
        };

        let Some(event_result) = maybe_event else {
            break;
        };

        match event_result {
            Ok(LLMStreamEvent::Token { delta }) => {
                token_count += 1;
                let visible_delta = if let Some(parser) = plan_parser.as_mut() {
                    parser.consume(&delta)
                } else {
                    delta
                };
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
                if !reasoning_accumulated.trim().is_empty() && !emitted_tokens {
                    pending_content.push_str(&visible_delta);
                    if pending_content.len() >= MAX_PENDING_CONTENT_BYTES {
                        aggregated.push_str(&pending_content);
                        pending_content.clear();
                        if supports_streaming_markdown {
                            rendered_line_count = renderer
                                .stream_markdown_response(&aggregated, rendered_line_count)
                                .map_err(|err| map_render_error(provider_name, err))?;
                            emitted_tokens = true;
                        }
                    }
                    continue;
                }

                aggregated.push_str(&visible_delta);
                if supports_streaming_markdown {
                    rendered_line_count = renderer
                        .stream_markdown_response(&aggregated, rendered_line_count)
                        .map_err(|err| map_render_error(provider_name, err))?;
                    emitted_tokens = true;
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
                let rendered = reasoning_state
                    .handle_delta(renderer, &delta)
                    .map_err(|err| map_render_error(provider_name, err))?;
                if rendered {
                    reasoning_emitted = true;
                }
            }
            Ok(LLMStreamEvent::ReasoningStage { stage }) => {
                spinner.set_reasoning_stage(Some(stage));
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

    if let Some(parser) = plan_parser.as_mut() {
        let trailing_plan_parse = parser.finish();
        if !trailing_plan_parse.stripped_text.is_empty() {
            if !reasoning_accumulated.trim().is_empty() && !emitted_tokens {
                pending_content.push_str(&trailing_plan_parse.stripped_text);
                if pending_content.len() >= MAX_PENDING_CONTENT_BYTES {
                    aggregated.push_str(&pending_content);
                    pending_content.clear();
                    if supports_streaming_markdown {
                        rendered_line_count = renderer
                            .stream_markdown_response(&aggregated, rendered_line_count)
                            .map_err(|err| map_render_error(provider_name, err))?;
                        emitted_tokens = true;
                    }
                }
            } else {
                aggregated.push_str(&trailing_plan_parse.stripped_text);
                if supports_streaming_markdown {
                    rendered_line_count = renderer
                        .stream_markdown_response(&aggregated, rendered_line_count)
                        .map_err(|err| map_render_error(provider_name, err))?;
                    emitted_tokens = true;
                }
            }
        }
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
                renderer
                    .stream_markdown_response(&aggregated, prev_count)
                    .map_err(|err| map_render_error(provider_name, err))?;
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
                    renderer
                        .stream_markdown_response(content, prev_count)
                        .map_err(|err| map_render_error(provider_name, err))?;
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
                renderer
                    .stream_markdown_response(&reasoning_trimmed, 0)
                    .map_err(|err| map_render_error(provider_name, err))?;
            } else {
                renderer
                    .line(MessageStyle::Response, &reasoning_trimmed)
                    .map_err(|err| map_render_error(provider_name, err))?;
            }
            emitted_tokens = true;
        }
    }

    Ok((response, emitted_tokens))
}
