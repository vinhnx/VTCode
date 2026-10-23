use super::*;
use crate::tools::web_fetch::classify_helpers::decode_html_entities;

/// A small fixture that matches the markup the live DuckDuckGo HTML
/// endpoint emits. We do not assert on whitespace; we just need a stable
/// shape so the parser logic is exercised end-to-end without network.
const DDG_FIXTURE: &str = r#"
    <a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fgithub.com%2Fvinhnx&rut=abc">vinhnx (vinhnguyenxuan) · GitHub</a>
    <a class="result__snippet" href="https://github.com/vinhnx">Personal GitHub profile of Vinh Nguyen Xuan.</a>
    <a class="result__a" href="https://example.com/page">Example Page</a>
    <a class="result__snippet" href="https://example.com/page">An example page used in tests.</a>
    <a class="result__a" href="javascript:alert(1)">Should be skipped</a>
"#;

#[test]
fn normalize_ddg_url_extracts_uddg_target() {
    let href = "//duckduckgo.com/l/?uddg=https%3A%2F%2Fgithub.com%2Fvinhnx&rut=abc";
    assert_eq!(normalize_ddg_url(href).as_deref(), Some("https://github.com/vinhnx"));
}

#[test]
fn normalize_ddg_url_passes_through_direct_https() {
    let href = "https://example.com/page";
    assert_eq!(normalize_ddg_url(href).as_deref(), Some("https://example.com/page"));
}

#[test]
fn validate_result_url_rejects_non_http_schemes() {
    assert!(validate_result_url("javascript:alert(1)").is_none());
    assert!(validate_result_url("data:text/html,hi").is_none());
    assert!(validate_result_url("file:///etc/passwd").is_none());
    assert!(validate_result_url("https://user:secret@example.com/").is_none());
    assert!(validate_result_url("https://example.com").is_some());
}

#[test]
fn parse_duckduckgo_html_extracts_results_from_fixture() {
    let results = parse_duckduckgo_html(DDG_FIXTURE, 10);
    assert_eq!(results.len(), 2);

    assert_eq!(results[0].title, "vinhnx (vinhnguyenxuan) · GitHub");
    assert_eq!(results[0].url, "https://github.com/vinhnx");
    assert_eq!(results[0].snippet, "Personal GitHub profile of Vinh Nguyen Xuan.");

    assert_eq!(results[1].title, "Example Page");
    assert_eq!(results[1].url, "https://example.com/page");
    assert_eq!(results[1].snippet, "An example page used in tests.");
}

#[test]
fn parse_duckduckgo_html_respects_max_results() {
    let results = parse_duckduckgo_html(DDG_FIXTURE, 1);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].url, "https://github.com/vinhnx");
}

#[test]
fn parse_duckduckgo_html_returns_empty_for_challenge_page() {
    // A real DuckDuckGo anti-bot challenge returns the homepage shell with
    // no `result__a` anchors. The parser should return an empty vec, not
    // panic or fabricate results; the response wrapper turns the marker
    // into a structured blocked-search error.
    let challenge = r#"<html><body>Anomaly detected.</body></html>"#;
    assert!(parse_duckduckgo_html(challenge, 10).is_empty());
}

#[test]
fn parse_accepts_href_before_class_and_single_quotes() {
    // Asymmetric attribute order + quote style: the old regex required
    // `class="result__a"` before `href="..."` with double quotes.
    let body = r#"
        <a href='https://example.com/b-first' class='result__a'>B first</a>
        <a class="result__snippet">Snippet B.</a>
        <a href="https://example.com/a-second" class="result__a extra">A second</a>
        <a class="result__snippet">Snippet A.</a>
    "#;
    let results = parse_duckduckgo_html(body, 10);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].url, "https://example.com/b-first");
    assert_eq!(results[0].snippet, "Snippet B.");
    assert_eq!(results[1].url, "https://example.com/a-second");
    assert_eq!(results[1].snippet, "Snippet A.");
}

#[test]
fn parse_accepts_uppercase_tags_and_classes() {
    let body = r#"
        <A CLASS="RESULT__A" HREF="https://example.com/upper">Upper Title</A>
        <A CLASS="RESULT__SNIPPET">Upper snippet.</A>
    "#;
    let results = parse_duckduckgo_html(body, 10);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Upper Title");
    assert_eq!(results[0].snippet, "Upper snippet.");
}

#[test]
fn parse_missing_first_snippet_does_not_shift_second() {
    // Regression for index-aligned snippet lookup: a missing snippet for
    // the first result must yield empty, not the second result's snippet.
    let body = r#"
        <a class="result__a" href="https://example.com/no-snippet">No Snippet</a>
        <a class="result__a" href="https://example.com/with-snippet">With Snippet</a>
        <a class="result__snippet">Second snippet.</a>
    "#;
    let results = parse_duckduckgo_html(body, 10);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].snippet, "");
    assert_eq!(results[1].snippet, "Second snippet.");
}

#[test]
fn parse_skipped_anchors_do_not_steal_snippets() {
    // Asymmetric: [bad, good] vs [good, bad]. A skipped `javascript:`
    // anchor must not consume or shift the good result's snippet.
    let bad_first = r#"
        <a class="result__a" href="javascript:alert(1)">Bad</a>
        <a class="result__snippet">Bad snippet.</a>
        <a class="result__a" href="https://example.com/good">Good</a>
        <a class="result__snippet">Good snippet.</a>
    "#;
    let results = parse_duckduckgo_html(bad_first, 10);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].url, "https://example.com/good");
    assert_eq!(results[0].snippet, "Good snippet.");

    let good_first = r#"
        <a class="result__a" href="https://example.com/good">Good</a>
        <a class="result__snippet">Good snippet.</a>
        <a class="result__a" href="javascript:alert(1)">Bad</a>
        <a class="result__snippet">Bad snippet.</a>
    "#;
    let results = parse_duckduckgo_html(good_first, 10);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].snippet, "Good snippet.");
}

#[test]
fn parse_ignores_anchors_inside_script() {
    // Tokenizer advantage over regex: `script` content is raw text, so
    // a fake anchor inside JavaScript never becomes a result.
    let body = concat!(
        "<script>var x = '<a class=\"result__a\" href=\"https://example.com/evil\">Evil</a>';</script>",
        "<a class=\"result__a\" href=\"https://example.com/good\">Good</a>",
        "<a class=\"result__snippet\">Good snippet.</a>",
    );
    let results = parse_duckduckgo_html(body, 10);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].url, "https://example.com/good");
    assert_eq!(results[0].snippet, "Good snippet.");
}

#[test]
fn parse_collects_nested_formatting_text_in_title() {
    let body = concat!(
        "<a class=\"result__a\" href=\"https://example.com/fmt\">Hello <b>Bold</b> tail</a>",
        "<a class=\"result__snippet\">S.</a>",
    );
    let results = parse_duckduckgo_html(body, 10);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].title, "Hello Bold tail");
}

#[test]
fn decode_numeric_hex_and_named_entities() {
    assert_eq!(decode_html_entities("&#65;&#x42;&#X43;"), "ABC");
    assert_eq!(decode_html_entities("&rsquo;&ldquo;&mdash;"), "’“—");
    assert_eq!(decode_html_entities("&amp;lt;"), "&lt;");
    assert_eq!(decode_html_entities("&#9999999;"), "&#9999999;");
}

#[test]
fn parse_accepts_unquoted_href_and_td_closed_snippet() {
    let body = r#"
        <a class=result__a href=https://example.com/unquoted>Unquoted</a>
        <td class="result__snippet">TD snippet.</td>
        <abbr title="abbreviation">abbr</abbr>
        <a class="result__a" href="https://example.com/second">Second</a>
        <a class="result__snippet">Second snippet.</a>
    "#;
    let results = parse_duckduckgo_html(body, 10);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].url, "https://example.com/unquoted");
    assert_eq!(results[0].snippet, "TD snippet.");
    assert_eq!(results[1].url, "https://example.com/second");
    assert_eq!(results[1].snippet, "Second snippet.");
}
