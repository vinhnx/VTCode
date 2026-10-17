//! Streaming chain-of-thought render state for response streaming.
//
//Buffers reasoning deltas, compacts them for display, and emits each chunk
//exactly once (blank-line spam collapsed). Extracted from the stream
//orchestration module; rendering entry points live there.
use std::time::Instant;

use anyhow::Result;

use crate::agent::runloop::unified::reasoning::{analyze_reasoning, is_decision_or_tool_line};
use crate::agent::runloop::unified::ui_interaction::StreamProgressEvent;
use crate::agent::runloop::unified::ui_interaction_stream_helpers::{map_render_error, render_compact_reasoning_block};
use vtcode_commons::formatting::compact_reasoning_text;
use vtcode_core::llm::provider::{self as uni};
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

#[derive(Default)]
pub(super) struct StreamingReasoningState {
    buffered: String,
    /// Number of chars of `buffered`'s compacted form already rendered.
    printed_chars: usize,
    render_output: bool,
    defer_rendering: bool,
    rendered_any: bool,
}

impl StreamingReasoningState {
    pub(super) fn new(inline_enabled: bool) -> Self {
        Self {
            buffered: String::new(),
            printed_chars: 0,
            render_output: true,
            defer_rendering: !inline_enabled,
            rendered_any: false,
        }
    }

    /// Append a reasoning delta and emit any newly-stabilized, compacted tail.
    ///
    /// Each chunk is printed exactly once and blank-line spam from the model is
    /// collapsed, so the on-screen chain-of-thought stays compact. When rendering
    /// is deferred (non-streaming), the delta is buffered and emitted at
    /// [`Self::finalize`].
    pub(super) fn handle_delta(&mut self, renderer: &mut AnsiRenderer, delta: &str) -> Result<bool> {
        if !self.render_output {
            return Ok(false);
        }
        self.buffered.push_str(delta);
        if self.defer_rendering {
            return Ok(false);
        }
        self.render_new_tail(renderer)
    }

    /// Render the newly-stabilized tail of the compacted reasoning buffer.
    pub(super) fn render_new_tail(&mut self, renderer: &mut AnsiRenderer) -> Result<bool> {
        if self.buffered.is_empty() {
            return Ok(self.rendered_any);
        }

        let compact = compact_reasoning_text(&self.buffered);
        let compact_len = compact.chars().count();
        if self.printed_chars > compact_len {
            self.printed_chars = compact_len;
        }
        let tail: String = compact.chars().skip(self.printed_chars).collect();
        if tail.is_empty() {
            return Ok(self.rendered_any);
        }

        let inline = renderer.writes_to_inline_sink();
        for line in tail.split('\n') {
            let is_blank = line.trim().is_empty();
            if is_blank {
                if !inline {
                    renderer.line(MessageStyle::Reasoning, "")?;
                }
                // Inline sink: drop blank lines entirely to avoid blank-line spam.
                continue;
            }
            let style = if is_decision_or_tool_line(line) {
                MessageStyle::ReasoningEmphasis
            } else {
                MessageStyle::Reasoning
            };
            if inline {
                // Preserve line breaks while dropping blank-line spam: each
                // non-blank line is streamed with a trailing newline.
                let mut piece = String::with_capacity(line.len() + 1);
                piece.push_str(line);
                piece.push('\n');
                renderer.inline_with_style(style, &piece)?;
            } else {
                renderer.line(style, line)?;
            }
        }
        self.printed_chars = compact_len;
        self.rendered_any = true;
        Ok(true)
    }

    pub(super) fn flush_pending(&mut self, renderer: &mut AnsiRenderer) -> Result<bool> {
        if !self.render_output {
            self.buffered.clear();
            self.printed_chars = 0;
            return Ok(false);
        }
        self.render_new_tail(renderer)
    }

    pub(super) fn finalize(
        &mut self,
        renderer: &mut AnsiRenderer,
        final_reasoning: Option<&str>,
        reasoning_already_emitted: bool,
        suppress_reasoning: bool,
    ) -> Result<()> {
        if !self.render_output {
            self.buffered.clear();
            self.printed_chars = 0;
            return Ok(());
        }
        if suppress_reasoning {
            self.buffered.clear();
            self.printed_chars = 0;
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
            let compact = compact_reasoning_text(reasoning_text);
            if !compact.trim().is_empty() {
                render_compact_reasoning_block(renderer, reasoning_text)?;
                self.rendered_any = true;

                let analysis = analyze_reasoning(&compact);
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

    pub(super) fn handle_stream_failure(&mut self, _renderer: &mut AnsiRenderer) -> Result<()> {
        self.buffered.clear();
        Ok(())
    }

    pub(super) fn rendered_reasoning(&self) -> bool {
        self.rendered_any
    }

    pub(super) fn is_deferred(&self) -> bool {
        self.defer_rendering
    }
}

pub(super) fn flush_pending_reasoning_delta(
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

pub(super) fn flush_pending_reasoning(
    provider_name: &str,
    renderer: &mut AnsiRenderer,
    reasoning_state: &mut StreamingReasoningState,
    on_progress: &mut Option<&mut (dyn FnMut(StreamProgressEvent) + Send)>,
    pending_delta: &mut String,
    pending_render_bytes: &mut usize,
    last_render_at: &mut Instant,
    reasoning_emitted: &mut bool,
) -> Result<(), uni::LLMError> {
    let rendered = flush_pending_reasoning_delta(provider_name, renderer, reasoning_state, on_progress, pending_delta)?;
    if rendered {
        *reasoning_emitted = true;
    }
    *pending_render_bytes = 0;
    *last_render_at = Instant::now();
    Ok(())
}
