//! Structured error classification and error-response shaping for `web_fetch`.

use serde_json::{Value, json};

use super::classify_helpers::extract_http_status;

/// Build a structured error response for a failed `web_fetch` call.
///
/// The error category (`http_error`, `network_error`, `policy_blocked`,
/// `empty_content`, etc.) plus a `next_action` hint is much more useful to
/// the agent loop than a flat string. The hint is what unblocks the loop
/// in cases like turn_578, where the agent was getting real HTTP 4xx
/// responses but reported the hosts as "blocked" because the error format
/// only carried a free-form message.
pub(super) fn web_fetch_error_response(url: &str, max_bytes: usize, timeout_secs: u64, err: &anyhow::Error) -> Value {
    let message = err.to_string();
    let (category, http_status, next_action) = classify_web_fetch_error(&message);

    let mut payload = json!({
        "error": format!("web_fetch: failed to fetch URL '{}': {}", url, message),
        "url": url,
        "max_bytes": max_bytes,
        "timeout_secs": timeout_secs,
        "error_type": category,
        "next_action": next_action,
    });
    if let Some(status) = http_status {
        payload["http_status"] = json!(status);
    }
    payload
}

/// Map a `web_fetch` error string into a stable (category, status,
/// next_action) tuple. The agent loop reads `next_action` to decide whether
/// to retry, fall back to a different tool, or give up.
pub(super) fn classify_web_fetch_error(message: &str) -> (&'static str, Option<u16>, &'static str) {
    // The reqwest error string is "<METHOD> <URL> error: <chain>". We only
    // look at the error portion because the URL is already in the response.
    let lower = message.to_lowercase();

    // Check timeout / network-class failures BEFORE HTTP status: a slow
    // upstream that returns a 5xx AFTER the body stream stalls can
    // produce a string like "request timed out (last status: 503)".
    // Status-based hints are not the right next-action for that case —
    // the cause is timeout, not server outage.
    if lower.contains("timeout") || lower.contains("timed out") {
        return (
            "network_error",
            None,
            "The request timed out. The host may be slow or unreachable. Try web_search to look up the page title first, or retry with a larger timeout_secs.",
        );
    }
    if lower.contains("only accepts remote http") || lower.contains("local files") {
        return (
            "local_resource",
            None,
            "web_fetch is for remote HTTP(S) URLs only. Use read_file or unified_file to read a local workspace file; do not retry web_fetch with a file:// URL.",
        );
    }
    if lower.contains("dns") || lower.contains("name resolution") || lower.contains("connection refused") {
        return (
            "network_error",
            None,
            "The host could not be reached. Try web_search to look up the page title first; if the host is down, retry later.",
        );
    }
    if let Some(status) = extract_http_status(&lower) {
        return http_status_to_category(status);
    }
    if lower.contains("ssl") || lower.contains("certificate") || lower.contains("tls") {
        return (
            "tls_error",
            None,
            "TLS handshake failed. The host may have an invalid or self-signed certificate. Try web_search as a fallback.",
        );
    }
    if lower.contains("redirect") {
        return (
            "redirect_error",
            None,
            "Redirect chain failed validation. The host may redirect to a blocked domain or loop. Try web_search to find the canonical URL.",
        );
    }
    if lower.contains("unsupported content type") || lower.contains("not supported") {
        return (
            "content_type_error",
            None,
            "The server returned a non-text content type (e.g., image or PDF). The web_fetch tool only handles text/HTML/JSON. Try web_search instead, or use a different tool to read the resource.",
        );
    }
    if lower.contains("blocked") || lower.contains("sensitive") {
        return (
            "policy_blocked",
            None,
            "The URL is on the blocklist for this tool. Use web_search to find an alternative source, or ask the user to whitelist the host via [web_fetch] allowed_domains in vtcode.toml.",
        );
    }
    (
        "unknown_error",
        None,
        "An unexpected error occurred. The error message above is the upstream cause; if it repeats, surface it to the user rather than retrying.",
    )
}

/// Map an HTTP status code into a stable (category, status, next_action)
/// tuple with a per-class recovery hint for the agent loop.
pub(super) fn http_status_to_category(status: u16) -> (&'static str, Option<u16>, &'static str) {
    match status {
        401 | 407 => (
            "http_error",
            Some(status),
            "The host requires authentication that web_fetch cannot provide. Try web_search for a cached version, or ask the user for credentials.",
        ),
        403 => (
            "http_error",
            Some(status),
            "The host explicitly blocked this request (often anti-bot). Try web_search, or wait a few seconds and retry with a normal browser User-Agent.",
        ),
        404 => (
            "http_error",
            Some(status),
            "The host returned 404: the page or resource does not exist. Verify the URL, or try web_search to find the correct one.",
        ),
        410 => (
            "http_error",
            Some(status),
            "The host returned 410: the resource is permanently gone. Try web_search for an alternative.",
        ),
        429 => (
            "http_error",
            Some(status),
            "The host rate-limited this client. Wait a few seconds and retry, or use web_search instead.",
        ),
        500..=599 => (
            "http_error",
            Some(status),
            "The host returned a server error. Retry after a short delay, or use web_search as a fallback.",
        ),
        _ => (
            "http_error",
            Some(status),
            "The host returned an unexpected status. The error message above is the upstream response; treat it as terminal unless the user asks for a retry.",
        ),
    }
}
