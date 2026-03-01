use ratatui::prelude::*;
use unicode_width::UnicodeWidthStr;

/// Ellipsis character used to indicate truncated text.
const ELLIPSIS: char = '…';

/// Truncate a styled line to `max_width` and append an ellipsis on overflow.
///
/// This function preserves a fast no-overflow path (returns original line
/// unchanged if it fits) and uses `truncate_line_to_width` for overflow cases,
/// appending `…` (ellipsis character) when truncation occurs.
///
/// # Arguments
///
/// * `line` - The line to potentially truncate
/// * `max_width` - The maximum width in display columns
///
/// # Returns
///
/// The original line if it fits, or a truncated line with ellipsis appended
pub(crate) fn truncate_line_with_ellipsis_if_overflow(
    line: Line<'static>,
    max_width: usize,
) -> Line<'static> {
    let total_width: usize = line.spans.iter().map(|s| s.width()).sum();
    if total_width <= max_width {
        // Fast path: no truncation needed
        return line;
    }

    // Reserve space for ellipsis (1 character width)
    let available_width = max_width.saturating_sub(ELLIPSIS.len_utf8());
    if available_width == 0 {
        // Edge case: not enough room for even the ellipsis
        return Line::from(Span::raw(ELLIPSIS.to_string()));
    }

    let mut truncated_line = truncate_line_to_width(line, available_width);
    truncated_line.spans.push(Span::raw(ELLIPSIS.to_string()));
    truncated_line
}

/// Truncate a `Line` to fit within `max_width` display columns.
///
/// Used for table lines where word-wrapping would break the box-drawing
/// alignment. Spans are trimmed at the character boundary that exceeds the
/// width; any remaining spans are dropped.
fn truncate_line_to_width(line: Line<'static>, max_width: usize) -> Line<'static> {
    let total: usize = line.spans.iter().map(|s| s.width()).sum();
    if total <= max_width {
        return line;
    }

    let mut remaining = max_width;
    let mut truncated_spans: Vec<Span<'static>> = Vec::with_capacity(line.spans.len());
    for span in line.spans {
        let span_width = span.width();
        if span_width <= remaining {
            remaining -= span_width;
            truncated_spans.push(span);
        } else {
            // Truncate within this span at a char boundary
            let mut chars_width = 0usize;
            let mut byte_end = 0usize;
            for ch in span.content.chars() {
                let cw = UnicodeWidthStr::width(ch.encode_utf8(&mut [0u8; 4]) as &str);
                if chars_width + cw > remaining {
                    break;
                }
                chars_width += cw;
                byte_end += ch.len_utf8();
            }
            if byte_end > 0 {
                let fragment: String = span.content[..byte_end].to_string();
                truncated_spans.push(Span::styled(fragment, span.style));
            }
            break;
        }
    }
    Line::from(truncated_spans)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_truncation_when_fits() {
        let line = Line::from("Hello");
        let result = truncate_line_with_ellipsis_if_overflow(line.clone(), 10);
        assert_eq!(result.spans.len(), 1);
        assert_eq!(result.spans[0].content, "Hello");
    }

    #[test]
    fn test_truncation_with_ellipsis() {
        let line = Line::from("Hello World");
        let result = truncate_line_with_ellipsis_if_overflow(line, 8);
        let result_text: String = result.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(result_text.ends_with(ELLIPSIS));
        assert!(result_text.len() < "Hello World".len());
    }

    #[test]
    fn test_ellipsis_only_when_very_narrow() {
        let line = Line::from("Hello");
        let result = truncate_line_with_ellipsis_if_overflow(line, 1);
        assert_eq!(result.spans[0].content, ELLIPSIS.to_string());
    }
}
