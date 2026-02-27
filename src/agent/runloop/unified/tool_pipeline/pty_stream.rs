use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use anstyle::{AnsiColor, Color as AnsiColorEnum, Effects, Style as AnsiStyle};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use vtcode_core::command_safety::shell_parser::parse_shell_commands_tree_sitter;
use vtcode_core::tools::registry::ToolProgressCallback;
use vtcode_core::ui::theme;
use vtcode_tui::{InlineHandle, InlineMessageKind, InlineSegment, InlineTextStyle, convert_style};

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
        let output = Arc::new(InlineTextStyle::default());
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

fn is_valid_bash_grammar(command: &str) -> bool {
    parse_shell_commands_tree_sitter(command)
        .map(|commands| !commands.is_empty())
        .unwrap_or(false)
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
        if is_valid_bash_grammar(command_text) {
            segments.extend(bash_segments(command_text, styles, true));
        } else {
            segments.push(InlineSegment {
                text: command_text.to_string(),
                style: Arc::clone(&styles.args),
            });
        }
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
        if is_valid_bash_grammar(text) {
            segments.extend(bash_segments(text, styles, false));
        } else {
            segments.push(InlineSegment {
                text: text.to_string(),
                style: Arc::clone(&styles.args),
            });
        }
        return segments;
    }

    if let Some(text) = line.strip_prefix("  └ ") {
        return vec![
            InlineSegment {
                text: "  ".to_string(),
                style: Arc::clone(&styles.output),
            },
            InlineSegment {
                text: "└".to_string(),
                style: Arc::clone(&styles.glyph),
            },
            InlineSegment {
                text: format!(" {}", text),
                style: Arc::clone(&styles.output),
            },
        ];
    }

    if line.trim_start().starts_with('…') {
        return vec![InlineSegment {
            text: line.to_string(),
            style: Arc::clone(&styles.truncation),
        }];
    }

    if let Some(text) = line.strip_prefix("    ") {
        return vec![
            InlineSegment {
                text: "    ".to_string(),
                style: Arc::clone(&styles.output),
            },
            InlineSegment {
                text: text.to_string(),
                style: Arc::clone(&styles.output),
            },
        ];
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

                let cleaned_output = vtcode_core::utils::ansi_parser::strip_ansi(&output);
                if cleaned_output.is_empty() {
                    continue;
                }

                let (replace_count, segments, last_line) =
                    state.render_segments(&cleaned_output, effective_tail_limit);
                if !segments.is_empty() && worker_active.load(Ordering::Relaxed) {
                    handle.replace_last(replace_count, InlineMessageKind::Pty, segments);
                }

                if let Some(last_line) = last_line {
                    progress_reporter.set_message(last_line).await;
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
        let cargo_style = segments
            .iter()
            .find(|segment| segment.text == "cargo")
            .map(|segment| Arc::clone(&segment.style))
            .expect("cargo token should be present");
        let fmt_style = segments
            .iter()
            .find(|segment| segment.text == "fmt")
            .map(|segment| Arc::clone(&segment.style))
            .expect("fmt token should be present");
        assert_ne!(*cargo_style, *fmt_style);
    }

    #[test]
    fn line_to_segments_skips_highlighting_for_invalid_bash_grammar() {
        let styles = PtyLineStyles::new();
        let segments = line_to_segments("• Ran )(", &styles);
        let invalid_style = segments
            .iter()
            .find(|segment| segment.text == ")(")
            .map(|segment| Arc::clone(&segment.style))
            .expect("invalid token should be present");
        assert_eq!(*invalid_style, *styles.args);
    }
}
