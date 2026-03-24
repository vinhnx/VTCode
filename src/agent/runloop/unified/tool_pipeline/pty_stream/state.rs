use std::collections::VecDeque;

use vtcode_core::config::PtyConfig;
use vtcode_core::tools::pty::PtyPreviewRenderer;
use vtcode_core::utils::ansi_parser::strip_ansi;
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
        let line = self.current_line.clone();
        if self.head_lines.len() < LIVE_PREVIEW_HEAD_LINES {
            self.head_lines.push(line);
        } else {
            self.tail_lines.push_back(line);
            while self.tail_lines.len() > MAX_BUFFERED_TAIL_LINES {
                let _ = self.tail_lines.pop_front();
            }
        }
        self.total_lines += 1;
        self.current_line.clear();
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

        let mut all_available = self.head_lines.clone();
        all_available.extend(self.tail_lines.iter().cloned());
        if has_current {
            all_available.push(self.current_line.clone());
        }

        if total <= all_available.len() {
            return RenderedPtyOutput {
                lines: render_visible_output_lines(&all_available, limit),
                last_line,
            };
        }

        if limit <= 2 {
            let mut tail_preview = self.tail_lines.iter().cloned().collect::<Vec<_>>();
            if has_current {
                tail_preview.push(self.current_line.clone());
            }
            if tail_preview.len() > limit {
                let drop = tail_preview.len() - limit;
                tail_preview.drain(..drop);
            }
            return RenderedPtyOutput {
                lines: prefix_all_lines(&tail_preview),
                last_line,
            };
        }

        let (head_count, tail_count) = summary_window(limit);
        let head_count = head_count.min(self.head_lines.len());

        let mut rendered = Vec::new();
        let mut first_output_line = true;
        for line in self.head_lines.iter().take(head_count) {
            rendered.push(prefix_stream_line(line, first_output_line));
            first_output_line = false;
        }

        let mut tail_preview = self.tail_lines.iter().cloned().collect::<Vec<_>>();
        if has_current {
            tail_preview.push(self.current_line.clone());
        }
        if tail_preview.len() > tail_count {
            let drop = tail_preview.len() - tail_count;
            tail_preview.drain(..drop);
        }

        let hidden_lines = total.saturating_sub(head_count + tail_preview.len());
        if hidden_lines > 0 {
            rendered.push(format_hidden_lines_summary(hidden_lines));
        }

        for line in tail_preview {
            rendered.push(prefix_stream_line(&line, first_output_line));
            first_output_line = false;
        }

        RenderedPtyOutput {
            lines: rendered,
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
    }

    fn select_render_output(&self, limit: usize) -> RenderedPtyOutput {
        let preview = self.render_preview_output(limit);
        let legacy = self.legacy.render_output(limit);
        if should_use_legacy_output(&legacy, &preview) {
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

fn should_use_legacy_output(legacy: &RenderedPtyOutput, preview: &RenderedPtyOutput) -> bool {
    if preview.lines.is_empty() {
        return true;
    }
    if legacy.lines.is_empty() {
        return false;
    }

    normalize_rendered_lines(&legacy.lines) == normalize_rendered_lines(&preview.lines)
}

fn normalize_rendered_lines(lines: &[String]) -> Vec<String> {
    lines.iter().map(|line| strip_ansi(line)).collect()
}

fn render_visible_output_lines(lines: &[String], limit: usize) -> Vec<String> {
    if limit == 0 || lines.is_empty() {
        return Vec::new();
    }

    if lines.len() <= limit {
        return prefix_all_lines(lines);
    }

    if limit <= 2 {
        return prefix_all_lines(&lines[lines.len() - limit..]);
    }

    let (head_count, tail_count) = summary_window(limit);
    let mut rendered = Vec::with_capacity(limit);
    let mut first_output_line = true;

    for line in lines.iter().take(head_count) {
        rendered.push(prefix_stream_line(line, first_output_line));
        first_output_line = false;
    }

    let hidden_lines = lines.len().saturating_sub(head_count + tail_count);
    if hidden_lines > 0 {
        rendered.push(format_hidden_lines_summary(hidden_lines));
    }

    for line in lines.iter().skip(lines.len() - tail_count) {
        rendered.push(prefix_stream_line(line, first_output_line));
        first_output_line = false;
    }

    rendered
}

fn summary_window(limit: usize) -> (usize, usize) {
    if limit <= 2 {
        return (0, limit);
    }

    let head = LIVE_PREVIEW_HEAD_LINES.min((limit - 1) / 2).max(1);
    let tail = limit.saturating_sub(head + 1).max(1);
    (head, tail)
}

fn prefix_all_lines(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| prefix_stream_line(line, index == 0))
        .collect()
}

fn last_non_empty_line(lines: &[String]) -> Option<String> {
    lines
        .iter()
        .rev()
        .find(|line| !line.trim().is_empty())
        .cloned()
}

fn format_hidden_lines_summary(hidden: usize) -> String {
    if hidden == 1 {
        "    … +1 line".to_string()
    } else {
        format!("    … +{} lines", hidden)
    }
}

fn normalize_command_prompt(command_prompt: Option<String>) -> Option<String> {
    command_prompt.and_then(|value| {
        let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
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

fn wrap_text_words(text: &str, first_width: usize, continuation_width: usize) -> Vec<String> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let mut result = Vec::new();
    let mut remaining = trimmed;
    let mut width = first_width.max(1);

    while char_count(remaining) > width {
        let split = split_at_word_boundary(remaining, width);
        let (head, tail) = remaining.split_at(split);
        let head = head.trim();
        if head.is_empty() {
            break;
        }
        result.push(head.to_string());
        remaining = tail.trim_start();
        if remaining.is_empty() {
            break;
        }
        width = continuation_width.max(1);
    }

    if !remaining.is_empty() {
        result.push(remaining.to_string());
    }
    result
}

fn split_at_word_boundary(input: &str, width: usize) -> usize {
    let capped = byte_index_for_char_count(input, width);
    let candidate = &input[..capped];
    if let Some(boundary) = candidate.rfind(char::is_whitespace) {
        boundary
    } else {
        capped
    }
}

fn byte_index_for_char_count(input: &str, chars: usize) -> usize {
    if chars == 0 {
        return 0;
    }
    let mut seen = 0usize;
    for (idx, ch) in input.char_indices() {
        seen += 1;
        if seen == chars {
            return idx + ch.len_utf8();
        }
    }
    input.len()
}

fn char_count(input: &str) -> usize {
    input.chars().count()
}
