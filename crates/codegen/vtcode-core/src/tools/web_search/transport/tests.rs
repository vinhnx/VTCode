use super::*;

#[test]
fn capped_body_text_passes_small_bodies_through() {
    assert_eq!(capped_body_text(b"<html>hi</html>"), "<html>hi</html>");
    assert_eq!(capped_body_text(b""), "");
}

#[test]
fn capped_body_text_truncates_huge_bodies_without_panicking_on_split_chars() {
    // 999_999 ASCII bytes + one 2-byte `é`: the 1MB cap lands mid-char.
    // The ASCII prefix survives; the split tail becomes U+FFFD.
    let mut bytes = vec![b'a'; 999_999];
    bytes.extend_from_slice("é".as_bytes());
    let text = capped_body_text(&bytes);
    assert!(text[..999_999].bytes().all(|b| b == b'a'));

    let huge = vec![b'a'; MAX_DDG_BODY_BYTES + 5];
    assert_eq!(capped_body_text(&huge).len(), MAX_DDG_BODY_BYTES);
}

#[test]
fn capped_body_text_is_lossy_not_panicky_on_invalid_utf8() {
    let text = capped_body_text(b"\xff\xfe<html></html>");
    assert!(text.contains("<html>"), "got: {text}");
}

#[test]
fn timeout_is_clamped_to_a_usable_range() {
    assert_eq!(effective_timeout_secs(0), MIN_TIMEOUT_SECS);
    assert_eq!(effective_timeout_secs(15), 15);
    assert_eq!(effective_timeout_secs(MAX_TIMEOUT_SECS + 1), MAX_TIMEOUT_SECS);
}

#[test]
fn redirects_stay_on_the_https_ddg_html_host() {
    assert!(is_allowed_ddg_redirect(&Url::parse("https://html.duckduckgo.com/html/").unwrap()));
    assert!(!is_allowed_ddg_redirect(&Url::parse("http://html.duckduckgo.com/html/").unwrap()));
    assert!(!is_allowed_ddg_redirect(&Url::parse("https://duckduckgo.com/html/").unwrap()));
    assert!(!is_allowed_ddg_redirect(&Url::parse("https://127.0.0.1/html/").unwrap()));
}
