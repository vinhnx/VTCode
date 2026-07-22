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

fn trim_leading_whitespace(line: &mut Line<'static>) {
    while let Some(first) = line.spans.first_mut() {
        let trimmed = first.content.trim_start();
        let trimmed_len = trimmed.len();
        let original_len = first.content.len();
        if trimmed_len == 0 {
            line.spans.remove(0);
        } else if trimmed_len < original_len {
            let content = first.content.to_mut();
            content.drain(0..original_len - trimmed_len);
            break;
        } else {
            break;
        }
    }
}

impl Session {
    /// Reflow the thinking/reasoning run containing `index`.
    ///
    /// Returns `None` when `index` is not part of a `Policy` run. When it is,
    /// returns `Some(lines)`: the run's start index emits the full block, while
    /// continuation lines emit an empty vector (the run is rendered once, from
    /// its start). The caller (the general message reflow path) uses these lines
    /// in place of the default per-line reflow.
    pub(super) fn reflow_thinking_lines(&self, index: usize, width: u16) -> Option<Vec<TranscriptLine>> {
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

    pub(crate) fn thinking_collapsed(&self, start: usize) -> bool {
        self.thinking_runs
            .is_collapsed(start, self.appearance.thinking_collapsed_by_default())
    }

    /// Render a thinking/reasoning block (a contiguous `Policy` run) as a single
    /// coherent section: a header (`Thinking`) followed by the wrapped, dimmed
    /// body when expanded.
    fn render_thinking_block(&self, start: usize, width: u16) -> Vec<TranscriptLine> {
        let collapsed = self.thinking_collapsed(start);
        let run_len = self.thinking_run_len(start);
        let accent = ratatui_style_from_inline(&self.styles.accent_inline_style(), self.theme.foreground);

        let is_active = self.thinking_runs.is_active(start);
        let header_text = if is_active {
            "Thinking...".to_string()
        } else {
            let secs = self.thinking_runs.duration(start).map(|d| d.as_secs()).unwrap_or(0);
            if secs > 0 {
                format!("Thought for {}s", secs)
            } else {
                "Thought for 1s".to_string()
            }
        };

        let header = Line::from(vec![Span::styled("• ", accent), Span::styled(header_text, accent)]);
        let mut result = vec![transcript_line_with_detected_links(
            header,
            self.workspace_root.as_deref(),
        )];

        if !collapsed {
            let content_width = match width {
                0 => None,
                w => Some((w as usize).saturating_sub(2).max(10)),
            };

            let mut is_first_body_line = true;
            for idx in start..start + run_len {
                let spans = render::render_message_spans(self, idx);
                if spans.is_empty() || (spans.len() == 1 && spans[0].content.is_empty()) {
                    let line = Line::from(vec![Span::raw("  ")]);
                    result.push(transcript_line_with_detected_links(line, self.workspace_root.as_deref()));
                    continue;
                }

                let mut wrapped = match content_width {
                    None => vec![Line::from(spans)],
                    Some(cw) => text_utils::wrap_line(Line::from(spans), cw),
                };

                for (line_idx, line) in wrapped.iter_mut().enumerate() {
                    trim_leading_whitespace(line);
                    let prefix_span = if is_first_body_line && line_idx == 0 {
                        is_first_body_line = false;
                        Span::styled("└ ", accent)
                    } else {
                        Span::raw("  ")
                    };
                    line.spans.insert(0, prefix_span);
                    result.push(transcript_line_with_detected_links(line.clone(), self.workspace_root.as_deref()));
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
