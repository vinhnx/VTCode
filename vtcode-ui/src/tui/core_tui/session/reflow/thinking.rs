/// Thinking/reasoning block rendering for the session transcript.
///
/// A thinking block is a contiguous run of `InlineMessageKind::Policy` lines.
/// This module owns the full reflow of such a run — both the collapsed summary
/// and the expanded body — keeping that logic out of the general message reflow
/// path in `mod.rs`. The general path delegates here via `reflow_thinking_lines`
/// and never reaches into thinking internals directly.
use ratatui::prelude::*;

use super::super::super::style::ratatui_style_from_inline;
use super::super::super::types::InlineMessageKind;
use super::super::Session;
use super::super::message::TranscriptLine;
use super::super::render;
use super::super::text_utils;
use super::transcript_line_with_detected_links;
use crate::tui::config::constants::ui;

impl Session {
    /// Reflow the thinking/reasoning run containing `index`.
    ///
    /// Returns `None` when `index` is not part of a `Policy` run. When it is,
    /// returns `Some(lines)`: the run's start index emits the full block, while
    /// continuation lines emit an empty vector (the run is rendered once, from
    /// its start). The caller (the general message reflow path) uses these lines
    /// in place of the default per-line reflow.
    pub(super) fn reflow_thinking_lines(
        &self,
        index: usize,
        width: u16,
    ) -> Option<Vec<TranscriptLine>> {
        if index >= self.lines.len() {
            return None;
        }
        if self.lines[index].kind != InlineMessageKind::Policy {
            return None;
        }
        let run_start = self.thinking_run_start(index);
        if run_start != index {
            return Some(Vec::new());
        }
        Some(self.render_thinking_block(run_start, width))
    }

    /// Number of contiguous `Policy` lines starting at `start`.
    ///
    /// Also used by the toggle handler in `messages.rs` to invalidate every line
    /// in a run after a collapse/expand.
    pub(crate) fn thinking_run_len(&self, start: usize) -> usize {
        let mut len = 0;
        for line in self.lines.iter().skip(start) {
            if line.kind == InlineMessageKind::Policy {
                len += 1;
            } else {
                break;
            }
        }
        len
    }

    /// Start line index of the contiguous `Policy` (thinking/reasoning) run that
    /// contains `index`.
    fn thinking_run_start(&self, index: usize) -> usize {
        let mut start = index;
        while start > 0 {
            let Some(prev) = self.lines.get(start - 1) else {
                break;
            };
            if prev.kind != InlineMessageKind::Policy {
                break;
            }
            start -= 1;
        }
        start
    }

    fn thinking_collapsed(&self, start: usize) -> bool {
        self.thinking_runs
            .is_collapsed(start, self.appearance.thinking_collapsed_by_default())
    }

    /// Whether the thinking run starting at `start` should render collapsed,
    /// resolving the config default. Shared with the toggle handler in
    /// `messages.rs` so the default is resolved in exactly one place.
    pub(crate) fn thinking_is_collapsed(&self, start: usize) -> bool {
        self.thinking_collapsed(start)
    }

    /// Render a thinking/reasoning block (a contiguous `Policy` run) as a single
    /// coherent section: an arrow-prefixed header (`→ Thinking (N lines)` when
    /// collapsed, `↓ Thinking` when expanded) followed by the wrapped, dimmed
    /// body when expanded. Both states share the same left alignment so toggling
    /// preserves the block position.
    fn render_thinking_block(&self, start: usize, width: u16) -> Vec<TranscriptLine> {
        let collapsed = self.thinking_collapsed(start);
        let run_len = self.thinking_run_len(start);
        let accent =
            ratatui_style_from_inline(&self.styles.accent_inline_style(), self.theme.foreground);

        let chevron = if collapsed {
            ui::INLINE_THINKING_COLLAPSED_CHEVRON
        } else {
            ui::INLINE_THINKING_EXPANDED_CHEVRON
        };

        let streaming =
            collapsed && self.thinking_spinner.is_active && !self.appearance.motion_reduced();
        let header_text = if streaming {
            format!(
                "{chevron} {} Thinking…",
                self.thinking_spinner.current_frame()
            )
        } else if collapsed {
            let noun = if run_len == 1 { "line" } else { "lines" };
            format!("{chevron} Thinking ({run_len} {noun})")
        } else {
            format!("{chevron} Thinking")
        };
        let header = Line::from(Span::styled(header_text, accent));
        let mut result = vec![transcript_line_with_detected_links(
            header,
            self.workspace_root.as_deref(),
        )];

        if !collapsed {
            let indent = ui::INLINE_THINKING_BODY_INDENT;
            let indent_width = indent.chars().count();
            let max_width = if width == 0 {
                usize::MAX
            } else {
                width as usize
            };
            let content_width = if max_width == usize::MAX {
                usize::MAX
            } else {
                max_width.saturating_sub(indent_width)
            };

            for idx in start..start + run_len {
                let spans = render::render_message_spans(self, idx);
                if spans.iter().all(|span| span.content.is_empty()) {
                    continue;
                }
                let dimmed: Vec<Span<'static>> = spans
                    .into_iter()
                    .map(|mut span| {
                        span.style = span.style.add_modifier(Modifier::DIM);
                        span
                    })
                    .collect();
                let content_line = Line::from(dimmed);
                let mut wrapped = if content_width == usize::MAX {
                    vec![content_line]
                } else {
                    text_utils::wrap_line_with_hanging_prefix(content_line, content_width, indent)
                };
                // `wrap_line_with_hanging_prefix` only indents continuation rows, so
                // prepend the indent to the first row to keep the whole body aligned.
                if let Some(first) = wrapped.first_mut() {
                    let mut new_spans = vec![Span::raw(indent.to_owned())];
                    new_spans.append(&mut first.spans);
                    first.spans = new_spans;
                }
                for line in wrapped {
                    result.push(transcript_line_with_detected_links(
                        line,
                        self.workspace_root.as_deref(),
                    ));
                }
            }
        }

        // Trailing spacing. A collapsed summary is a compact block and must be
        // separated from the agent response that follows it by one blank line.
        // Expanded reasoning flows directly into that response (no gap), and
        // runs followed by a non-agent line keep `message_block_spacing`.
        let run_end = start + run_len;
        let next_kind = self.lines.get(run_end).map(|line| line.kind);
        if collapsed && next_kind == Some(InlineMessageKind::Agent) {
            result.push(TranscriptLine::default());
        } else if next_kind != Some(InlineMessageKind::Agent) {
            let spacing = self.appearance.message_block_spacing.min(2) as usize;
            for _ in 0..spacing {
                result.push(TranscriptLine::default());
            }
        }

        result
    }
}
