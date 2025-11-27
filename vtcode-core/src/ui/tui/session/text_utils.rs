use std::mem;

use line_clipping::cohen_sutherland::clip_line;
use line_clipping::{LineSegment, Point, Window};
use ratatui::{
    style::Style,
    text::{Line, Span},
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Strips ANSI escape codes from text to ensure plain text output
pub fn strip_ansi_codes(text: &str) -> String {
    // Comprehensive ANSI code stripping by looking for various escape sequences
    let mut result = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            // Found escape character, check what follows
            match chars.peek() {
                Some('[') => {
                    // CSI (Control Sequence Introducer): ESC[...m
                    chars.next(); // consume the '['
                    // Skip the parameters and final character
                    let mut param_length = 0;
                    while let Some(&next_ch) = chars.peek() {
                        chars.next(); // consume the character
                        param_length += 1;
                        if next_ch.is_ascii_digit() || next_ch == ';' || next_ch == ':' {
                            // These are parameter characters, continue
                            continue;
                        } else if ('@'..='~').contains(&next_ch) {
                            // This is the final command character, stop here
                            break;
                        } else if param_length > 20 {
                            // prevent infinite loops
                            break;
                        } else {
                            // Some other character, continue
                            continue;
                        }
                    }
                }
                Some(']') => {
                    // OSC (Operating System Command): ESC]...ST (where ST is \x1b\\ or BEL)
                    chars.next(); // consume the ']'
                    // Skip until we find the string terminator
                    loop {
                        match chars.peek() {
                            Some(&'\x07') => {
                                // BEL character (0x07) terminates the sequence
                                chars.next(); // consume the BEL
                                break;
                            }
                            Some(&'\x1b') => {
                                // Check if next char after ESC is backslash to form ST
                                let mut peekable_copy = chars.clone();
                                peekable_copy.next(); // consume the second ESC
                                if peekable_copy.peek() == Some(&'\\') {
                                    chars.next(); // consume the second ESC
                                    chars.next(); // consume the '\\'
                                    break; // ESC\\ terminates the sequence
                                } else {
                                    // This is just part of the OSC data, continue
                                    chars.next();
                                    continue;
                                }
                            }
                            Some(_) => {
                                // Continue consuming characters in the sequence
                                chars.next();
                            }
                            None => {
                                // End of string
                                break;
                            }
                        }
                    }
                }
                Some('(') | Some(')') | Some('*') | Some('+') | Some('-') | Some('.') => {
                    // G0, G1, G2, G3 character set selection: ESC(...)
                    chars.next(); // consume the special character
                    // consume one more parameter character if available
                    if chars.peek().is_some() {
                        chars.next();
                    }
                }
                Some(_) => {
                    // Other ESC sequences with specific characters
                    let next_ch = chars.peek().unwrap();
                    match next_ch {
                        '7' | '8' | '=' | '>' | 'D' | 'E' | 'H' | 'M' | 'O' | 'P' | 'V' | 'W'
                        | 'X' | 'Z' | '[' | '\\' | ']' | '^' | '_' => {
                            // These are single-character ESC sequences, consume the character
                            chars.next();
                        }
                        _ => {
                            // Not a known ESC sequence, treat as regular character
                            result.push(ch);
                        }
                    }
                }
                None => {
                    // End of string, just add the escape character
                    result.push(ch);
                }
            }
        } else {
            result.push(ch);
        }
    }

    result
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
}
