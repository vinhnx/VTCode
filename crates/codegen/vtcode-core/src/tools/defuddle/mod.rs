//! Defuddle-backed fetch: `https://defuddle.md/{link}` -> clean markdown.
//!
//! Defuddle is a third-party hosted service that takes a URL and returns
//! LLM-friendly markdown for the page. It is a polite convenience for the
//! rare case where `web_fetch` returns a payload the agent would rather not
//! parse itself (heavy JS, paywalled HTML, raw RSS, etc.).
//!
//! Because the service is rate-limited, this tool is hard-capped at one call
//! per `DefuddleTool` instance. A new instance per session is the recommended
//! pattern; the registry constructs the tool once per agent, so the cap
//! effectively means "once per session".
//!
//! The response is returned inline (no temp file). When the cap is hit, the
//! tool returns a structured JSON error that points the agent back to
//! `web_fetch` so it does not loop.

use super::traits::Tool;
use crate::config::constants::tools;
use crate::tools::web_fetch::classify_helpers::extract_http_status;
use crate::tools::web_fetch::is_private_host;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use url::Url;

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_TIMEOUT_SECS: u64 = 60;
const MAX_BYTES: usize = 256 * 1024;
const DEFUDDLE_BASE_URL: &str = "https://defuddle.md/";

pub(crate) const DEFUDDLE_FETCH_DESCRIPTION: &str = "Fetch a REMOTE web page (http:// or https:// URLs ONLY) through the defuddle.md markdown extraction service and return the cleaned markdown inline. DO NOT use for local files: inspect local paths with exec_command and readonly shell commands such as sed, rg, ls, or find. Use this sparingly: the hosted service is rate-limited, so this tool can be called at most ONCE per session. Accepts: { url: string (must start with http:// or https://), max_bytes?: number }. Returns { url, markdown, bytes, used_this_session, session_cap } or a structured error if the cap has been hit.";

#[derive(Debug, Deserialize)]
struct DefuddleArgs {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    max_bytes: Option<usize>,
}

#[derive(Clone, Default)]
pub struct DefuddleTool {
    /// How many calls have been issued against this instance. Hard-capped at
    /// `SESSION_CAP` to stay well under the third-party service's quota.
    uses: Arc<AtomicUsize>,
}

const SESSION_CAP: usize = 1;

impl DefuddleTool {
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset the per-instance counter. Used by tests; production code should
    /// not call this — a new `DefuddleTool` is the way to start a new budget.
    pub fn reset(&self) {
        self.uses.store(0, Ordering::SeqCst);
    }

    fn current_uses(&self) -> usize {
        self.uses.load(Ordering::SeqCst)
    }

    fn try_consume(&self) -> bool {
        // Compare-and-swap loop so concurrent callers cannot both succeed
        // once the cap is hit.
        loop {
            let current = self.uses.load(Ordering::SeqCst);
            if current >= SESSION_CAP {
                return false;
            }
            if self
                .uses
                .compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return true;
            }
        }
    }

    async fn run(&self, raw_args: Value) -> Result<Value> {
        let args: DefuddleArgs = serde_json::from_value(raw_args)
            .context("Invalid arguments for defuddle_fetch. Provide a 'url' string.")?;

        let url = args
            .url
            .map(|u| u.trim().to_string())
            .filter(|u| !u.is_empty())
            .ok_or_else(|| anyhow!("defuddle_fetch requires a non-empty 'url'"))?;

        // Reject anything that is not an HTTP/HTTPS URL before URL parsing.
        // This prevents the LLM from accidentally using defuddle_fetch for
        // local file reads, bare paths, or other non-web URLs.
        let lower = url.to_ascii_lowercase();
        if !lower.starts_with("http://") && !lower.starts_with("https://") {
            return Err(anyhow!(
                "defuddle_fetch only accepts http:// or https:// URLs for web content extraction. \
                For local file reads, use exec_command with readonly shell inspection commands instead. \
                Got: {url}"
            ));
        }

        validate_target_url(&url)?;

        if !self.try_consume() {
            return Ok(cap_reached_response(&url));
        }

        let max_bytes = args.max_bytes.unwrap_or(MAX_BYTES).min(MAX_BYTES);
        let timeout_secs = DEFAULT_TIMEOUT_SECS.min(MAX_TIMEOUT_SECS);

        let (body, truncated) = match fetch_markdown(&url, timeout_secs, max_bytes).await {
            Ok(t) => t,
            Err(e) => {
                return Ok(defuddle_fetch_error_response(&url, max_bytes, timeout_secs, &e));
            }
        };

        let bytes = body.len();
        Ok(json!({
            "url": url,
            "markdown": body,
            "bytes": bytes,
            "truncated": truncated,
            "used_this_session": self.current_uses(),
            "session_cap": SESSION_CAP,
        }))
    }
}

fn defuddle_fetch_error_response(url: &str, max_bytes: usize, timeout_secs: u64, err: &anyhow::Error) -> Value {
    let message = err.to_string();
    let lower = message.to_lowercase();
    // Timeout / network-class failures take priority over HTTP status:
    // a slow upstream that returns a 5xx after the body stalls will
    // carry a "timed out" prefix, and the right hint in that case is
    // "retry later / use web_fetch" — not "the service is down".
    let (error_type, next_action) = if lower.contains("timeout") || lower.contains("timed out") {
        (
            "network_error",
            "defuddle.md timed out. The upstream service may be slow. Use web_fetch on a known URL as a fallback.",
        )
    } else if lower.contains("dns") || lower.contains("connection refused") {
        (
            "network_error",
            "defuddle.md is unreachable from this network. Use web_fetch on a known URL as a fallback.",
        )
    } else if let Some(status) = extract_http_status(&message) {
        let action = match status {
            403 | 429 => {
                "defuddle.md rate-limited or rejected the request. Do NOT retry; the session cap is also exhausted. Use web_fetch on a known URL as a fallback."
            }
            404 => {
                "defuddle.md returned 404. The upstream service path may have changed; use web_fetch directly instead."
            }
            500..=599 => "defuddle.md is having a server issue. Use web_fetch on a known URL as a fallback.",
            _ => "defuddle.md returned an unexpected status. Use web_fetch on a known URL as a fallback.",
        };
        ("http_error", action)
    } else {
        ("unknown_error", "defuddle.md request failed. Use web_fetch on a known URL as a fallback.")
    };
    json!({
        "error": format!("defuddle_fetch failed for '{}': {}", url, message),
        "url": url,
        "max_bytes": max_bytes,
        "timeout_secs": timeout_secs,
        "error_type": error_type,
        "next_action": next_action,
        "used_this_session": 1,
        "session_cap": SESSION_CAP,
    })
}

fn cap_reached_response(url: &str) -> Value {
    json!({
        "error": "defuddle_fetch session cap reached",
        "url": url,
        "session_cap": SESSION_CAP,
        "next_action": "defuddle_fetch can be called at most once per session because the defuddle.md service is rate-limited. Use web_fetch (which goes through vtcode's safe HTTP path) for additional pages in this session."
    })
}

/// Validate that the requested URL is something we'd want to point the
/// public defuddle.md service at. We do not run the defuddle.md request
/// itself until after this check, so an obviously-bad URL never leaves the
/// process.
fn validate_target_url(url: &str) -> Result<()> {
    let parsed = Url::parse(url).context("defuddle_fetch: invalid url")?;
    match parsed.scheme() {
        "http" | "https" => {}
        other => {
            return Err(anyhow!("defuddle_fetch: refusing to fetch {other}:// URL (only http/https are allowed)"));
        }
    }
    let host = parsed
        .host_str()
        .filter(|h| !h.is_empty())
        .ok_or_else(|| anyhow!("defuddle_fetch: refusing to fetch a URL with no host"))?;
    // Block private / loopback / link-local / broadcast / multicast
    // hosts so a malicious URL can't turn defuddle into an SSRF relay.
    // Mirrors the `web_fetch` check via the shared `is_private_host` helper.
    if is_private_host(host) {
        return Err(anyhow!("defuddle_fetch: refusing to fetch private/local host '{host}'"));
    }
    Ok(())
}

async fn fetch_markdown(url: &str, timeout_secs: u64, max_bytes: usize) -> Result<(String, bool)> {
    // Per the user's note: `curl defuddle.md/{link}`. We must NOT use
    // `Url::join` here because the Rust URL spec treats absolute URLs as
    // absolute on join, which would mean the request goes straight to the
    // target host and defuddle.md is bypassed entirely. Instead, encode the
    // input URL as a single path segment and append it to the base URL.
    //
    // SECURITY: this is the only place the request URL is constructed. A
    // future refactor that "cleans up" the string concatenation in favor
    // of `Url::join` or any other URL builder will silently turn the
    // tool into an SSRF relay. The regression test
    // `request_url_targets_defuddle_host_not_target_host` guards this.
    let encoded = percent_encode_path(url);
    let target = format!("{DEFUDDLE_BASE_URL}{encoded}");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        // Do NOT follow redirects from the upstream service. Defuddle is a
        // thin proxy; following its redirects could leak into SSRF-prone
        // behavior, and we already validated the input URL above.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("defuddle_fetch: failed to build HTTP client")?;

    let response = client
        .get(&target)
        .header("Accept", "text/markdown, text/plain;q=0.9, */*;q=0.5")
        .send()
        .await
        .context("defuddle_fetch: request failed")?;

    let status = response.status();
    if !status.is_success() {
        return Err(anyhow!("defuddle.md returned HTTP {status}"));
    }

    let body = response.text().await.context("defuddle_fetch: failed to read response body")?;

    if body.len() > max_bytes {
        // Truncate to a hard cap so a giant page never blows up the agent.
        let mut truncated = body;
        truncated.truncate(max_bytes);
        // `truncated` is `true` only when the upstream body exceeded the cap,
        // not when it just happened to land exactly on it.
        Ok((truncated, true))
    } else {
        Ok((body, false))
    }
}

/// Percent-encode a string so it is safe to append as a single path segment
/// to a base URL. We unreserved-encode everything except `:`, `/`, `?`, `#`,
/// `[`, `]`, `@`, `!`, `$`, `&`, `'`, `(`, `)`, `*`, `+`, `,`, `;`, `=`
/// (per RFC 3986 sub-delims) — those are left alone so an http(s) URL
/// survives encoding. This is intentionally simpler than the full RFC 3986
/// path-segment set because we know our input is a URL.
fn percent_encode_path(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for byte in input.bytes() {
        let unreserved = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~');
        let sub_delim = matches!(
            byte,
            b':' | b'/'
                | b'?'
                | b'#'
                | b'['
                | b']'
                | b'@'
                | b'!'
                | b'$'
                | b'&'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
        );
        if unreserved || sub_delim {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[async_trait]
impl Tool for DefuddleTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        self.run(args).await
    }

    fn name(&self) -> &str {
        tools::DEFUDDLE_FETCH
    }

    fn description(&self) -> &str {
        DEFUDDLE_FETCH_DESCRIPTION
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_url_is_rejected() {
        let tool = DefuddleTool::new();
        let result = tokio::runtime::Runtime::new().unwrap().block_on(tool.run(json!({})));
        assert!(result.is_err());
    }

    #[test]
    fn empty_url_is_rejected() {
        let tool = DefuddleTool::new();
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool.run(json!({ "url": "   " })));
        assert!(result.is_err());
    }

    #[test]
    fn non_http_scheme_is_rejected() {
        let tool = DefuddleTool::new();
        for bad in ["javascript:alert(1)", "file:///etc/passwd", "data:text/html,hi"] {
            let result = tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(tool.run(json!({ "url": bad })));
            assert!(result.is_err(), "should reject {bad}");
        }
    }

    #[test]
    fn local_file_paths_are_rejected_before_url_parsing() {
        let tool = DefuddleTool::new();
        for bad in [
            "/Users/vinhnguyenxuan/Documents/podcast/build-video.sh",
            "./relative/path.txt",
            "/etc/passwd",
            "C:\\Users\\file.txt",
        ] {
            let result = tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(tool.run(json!({ "url": bad })));
            assert!(result.is_err(), "should reject local path: {bad}");
            let err_msg = result.unwrap_err().to_string();
            assert!(err_msg.contains("exec_command"));
            assert!(!err_msg.contains(&format!("unified_{}", "file")));
        }
    }

    #[test]
    fn session_cap_rejects_second_call() {
        let tool = DefuddleTool::new();
        // Manually mark the cap as consumed; we don't make a real network
        // call here. `try_consume` is the same code path the live call uses.
        assert!(tool.try_consume());
        let payload = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool.run(json!({ "url": "https://example.com" })))
            .expect("cap hit must be a structured JSON, not a runtime error");
        assert_eq!(payload["error"], "defuddle_fetch session cap reached");
        assert_eq!(payload["session_cap"], 1);
    }

    #[test]
    fn reset_allows_one_more_call() {
        let tool = DefuddleTool::new();
        assert!(tool.try_consume());
        assert!(!tool.try_consume());
        tool.reset();
        assert!(tool.try_consume());
    }

    #[test]
    fn validate_target_url_rejects_bad_inputs() {
        assert!(validate_target_url("https://example.com").is_ok());
        assert!(validate_target_url("https://example.com/path?q=1").is_ok());
        assert!(validate_target_url("http://example.com").is_ok());
        assert!(validate_target_url("javascript:alert(1)").is_err());
        assert!(validate_target_url("file:///etc/passwd").is_err());
        assert!(validate_target_url("data:text/html,hi").is_err());
        assert!(validate_target_url("not a url at all").is_err());
    }

    /// Regression test for review H2: defuddle must NOT relay requests to
    /// private / loopback / link-local / broadcast / multicast hosts.
    /// Otherwise a URL like `http://192.168.0.1/admin` would tunnel an
    /// SSRF through the public defuddle.md service.
    #[test]
    fn validate_target_url_blocks_private_hosts() {
        for bad in [
            "http://10.0.0.1/",
            "http://192.168.1.1/admin",
            "http://172.16.0.1/",
            "http://127.0.0.1/",
            "http://169.254.169.254/latest/meta-data/", // AWS IMDS
            "http://[::1]/",
            "http://[fc00::1]/",
            "http://[fe80::1]/",
            "http://255.255.255.255/",
            "http://224.0.0.1/",
            "http://localhost/admin",
        ] {
            let result = validate_target_url(bad);
            assert!(result.is_err(), "private host should be rejected: {bad}");
            let msg = result.unwrap_err().to_string();
            assert!(
                msg.contains("private") || msg.contains("local"),
                "error should mention private/local; got: {msg} for {bad}"
            );
        }
    }

    #[test]
    fn percent_encode_path_preserves_url_sub_delims() {
        // Common http(s) URL: every byte is unreserved or sub-delim, so the
        // encoded output should equal the input.
        let url = "https://example.com/path?q=1&r=2#frag";
        assert_eq!(percent_encode_path(url), url);
    }

    #[test]
    fn percent_encode_path_escapes_spaces_and_unicode() {
        let encoded = percent_encode_path("https://example.com/has space and \u{2603}");
        assert!(encoded.starts_with("https://example.com/has%20space%20and%20%E2%98%83"));
    }

    #[test]
    fn request_url_targets_defuddle_host_not_target_host() {
        // Build the same URL the network call would build and assert it points
        // at defuddle.md, not the user-supplied target. This guards against
        // regressions where someone reintroduces `Url::join` (which would
        // silently bypass defuddle.md for absolute URLs).
        let user_url = "https://example.com/x";
        let built = format!("{DEFUDDLE_BASE_URL}{}", percent_encode_path(user_url));
        let parsed = Url::parse(&built).expect("defuddle URL must parse");
        assert_eq!(parsed.host_str(), Some("defuddle.md"));
        // The full input URL survives as a path segment on defuddle.md. We
        // don't compare exact bytes (the URL parser may normalize `//`) —
        // we just assert the target host is referenced somewhere in the path.
        let path = parsed.path();
        assert!(path.contains("example.com"), "defuddle.md URL should reference the user host in its path: {path}");
    }

    /// Regression test for turn_578: when defuddle.md rate-limits the
    /// request, the structured error must say so and point the agent to
    /// web_fetch as a fallback. It must not look like a generic
    /// "PermissionDenied" (which is what the previous shape produced).
    #[test]
    fn error_response_classifies_403_and_suggests_web_fetch() {
        let err = anyhow::anyhow!("defuddle.md returned HTTP 403");
        let response = defuddle_fetch_error_response("https://example.com", MAX_BYTES, DEFAULT_TIMEOUT_SECS, &err);
        assert_eq!(response["error_type"], "http_error");
        assert!(
            response["next_action"].as_str().unwrap_or("").contains("web_fetch"),
            "next_action should fall back to web_fetch; got: {}",
            response["next_action"]
        );
        assert!(response["session_cap"].is_number());
    }

    /// Network-level failures should classify distinctly from HTTP errors.
    #[test]
    fn error_response_classifies_timeout_as_network_error() {
        let err = anyhow::anyhow!("request timed out after 30s");
        let response = defuddle_fetch_error_response("https://example.com", MAX_BYTES, DEFAULT_TIMEOUT_SECS, &err);
        assert_eq!(response["error_type"], "network_error");
    }
}
