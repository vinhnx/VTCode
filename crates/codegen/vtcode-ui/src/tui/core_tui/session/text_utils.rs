use std::mem;

use line_clipping::cohen_sutherland::clip_line;
use line_clipping::{LineSegment, Point, Window};
use ratatui::prelude::*;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub use vtcode_commons::ansi::strip_ansi_codes;

pub(super) fn trim_trailing_wrap_whitespace(spans: &mut Vec<Span<'static>>) {
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

/// Terminal display width of `text` in cells.
///
/// This is `unicode-width` plus the halfwidth-voiced-sound-mark correction
/// that terminals apply: `U+FF9E` (dakuten) and `U+FF9F` (handakuten) report
/// width 0 but occupy one cell each (same correction as
/// `ratatui::buffer::CellWidth`). Without it, strings like `ｶﾞ` measure 1
/// cell wide here but render 2 cells wide, breaking wrap and truncation math.
///
/// Prefer this over calling `unicode-width` directly on the wrapping path.
pub(crate) fn display_width(text: &str) -> usize {
    UnicodeWidthStr::width(text).saturating_add(text.chars().filter(|ch| matches!(ch, '\u{FF9E}' | '\u{FF9F}')).count())
}

/// Simplify tool call display text for better human readability
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
    let hang_width = if stripped.starts_with("  └ ") || stripped.starts_with("  │ ") || stripped.starts_with("    ")
    {
        4
    } else if stripped.trim_start().starts_with("└ ")
        || stripped.trim_start().starts_with("│ ")
        || stripped.trim_start().starts_with("├ ")
    {
        // Tree detail without leading gutter: "└ " width 2
        2
    } else if let Some((_, prefix)) = super::reflow::parse_tool_call_prefix(&stripped) {
        UnicodeWidthStr::width(prefix) + 1
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

pub(crate) fn compact_tree_continuation_prefix(line: &str) -> Option<String> {
    let leading_spaces = line.chars().take_while(|character| *character == ' ').count();
    let rest = line.get(leading_spaces..)?;
    let is_tree_row = ["├ ", "└ ", "│ ", "[-] ", "□ ", "[x] ", "[!] "]
        .iter()
        .any(|prefix| rest.starts_with(prefix));
    is_tree_row.then(|| " ".repeat(leading_spaces + 2))
}

pub(crate) fn wrap_line_with_hanging_prefix(
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

    // Fast path: single-style ASCII prose (the common transcript case). Avoids
    // grapheme clustering, f64 clip_line, and per-token String rebuilds
    // (hotpath: wrap_line was ~45% of TUI reflow time).
    if prefer_word_boundaries && continuation_prefix.is_empty() && line.spans.len() == 1 {
        let span = &line.spans[0];
        let text = span.content.as_ref();
        if text.is_ascii() && !text.contains('\n') && !text.contains('\r') {
            return wrap_ascii_word_boundaries(text, span.style, max_width);
        }
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

    let continuation_width = UnicodeWidthStr::width(continuation_prefix);
    let use_continuation_prefix =
        !continuation_prefix.is_empty() && continuation_width > 0 && continuation_width < max_width;
    // Diff rows carry a tinted bg on every span; paint the hanging-indent
    // prefix with the same bg so wrapped continuation rows don't start with
    // an unpainted strip. Non-diff lines keep the default prefix style.
    // Blockquote continuations (`│ `) keep the bar's own style so the wrapped
    // bar matches the first row's dimmed gutter instead of rendering bright.
    let continuation_prefix_style = if continuation_prefix.contains('│') {
        line.spans
            .iter()
            .find(|span| span.content.contains('│'))
            .map(|span| span.style)
            .unwrap_or_else(|| {
                line.spans
                    .iter()
                    .find_map(|span| span.style.bg)
                    .map(|bg| Style::default().bg(bg))
                    .unwrap_or_default()
            })
    } else {
        line.spans
            .iter()
            .find_map(|span| span.style.bg)
            .map(|bg| Style::default().bg(bg))
            .unwrap_or_default()
    };

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
                push_span(spans, &continuation_prefix_style, continuation_prefix);
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

            let width = display_width(grapheme);
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

                        let token_width = display_width(token);
                        if token_width == 0 {
                            ensure_continuation_prefix(&mut current_spans, &mut current_width, &rows);
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
                                ensure_continuation_prefix(&mut current_spans, &mut current_width, &rows);
                            }
                            push_span(&mut current_spans, &style, token);
                            current_width += token_width;
                            continue;
                        }

                        push_wrapped_token(token, &style, &mut current_spans, &mut current_width, &mut rows);
                    }
                } else {
                    push_wrapped_token(text, &style, &mut current_spans, &mut current_width, &mut rows);
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

/// Word-wrap a single-style ASCII string at spaces. Each output row is one
/// `Span` sliced from the source (no per-token reallocation). Long words hard-
/// break at `max_width` cells (ASCII ⇒ bytes == cells).
fn wrap_ascii_word_boundaries(text: &str, style: Style, max_width: usize) -> Vec<Line<'static>> {
    if text.len() <= max_width {
        return vec![Line::from(Span::styled(text.to_owned(), style))];
    }

    let mut rows = Vec::with_capacity(text.len() / max_width + 1);
    let bytes = text.as_bytes();
    let mut line_start = 0usize;

    while line_start < text.len() {
        let remaining = text.len() - line_start;
        if remaining <= max_width {
            rows.push(Line::from(Span::styled(text[line_start..].to_owned(), style)));
            break;
        }

        // Prefer breaking at the last space within the window.
        let window_end = line_start + max_width;
        let mut break_at = None;
        let mut i = window_end;
        while i > line_start {
            i -= 1;
            if bytes[i] == b' ' {
                break_at = Some(i);
                break;
            }
        }

        match break_at {
            Some(space) if space > line_start => {
                let row = text[line_start..space].trim_end_matches(char::is_whitespace);
                rows.push(Line::from(Span::styled(row.to_owned(), style)));
                line_start = space + 1; // drop the break space
            }
            _ => {
                // Hard break a long word.
                let row = text[line_start..window_end].trim_end_matches(char::is_whitespace);
                rows.push(Line::from(Span::styled(row.to_owned(), style)));
                line_start = window_end;
            }
        }
    }

    if rows.is_empty() {
        rows.push(Line::default());
    }
    // Last row: match wrap_line_internal's flush trim.
    if let Some(last) = rows.last_mut() {
        if let Some(span) = last.spans.first_mut() {
            let trimmed = span.content.as_ref().trim_end_matches(char::is_whitespace).to_owned();
            span.content = trimmed.into();
        }
    }
    rows
}

fn coalesce_adjacent_spans(mut spans: Vec<Span<'static>>) -> Vec<Span<'static>> {
    if spans.is_empty() {
        return spans;
    }

    let mut write = 0usize;
    let mut last_style: Option<Style> = Some(spans[0].style);

    for read in 0..spans.len() {
        let content = spans[read].content.clone();
        let style = spans[read].style;
        if content.is_empty() {
            continue;
        }

        if let Some(last) = last_style
            && last == style
            && write > 0
        {
            spans[write - 1].content.to_mut().push_str(content.as_ref());
        } else {
            if write != read {
                spans[write] = Span::styled(content, style);
            }
            write += 1;
            last_style = Some(style);
        }
    }

    spans.truncate(write);
    spans
}

fn wrapped_continuation_prefix(line: &Line<'static>) -> String {
    let text: String = line.spans.iter().map(|span| span.content.as_ref()).collect();
    structural_continuation_prefix(&text)
}

pub(crate) fn hanging_prefix_for_text(text: &str) -> String {
    structural_continuation_prefix(text)
}

fn structural_continuation_prefix(text: &str) -> String {
    let stripped = strip_ansi_codes(text);
    let text = stripped.as_ref();
    let bytes = text.as_bytes();
    let mut index = 0usize;
    let mut leading_width = 0usize;

    while index < bytes.len() {
        let Some(ch) = text[index..].chars().next() else {
            break;
        };
        if !ch.is_whitespace() || ch == '\n' || ch == '\r' {
            break;
        }
        leading_width += UnicodeWidthStr::width(ch.encode_utf8(&mut [0u8; 4]) as &str);
        index += ch.len_utf8();
    }

    let mut blockquote_depth = 0usize;
    while text[index..].starts_with("│ ") {
        blockquote_depth += 1;
        index += "│ ".len();
    }

    let remaining = &text[index..];
    let marker_width = if remaining.starts_with("- ") {
        Some(UnicodeWidthStr::width("- "))
    } else if remaining.starts_with("* ") {
        Some(UnicodeWidthStr::width("* "))
    } else if remaining.starts_with("+ ") {
        Some(UnicodeWidthStr::width("+ "))
    } else if let Some(after_bullet) = remaining.strip_prefix("• ") {
        let verb_end = after_bullet.find(|c: char| c.is_whitespace()).unwrap_or(after_bullet.len());
        let verb = &after_bullet[..verb_end];
        let is_tool_verb = !verb.is_empty()
            && after_bullet.len() > verb_end
            && after_bullet[verb_end..].starts_with(' ')
            && matches!(
                verb.to_ascii_lowercase().as_str(),
                "ran"
                    | "run"
                    | "read"
                    | "write"
                    | "edit"
                    | "search"
                    | "grep"
                    | "find"
                    | "glob"
                    | "list"
                    | "exec"
                    | "bash"
                    | "update"
                    | "create"
                    | "delete"
                    | "move"
            );
        if is_tool_verb {
            // "• Verb " → include verb plus trailing space
            Some(UnicodeWidthStr::width(&remaining[.."• ".len() + verb.len() + 1]))
        } else {
            Some(UnicodeWidthStr::width("• "))
        }
    } else if remaining.starts_with("◦ ") {
        Some(UnicodeWidthStr::width("◦ "))
    } else if remaining.starts_with("▪ ") {
        Some(UnicodeWidthStr::width("▪ "))
    } else {
        numbered_list_marker_width(remaining)
    };

    if let Some(marker_width) = marker_width {
        let mut prefix = String::new();
        if leading_width > 0 {
            prefix.push_str(&" ".repeat(leading_width));
        }
        for _ in 0..blockquote_depth {
            prefix.push_str("│ ");
        }
        prefix.push_str(&" ".repeat(marker_width));
        return prefix;
    }

    if blockquote_depth == 0 {
        if leading_width > 0 {
            return " ".repeat(leading_width);
        }
        return String::new();
    }

    let mut prefix = String::new();
    if leading_width > 0 {
        prefix.push_str(&" ".repeat(leading_width));
    }
    for _ in 0..blockquote_depth {
        prefix.push_str("│ ");
    }
    prefix
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

    Some(UnicodeWidthStr::width(&text[..end.max(end_after_separator)]))
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
    let patterns_completed = ["- [x]", "- [X]", "* [x]", "* [X]", "+ [x]", "+ [X]", "[x]", "[X]"];

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
pub fn is_list_item(text: &str) -> bool {
    let trimmed = text.trim_start();

    // Bullet patterns
    if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") || trimmed.starts_with("• ")
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
    fn trailing_wrap_whitespace_keeps_unicode_and_source_style() {
        let style = Style::default().fg(Color::Red);
        let mut spans = vec![
            Span::styled("日本\u{2003}".to_string(), style),
            Span::styled(" \t".to_string(), Style::default().fg(Color::Blue)),
        ];
        trim_trailing_wrap_whitespace(&mut spans);
        assert_eq!(spans, vec![Span::styled("日本".to_string(), style)]);
        trim_trailing_wrap_whitespace(&mut spans);
        assert_eq!(spans, vec![Span::styled("日本".to_string(), style)]);

        let mut whitespace = vec![Span::raw("\u{2003}\t".to_string())];
        trim_trailing_wrap_whitespace(&mut whitespace);
        assert!(whitespace.is_empty());
        trim_trailing_wrap_whitespace(&mut whitespace);
        assert!(whitespace.is_empty());
    }

    #[test]
    fn wrap_ascii_fits_single_row() {
        let rows = wrap_ascii_word_boundaries("hello world", Style::default(), 20);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].spans[0].content.as_ref(), "hello world");
    }

    #[test]
    fn wrap_ascii_breaks_on_spaces() {
        let rows = wrap_ascii_word_boundaries("hello brave new world", Style::default(), 10);
        let texts: Vec<&str> = rows.iter().map(|r| r.spans[0].content.as_ref()).collect();
        assert_eq!(texts, vec!["hello", "brave new", "world"]);
    }

    #[test]
    fn wrap_ascii_hard_breaks_long_word() {
        let rows = wrap_ascii_word_boundaries("abcdefghijklmnop", Style::default(), 5);
        let texts: Vec<&str> = rows.iter().map(|r| r.spans[0].content.as_ref()).collect();
        assert_eq!(texts, vec!["abcde", "fghij", "klmno", "p"]);
    }

    #[test]
    fn test_strip_ansi_codes() {
        assert_eq!(strip_ansi_codes("\x1b[31mRed text\x1b[0m"), "Red text");
        assert_eq!(strip_ansi_codes("No codes here"), "No codes here");
        assert_eq!(strip_ansi_codes("\x1b[1;32mBold green\x1b[0m"), "Bold green");
    }

    #[test]
    fn test_simplify_tool_display() {
        assert_eq!(simplify_tool_display("file path/to/file"), "accessing path/to/file");
        assert_eq!(simplify_tool_display("path: path/to/file"), "file: path/to/file");
        assert_eq!(simplify_tool_display("grep pattern"), "searching for pattern");
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
        assert_eq!(detect_todo_state("  - [ ] Indented task"), TodoState::Pending);
    }

    #[test]
    fn test_detect_todo_state_completed() {
        assert_eq!(detect_todo_state("- [x] Done"), TodoState::Completed);
        assert_eq!(detect_todo_state("- [X] Done"), TodoState::Completed);
        assert_eq!(detect_todo_state("* [x] Done"), TodoState::Completed);
        assert_eq!(detect_todo_state("[x] Done"), TodoState::Completed);
        assert_eq!(detect_todo_state("  - [x] Indented done"), TodoState::Completed);
        assert_eq!(detect_todo_state("~~Strikethrough text~~"), TodoState::Completed);
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
    fn test_coalesce_adjacent_spans_merges_same_style() {
        use ratatui::style::Style;
        use ratatui::text::Span;

        let spans = vec![
            Span::styled("a", Style::default()),
            Span::styled("b", Style::default()),
            Span::styled("c", Style::new().bold()),
            Span::styled("d", Style::new().bold()),
            Span::styled("e", Style::default()),
        ];
        let merged = coalesce_adjacent_spans(spans);
        assert_eq!(merged.len(), 3);
        assert_eq!(merged[0].content.as_ref(), "ab");
        assert_eq!(merged[1].content.as_ref(), "cd");
        assert_eq!(merged[2].content.as_ref(), "e");
    }

    #[test]
    fn test_pty_wrapped_continuation_prefix() {
        assert_eq!(pty_wrapped_continuation_prefix("  ", "  └ cargo check"), "      ");
        assert_eq!(
            pty_wrapped_continuation_prefix("  ", "\u{1b}[32m• Ran cargo check -p vtcode\u{1b}[0m",),
            "        "
        );
        assert_eq!(pty_wrapped_continuation_prefix("  ", "• Ran cargo check -p vtcode"), "        ");
        assert_eq!(pty_wrapped_continuation_prefix("  ", "plain output"), "  ");
    }

    #[test]
    fn test_display_width_counts_halfwidth_sound_marks() {
        assert_eq!(display_width(""), 0);
        assert_eq!(display_width("hello"), 5);
        assert_eq!(display_width("日本"), 4);
        // Halfwidth katakana with dakuten/handakuten: the sound mark reports
        // width 0 to unicode-width but occupies one terminal cell.
        assert_eq!(display_width("ｶﾞ"), 2);
        assert_eq!(display_width("ﾊﾟ"), 2);
        assert_eq!(display_width("\u{FF9E}"), 1);
        assert_eq!(display_width("aｶﾞ"), 3);
    }

    #[test]
    fn test_wrap_line_splits_halfwidth_katakana_with_dakuten() {
        let wrapped = wrap_line(Line::from("aｶﾞ"), 2);
        let rendered: String = wrapped
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.clone()))
            .collect();
        assert_eq!(rendered, "aｶﾞ", "wrapping must preserve all characters");
        assert_eq!(wrapped.len(), 2, "dakuten occupies a cell, so width-3 content must wrap at width 2");
    }

    #[test]
    fn test_wrap_continuation_prefix_inherits_diff_bg() {
        use ratatui::style::{Color, Style};
        use ratatui::text::Span;

        let bg = Style::default().bg(Color::Rgb(20, 58, 45));
        let line = Line::from(vec![Span::styled("- long diff body that must wrap onto a second row", bg)]);
        let wrapped = wrap_line_with_hanging_prefix(line, 20, "  ");
        assert!(wrapped.len() > 1, "narrow width must wrap");
        assert_eq!(
            wrapped[1].spans[0].style.bg,
            Some(Color::Rgb(20, 58, 45)),
            "wrapped diff rows must not start with an unpainted strip"
        );
    }

    #[test]
    fn test_wrap_continuation_prefix_stays_plain_without_bg() {
        use ratatui::text::Span;

        let line = Line::from(vec![Span::raw("plain prose that must wrap onto a second row here")]);
        let wrapped = wrap_line_with_hanging_prefix(line, 20, "  ");
        assert!(wrapped.len() > 1, "narrow width must wrap");
        assert_eq!(wrapped[1].spans[0].style.bg, None);
    }

    #[test]
    fn test_blockquote_continuation_preserves_bar() {
        assert_eq!(structural_continuation_prefix("│ quote"), "│ ");
        assert_eq!(structural_continuation_prefix("│ │ nested"), "│ │ ");
        assert_eq!(structural_continuation_prefix("│ • item"), "│   ");
        assert_eq!(structural_continuation_prefix("  │ indented"), "  │ ");
        assert_eq!(structural_continuation_prefix("• item"), "  ");
        assert_eq!(structural_continuation_prefix("plain"), String::new());
    }

    #[test]
    fn test_wrap_line_preserves_blockquote_bar() {
        let line = Line::from("│ alpha beta gamma delta epsilon zeta eta theta");
        let wrapped = wrap_line(line, 12);
        assert!(wrapped.len() > 1, "narrow width must wrap, got {wrapped:?}");
        let rendered: Vec<String> = wrapped
            .iter()
            .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
            .collect();
        assert!(rendered[0].starts_with("│ "), "first row keeps bar, got {:?}", rendered[0]);
        for (idx, text) in rendered.iter().enumerate().skip(1) {
            assert!(
                text.starts_with("│ "),
                "wrapped blockquote continuation {idx} must keep bar, got {text:?} in {rendered:?}"
            );
        }
    }
}
