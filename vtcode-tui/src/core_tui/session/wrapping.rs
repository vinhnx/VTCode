//! URL-preserving text wrapping.
//!
//! Wraps text while keeping URLs as atomic units to preserve terminal link detection.

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use regex::Regex;
use std::sync::LazyLock;
use unicode_width::UnicodeWidthStr;

/// URL detection pattern - matches common URL formats.
static URL_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"[a-zA-Z][a-zA-Z0-9+.-]*://[^\s<>\[\]{}|^]+|[a-zA-Z0-9][-a-zA-Z0-9]*\.[a-zA-Z]{2,}(/[^\s<>\[\]{}|^]*)?|localhost:\d+|\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}(:\d+)?",
    )
    .unwrap()
});

/// Check if text contains a URL.
pub fn contains_url(text: &str) -> bool {
    URL_PATTERN.is_match(text)
}

/// Wrap a line, preserving URLs as atomic units.
///
/// - Lines without URLs: delegated to standard wrapping
/// - URL-only lines: returned unwrapped if they fit
/// - Mixed lines: URLs kept intact, surrounding text wrapped normally
pub fn wrap_line_preserving_urls(line: Line<'static>, max_width: usize) -> Vec<Line<'static>> {
    if max_width == 0 {
        return vec![Line::default()];
    }

    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();

    // No URLs - use standard wrapping (delegates to text_utils)
    if !contains_url(&text) {
        return super::text_utils::wrap_line(line, max_width);
    }

    // Find all URLs in the text
    let urls: Vec<_> = URL_PATTERN
        .find_iter(&text)
        .map(|m| (m.start(), m.end(), m.as_str()))
        .collect();

    // Single URL that fits - return unwrapped for terminal link detection
    if urls.len() == 1 && urls[0].0 == 0 && urls[0].1 == text.len() {
        if text.width() <= max_width {
            return vec![line];
        }
        // URL too wide - fall through to wrap it
    }

    // Mixed content - split around URLs and wrap each segment
    wrap_mixed_content(line, max_width, &urls)
}

/// Wrap text that contains URLs, keeping URLs intact.
fn wrap_mixed_content(
    line: Line<'static>,
    max_width: usize,
    urls: &[(usize, usize, &str)],
) -> Vec<Line<'static>> {
    use unicode_segmentation::UnicodeSegmentation;
    use unicode_width::UnicodeWidthStr;

    let mut result = Vec::new();
    let mut current_line: Vec<Span<'static>> = Vec::new();
    let mut current_width = 0usize;
    let mut text_pos = 0usize;

    let flush_line = |spans: &mut Vec<Span<'static>>, result: &mut Vec<Line<'static>>| {
        if spans.is_empty() {
            result.push(Line::default());
        } else {
            result.push(Line::from(std::mem::take(spans)));
        }
    };

    // Merge spans into a single style for simplicity when dealing with URLs
    let default_style = line.spans.first().map(|s| s.style).unwrap_or_default();

    for (url_start, url_end, url_text) in urls {
        // Process text before this URL
        if *url_start > text_pos {
            let before = &line
                .spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()[text_pos..*url_start];

            for grapheme in UnicodeSegmentation::graphemes(before, true) {
                let gw = grapheme.width();
                if current_width + gw > max_width && current_width > 0 {
                    flush_line(&mut current_line, &mut result);
                    current_width = 0;
                }
                current_line.push(Span::styled(grapheme.to_string(), default_style));
                current_width += gw;
            }
        }

        // Add URL as atomic unit
        let url_width = url_text.width();
        if current_width > 0 && current_width + url_width > max_width {
            flush_line(&mut current_line, &mut result);
            current_width = 0;
        }
        current_line.push(Span::styled(url_text.to_string(), default_style));
        current_width += url_width;

        text_pos = *url_end;
    }

    // Process remaining text after last URL
    if text_pos < line.spans.iter().map(|s| s.content.len()).sum::<usize>() {
        let full_text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        let remaining = &full_text[text_pos..];

        for grapheme in UnicodeSegmentation::graphemes(remaining, true) {
            let gw = grapheme.width();
            if current_width + gw > max_width && current_width > 0 {
                flush_line(&mut current_line, &mut result);
                current_width = 0;
            }
            current_line.push(Span::styled(grapheme.to_string(), default_style));
            current_width += gw;
        }
    }

    flush_line(&mut current_line, &mut result);
    if result.is_empty() {
        result.push(Line::default());
    }
    result
}

/// Wrap multiple lines with URL preservation.
pub fn wrap_lines_preserving_urls(
    lines: Vec<Line<'static>>,
    max_width: usize,
) -> Vec<Line<'static>> {
    if max_width == 0 {
        return vec![Line::default()];
    }
    lines
        .into_iter()
        .flat_map(|line| wrap_line_preserving_urls(line, max_width))
        .collect()
}

/// Calculate wrapped height using Paragraph::line_count.
pub fn calculate_wrapped_height(text: &str, width: u16) -> usize {
    if width == 0 {
        return text.lines().count().max(1);
    }
    Paragraph::new(text).line_count(width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_detection() {
        assert!(contains_url("https://example.com"));
        assert!(contains_url("example.com/path"));
        assert!(contains_url("localhost:8080"));
        assert!(contains_url("192.168.1.1:8080"));
        assert!(!contains_url("not a url"));
    }

    #[test]
    fn test_url_only_preserved() {
        let line = Line::from(Span::raw("https://example.com"));
        let wrapped = wrap_line_preserving_urls(line, 80);
        assert_eq!(wrapped.len(), 1);
        assert!(
            wrapped[0]
                .spans
                .iter()
                .any(|s| s.content.contains("https://"))
        );
    }

    #[test]
    fn test_mixed_content() {
        let line = Line::from(Span::raw("See https://example.com for info"));
        let wrapped = wrap_line_preserving_urls(line, 25);
        let all_text: String = wrapped
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(all_text.contains("https://example.com"));
        assert!(all_text.contains("See"));
    }

    #[test]
    fn test_no_url_delegates() {
        let line = Line::from(Span::raw("Regular text without URLs"));
        let wrapped = wrap_line_preserving_urls(line, 10);
        assert!(!wrapped.is_empty());
    }
}
