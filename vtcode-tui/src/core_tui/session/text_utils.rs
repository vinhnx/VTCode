use std::borrow::Cow;
use std::mem;

use line_clipping::cohen_sutherland::clip_line;
use line_clipping::{LineSegment, Point, Window};
use ratatui::prelude::*;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use vtcode_commons::ansi_codes::ESC_CHAR;

use crate::utils::ansi_parser::strip_ansi;

/// Strips ANSI escape codes from text to ensure plain text output
pub fn strip_ansi_codes(text: &str) -> Cow<'_, str> {
    if !text.contains(ESC_CHAR) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(strip_ansi(text))
}

/// Simplify tool call display text for better human readability
#[allow(dead_code)]
pub fn simplify_tool_display(text: &str) -> String {
    // Common patterns to simplify for human readability
    let simplified = if text.starts_with("file ") {
        // Convert "file path/to/file" to "accessing path/to/file"
        text.replacen("file ", "accessing ", 1)
    } else if text.starts_with("path: ") {
        // Convert "path: path/to/file" to "file: path/to/file"
        text.replacen("path: ", "file: ", 1)
    } else if text.contains(" → file ") {
        // Convert complex patterns to simpler ones
        text.replace(" → file ", " → ")
    } else if text.starts_with("grep ") {
        // Simplify grep patterns for better readability
        text.replacen("grep ", "searching for ", 1)
    } else if text.starts_with("find ") {
        // Simplify find patterns
        text.replacen("find ", "finding ", 1)
    } else if text.starts_with("list ") {
        // Simplify list patterns
        text.replacen("list ", "listing ", 1)
    } else {
        // Return original text if no simplification needed
        text.to_owned()
    };

    // Further simplify parameter displays
    format_tool_parameters(&simplified)
}

/// Format tool parameters for better readability
#[allow(dead_code)]
pub fn format_tool_parameters(text: &str) -> String {
    // Convert common parameter patterns to more readable formats
    let mut formatted = text.to_owned();

    // Convert "pattern: xyz" to "matching 'xyz'"
    if formatted.contains("pattern: ") {
        formatted = formatted.replace("pattern: ", "matching '");
        // Close the quote if there's a parameter separator
        if formatted.contains(" · ") {
            formatted = formatted.replacen(" · ", "' · ", 1);
        } else if formatted.contains("  ") {
            formatted = formatted.replacen("  ", "' ", 1);
        } else {
            formatted.push('\'');
        }
    }

    // Convert "path: xyz" to "in 'xyz'"
    if formatted.contains("path: ") {
        formatted = formatted.replace("path: ", "in '");
        // Close the quote if there's a parameter separator
        if formatted.contains(" · ") {
            formatted = formatted.replacen(" · ", "' · ", 1);
        } else if formatted.contains("  ") {
            formatted = formatted.replacen("  ", "' ", 1);
        } else {
            formatted.push('\'');
        }
    }

    formatted
}

pub(super) fn pty_wrapped_continuation_prefix(base_prefix: &str, line_text: &str) -> String {
    let stripped = strip_ansi_codes(line_text);
    let hang_width = if stripped.starts_with("  └ ")
        || stripped.starts_with("  │ ")
        || stripped.starts_with("    ")
    {
        4
    } else if stripped.starts_with("• Ran ") {
        "• Ran ".chars().count()
    } else {
        0
    };
    format!("{}{}", base_prefix, " ".repeat(hang_width))
}

/// Wrap a line of text to fit within the specified width.
///
/// This is the standard wrapping function for plain transcript text. It prefers
/// word boundaries for readable prose and falls back to grapheme wrapping for
/// oversized tokens. For URL-aware wrapping that preserves URLs as atomic units,
/// use `super::wrapping::adaptive_wrap_line` instead.
pub fn wrap_line(line: Line<'static>, max_width: usize) -> Vec<Line<'static>> {
    wrap_line_internal(line, max_width, "", true)
}

pub(super) fn wrap_line_with_hanging_prefix(
    line: Line<'static>,
    max_width: usize,
    continuation_prefix: &str,
) -> Vec<Line<'static>> {
    wrap_line_internal(line, max_width, continuation_prefix, false)
}

fn wrap_line_internal(
    mut line: Line<'static>,
    max_width: usize,
    continuation_prefix: &str,
    prefer_word_boundaries: bool,
) -> Vec<Line<'static>> {
    if max_width == 0 {
        return vec![Line::default()];
    }

    line.spans = coalesce_adjacent_spans(line.spans);
    let derived_continuation_prefix = if prefer_word_boundaries && continuation_prefix.is_empty() {
        wrapped_continuation_prefix(&line)
    } else {
        String::new()
    };
    let continuation_prefix = if continuation_prefix.is_empty() {
        derived_continuation_prefix.as_str()
    } else {
        continuation_prefix
    };

    fn push_span(spans: &mut Vec<Span<'static>>, style: &Style, text: &str) {
        if text.is_empty() {
            return;
        }

        if let Some(last) = spans.last_mut().filter(|last| last.style == *style) {
            last.content.to_mut().push_str(text);
            return;
        }

        spans.push(Span::styled(text.to_owned(), *style));
    }

    fn trim_trailing_wrap_whitespace(spans: &mut Vec<Span<'static>>) {
        while let Some(last) = spans.last_mut() {
            let trimmed_len = last.content.trim_end_matches(char::is_whitespace).len();
            if trimmed_len == last.content.len() {
                break;
            }
            if trimmed_len == 0 {
                spans.pop();
                continue;
            }
            last.content.to_mut().truncate(trimmed_len);
            break;
        }
    }

    let continuation_width = UnicodeWidthStr::width(continuation_prefix);
    let use_continuation_prefix =
        !continuation_prefix.is_empty() && continuation_width > 0 && continuation_width < max_width;

    let mut rows = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_width = 0usize;
    let window = Window::new(0.0, max_width as f64, -1.0, 1.0);

    let flush_current = |spans: &mut Vec<Span<'static>>, rows: &mut Vec<Line<'static>>| {
        if spans.is_empty() {
            rows.push(Line::default());
        } else {
            if prefer_word_boundaries {
                trim_trailing_wrap_whitespace(spans);
            }
            rows.push(Line::from(mem::take(spans)));
        }
    };

    let ensure_continuation_prefix =
        |spans: &mut Vec<Span<'static>>, current_width: &mut usize, rows: &[Line<'static>]| {
            if use_continuation_prefix && spans.is_empty() && !rows.is_empty() {
                push_span(spans, &Style::default(), continuation_prefix);
                *current_width = continuation_width;
            }
        };

    let line_start_width = |rows: &[Line<'static>]| -> usize {
        if use_continuation_prefix && !rows.is_empty() {
            continuation_width
        } else {
            0
        }
    };

    let push_wrapped_token = |token: &str,
                              style: &Style,
                              current_spans: &mut Vec<Span<'static>>,
                              current_width: &mut usize,
                              rows: &mut Vec<Line<'static>>| {
        for grapheme in UnicodeSegmentation::graphemes(token, true) {
            if grapheme.is_empty() {
                continue;
            }

            let width = UnicodeWidthStr::width(grapheme);
            if width == 0 {
                ensure_continuation_prefix(current_spans, current_width, rows);
                push_span(current_spans, style, grapheme);
                continue;
            }

            let mut attempts = 0usize;
            loop {
                ensure_continuation_prefix(current_spans, current_width, rows);
                let segment = LineSegment::new(
                    Point::new(*current_width as f64, 0.0),
                    Point::new((*current_width + width) as f64, 0.0),
                );

                match clip_line(segment, window) {
                    Some(clipped) => {
                        let visible = (clipped.p2.x - clipped.p1.x).round() as usize;
                        if visible == width {
                            push_span(current_spans, style, grapheme);
                            *current_width += width;
                            break;
                        }

                        if *current_width == 0 {
                            push_span(current_spans, style, grapheme);
                            *current_width += width;
                            break;
                        }

                        flush_current(current_spans, rows);
                        *current_width = 0;
                    }
                    None => {
                        if *current_width == 0 {
                            push_span(current_spans, style, grapheme);
                            *current_width += width;
                            break;
                        }

                        flush_current(current_spans, rows);
                        *current_width = 0;
                    }
                }

                attempts += 1;
                if attempts > 4 {
                    push_span(current_spans, style, grapheme);
                    *current_width += width;
                    break;
                }
            }

            if *current_width >= max_width {
                flush_current(current_spans, rows);
                *current_width = 0;
            }
        }
    };

    for span in line.spans.into_iter() {
        let style = span.style;
        let content = span.content.into_owned();
        if content.is_empty() {
            continue;
        }

        for piece in content.split_inclusive('\n') {
            let mut text = piece;
            let mut had_newline = false;
            if let Some(stripped) = text.strip_suffix('\n') {
                text = stripped;
                had_newline = true;
                if let Some(without_carriage) = text.strip_suffix('\r') {
                    text = without_carriage;
                }
            }

            if !text.is_empty() {
                if prefer_word_boundaries {
                    for token in UnicodeSegmentation::split_word_bounds(text) {
                        if token.is_empty() {
                            continue;
                        }

                        let token_width = UnicodeWidthStr::width(token);
                        if token_width == 0 {
                            ensure_continuation_prefix(
                                &mut current_spans,
                                &mut current_width,
                                &rows,
                            );
                            push_span(&mut current_spans, &style, token);
                            continue;
                        }

                        let token_is_whitespace = token.chars().all(char::is_whitespace);
                        let line_start = line_start_width(&rows);
                        let has_content = current_width > line_start;

                        if token_is_whitespace && !rows.is_empty() && !has_content {
                            continue;
                        }

                        ensure_continuation_prefix(&mut current_spans, &mut current_width, &rows);
                        if current_width + token_width <= max_width {
                            push_span(&mut current_spans, &style, token);
                            current_width += token_width;
                            continue;
                        }

                        if token_is_whitespace {
                            if has_content {
                                flush_current(&mut current_spans, &mut rows);
                                current_width = 0;
                            }
                            continue;
                        }

                        if token_width <= max_width {
                            if has_content {
                                flush_current(&mut current_spans, &mut rows);
                                current_width = 0;
                                ensure_continuation_prefix(
                                    &mut current_spans,
                                    &mut current_width,
                                    &rows,
                                );
                            }
                            push_span(&mut current_spans, &style, token);
                            current_width += token_width;
                            continue;
                        }

                        push_wrapped_token(
                            token,
                            &style,
                            &mut current_spans,
                            &mut current_width,
                            &mut rows,
                        );
                    }
                } else {
                    push_wrapped_token(
                        text,
                        &style,
                        &mut current_spans,
                        &mut current_width,
                        &mut rows,
                    );
                }
            }

            if had_newline {
                flush_current(&mut current_spans, &mut rows);
                current_width = 0;
            }
        }
    }

    if !current_spans.is_empty() {
        flush_current(&mut current_spans, &mut rows);
    } else if rows.is_empty() {
        rows.push(Line::default());
    }

    rows
}

fn coalesce_adjacent_spans(spans: Vec<Span<'static>>) -> Vec<Span<'static>> {
    let mut merged: Vec<Span<'static>> = Vec::with_capacity(spans.len());
    for span in spans {
        if span.content.is_empty() {
            continue;
        }
        if let Some(last) = merged.last_mut().filter(|last| last.style == span.style) {
            last.content.to_mut().push_str(span.content.as_ref());
        } else {
            merged.push(span);
        }
    }
    merged
}

fn wrapped_continuation_prefix(line: &Line<'static>) -> String {
    let text: String = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    structural_continuation_prefix(&text)
}

fn structural_continuation_prefix(text: &str) -> String {
    let stripped = strip_ansi_codes(text);
    let text = stripped.as_ref();
    let bytes = text.as_bytes();
    let mut index = 0usize;
    let mut width = 0usize;

    while index < bytes.len() {
        let Some(ch) = text[index..].chars().next() else {
            break;
        };
        if !ch.is_whitespace() || ch == '\n' || ch == '\r' {
            break;
        }
        width += UnicodeWidthStr::width(ch.encode_utf8(&mut [0u8; 4]) as &str);
        index += ch.len_utf8();
    }

    while text[index..].starts_with("│ ") {
        width += UnicodeWidthStr::width("│ ");
        index += "│ ".len();
    }

    let remaining = &text[index..];
    let marker_width = if remaining.starts_with("- ") {
        Some(UnicodeWidthStr::width("- "))
    } else if remaining.starts_with("* ") {
        Some(UnicodeWidthStr::width("* "))
    } else if remaining.starts_with("+ ") {
        Some(UnicodeWidthStr::width("+ "))
    } else if remaining.starts_with("• ") {
        Some(UnicodeWidthStr::width("• "))
    } else if remaining.starts_with("◦ ") {
        Some(UnicodeWidthStr::width("◦ "))
    } else if remaining.starts_with("▪ ") {
        Some(UnicodeWidthStr::width("▪ "))
    } else {
        numbered_list_marker_width(remaining)
    };

    if let Some(marker_width) = marker_width {
        return " ".repeat(width + marker_width);
    }

    if width > 0 {
        " ".repeat(width)
    } else {
        String::new()
    }
}

fn numbered_list_marker_width(text: &str) -> Option<usize> {
    let mut chars = text.char_indices().peekable();
    let mut end_after_head = None;

    while let Some((idx, ch)) = chars.peek().copied() {
        if ch.is_ascii_digit() || ch.is_ascii_alphabetic() {
            end_after_head = Some(idx + ch.len_utf8());
            chars.next();
        } else {
            break;
        }
    }

    end_after_head?;

    let (idx, separator) = chars.next()?;
    if separator != '.' && separator != ')' {
        return None;
    }
    let end_after_separator = idx + separator.len_utf8();

    let (idx, space) = chars.next()?;
    if !space.is_whitespace() {
        return None;
    }
    let end = idx + space.len_utf8();

    Some(UnicodeWidthStr::width(
        &text[..end.max(end_after_separator)],
    ))
}

/// Detect if a line is a todo/checkbox item and its state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TodoState {
    /// Unchecked: `- [ ]`, `* [ ]`, `[ ]`
    Pending,
    /// Checked: `- [x]`, `- [X]`, `* [x]`, `[x]`
    Completed,
    /// Not a todo item
    None,
}

/// Detect if a line contains a todo/checkbox pattern
pub fn detect_todo_state(text: &str) -> TodoState {
    let trimmed = text.trim_start();

    // Common patterns: "- [ ]", "* [ ]", "[ ]", "- [x]", "* [x]", "[x]"
    let patterns_pending = ["- [ ]", "* [ ]", "+ [ ]", "[ ]"];
    let patterns_completed = [
        "- [x]", "- [X]", "* [x]", "* [X]", "+ [x]", "+ [X]", "[x]", "[X]",
    ];

    for pattern in patterns_completed {
        if trimmed.starts_with(pattern) {
            return TodoState::Completed;
        }
    }

    for pattern in patterns_pending {
        if trimmed.starts_with(pattern) {
            return TodoState::Pending;
        }
    }

    // Also check for strikethrough markers (~~text~~)
    if trimmed.starts_with("~~") && trimmed.contains("~~") {
        return TodoState::Completed;
    }

    TodoState::None
}

/// Check if text appears to be a list item (bullet or numbered)
#[allow(dead_code)]
pub fn is_list_item(text: &str) -> bool {
    let trimmed = text.trim_start();

    // Bullet patterns
    if trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || trimmed.starts_with("+ ")
        || trimmed.starts_with("• ")
    {
        return true;
    }

    // Numbered patterns: "1.", "1)", "a.", "a)"
    let mut chars = trimmed.chars();
    if let Some(first) = chars.next()
        && (first.is_ascii_digit() || first.is_ascii_alphabetic())
        && let Some(second) = chars.next()
        && (second == '.' || second == ')')
        && let Some(third) = chars.next()
    {
        return third == ' ';
    }

    false
}

/// Justify plain text by distributing spaces evenly
pub fn justify_plain_text(text: &str, max_width: usize) -> Option<String> {
    let trimmed = text.trim();
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    if words.len() <= 1 {
        return None;
    }

    let total_word_width: usize = words.iter().map(|word| UnicodeWidthStr::width(*word)).sum();
    if total_word_width >= max_width {
        return None;
    }

    let gaps = words.len() - 1;
    let spaces_needed = max_width.saturating_sub(total_word_width);
    if spaces_needed <= gaps {
        return None;
    }

    let base_space = spaces_needed / gaps;
    if base_space == 0 {
        return None;
    }
    let extra = spaces_needed % gaps;

    let mut output = String::with_capacity(max_width + gaps);
    for (index, word) in words.iter().enumerate() {
        output.push_str(word);
        if index < gaps {
            let mut count = base_space;
            if index < extra {
                count += 1;
            }
            for _ in 0..count {
                output.push(' ');
            }
        }
    }

    Some(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi_codes() {
        assert_eq!(strip_ansi_codes("\x1b[31mRed text\x1b[0m"), "Red text");
        assert_eq!(strip_ansi_codes("No codes here"), "No codes here");
        assert_eq!(
            strip_ansi_codes("\x1b[1;32mBold green\x1b[0m"),
            "Bold green"
        );
    }

    #[test]
    fn test_simplify_tool_display() {
        assert_eq!(
            simplify_tool_display("file path/to/file"),
            "accessing path/to/file"
        );
        assert_eq!(
            simplify_tool_display("path: path/to/file"),
            "file: path/to/file"
        );
        assert_eq!(
            simplify_tool_display("grep pattern"),
            "searching for pattern"
        );
    }

    #[test]
    fn test_justify_plain_text() {
        let result = justify_plain_text("Hello world", 15);
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 15);

        assert_eq!(justify_plain_text("Short", 10), None);
        assert_eq!(justify_plain_text("Very long text string", 10), None);
    }

    #[test]
    fn test_detect_todo_state_pending() {
        assert_eq!(detect_todo_state("- [ ] Task"), TodoState::Pending);
        assert_eq!(detect_todo_state("* [ ] Task"), TodoState::Pending);
        assert_eq!(detect_todo_state("+ [ ] Task"), TodoState::Pending);
        assert_eq!(detect_todo_state("[ ] Task"), TodoState::Pending);
        assert_eq!(
            detect_todo_state("  - [ ] Indented task"),
            TodoState::Pending
        );
    }

    #[test]
    fn test_detect_todo_state_completed() {
        assert_eq!(detect_todo_state("- [x] Done"), TodoState::Completed);
        assert_eq!(detect_todo_state("- [X] Done"), TodoState::Completed);
        assert_eq!(detect_todo_state("* [x] Done"), TodoState::Completed);
        assert_eq!(detect_todo_state("[x] Done"), TodoState::Completed);
        assert_eq!(
            detect_todo_state("  - [x] Indented done"),
            TodoState::Completed
        );
        assert_eq!(
            detect_todo_state("~~Strikethrough text~~"),
            TodoState::Completed
        );
    }

    #[test]
    fn test_detect_todo_state_none() {
        assert_eq!(detect_todo_state("Regular text"), TodoState::None);
        assert_eq!(detect_todo_state("- Regular list item"), TodoState::None);
        assert_eq!(detect_todo_state("* Bullet point"), TodoState::None);
        assert_eq!(detect_todo_state("1. Numbered item"), TodoState::None);
    }

    #[test]
    fn test_is_list_item() {
        assert!(is_list_item("- Item"));
        assert!(is_list_item("* Item"));
        assert!(is_list_item("+ Item"));
        assert!(is_list_item("• Item"));
        assert!(is_list_item("1. Item"));
        assert!(is_list_item("a) Item"));
        assert!(is_list_item("  - Indented"));
        assert!(!is_list_item("Regular text"));
        assert!(!is_list_item(""));
    }

    #[test]
    fn test_pty_wrapped_continuation_prefix() {
        assert_eq!(
            pty_wrapped_continuation_prefix("  ", "  └ cargo check"),
            "      "
        );
        assert_eq!(
            pty_wrapped_continuation_prefix("  ", "\u{1b}[32m• Ran cargo check -p vtcode\u{1b}[0m",),
            "        "
        );
        assert_eq!(
            pty_wrapped_continuation_prefix("  ", "• Ran cargo check -p vtcode"),
            "        "
        );
        assert_eq!(pty_wrapped_continuation_prefix("  ", "plain output"), "  ");
    }
}
