//! Bounded HTML-to-readable-text extraction for `web_fetch` previews.
//!
//! Fetched HTML pages used to preview as raw markup (scripts, styles, and
//! navigation chrome included), spending model context on non-content. This
//! module extracts the readable text instead: non-readable containers and
//! comments are stripped, remaining tags are removed, entities are decoded,
//! and whitespace is collapsed. The raw body is untouched — it still lands
//! in `temp_file` for full reads.
//!
//! Deliberately dependency-free (regex only): preview extraction does not
//! need full document parsing. Entity decoding and whitespace collapsing
//! reuse the shared helpers rather than re-implementing them.

use regex::Regex;
use std::sync::LazyLock;
use vtcode_commons::formatting::collapse_whitespace;

use super::classify_helpers::decode_html_entities;

static STRIP_UNREADABLE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"(?is)<script[^>]*>.*?</script>|<style[^>]*>.*?</style>|<noscript[^>]*>.*?</noscript>|<template[^>]*>.*?</template>|<svg[^>]*>.*?</svg>",
    )
    .expect("valid unreadable-block regex")
});
static HTML_COMMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)<!--.*?-->").expect("valid html comment regex"));
static HTML_HEAD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<head[^>]*>.*?</head>").expect("valid html head regex"));
static HTML_TITLE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?is)<title[^>]*>(?P<title>.*?)</title>").expect("valid html title regex"));
static HTML_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").expect("valid html tag regex"));

/// Whether `content` looks like an HTML document: first non-whitespace
/// (BOM-tolerant) byte is `<` and a tag close exists. Full pages from
/// `web_fetch` start with `<!doctype` or `<html>`; JSON, Markdown, and
/// plain text take the raw preview path instead.
pub(super) fn looks_like_html(content: &str) -> bool {
    let trimmed = content.trim_start().trim_start_matches('\u{FEFF}');
    trimmed.as_bytes().first() == Some(&b'<') && trimmed.contains('>')
}

/// Extract readable text from an HTML document: the `<title>` first (when
/// present), then the body with scripts, styles, comments, and head
/// metadata removed. Returns empty when nothing readable remains.
pub(super) fn extract_text_preview(html: &str) -> String {
    let title = HTML_TITLE
        .captures(html)
        .and_then(|caps| caps.name("title"))
        .map(|found| clean_fragment(found.as_str()))
        .unwrap_or_default();
    let without_blocks = STRIP_UNREADABLE.replace_all(html, " ");
    let without_comments = HTML_COMMENT.replace_all(&without_blocks, " ");
    let without_head = HTML_HEAD.replace_all(&without_comments, " ");
    let body = clean_fragment(&without_head);
    if title.is_empty() {
        body
    } else if body.is_empty() {
        title
    } else {
        format!("{title}\n\n{body}")
    }
}

/// Strip tags, decode entities, and collapse whitespace in one fragment.
fn clean_fragment(fragment: &str) -> String {
    let without_tags = HTML_TAG.replace_all(fragment, " ");
    collapse_whitespace(&decode_html_entities(without_tags.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_unreadable_blocks_and_comments_but_keeps_title_and_body() {
        let html = "<!doctype html><html><head><title>Example &amp; Co</title>\
            <style>.x{color:red}</style><script>alert(1)</script></head>\
            <body><!-- nav --><nav><a href=\"/\">Home</a></nav>\
            <main><h1>Hello &#65;</h1><p>World</p></main>\
            <svg><circle cx=\"1\"/></svg></body></html>";
        let text = extract_text_preview(html);
        assert!(text.starts_with("Example & Co"), "title first; got: {text}");
        assert!(text.contains("Hello A"), "entity decoded; got: {text}");
        assert!(text.contains("World"), "body kept; got: {text}");
        assert!(!text.contains("alert"), "script stripped; got: {text}");
        assert!(!text.contains("color:red"), "style stripped; got: {text}");
        assert!(!text.contains("nav"), "comment stripped; got: {text}");
        assert!(!text.contains('<'), "no tags remain; got: {text}");
    }

    #[test]
    fn style_after_content_does_not_eat_body() {
        // Asymmetric order: unreadable block after the content must not
        // swallow it (non-greedy same-tag close).
        let html = "<div><p>Keep me</p></div><style>.late{display:none}</style><p>And me</p>";
        let text = extract_text_preview(html);
        assert!(text.contains("Keep me"), "got: {text}");
        assert!(text.contains("And me"), "got: {text}");
        assert!(!text.contains("display"), "got: {text}");
    }

    #[test]
    fn title_only_page_returns_title() {
        let html = "<html><head><title>Lonely</title></head><body><script>var x = 1;</script></body></html>";
        assert_eq!(extract_text_preview(html), "Lonely");
    }

    #[test]
    fn looks_like_html_gates_on_leading_angle_bracket() {
        assert!(looks_like_html("<!doctype html><html></html>"));
        assert!(looks_like_html("  \n\t<html><body>hi</body></html>"));
        assert!(looks_like_html("\u{FEFF}<html></html>"));
        assert!(!looks_like_html("{\"key\": \"value > 1\"}"));
        assert!(!looks_like_html("# Markdown docs"));
        assert!(!looks_like_html(""));
        assert!(!looks_like_html("   "));
    }
}
