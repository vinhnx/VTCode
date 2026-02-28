use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anstyle::{
    Ansi256Color, AnsiColor, Color as AnsiColorEnum, Effects, RgbColor, Style as AnsiStyle,
};
use tokio::{sync::mpsc, task::JoinHandle};
use vtcode_core::tools::registry::ToolProgressCallback;
use vtcode_core::ui::theme;
use vtcode_tui::{
    InlineHandle, InlineMessageKind, InlineSegment, InlineTextStyle, convert_style,
    ui::syntax_highlight,
};

use crate::agent::runloop::unified::progress::ProgressReporter;

const LIVE_PREVIEW_HEAD_LINES: usize = 3;
const LIVE_PREVIEW_TAIL_LINES: usize = 3;

struct PtyLineStyles {
    output: Arc<InlineTextStyle>,
    glyph: Arc<InlineTextStyle>,
    verb: Arc<InlineTextStyle>,
    command: Arc<InlineTextStyle>,
    args: Arc<InlineTextStyle>,
    keyword: Arc<InlineTextStyle>,
    variable: Arc<InlineTextStyle>,
    string: Arc<InlineTextStyle>,
    option: Arc<InlineTextStyle>,
    truncation: Arc<InlineTextStyle>,
}

impl PtyLineStyles {
    fn new() -> Self {
        let theme_styles = theme::active_styles();
        let output = Arc::new(convert_style(theme_styles.tool_detail.dimmed()));
        let glyph = Arc::new(convert_style(theme_styles.tool_detail.dimmed()));
        let verb = Arc::new(convert_style(
            AnsiStyle::new()
                .fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Magenta)))
                .effects(Effects::BOLD),
        ));
        let command = Arc::new(convert_style(
            AnsiStyle::new()
                .fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Green)))
                .effects(Effects::BOLD),
        ));
        let args = Arc::new(convert_style(
            AnsiStyle::new()
                .fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::White)))
                .effects(Effects::DIMMED),
        ));
        let keyword = Arc::new(convert_style(
            AnsiStyle::new()
                .fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Magenta)))
                .effects(Effects::BOLD),
        ));
        let variable = Arc::new(convert_style(
            AnsiStyle::new().fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Yellow))),
        ));
        let string = Arc::new(convert_style(
            AnsiStyle::new().fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Yellow))),
        ));
        let option = Arc::new(convert_style(
            AnsiStyle::new().fg_color(Some(AnsiColorEnum::Ansi(AnsiColor::Red))),
        ));
        let truncation = Arc::new(convert_style(theme_styles.tool_detail.dimmed()));

        Self {
            output,
            glyph,
            verb,
            command,
            args,
            keyword,
            variable,
            string,
            option,
            truncation,
        }
    }
}

fn is_bash_keyword(token: &str) -> bool {
    matches!(
        token,
        "if" | "then"
            | "else"
            | "elif"
            | "fi"
            | "for"
            | "in"
            | "do"
            | "done"
            | "while"
            | "until"
            | "case"
            | "esac"
            | "function"
            | "select"
            | "time"
            | "coproc"
            | "{"
            | "}"
            | "[["
            | "]]"
    )
}

fn is_command_separator(token: &str) -> bool {
    matches!(token, "|" | "||" | "&&" | ";" | ";;" | "&")
}

fn tokenize_preserve_whitespace(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut in_single = false;
    let mut in_double = false;
    let mut escaped = false;
    let mut token_start: Option<usize> = None;
    let mut token_is_whitespace = false;

    for (idx, ch) in text.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' && !in_single {
            escaped = true;
        } else if ch == '\'' && !in_double {
            in_single = !in_single;
        } else if ch == '"' && !in_single {
            in_double = !in_double;
        }

        let is_whitespace = !in_single && !in_double && ch.is_whitespace();
        match token_start {
            None => {
                token_start = Some(idx);
                token_is_whitespace = is_whitespace;
            }
            Some(start) if token_is_whitespace != is_whitespace => {
                parts.push(&text[start..idx]);
                token_start = Some(idx);
                token_is_whitespace = is_whitespace;
            }
            _ => {}
        }
    }

    if let Some(start) = token_start {
        parts.push(&text[start..]);
    }

    parts
}

fn style_for_token<'a>(
    token: &'a str,
    expect_command: &mut bool,
    styles: &'a PtyLineStyles,
) -> Arc<InlineTextStyle> {
    if token.trim().is_empty() {
        return Arc::clone(&styles.output);
    }

    if is_command_separator(token) {
        *expect_command = true;
        return Arc::clone(&styles.args);
    }

    if token.starts_with('"')
        || token.starts_with('\'')
        || token.ends_with('"')
        || token.ends_with('\'')
    {
        *expect_command = false;
        return Arc::clone(&styles.string);
    }

    if token.starts_with('$') || token.contains("=$") || token.starts_with("${") {
        *expect_command = false;
        return Arc::clone(&styles.variable);
    }

    if token.starts_with('-') && token.len() > 1 {
        *expect_command = false;
        return Arc::clone(&styles.option);
    }

    if is_bash_keyword(token) {
        *expect_command = true;
        return Arc::clone(&styles.keyword);
    }

    if *expect_command {
        *expect_command = false;
        return Arc::clone(&styles.command);
    }

    Arc::clone(&styles.args)
}

fn bash_segments(text: &str, styles: &PtyLineStyles, expect_command: bool) -> Vec<InlineSegment> {
    let mut segments = Vec::new();
    let mut command_expected = expect_command;
    for token in tokenize_preserve_whitespace(text) {
        segments.push(InlineSegment {
            text: token.to_string(),
            style: style_for_token(token, &mut command_expected, styles),
        });
    }
    segments
}

fn shell_syntax_segments(
    text: &str,
    styles: &PtyLineStyles,
    expect_command: bool,
) -> Vec<InlineSegment> {
    let semantic = bash_segments(text, styles, expect_command);
    let Some(highlighted) = syntax_highlight::highlight_line_to_anstyle_segments(
        text,
        Some("bash"),
        syntax_highlight::get_active_syntax_theme(),
        true,
    ) else {
        return semantic;
    };

    if highlighted.is_empty() {
        return semantic;
    }

    let converted = highlighted
        .into_iter()
        .map(|(style, text)| InlineSegment {
            text,
            style: Arc::new(convert_style(style).merge_color(styles.args.color)),
        })
        .collect::<Vec<_>>();

    let converted_text = converted
        .iter()
        .map(|segment| segment.text.as_str())
        .collect::<String>();
    if converted_text != text {
        return semantic;
    }

    let non_ws_count = semantic
        .iter()
        .filter(|segment| !segment.text.trim().is_empty())
        .count();
    if non_ws_count > 1 {
        let mut first: Option<&InlineTextStyle> = None;
        let mut has_distinct = false;
        for style in converted
            .iter()
            .filter(|segment| !segment.text.trim().is_empty())
            .map(|segment| segment.style.as_ref())
        {
            if let Some(seed) = first {
                if style != seed {
                    has_distinct = true;
                    break;
                }
            } else {
                first = Some(style);
            }
        }
        if !has_distinct {
            return semantic;
        }
    }

    converted
}

fn ansi_color_from_ansi_code(code: u16) -> Option<AnsiColorEnum> {
    let color = match code {
        30 | 90 => AnsiColor::Black,
        31 | 91 => AnsiColor::Red,
        32 | 92 => AnsiColor::Green,
        33 | 93 => AnsiColor::Yellow,
        34 | 94 => AnsiColor::Blue,
        35 | 95 => AnsiColor::Magenta,
        36 | 96 => AnsiColor::Cyan,
        37 | 97 => AnsiColor::White,
        _ => return None,
    };
    Some(AnsiColorEnum::Ansi(color))
}

fn clear_sgr_effects(effects: &mut Effects, code: u16) {
    match code {
        22 => {
            let _ = effects.remove(Effects::BOLD);
            let _ = effects.remove(Effects::DIMMED);
        }
        23 => {
            let _ = effects.remove(Effects::ITALIC);
        }
        24 => {
            let _ = effects.remove(Effects::UNDERLINE);
        }
        _ => {}
    }
}

fn apply_sgr_codes(sequence: &str, current: &mut InlineTextStyle, fallback: &InlineTextStyle) {
    let params: Vec<u16> = if sequence.trim().is_empty() {
        vec![0]
    } else {
        sequence
            .split(';')
            .map(|value| value.parse::<u16>().unwrap_or(0))
            .collect()
    };

    let mut index = 0usize;
    while index < params.len() {
        let code = params[index];
        match code {
            0 => *current = fallback.clone(),
            1 => current.effects |= Effects::BOLD,
            2 => current.effects |= Effects::DIMMED,
            3 => current.effects |= Effects::ITALIC,
            4 => current.effects |= Effects::UNDERLINE,
            22 | 23 | 24 => clear_sgr_effects(&mut current.effects, code),
            30..=37 | 90..=97 => current.color = ansi_color_from_ansi_code(code),
            39 => current.color = fallback.color,
            40..=47 | 100..=107 => {
                let fg_code = code - 10;
                current.bg_color = ansi_color_from_ansi_code(fg_code);
            }
            49 => current.bg_color = fallback.bg_color,
            38 | 48 => {
                let is_fg = code == 38;
                if let Some(mode) = params.get(index + 1).copied() {
                    match mode {
                        5 => {
                            if let Some(value) = params.get(index + 2).copied() {
                                let color = AnsiColorEnum::Ansi256(Ansi256Color(value as u8));
                                if is_fg {
                                    current.color = Some(color);
                                } else {
                                    current.bg_color = Some(color);
                                }
                                index += 2;
                            }
                        }
                        2 => {
                            if index + 4 < params.len() {
                                let r = params[index + 2] as u8;
                                let g = params[index + 3] as u8;
                                let b = params[index + 4] as u8;
                                let color = AnsiColorEnum::Rgb(RgbColor(r, g, b));
                                if is_fg {
                                    current.color = Some(color);
                                } else {
                                    current.bg_color = Some(color);
                                }
                                index += 4;
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        index += 1;
    }
}

fn sgr_payload(sequence: &str) -> Option<&str> {
    if sequence.starts_with("\u{1b}[") && sequence.ends_with('m') {
        Some(&sequence[2..sequence.len().saturating_sub(1)])
    } else {
        None
    }
}

fn ansi_output_segments(text: &str, styles: &PtyLineStyles) -> Option<Vec<InlineSegment>> {
    if !text.contains('\u{1b}') {
        return None;
    }

    let mut segments = Vec::new();
    let mut current = styles.output.as_ref().clone();
    let fallback = styles.output.as_ref().clone();
    let mut index = 0usize;
    let mut text_buffer = String::new();

    while index < text.len() {
        let Some(remaining) = text.get(index..) else {
            break;
        };
        let Some(first) = remaining.as_bytes().first() else {
            break;
        };

        if *first == 0x1b {
            if !text_buffer.is_empty() {
                segments.push(InlineSegment {
                    text: std::mem::take(&mut text_buffer),
                    style: Arc::new(current.clone()),
                });
            }

            if let Some(len) = vtcode_core::utils::ansi_parser::parse_ansi_sequence(remaining) {
                if let Some(sequence) = remaining.get(..len)
                    && let Some(payload) = sgr_payload(sequence)
                {
                    apply_sgr_codes(payload, &mut current, &fallback);
                }
                index += len;
                continue;
            }

            // Incomplete ANSI sequence: preserve remaining text as-is.
            text_buffer.push_str(remaining);
            index = text.len();
            continue;
        }

        let mut chars = remaining.chars();
        if let Some(ch) = chars.next() {
            text_buffer.push(ch);
            index += ch.len_utf8();
        } else {
            break;
        }
    }

    if !text_buffer.is_empty() {
        segments.push(InlineSegment {
            text: text_buffer,
            style: Arc::new(current),
        });
    }

    if segments.is_empty() {
        return None;
    }
    Some(
        segments
            .into_iter()
            .filter(|segment| !segment.text.is_empty())
            .collect(),
    )
}

fn append_output_segments_with_ansi(
    segments: &mut Vec<InlineSegment>,
    text: &str,
    styles: &PtyLineStyles,
) {
    if let Some(mut ansi_segments) = ansi_output_segments(text, styles) {
        segments.append(&mut ansi_segments);
    } else {
        segments.push(InlineSegment {
            text: text.to_string(),
            style: Arc::clone(&styles.output),
        });
    }
}

fn line_to_segments(line: &str, styles: &PtyLineStyles) -> Vec<InlineSegment> {
    if let Some(command_text) = line.strip_prefix("• Ran ") {
        let mut segments = vec![
            InlineSegment {
                text: "• ".to_string(),
                style: Arc::clone(&styles.glyph),
            },
            InlineSegment {
                text: "Ran".to_string(),
                style: Arc::clone(&styles.verb),
            },
            InlineSegment {
                text: " ".to_string(),
                style: Arc::clone(&styles.output),
            },
        ];
        segments.extend(shell_syntax_segments(command_text, styles, true));
        return segments;
    }

    if let Some(text) = line.strip_prefix("  │ ") {
        let mut segments = vec![
            InlineSegment {
                text: "  ".to_string(),
                style: Arc::clone(&styles.output),
            },
            InlineSegment {
                text: "│".to_string(),
                style: Arc::clone(&styles.glyph),
            },
            InlineSegment {
                text: " ".to_string(),
                style: Arc::clone(&styles.output),
            },
        ];
        segments.extend(shell_syntax_segments(text, styles, false));
        return segments;
    }

    if let Some(text) = line.strip_prefix("  └ ") {
        let mut segments = vec![
            InlineSegment {
                text: "  ".to_string(),
                style: Arc::clone(&styles.output),
            },
            InlineSegment {
                text: "└".to_string(),
                style: Arc::clone(&styles.glyph),
            },
            InlineSegment {
                text: " ".to_string(),
                style: Arc::clone(&styles.output),
            },
        ];
        append_output_segments_with_ansi(&mut segments, text, styles);
        return segments;
    }

    if line.trim_start().starts_with('…') {
        return vec![InlineSegment {
            text: line.to_string(),
            style: Arc::clone(&styles.truncation),
        }];
    }

    if let Some(text) = line.strip_prefix("    ") {
        let mut segments = vec![InlineSegment {
            text: "    ".to_string(),
            style: Arc::clone(&styles.output),
        }];
        append_output_segments_with_ansi(&mut segments, text, styles);
        return segments;
    }

    vec![InlineSegment {
        text: line.to_string(),
        style: Arc::clone(&styles.output),
    }]
}

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

    fn render_lines(&self, limit: usize) -> Vec<String> {
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

    fn last_display_line(&self) -> Option<String> {
        if !self.current_line.is_empty() {
            return Some(self.current_line.clone());
        }
        self.tail_lines
            .back()
            .cloned()
            .or_else(|| self.head_lines.last().cloned())
    }

    pub(super) fn render_segments(
        &mut self,
        chunk: &str,
        tail_limit: usize,
    ) -> (usize, Vec<Vec<InlineSegment>>, Option<String>) {
        self.apply_chunk(chunk, tail_limit);
        let rendered = self.render_lines(tail_limit);
        let styles = PtyLineStyles::new();
        let segments = rendered
            .into_iter()
            .map(|line| line_to_segments(&line, &styles))
            .collect::<Vec<_>>();
        let replace_count = self.displayed_count;
        self.displayed_count = segments.len();
        let last_line = self.last_display_line();
        (replace_count, segments, last_line)
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

pub(super) struct PtyStreamRuntime {
    sender: Option<mpsc::UnboundedSender<String>>,
    task: Option<JoinHandle<()>>,
    active: Arc<AtomicBool>,
}

impl PtyStreamRuntime {
    const MAX_LIVE_STREAM_LINES: usize = 12;

    pub(super) fn start(
        handle: InlineHandle,
        progress_reporter: ProgressReporter,
        tail_limit: usize,
        command_prompt: Option<String>,
    ) -> (Self, ToolProgressCallback) {
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let active = Arc::new(AtomicBool::new(true));
        let worker_active = Arc::clone(&active);
        let effective_tail_limit = tail_limit.clamp(1, Self::MAX_LIVE_STREAM_LINES);

        let task = tokio::spawn(async move {
            let mut state = PtyStreamState::new(command_prompt);
            let (replace_count, segments, _) = state.render_segments("", effective_tail_limit);
            if !segments.is_empty() && worker_active.load(Ordering::Relaxed) {
                handle.replace_last(replace_count, InlineMessageKind::Pty, segments);
            }

            while let Some(output) = rx.recv().await {
                if !worker_active.load(Ordering::Relaxed) {
                    break;
                }
                if output.is_empty() {
                    continue;
                }

                let visible_output = vtcode_core::utils::ansi_parser::strip_ansi(&output);
                if visible_output.trim().is_empty() {
                    continue;
                }

                let (replace_count, segments, last_line) =
                    state.render_segments(&output, effective_tail_limit);
                if !segments.is_empty() && worker_active.load(Ordering::Relaxed) {
                    handle.replace_last(replace_count, InlineMessageKind::Pty, segments);
                }

                if let Some(last_line) = last_line {
                    let cleaned_last_line = vtcode_core::utils::ansi_parser::strip_ansi(&last_line);
                    if !cleaned_last_line.trim().is_empty() {
                        progress_reporter.set_message(cleaned_last_line).await;
                    }
                }
            }
        });

        let callback_active = Arc::clone(&active);
        let callback_tx = tx.clone();
        let callback: ToolProgressCallback = Arc::new(move |_name: &str, output: &str| {
            if !callback_active.load(Ordering::Relaxed) || output.is_empty() {
                return;
            }
            let _ = callback_tx.send(output.to_string());
        });

        (
            Self {
                sender: Some(tx),
                task: Some(task),
                active,
            },
            callback,
        )
    }

    pub(super) async fn shutdown(mut self) {
        self.active.store(false, Ordering::Relaxed);
        let _ = self.sender.take();
        if let Some(task) = self.task.take() {
            let _ = tokio::time::timeout(Duration::from_millis(250), task).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flatten_text(segments: &[InlineSegment]) -> String {
        segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect::<Vec<_>>()
            .join("")
    }

    #[test]
    fn pty_stream_state_streams_incremental_chunks() {
        let mut state = PtyStreamState::new(None);
        state.apply_chunk("line1\nline2", 5);
        let rendered = state.render_lines(5);
        assert_eq!(
            rendered,
            vec!["  └ line1".to_string(), "    line2".to_string()]
        );
        assert_eq!(
            state.last_display_line(),
            Some("line2".to_string()),
            "expected partial line to be tracked"
        );
    }

    #[test]
    fn pty_stream_state_handles_carriage_return_overwrite() {
        let mut state = PtyStreamState::new(None);
        state.apply_chunk("start\rreplace\n", 5);
        let rendered = state.render_lines(5);
        assert_eq!(rendered, vec!["  └ replace".to_string()]);
        assert_eq!(
            state.last_display_line(),
            Some("replace".to_string()),
            "expected overwritten line to be retained"
        );
    }

    #[test]
    fn pty_stream_state_applies_tail_truncation() {
        let mut state = PtyStreamState::new(None);
        state.apply_chunk("a\nb\nc\nd\ne\nf\ng\n", 5);
        let rendered = state.render_lines(5);
        assert_eq!(
            rendered,
            vec![
                "  └ a".to_string(),
                "    b".to_string(),
                "    c".to_string(),
                "    … +1 line".to_string(),
                "    e".to_string(),
                "    f".to_string(),
                "    g".to_string(),
            ]
        );
    }

    #[test]
    fn pty_stream_state_formats_hidden_line_summary() {
        let mut state = PtyStreamState::new(None);
        state.apply_chunk("a\nb\nc\nd\ne\nf\ng\nh\n", 5);
        let rendered = state.render_lines(5);
        assert!(rendered.contains(&"    … +2 lines".to_string()));
    }

    #[test]
    fn pty_stream_state_deduplicates_consecutive_lines() {
        let mut state = PtyStreamState::new(None);
        state.apply_chunk("same\nsame\nnext\n", 5);
        let rendered = state.render_lines(5);
        assert_eq!(
            rendered,
            vec!["  └ same".to_string(), "    next".to_string()]
        );
    }

    #[test]
    fn pty_stream_state_preserves_indentation_and_blank_lines() {
        let mut state = PtyStreamState::new(None);
        state.apply_chunk("  fn main() {\n\n    println!(\"hi\");\n  }\n", 8);
        let rendered = state.render_lines(8);
        assert_eq!(
            rendered,
            vec![
                "  └   fn main() {".to_string(),
                "    ".to_string(),
                "        println!(\"hi\");".to_string(),
                "      }".to_string(),
            ]
        );
    }

    #[test]
    fn pty_stream_state_renders_command_prompt_without_output() {
        let state = PtyStreamState::new(Some("cargo check".to_string()));
        let rendered = state.render_lines(5);
        assert_eq!(rendered, vec!["• Ran cargo check".to_string()]);
    }

    #[test]
    fn pty_stream_state_keeps_command_prompt_with_truncated_tail() {
        let mut state = PtyStreamState::new(Some("cargo check".to_string()));
        state.apply_chunk("a\nb\nc\nd\ne\nf\ng\n", 5);
        let rendered = state.render_lines(5);
        assert_eq!(
            rendered,
            vec![
                "• Ran cargo check".to_string(),
                "  └ a".to_string(),
                "    b".to_string(),
                "    c".to_string(),
                "    … +1 line".to_string(),
                "    e".to_string(),
                "    f".to_string(),
                "    g".to_string(),
            ]
        );
    }

    #[test]
    fn normalizes_command_prompt_whitespace() {
        let state = PtyStreamState::new(Some("  cargo   check \n -p  vtcode  ".to_string()));
        let rendered = state.render_lines(5);
        assert_eq!(rendered, vec!["• Ran cargo check -p vtcode".to_string()]);
    }

    #[test]
    fn wraps_long_command_header() {
        let command = "cargo test -p vtcode run_command_preview_ build_tool_summary_formats_run_command_as_ran";
        let state = PtyStreamState::new(Some(command.to_string()));
        let rendered = state.render_lines(5);
        assert_eq!(rendered.len(), 2);
        assert!(rendered[0].starts_with("• Ran cargo test -p vtcode run_command_preview_"));
        assert!(rendered[1].starts_with("  │ build_tool_summary_formats_run_command_as_ran"));
    }

    #[test]
    fn tokenization_preserves_whitespace() {
        let tokens = tokenize_preserve_whitespace("cargo   check -p  vtcode");
        assert_eq!(
            tokens,
            vec!["cargo", "   ", "check", " ", "-p", "  ", "vtcode"]
        );
    }

    #[test]
    fn line_to_segments_preserves_command_text() {
        let styles = PtyLineStyles::new();
        let line = "• Ran echo \"$HOME\" && cargo check";
        let segments = line_to_segments(line, &styles);
        assert_eq!(flatten_text(&segments), line);
    }

    #[test]
    fn line_to_segments_distinguishes_command_and_args_styles() {
        let styles = PtyLineStyles::new();
        let segments = line_to_segments("• Ran cargo fmt", &styles);
        assert_eq!(flatten_text(&segments), "• Ran cargo fmt");
        assert!(
            segments
                .iter()
                .any(|segment| !segment.text.trim().is_empty() && segment.style.color.is_some()),
            "expected syntax-highlighted command segments"
        );
    }

    #[test]
    fn line_to_segments_handles_invalid_bash_input_without_dropping_text() {
        let styles = PtyLineStyles::new();
        let segments = line_to_segments("• Ran )(", &styles);
        assert_eq!(flatten_text(&segments), "• Ran )(");
    }

    #[test]
    fn line_to_segments_preserves_stdout_ansi_styles() {
        let styles = PtyLineStyles::new();
        let segments = line_to_segments("  └ \u{1b}[31mERR\u{1b}[0m done", &styles);
        assert_eq!(flatten_text(&segments), "  └ ERR done");

        let err_segment = segments
            .iter()
            .find(|segment| segment.text.contains("ERR"))
            .expect("colored text segment should be present");
        assert_eq!(
            err_segment.style.color,
            Some(AnsiColorEnum::Ansi(AnsiColor::Red))
        );
    }

    #[test]
    fn line_to_segments_ignores_non_sgr_ansi_sequences_without_dropping_text() {
        let styles = PtyLineStyles::new();
        let segments = line_to_segments("  └ \u{1b}[2Kclean", &styles);
        assert_eq!(flatten_text(&segments), "  └ clean");
        let clean_segment = segments
            .iter()
            .find(|segment| segment.text.contains("clean"))
            .expect("text segment should be present");
        assert_eq!(*clean_segment.style, *styles.output);
    }

    #[test]
    fn line_to_segments_stdout_defaults_to_dimmed_style() {
        let styles = PtyLineStyles::new();
        let segments = line_to_segments("  └ cargo check done", &styles);
        let output_segment = segments
            .iter()
            .find(|segment| segment.text.contains("cargo check done"))
            .expect("stdout segment should be present");
        assert!(output_segment.style.effects.contains(Effects::DIMMED));
    }

    #[test]
    fn line_to_segments_continuation_line_keeps_first_token_as_arg_style() {
        let styles = PtyLineStyles::new();
        let segments = line_to_segments("  │ --flag value", &styles);
        assert_eq!(flatten_text(&segments), "  │ --flag value");
        assert!(
            segments
                .iter()
                .any(|segment| !segment.text.trim().is_empty() && segment.style.color.is_some()),
            "expected syntax-highlighted continuation segments"
        );
    }
}
