use std::borrow::Cow;
use std::mem;

use line_clipping::cohen_sutherland::clip_line;
use line_clipping::{LineSegment, Point, Window};
use ratatui::prelude::*;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::utils::ansi_parser::strip_ansi;

/// Strips ANSI escape codes from text to ensure plain text output
pub fn strip_ansi_codes(text: &str) -> Cow<'_, str> {
    if !text.contains('\x1b') {
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
    let hang_width = if line_text.starts_with("  └ ")
        || line_text.starts_with("  │ ")
        || line_text.starts_with("    ")
    {
        4
    } else if line_text.starts_with("• Ran ") {
        "• Ran ".chars().count()
    } else {
        0
    };
    format!("{}{}", base_prefix, " ".repeat(hang_width))
}

/// Wrap a line of text to fit within the specified width
pub fn wrap_line(line: Line<'static>, max_width: usize) -> Vec<Line<'static>> {
    if max_width == 0 {
        return vec![Line::default()];
    }

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

    let mut rows = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_width = 0usize;
    let window = Window::new(0.0, max_width as f64, -1.0, 1.0);

    let flush_current = |spans: &mut Vec<Span<'static>>, rows: &mut Vec<Line<'static>>| {
        if spans.is_empty() {
            rows.push(Line::default());
        } else {
            rows.push(Line::from(mem::take(spans)));
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
                for grapheme in UnicodeSegmentation::graphemes(text, true) {
                    if grapheme.is_empty() {
                        continue;
                    }

                    let width = UnicodeWidthStr::width(grapheme);
                    if width == 0 {
                        push_span(&mut current_spans, &style, grapheme);
                        continue;
                    }

                    let mut attempts = 0usize;
                    loop {
                        let line_segment = LineSegment::new(
                            Point::new(current_width as f64, 0.0),
                            Point::new((current_width + width) as f64, 0.0),
                        );

                        match clip_line(line_segment, window) {
                            Some(clipped) => {
                                let visible = (clipped.p2.x - clipped.p1.x).round() as usize;
                                if visible == width {
                                    push_span(&mut current_spans, &style, grapheme);
                                    current_width += width;
                                    break;
                                }

                                if current_width == 0 {
                                    push_span(&mut current_spans, &style, grapheme);
                                    current_width += width;
                                    break;
                                }

                                flush_current(&mut current_spans, &mut rows);
                                current_width = 0;
                            }
                            None => {
                                if current_width == 0 {
                                    push_span(&mut current_spans, &style, grapheme);
                                    current_width += width;
                                    break;
                                }

                                flush_current(&mut current_spans, &mut rows);
                                current_width = 0;
                            }
                        }

                        attempts += 1;
                        if attempts > 4 {
                            push_span(&mut current_spans, &style, grapheme);
                            current_width += width;
                            break;
                        }
                    }

                    if current_width >= max_width {
                        flush_current(&mut current_spans, &mut rows);
                        current_width = 0;
                    }
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
    if let Some(first) = chars.next() {
        if first.is_ascii_digit() || first.is_ascii_alphabetic() {
            if let Some(second) = chars.next() {
                if second == '.' || second == ')' {
                    if let Some(third) = chars.next() {
                        return third == ' ';
                    }
                }
            }
        }
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
            pty_wrapped_continuation_prefix("  ", "• Ran cargo check -p vtcode"),
            "        "
        );
        assert_eq!(pty_wrapped_continuation_prefix("  ", "plain output"), "  ");
    }
}
