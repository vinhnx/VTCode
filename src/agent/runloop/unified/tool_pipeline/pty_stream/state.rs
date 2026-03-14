use std::collections::VecDeque;

use vtcode_tui::InlineLinkRange;
use vtcode_tui::InlineSegment;

use super::segments::{PtyLineStyles, line_to_segments};

const LIVE_PREVIEW_HEAD_LINES: usize = 3;
const LIVE_PREVIEW_TAIL_LINES: usize = 3;

type RenderedPtyPreview = (
    usize,
    Vec<Vec<InlineSegment>>,
    Vec<Vec<InlineLinkRange>>,
    Option<String>,
);

pub(super) struct PtyStreamState {
    command_header: Vec<String>,
    head_lines: Vec<String>,
    tail_lines: VecDeque<String>,
    current_line: String,
    displayed_count: usize,
    total_lines: usize,
    last_pushed_line: Option<String>,
}

impl PtyStreamState {
    pub(super) fn new(command_prompt: Option<String>) -> Self {
        Self {
            command_header: normalize_command_prompt(command_prompt)
                .map(|command| format_command_header_lines(&command))
                .unwrap_or_default(),
            head_lines: Vec::new(),
            tail_lines: VecDeque::new(),
            current_line: String::new(),
            displayed_count: 0,
            total_lines: 0,
            last_pushed_line: None,
        }
    }

    pub(super) fn apply_chunk(&mut self, chunk: &str, limit: usize) {
        if limit == 0 {
            self.head_lines.clear();
            self.tail_lines.clear();
            self.current_line.clear();
            self.displayed_count = 0;
            self.total_lines = 0;
            self.last_pushed_line = None;
            return;
        }

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

        if self
            .last_pushed_line
            .as_ref()
            .is_some_and(|previous| previous == &line)
        {
            self.current_line.clear();
            return;
        }

        self.last_pushed_line = Some(line.clone());
        if self.head_lines.len() < LIVE_PREVIEW_HEAD_LINES {
            self.head_lines.push(line);
        } else {
            self.tail_lines.push_back(line);
            while self.tail_lines.len() > LIVE_PREVIEW_TAIL_LINES {
                let _ = self.tail_lines.pop_front();
            }
        }
        self.total_lines += 1;
        self.current_line.clear();
    }

    pub(super) fn render_lines(&self, limit: usize) -> Vec<String> {
        let mut rendered = self.command_header.clone();
        if limit == 0 {
            return rendered;
        }

        let has_current = !self.current_line.is_empty();
        let total = self.total_lines + usize::from(has_current);
        if total == 0 {
            return rendered;
        }

        let mut first_output_line = true;

        if total <= LIVE_PREVIEW_HEAD_LINES + LIVE_PREVIEW_TAIL_LINES {
            for line in &self.head_lines {
                rendered.push(prefix_stream_line(line, first_output_line));
                first_output_line = false;
            }
            for line in &self.tail_lines {
                rendered.push(prefix_stream_line(line, first_output_line));
                first_output_line = false;
            }
            if has_current {
                rendered.push(prefix_stream_line(&self.current_line, first_output_line));
            }
            return rendered;
        }

        for line in &self.head_lines {
            rendered.push(prefix_stream_line(line, first_output_line));
            first_output_line = false;
        }

        let hidden_lines = total.saturating_sub(LIVE_PREVIEW_HEAD_LINES + LIVE_PREVIEW_TAIL_LINES);
        if hidden_lines > 0 {
            rendered.push(format_hidden_lines_summary(hidden_lines));
        }

        let mut tail_preview: Vec<String> = self.tail_lines.iter().cloned().collect();
        if has_current {
            tail_preview.push(self.current_line.clone());
            if tail_preview.len() > LIVE_PREVIEW_TAIL_LINES {
                let drop = tail_preview.len() - LIVE_PREVIEW_TAIL_LINES;
                tail_preview.drain(..drop);
            }
        }
        for line in tail_preview {
            rendered.push(prefix_stream_line(&line, false));
        }

        rendered
    }

    pub(super) fn last_display_line(&self) -> Option<String> {
        if !self.current_line.is_empty() {
            return Some(self.current_line.clone());
        }
        self.tail_lines
            .back()
            .cloned()
            .or_else(|| self.head_lines.last().cloned())
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
        let last_line = self.last_display_line();
        (replace_count, segments, link_ranges, last_line)
    }
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
