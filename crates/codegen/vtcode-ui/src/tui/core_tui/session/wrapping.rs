//! URL and path-preserving text wrapping.
//!
//! Wraps text while keeping URLs and file paths as atomic units to preserve
//! terminal link detection and transcript file hit-testing.

use super::text_utils::trim_trailing_wrap_whitespace;
use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use regex::Regex;
use std::borrow::Cow;
use std::sync::LazyLock;

/// URL/file token detection pattern - matches common URL formats and path-like tokens.
static PRESERVED_TOKEN_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r#"[a-zA-Z][a-zA-Z0-9+.-]*://[^\s<>\[\]{}|^]+|[a-zA-Z0-9][-a-zA-Z0-9]*\.[a-zA-Z]{2,}(/[^\s<>\[\]{}|^]*)?|localhost:\d+|\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}(:\d+)?|`(?:file://|~/|/|\./|\.\./|[A-Za-z]:[\\/]|[A-Za-z0-9._-]+[\\/])[^`]+`|"(?:file://|~/|/|\./|\.\./|[A-Za-z]:[\\/]|[A-Za-z0-9._-]+[\\/])[^"]+"|'(?:file://|~/|/|\./|\.\./|[A-Za-z]:[\\/]|[A-Za-z0-9._-]+[\\/])[^']+'|(?:\./|\../|~/|/)[^\s<>\[\]{}|^]+|(?:[A-Za-z]:[\\/][^\s<>\[\]{}|^]+)|(?:[A-Za-z0-9._-]+[\\/][^\s<>\[\]{}|^]+)"#,
    )
    .expect("invalid URL/path token regex")
});

/// Check if text contains a preserved URL/path token.
fn contains_preserved_token(text: &str) -> bool {
    PRESERVED_TOKEN_PATTERN.is_match(text)
}

#[derive(Clone, Copy)]
struct SourceSpan {
    start: usize,
    end: usize,
    style: Style,
}

fn source_spans(line: &Line<'_>) -> Vec<SourceSpan> {
    let mut start = 0usize;
    let mut source_spans = Vec::with_capacity(line.spans.len());
    for span in &line.spans {
        let end = start.saturating_add(span.content.len());
        if start != end {
            source_spans.push(SourceSpan { start, end, style: span.style });
        }
        start = end;
    }
    source_spans
}

fn push_styled_span(spans: &mut Vec<Span<'static>>, text: &str, style: Style) {
    if text.is_empty() {
        return;
    }

    if let Some(last) = spans.last_mut().filter(|last| last.style == style) {
        last.content.to_mut().push_str(text);
    } else {
        spans.push(Span::styled(text.to_owned(), style));
    }
}

fn push_source_range(
    spans: &mut Vec<Span<'static>>,
    source_spans: &[SourceSpan],
    text: &str,
    start: usize,
    end: usize,
    fallback_style: Style,
) {
    let mut matched_source = false;
    let first_source = source_spans.partition_point(|source| source.end <= start);
    for source in source_spans.iter().skip(first_source) {
        if source.start >= end {
            break;
        }
        let fragment_start = start.max(source.start);
        let fragment_end = end.min(source.end);
        if fragment_start < fragment_end {
            push_styled_span(spans, &text[fragment_start..fragment_end], source.style);
            matched_source = true;
        }
    }

    if !matched_source {
        push_styled_span(spans, &text[start..end], fallback_style);
    }
}

/// Wrap a line, preserving URLs as atomic units.
///
/// - Lines without URLs: delegated to standard wrapping
/// - URL-only lines: returned unwrapped if they fit
/// - Mixed lines: URLs kept intact, surrounding text wrapped normally
pub(crate) fn wrap_line_preserving_urls(line: Line<'static>, max_width: usize) -> Vec<Line<'static>> {
    if max_width == 0 {
        return vec![Line::default()];
    }

    // Hotpath: the regex below is ~10% of TUI reflow — fast-reject plain
    // prose with the cheap link-candidate check first (no regex engine).
    // Consistent with link-projection gating: no candidate => no links to
    // preserve, so standard wrapping is safe.
    if !line
        .spans
        .iter()
        .any(|span| super::transcript_links::may_contain_link_candidate_text(span.content.as_ref()))
    {
        return super::text_utils::wrap_line(line, max_width);
    }

    // Fast path: check spans without joining text (hotpath: the join was a
    // per-row String alloc on every reflow).
    if !line.spans.iter().any(|span| contains_preserved_token(span.content.as_ref())) {
        return super::text_utils::wrap_line(line, max_width);
    }

    let text: Cow<'_, str> = match line.spans.as_slice() {
        [span] => span.content.clone(),
        _ => Cow::Owned(line.spans.iter().map(|s| s.content.as_ref()).collect()),
    };

    // Find all preserved tokens in the text
    let urls: Vec<_> = PRESERVED_TOKEN_PATTERN
        .find_iter(&text)
        .map(|m| (m.start(), m.end(), m.as_str()))
        .collect();

    // Single URL that fits - return unwrapped for terminal link detection
    if urls.len() == 1
        && urls[0].0 == 0
        && urls[0].1 == text.len()
        && super::text_utils::display_width(&text) <= max_width
    {
        return vec![line];
    }
    // URL too wide - fall through to wrap it

    // Mixed content - split around URLs and wrap each segment.
    // For bullet / tree list items (e.g. "• Ran", "  └ ") preserve hanging
    // indent via the standard wrapper, which already handles bullet/tree
    // continuation. This fixes long tool headers like
    // "• Ran cat docs/guides/agent-loop-contract.md '2> /dev/null' ..." that
    // contain file paths and would otherwise lose hanging indent in the
    // URL-aware path. We sacrifice URL atomicity for these structured lines;
    // the hanging indent is more valuable for readability.
    let stripped = crate::tui::core_tui::session::text_utils::strip_ansi_codes(&text);
    let is_structured_bullet_or_tree = stripped.trim_start().starts_with("• ")
        || stripped.trim_start().starts_with("  └ ")
        || stripped.trim_start().starts_with("  ├ ")
        || stripped.trim_start().starts_with("  │ ");
    if is_structured_bullet_or_tree {
        return super::text_utils::wrap_line(line, max_width);
    }
    wrap_mixed_content(line, &text, max_width, &urls)
}

/// Wrap text that contains URLs, keeping URLs intact.
fn wrap_mixed_content(
    line: Line<'static>,
    text: &str,
    max_width: usize,
    urls: &[(usize, usize, &str)],
) -> Vec<Line<'static>> {
    use unicode_segmentation::UnicodeSegmentation;

    let mut result = Vec::with_capacity(urls.len() + 1);
    let mut current_line: Vec<Span<'static>> = Vec::new();
    let mut current_width = 0usize;
    let mut text_pos = 0usize;
    let source_spans = source_spans(&line);
    let fallback_style = line.spans.first().map(|span| span.style).unwrap_or_default();
    // Blockquote (`│ `) and list hanging indents must survive wrapping so
    // wrapped quote continuations keep their bar instead of degrading to
    // bare spaces (or column 0 in this URL-aware path).
    let continuation_prefix = super::text_utils::hanging_prefix_for_text(text);
    let continuation_width = super::text_utils::display_width(&continuation_prefix);
    let use_continuation_prefix =
        !continuation_prefix.is_empty() && continuation_width > 0 && continuation_width < max_width;
    let continuation_style = if continuation_prefix.contains('│') {
        line.spans
            .iter()
            .find(|span| span.content.contains('│'))
            .map(|span| span.style)
            .unwrap_or(fallback_style)
    } else {
        fallback_style
    };

    let ensure_prefix = |current_line: &mut Vec<Span<'static>>, current_width: &mut usize, result: &[Line<'static>]| {
        if use_continuation_prefix && current_line.is_empty() && !result.is_empty() {
            push_styled_span(current_line, &continuation_prefix, continuation_style);
            *current_width = continuation_width;
        }
    };

    let line_start_width = |result: &[Line<'static>]| -> usize {
        if use_continuation_prefix && !result.is_empty() {
            continuation_width
        } else {
            0
        }
    };

    let flush_line = |spans: &mut Vec<Span<'static>>, result: &mut Vec<Line<'static>>| {
        if spans.is_empty() {
            result.push(Line::default());
        } else {
            trim_trailing_wrap_whitespace(spans);
            result.push(Line::from(std::mem::take(spans)));
        }
    };

    let push_wrapped_token = |token: &str,
                              token_start: usize,
                              current_line: &mut Vec<Span<'static>>,
                              current_width: &mut usize,
                              result: &mut Vec<Line<'static>>| {
        for (offset, grapheme) in UnicodeSegmentation::grapheme_indices(token, true) {
            let grapheme_width = super::text_utils::display_width(grapheme);
            let grapheme_start = token_start + offset;
            let grapheme_end = grapheme_start + grapheme.len();
            if grapheme_width == 0 {
                ensure_prefix(current_line, current_width, result);
                push_source_range(current_line, &source_spans, text, grapheme_start, grapheme_end, fallback_style);
                continue;
            }
            if *current_width + grapheme_width > max_width && *current_width > 0 {
                flush_line(current_line, result);
                *current_width = 0;
            }
            ensure_prefix(current_line, current_width, result);
            push_source_range(current_line, &source_spans, text, grapheme_start, grapheme_end, fallback_style);
            *current_width += grapheme_width;
        }
    };

    let push_wrapped_text = |segment: &str,
                             segment_start: usize,
                             current_line: &mut Vec<Span<'static>>,
                             current_width: &mut usize,
                             result: &mut Vec<Line<'static>>| {
        let mut piece_offset = 0usize;
        for piece in segment.split_inclusive('\n') {
            let mut piece_text = piece;
            let mut had_newline = false;
            if let Some(stripped) = piece_text.strip_suffix('\n') {
                piece_text = stripped;
                had_newline = true;
                if let Some(without_carriage) = piece_text.strip_suffix('\r') {
                    piece_text = without_carriage;
                }
            }

            let piece_start = segment_start + piece_offset;
            let mut token_offset = 0usize;
            for token in UnicodeSegmentation::split_word_bounds(piece_text) {
                if token.is_empty() {
                    continue;
                }

                let token_width = super::text_utils::display_width(token);
                let token_start = piece_start + token_offset;
                let token_end = token_start + token.len();
                token_offset += token.len();
                if token_width == 0 {
                    ensure_prefix(current_line, current_width, result);
                    push_source_range(current_line, &source_spans, text, token_start, token_end, fallback_style);
                    continue;
                }

                let token_is_whitespace = token.chars().all(char::is_whitespace);
                let line_start = line_start_width(result);
                let has_content = *current_width > line_start;

                if token_is_whitespace && !result.is_empty() && !has_content {
                    continue;
                }

                ensure_prefix(current_line, current_width, result);
                if *current_width + token_width <= max_width {
                    push_source_range(current_line, &source_spans, text, token_start, token_end, fallback_style);
                    *current_width += token_width;
                    continue;
                }

                if token_is_whitespace {
                    if has_content {
                        flush_line(current_line, result);
                        *current_width = 0;
                    }
                    continue;
                }

                if token_width <= max_width {
                    if has_content {
                        flush_line(current_line, result);
                        *current_width = 0;
                    }
                    ensure_prefix(current_line, current_width, result);
                    push_source_range(current_line, &source_spans, text, token_start, token_end, fallback_style);
                    *current_width += token_width;
                    continue;
                }

                push_wrapped_token(token, token_start, current_line, current_width, result);
            }

            if had_newline {
                flush_line(current_line, result);
                *current_width = 0;
            }
            piece_offset += piece.len();
        }
    };

    for (url_start, url_end, url_text) in urls {
        // Process text before this URL
        if *url_start > text_pos {
            push_wrapped_text(
                &text[text_pos..*url_start],
                text_pos,
                &mut current_line,
                &mut current_width,
                &mut result,
            );
        }

        // Add URL — keep atomic if it fits, otherwise break it across lines
        let url_width = super::text_utils::display_width(url_text);
        if url_width <= max_width {
            if current_width > 0 && current_width + url_width > max_width {
                flush_line(&mut current_line, &mut result);
                current_width = 0;
            }
            ensure_prefix(&mut current_line, &mut current_width, &result);
            push_source_range(&mut current_line, &source_spans, text, *url_start, *url_end, fallback_style);
            current_width += url_width;
        } else {
            // URL is wider than max_width — break it grapheme-by-grapheme
            if current_width > 0 {
                flush_line(&mut current_line, &mut result);
                current_width = 0;
            }
            push_wrapped_token(url_text, *url_start, &mut current_line, &mut current_width, &mut result);
        }

        text_pos = *url_end;
    }

    // Process remaining text after last URL
    if text_pos < text.len() {
        push_wrapped_text(&text[text_pos..], text_pos, &mut current_line, &mut current_width, &mut result);
    }

    flush_line(&mut current_line, &mut result);
    if result.is_empty() {
        result.push(Line::default());
    }
    result
}

/// Wrap multiple lines with URL preservation.
pub(crate) fn wrap_lines_preserving_urls(lines: Vec<Line<'static>>, max_width: usize) -> Vec<Line<'static>> {
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
    use ratatui::style::Color;

    #[test]
    fn test_url_detection() {
        assert!(contains_preserved_token("https://example.com"));
        assert!(contains_preserved_token("example.com/path"));
        assert!(contains_preserved_token("localhost:8080"));
        assert!(contains_preserved_token("192.168.1.1:8080"));
        assert!(!contains_preserved_token("not a url"));
    }

    #[test]
    fn test_file_path_detection() {
        assert!(contains_preserved_token("src/main.rs"));
        assert!(contains_preserved_token("./src/main.rs"));
        assert!(contains_preserved_token("/tmp/example.txt"));
        assert!(contains_preserved_token("\"./docs/My Notes.md\""));
        assert!(contains_preserved_token("`/Users/example/Library/Application Support/Code/User/settings.json`"));
    }

    #[test]
    fn test_url_only_preserved() {
        let line = Line::from(Span::raw("https://example.com"));
        let wrapped = wrap_line_preserving_urls(line, 80);
        assert_eq!(wrapped.len(), 1);
        assert!(wrapped[0].spans.iter().any(|s| s.content.contains("https://")));
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
    fn test_quoted_path_with_spaces_is_preserved() {
        let line = Line::from(Span::raw("Open \"./docs/My Notes.md\" for details"));
        let wrapped = wrap_line_preserving_urls(line, 18);
        let all_text: String = wrapped
            .iter()
            .flat_map(|line| line.spans.iter().map(|span| span.content.as_ref()))
            .collect();

        assert!(all_text.contains("\"./docs/My Notes.md\""));
    }

    #[test]
    fn test_long_url_breaks_across_lines() {
        let long_url = "https://auth.openai.com/oauth/authorize?response_type=code&client_id=app_EMoamEEZ73f0CkXaXp7hrann&redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback&scope=openid";
        let line = Line::from(Span::raw(long_url.to_string()));
        let wrapped = wrap_line_preserving_urls(line, 80);
        assert!(wrapped.len() > 1, "Long URL should wrap across multiple lines");
        let all_text: String = wrapped
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert_eq!(all_text, long_url, "All characters should be preserved");
    }

    #[test]
    fn test_no_url_delegates() {
        let line = Line::from(Span::raw("Regular text without URLs"));
        let wrapped = wrap_line_preserving_urls(line, 10);
        assert!(!wrapped.is_empty());
    }

    #[test]
    fn test_mixed_content_prefers_word_boundaries_around_urls() {
        let line = Line::from(Span::raw("alpha https://x.io beta gamma"));
        let wrapped = wrap_line_preserving_urls(line, 12);
        let rendered: Vec<String> = wrapped
            .iter()
            .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
            .collect();

        assert_eq!(
            rendered,
            vec![
                "alpha".to_string(),
                "https://x.io".to_string(),
                "beta gamma".to_string()
            ]
        );
    }

    #[test]
    fn mixed_url_wrapping_preserves_each_source_span_style() {
        let before_style = Style::default().fg(Color::Red);
        let url_scheme_style = Style::default().fg(Color::Blue);
        let url_path_style = Style::default().fg(Color::Green);
        let after_style = Style::default().fg(Color::Yellow);
        let line = Line::from(vec![
            Span::styled("open ", before_style),
            Span::styled("https://example.", url_scheme_style),
            Span::styled("com/path", url_path_style),
            Span::styled(" now", after_style),
        ]);

        let wrapped = wrap_line_preserving_urls(line, 80);
        assert_eq!(wrapped.len(), 1);
        assert_eq!(
            wrapped[0]
                .spans
                .iter()
                .map(|span| (span.content.as_ref(), span.style))
                .collect::<Vec<_>>(),
            vec![
                ("open ", before_style),
                ("https://example.", url_scheme_style),
                ("com/path", url_path_style),
                (" now", after_style),
            ]
        );
    }

    #[test]
    fn blockquote_mixed_content_keeps_bar_on_continuation() {
        let line = Line::from(Span::raw("│ alpha beta https://x.io gamma delta epsilon zeta eta theta"));
        let wrapped = wrap_line_preserving_urls(line, 20);
        assert!(wrapped.len() > 1, "narrow width must wrap, got {wrapped:?}");
        let rendered: Vec<String> = wrapped
            .iter()
            .map(|line| line.spans.iter().map(|span| span.content.as_ref()).collect())
            .collect();
        for (idx, text) in rendered.iter().enumerate() {
            assert!(
                text.starts_with("│ "),
                "blockquote continuation {idx} must keep bar, got {text:?} in {rendered:?}"
            );
        }
        let joined = rendered.join(" ");
        assert!(joined.contains("https://x.io"));
    }
}
