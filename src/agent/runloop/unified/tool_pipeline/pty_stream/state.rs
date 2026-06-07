use std::collections::VecDeque;

use vtcode_commons::formatting::wrap_text_words;
use vtcode_commons::preview::{
    format_hidden_lines_summary as shared_hidden_lines_summary, split_head_tail_preview_with_limit,
    summary_window as shared_summary_window,
};
use vtcode_core::config::PtyConfig;
use vtcode_core::tools::pty::PtyPreviewRenderer;

use vtcode_tui::app::InlineLinkRange;
use vtcode_tui::app::InlineSegment;

use super::segments::{PtyLineStyles, line_to_segments};

const LIVE_PREVIEW_HEAD_LINES: usize = 3;
const MAX_BUFFERED_TAIL_LINES: usize = 64;

type RenderedPtyPreview = (
    usize,
    Vec<Vec<InlineSegment>>,
    Vec<Vec<InlineLinkRange>>,
    Option<String>,
);

struct RenderedPtyOutput {
    lines: Vec<String>,
    last_line: Option<String>,
}

struct LegacyPtyStreamState {
    head_lines: Vec<String>,
    tail_lines: VecDeque<String>,
    current_line: String,
    total_lines: usize,
}

impl LegacyPtyStreamState {
    fn new() -> Self {
        Self {
            head_lines: Vec::new(),
            tail_lines: VecDeque::new(),
            current_line: String::new(),
            total_lines: 0,
        }
    }

    fn apply_chunk(&mut self, chunk: &str) {
        let mut chars = chunk.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\r' => {
                    if matches!(chars.peek(), Some('\n')) {
                        let _ = chars.next();
                        self.push_line();
                    } else {
                        self.current_line.clear();
                    }
                }
                '\n' => self.push_line(),
                _ => self.current_line.push(ch),
            }
        }
    }

    fn push_line(&mut self) {
        let line = std::mem::take(&mut self.current_line);
        if self.head_lines.len() < LIVE_PREVIEW_HEAD_LINES {
            self.head_lines.push(line);
        } else {
            self.tail_lines.push_back(line);
            while self.tail_lines.len() > MAX_BUFFERED_TAIL_LINES {
                let _ = self.tail_lines.pop_front();
            }
        }
        self.total_lines += 1;
    }

    fn render_output(&self, limit: usize) -> RenderedPtyOutput {
        if limit == 0 {
            return RenderedPtyOutput {
                lines: Vec::new(),
                last_line: None,
            };
        }

        let has_current = !self.current_line.is_empty();
        let total = self.total_lines + usize::from(has_current);
        if total == 0 {
            return RenderedPtyOutput {
                lines: Vec::new(),
                last_line: None,
            };
        }

        let last_line = if has_current {
            Some(self.current_line.clone())
        } else {
            self.tail_lines
                .back()
                .cloned()
                .or_else(|| self.head_lines.last().cloned())
        };

        let head_len = self.head_lines.len();
        let tail_len = self.tail_lines.len();
        if total <= head_len + tail_len + usize::from(has_current) {
            let mut lines = Vec::with_capacity(total);
            lines.extend(self.head_lines.iter().cloned());
            lines.extend(self.tail_lines.iter().cloned());
            if has_current {
                lines.push(self.current_line.clone());
            }
            return RenderedPtyOutput {
                lines: render_visible_output_lines(&lines, limit),
                last_line,
            };
        }

        let (head_count, tail_count) = shared_summary_window(limit, LIVE_PREVIEW_HEAD_LINES);
        let head_count = head_count.min(head_len);
        let mut tail_preview: Vec<_> = self
            .tail_lines
            .iter()
            .rev()
            .take(tail_count)
            .rev()
            .cloned()
            .collect();
        if has_current {
            tail_preview.push(self.current_line.clone());
        }

        let hidden_lines = total.saturating_sub(head_count + tail_preview.len());

        RenderedPtyOutput {
            lines: render_head_tail_lines(
                &self.head_lines[..head_count],
                hidden_lines,
                &tail_preview,
            ),
            last_line,
        }
    }
}

pub(super) struct PtyStreamState {
    pty_config: PtyConfig,
    command_header: Vec<String>,
    legacy: LegacyPtyStreamState,
    preview: PtyPreviewRenderer,
    displayed_count: usize,
    /// Tracks whether any applied chunk contained ANSI escape sequences.
    /// When true, the preview path (VT100 terminal emulator) is preferred
    /// because it correctly processes control sequences like screen rewrites
    /// and OSC8 hyperlinks.  When false, the legacy path is preferred because
    /// it preserves raw text formatting (leading indentation).
    input_has_ansi: bool,
}

impl PtyStreamState {
    pub(super) fn new(command_prompt: Option<String>, pty_config: PtyConfig) -> Self {
        let preview = PtyPreviewRenderer::from_config(&pty_config);
        Self {
            pty_config,
            command_header: normalize_command_prompt(command_prompt)
                .map(|command| format_command_header_lines(&command))
                .unwrap_or_default(),
            legacy: LegacyPtyStreamState::new(),
            preview,
            displayed_count: 0,
            input_has_ansi: false,
        }
    }

    pub(super) fn apply_chunk(&mut self, chunk: &str, limit: usize) {
        if limit == 0 {
            self.reset_output_state();
            return;
        }
        if chunk.is_empty() {
            return;
        }

        // Detect CSI sequences (ESC[) which indicate screen-altering operations
        // like clear screen, cursor positioning.  OSC sequences (ESC]) like
        // OSC8 hyperlinks should NOT trigger the preview path because the
        // terminal emulator strips link metadata from screen contents.
        if chunk.contains("\x1b[") {
            self.input_has_ansi = true;
        }
        self.legacy.apply_chunk(chunk);
        self.preview.push_str(chunk);
    }

    pub(super) fn render_lines(&self, limit: usize) -> Vec<String> {
        let mut rendered = self.command_header.clone();
        rendered.extend(self.select_render_output(limit).lines);
        rendered
    }

    pub(super) fn last_display_line(&self, limit: usize) -> Option<String> {
        self.select_render_output(limit).last_line
    }

    pub(super) fn render_segments(&mut self, chunk: &str, tail_limit: usize) -> RenderedPtyPreview {
        self.apply_chunk(chunk, tail_limit);
        self.render_current_segments(tail_limit)
    }

    pub(super) fn render_current_segments(&mut self, tail_limit: usize) -> RenderedPtyPreview {
        let rendered = self.render_lines(tail_limit);
        let styles = PtyLineStyles::new();
        let rendered_lines = rendered
            .into_iter()
            .map(|line| line_to_segments(&line, &styles))
            .collect::<Vec<_>>();
        let (segments, link_ranges): (Vec<_>, Vec<_>) = rendered_lines.into_iter().unzip();
        let replace_count = self.displayed_count;
        self.displayed_count = segments.len();
        let last_line = self.last_display_line(tail_limit);
        (replace_count, segments, link_ranges, last_line)
    }

    fn reset_output_state(&mut self) {
        self.legacy = LegacyPtyStreamState::new();
        self.preview = PtyPreviewRenderer::from_config(&self.pty_config);
        self.displayed_count = 0;
        self.input_has_ansi = false;
    }

    fn select_render_output(&self, limit: usize) -> RenderedPtyOutput {
        let preview = self.render_preview_output(limit);
        let legacy = self.legacy.render_output(limit);
        if should_use_legacy_output(&legacy, &preview, self.input_has_ansi) {
            legacy
        } else {
            preview
        }
    }

    fn render_preview_output(&self, limit: usize) -> RenderedPtyOutput {
        if limit == 0 {
            return RenderedPtyOutput {
                lines: Vec::new(),
                last_line: None,
            };
        }

        let snapshot = self.preview.snapshot_text();
        let lines = if snapshot.is_empty() {
            Vec::new()
        } else {
            snapshot
                .lines()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        };
        let last_line = last_non_empty_line(&lines).or_else(|| lines.last().cloned());

        RenderedPtyOutput {
            lines: render_visible_output_lines(&lines, limit),
            last_line,
        }
    }
}

fn should_use_legacy_output(
    legacy: &RenderedPtyOutput,
    preview: &RenderedPtyOutput,
    input_has_ansi: bool,
) -> bool {
    if preview.lines.is_empty() {
        return true;
    }
    if legacy.lines.is_empty() {
        return false;
    }
    // When input contained ANSI escape sequences (screen rewrites, OSC8
    // hyperlinks, cursor moves), the preview path — which processes through
    // a VT100 terminal emulator — produces the correct rendered output.
    // For plain text the legacy path preserves raw formatting (leading
    // indentation) that the terminal emulator strips, so prefer legacy.
    !input_has_ansi
}

fn render_visible_output_lines(lines: &[String], limit: usize) -> Vec<String> {
    if limit == 0 || lines.is_empty() {
        return Vec::new();
    }

    if lines.len() <= limit {
        return prefix_all_lines(lines);
    }

    let preview = split_head_tail_preview_with_limit(lines, limit, LIVE_PREVIEW_HEAD_LINES);
    render_head_tail_lines(preview.head, preview.hidden_count, preview.tail)
}

fn prefix_all_lines(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| prefix_stream_line(line, index == 0))
        .collect()
}

fn render_head_tail_lines(head: &[String], hidden_lines: usize, tail: &[String]) -> Vec<String> {
    let mut rendered = Vec::with_capacity(head.len() + tail.len() + usize::from(hidden_lines > 0));
    let mut first_output_line = true;

    for line in head {
        rendered.push(prefix_stream_line(line, first_output_line));
        first_output_line = false;
    }

    if hidden_lines > 0 {
        rendered.push(format_hidden_lines_summary(hidden_lines));
    }

    for line in tail {
        rendered.push(prefix_stream_line(line, first_output_line));
        first_output_line = false;
    }

    rendered
}

fn last_non_empty_line(lines: &[String]) -> Option<String> {
    lines
        .iter()
        .rev()
        .find(|line| !line.trim().is_empty())
        .cloned()
}

fn format_hidden_lines_summary(hidden: usize) -> String {
    format!("    {}", shared_hidden_lines_summary(hidden))
}

fn normalize_command_prompt(command_prompt: Option<String>) -> Option<String> {
    command_prompt.and_then(|value| {
        let collapsed = vtcode_commons::formatting::collapse_whitespace(&value);
        if collapsed.is_empty() {
            None
        } else {
            Some(collapsed)
        }
    })
}

fn format_command_header_lines(command: &str) -> Vec<String> {
    const FIRST_LINE_WIDTH: usize = 62;
    const CONTINUATION_WIDTH: usize = 58;

    let wrapped = wrap_text_words(command, FIRST_LINE_WIDTH, CONTINUATION_WIDTH);
    if wrapped.is_empty() {
        return vec!["• Ran command".to_string()];
    }

    let mut lines = Vec::with_capacity(wrapped.len());
    lines.push(format!("• Ran {}", wrapped[0]));
    for segment in wrapped.iter().skip(1) {
        lines.push(format!("  │ {}", segment));
    }
    lines
}

fn prefix_stream_line(line: &str, is_first_output_line: bool) -> String {
    if is_first_output_line {
        format!("  └ {}", line)
    } else {
        format!("    {}", line)
    }
}
