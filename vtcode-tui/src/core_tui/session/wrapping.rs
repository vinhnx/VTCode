//! Adaptive text wrapping with URL preservation.
//!
//! This module provides wrapping utilities that treat URLs as atomic units
//! that never get split across lines. This preserves terminal link detection
//! and ensures copy-paste works correctly for URLs.
//!
//! The wrapping strategy:
//! 1. **Non-URL lines:** Use standard word-wrapping
//! 2. **URL-only lines:** Emit unwrapped so terminal link detection works
//! 3. **Mixed lines (URL + prose):** Prose wraps naturally while URLs remain unsplit

use ratatui::prelude::*;
use regex::Regex;
use std::sync::LazyLock;
use unicode_width::UnicodeWidthStr;

/// URL detection regex pattern.
///
/// Matches:
/// - Full URLs with scheme: `https://example.com/path`
/// - Bare domains: `example.com/path`
/// - Localhost with port: `localhost:8080`
/// - IPv4 addresses: `192.168.1.1:8080`
///
/// IPv6 URLs are intentionally not matched in v1 due to complexity.
static URL_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?x)
        # Full URL with scheme
        [a-zA-Z][a-zA-Z0-9+.-]*://[^\s<>\[\]{}|\\^`\"']
        |
        # Bare domain (e.g., example.com/path)
        [a-zA-Z0-9][-a-zA-Z0-9]*\.[a-zA-Z]{2,}(/[^\s<>\[\]{}|\\^`\"']*)?
        |
        # Localhost with port
        localhost:\d+
        |
        # IPv4 with optional port
        \d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}(:\d+)?
    ",
    )
    .unwrap()
});

/// Detect if a string looks like a URL.
///
/// Uses conservative heuristics to avoid false positives.
pub fn is_url_like(text: &str) -> bool {
    URL_PATTERN.is_match(text.trim())
}

/// Find all URL spans in a line with their byte positions.
///
/// Returns a vector of (start, end) byte indices for each URL found.
fn find_url_spans(text: &str) -> Vec<(usize, usize)> {
    URL_PATTERN
        .find_iter(text)
        .map(|m| (m.start(), m.end()))
        .collect()
}

/// Wrap a line to fit within max_width, preserving URLs as atomic units.
///
/// This function implements a three-path wrapping strategy:
/// 1. If the line contains no URLs, use standard word wrapping
/// 2. If the line is only a URL, emit it unwrapped
/// 3. If the line mixes URLs and prose, wrap prose naturally while keeping URLs intact
///
/// # Arguments
///
/// * `line` - The line to wrap
/// * `max_width` - Maximum width in display columns
///
/// # Returns
///
/// A vector of wrapped lines
pub fn adaptive_wrap_line(line: Line<'static>, max_width: usize) -> Vec<Line<'static>> {
    if max_width == 0 {
        return vec![Line::default()];
    }

    // Reconstruct the full text to detect URLs
    let full_text: String = line
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();

    // If no URLs, use standard wrapping
    if !is_url_like(&full_text) {
        return wrap_line_standard(line, max_width);
    }

    // Find URL positions in the text
    let url_spans = find_url_spans(&full_text);

    // If the entire line is a single URL and it fits, emit unwrapped
    if url_spans.len() == 1
        && url_spans[0].0 == 0
        && url_spans[0].1 == full_text.len()
        && full_text.width() <= max_width
    {
        return vec![line];
    }

    // Complex case: mixed URLs and prose
    // We need to break the line into segments and wrap each appropriately
    adaptive_wrap_mixed_line(line, max_width, &url_spans)
}

/// Standard word wrapping for non-URL content.
///
/// This is the fallback wrapping behavior for text without URLs.
fn wrap_line_standard(line: Line<'static>, max_width: usize) -> Vec<Line<'static>> {
    use unicode_segmentation::UnicodeSegmentation;

    let mut rows = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_width = 0usize;

    let flush_current = |spans: &mut Vec<Span<'static>>, rows: &mut Vec<Line<'static>>| {
        if spans.is_empty() {
            rows.push(Line::default());
        } else {
            rows.push(Line::from(std::mem::take(spans)));
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
                        current_width += width;
                        continue;
                    }

                    if current_width + width > max_width && current_width > 0 {
                        flush_current(&mut current_spans, &mut rows);
                        current_width = 0;
                    }

                    push_span(&mut current_spans, &style, grapheme);
                    current_width += width;
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

/// Wrap a line that contains both URLs and regular prose.
///
/// URLs are kept as atomic units and never split, while prose wraps normally.
fn adaptive_wrap_mixed_line(
    line: Line<'static>,
    max_width: usize,
    url_spans: &[(usize, usize)],
) -> Vec<Line<'static>> {
    let mut rows = Vec::new();
    let mut current_spans: Vec<Span<'static>> = Vec::new();
    let mut current_width = 0usize;
    let mut byte_offset = 0usize;

    let flush_current = |spans: &mut Vec<Span<'static>>, rows: &mut Vec<Line<'static>>| {
        if spans.is_empty() {
            rows.push(Line::default());
        } else {
            rows.push(Line::from(std::mem::take(spans)));
        }
    };

    for span in line.spans.into_iter() {
        let style = span.style;
        let content = span.content.into_owned();
        let span_start = byte_offset;
        let span_end = byte_offset + content.len();
        byte_offset = span_end;

        if content.is_empty() {
            continue;
        }

        // Check if this span overlaps with any URL
        let mut url_overlaps = Vec::new();
        for (url_start, url_end) in url_spans {
            if *url_start < span_end && *url_end > span_start {
                // Calculate the overlap within this span
                let overlap_start = span_start.max(*url_start).saturating_sub(span_start);
                let overlap_end = span_end.min(*url_end).saturating_sub(span_start);
                url_overlaps.push((overlap_start, overlap_end));
            }
        }

        if url_overlaps.is_empty() {
            // No URL overlap - wrap normally
            wrap_and_push_text(&mut current_spans, &mut current_width, &style, &content, max_width);
        } else {
            // Has URL overlap - need to preserve URL integrity
            let mut pos = 0usize;
            for (url_rel_start, url_rel_end) in url_overlaps {
                // Add text before URL
                if pos < url_rel_start {
                    let before = &content[pos..url_rel_start];
                    wrap_and_push_text(
                        &mut current_spans,
                        &mut current_width,
                        &style,
                        before,
                        max_width,
                    );
                }

                // Add URL as atomic unit
                let url_text = &content[url_rel_start..url_rel_end];
                let url_width = url_text.width();

                if url_width > max_width {
                    // URL is too wide - must break it (last resort)
                    wrap_and_push_text(
                        &mut current_spans,
                        &mut current_width,
                        &style,
                        url_text,
                        max_width,
                    );
                } else {
                    // URL fits - emit as atomic unit
                    if current_width > 0 && current_width + url_width > max_width {
                        flush_current(&mut current_spans, &mut rows);
                        current_width = 0;
                    }
                    push_span(&mut current_spans, &style, url_text);
                    current_width += url_width;
                }

                pos = url_rel_end;
            }

            // Add remaining text after last URL
            if pos < content.len() {
                let after = &content[pos..];
                wrap_and_push_text(
                    &mut current_spans,
                    &mut current_width,
                    &style,
                    after,
                    max_width,
                );
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

/// Helper to wrap text and push to current spans, handling line breaks.
fn wrap_and_push_text(
    current_spans: &mut Vec<Span<'static>>,
    current_width: &mut usize,
    style: &Style,
    text: &str,
    max_width: usize,
) {
    use unicode_segmentation::UnicodeSegmentation;

    for grapheme in UnicodeSegmentation::graphemes(text, true) {
        if grapheme.is_empty() {
            continue;
        }

        let width = UnicodeWidthStr::width(grapheme);
        if width == 0 {
            push_span(current_spans, style, grapheme);
            continue;
        }

        if *current_width + width > max_width && *current_width > 0 {
            let flushed = std::mem::take(current_spans);
            current_spans.push(Span::styled("\n", *style));
            *current_width = 0;
        }

        push_span(current_spans, style, grapheme);
        *current_width += width;
    }
}

/// Helper to push a span, merging with the last one if styles match.
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

/// Wrap multiple lines with URL preservation.
///
/// Convenience function for wrapping a vector of lines.
pub fn adaptive_wrap_lines(lines: Vec<Line<'static>>, max_width: usize) -> Vec<Line<'static>> {
    if max_width == 0 {
        return vec![Line::default()];
    }

    let mut wrapped = Vec::new();
    for line in lines {
        wrapped.extend(adaptive_wrap_line(line, max_width));
    }
    if wrapped.is_empty() {
        wrapped.push(Line::default());
    }
    wrapped
}

/// Calculate the wrapped height of text using Paragraph::line_count.
///
/// This provides accurate height measurement that accounts for URL wrapping.
///
/// # Arguments
///
/// * `text` - The text to measure
/// * `width` - The available width
///
/// # Returns
///
/// The number of lines the text will occupy when wrapped
pub fn calculate_wrapped_height(text: &str, width: u16) -> usize {
    if width == 0 {
        return text.lines().count().max(1);
    }

    let paragraph = Paragraph::new(text);
    paragraph.line_count(width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_url_like_with_scheme() {
        assert!(is_url_like("https://example.com"));
        assert!(is_url_like("http://example.com/path"));
        assert!(is_url_like("ftp://files.example.com/file.txt"));
    }

    #[test]
    fn test_is_url_like_bare_domain() {
        assert!(is_url_like("example.com"));
        assert!(is_url_like("github.com/openai/codex"));
        assert!(is_url_like("docs.rs/vtcode"));
    }

    #[test]
    fn test_is_url_like_localhost() {
        assert!(is_url_like("localhost:8080"));
        assert!(is_url_like("localhost:3000/api"));
    }

    #[test]
    fn test_is_url_like_ipv4() {
        assert!(is_url_like("192.168.1.1"));
        assert!(is_url_like("127.0.0.1:8080"));
    }

    #[test]
    fn test_is_not_url_like() {
        assert!(!is_url_like("not a url"));
        assert!(!is_url_like("regular text"));
        assert!(!is_url_like("hello world"));
    }

    #[test]
    fn test_adaptive_wrap_url_only() {
        let line = Line::from(Span::raw("https://example.com/very/long/path"));
        let wrapped = adaptive_wrap_line(line, 80);
        assert_eq!(wrapped.len(), 1);
        let text: String = wrapped[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("https://"));
    }

    #[test]
    fn test_adaptive_wrap_mixed_content() {
        let line = Line::from(Span::raw(
            "Check out https://example.com for more information about the project",
        ));
        let wrapped = adaptive_wrap_line(line, 40);
        assert!(!wrapped.is_empty());

        // Verify URL is not split
        let all_text: String = wrapped
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(all_text.contains("https://example.com"));
    }

    #[test]
    fn test_adaptive_wrap_no_url_fallback() {
        let line = Line::from(Span::raw("This is regular text without any URLs"));
        let wrapped = adaptive_wrap_line(line.clone(), 20);
        assert!(!wrapped.is_empty());
    }

    #[test]
    fn test_calculate_wrapped_height() {
        let height = calculate_wrapped_height("Short line", 80);
        assert_eq!(height, 1);

        let height = calculate_wrapped_height("Very long line that should wrap", 20);
        assert!(height >= 1);
    }
}
