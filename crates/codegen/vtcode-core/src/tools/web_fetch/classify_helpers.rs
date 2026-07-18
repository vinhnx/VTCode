//! Shared error classification helpers used by `web_fetch`, `web_search`,
//! and `defuddle_fetch`.
//!
//! Each tool produces a different response shape, so the helpers here
//! focus on the lower-level primitives: pulling an HTTP status code out
//! of a reqwest error string, and mapping a status / keyword to a
//! category. The per-tool `next_action` text lives next to the tool's
//! own response builder.

use regex::Regex;
use std::sync::OnceLock;

/// Pull an HTTP status code out of a reqwest error string.
///
/// reqwest error messages take a few shapes:
///
/// - `error sending request for url (https://example.com/): ...`
/// - `HTTP status server error (503 Service Unavailable) for url (...)`
/// - `... status: 403 Forbidden ...` (custom errors that include a status)
/// - `request timed out after 30s` (no status; caller should treat as
///   network error)
///
/// A regex that matches either the `HTTP ... <3-digit>` shape (the
/// reqwest server-error format) or the `status: <3-digit>` shape
/// (custom errors that surface the status code) is the most robust
/// shape we can support without depending on reqwest's internals.
pub fn extract_http_status(message: &str) -> Option<u16> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        // Two alternations:
        //   1. `\bhttp` (case-insensitive) followed by non-digits and a
        //      3-digit number — matches "HTTP status server error
        //      (503 ...", "http 500 ...", etc.
        //   2. `status:` (case-insensitive) followed by a 3-digit
        //      number — matches "error: status: 403 Forbidden", custom
        //      error chains, and so on.
        // The non-digit gap on the first branch is bounded at 64 chars
        // to keep the match cheap while still covering the longest
        // realistic reqwest format.
        Regex::new(r"(?i)\bhttp[^0-9]{0,64}(\d{3})\b|\bstatus:\s*(\d{3})\b").expect("valid status regex")
    });
    let caps = re.captures(message)?;
    // The matched number is in either capture group 1 or 2 depending
    // on which alternation fired.
    caps.get(1).or_else(|| caps.get(2)).and_then(|m| m.as_str().parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_status_handles_common_shapes() {
        assert_eq!(
            extract_http_status("HTTP status server error (503 Service Unavailable) for url (https://example.com/)"),
            Some(503)
        );
        assert_eq!(extract_http_status("error sending request: status: 403 Forbidden"), Some(403));
        // Bare `<num>` without an `http` / `status:` prefix is not a
        // status code; the parser should not match. (Real reqwest error
        // chains always include a sentinel word before the code.)
        assert_eq!(extract_http_status("server returned 404"), None);
        assert_eq!(extract_http_status("request timed out after 30s"), None);
        assert_eq!(extract_http_status("no error here"), None);
    }

    #[test]
    fn extract_status_ignores_non_three_digit_numbers() {
        // The regex requires exactly 3 digits. Larger numbers that happen
        // to appear in error text (e.g. a timeout duration, a chunk
        // size) must not be misread.
        assert_eq!(extract_http_status("timed out after 30000ms"), None);
        assert_eq!(extract_http_status("response body 1234 bytes"), None);
    }

    #[test]
    fn extract_status_handles_status_with_spaces() {
        // `status:` followed by whitespace and a 3-digit code.
        assert_eq!(extract_http_status("error: status:    429 Too Many Requests"), Some(429));
    }
}
