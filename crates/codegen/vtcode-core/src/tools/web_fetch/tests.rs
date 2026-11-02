use super::*;
use serde_json::{Value, json};

async fn execute_json(tool: &WebFetchTool, args: Value) -> Value {
    tool.execute(args)
        .await
        .expect("web_fetch should return structured JSON output")
}

fn error_text(result: &Value) -> Option<&str> {
    result.get("error").and_then(Value::as_str)
}

#[tokio::test]
async fn rejects_non_https_urls() {
    let tool = WebFetchTool::new();
    let result = execute_json(
        &tool,
        json!({
            "url": "http://example.com",
            "prompt": "Extract the main content"
        }),
    )
    .await;
    let error = error_text(&result).unwrap_or("");
    assert!(error.contains("Only HTTPS URLs are allowed"));
}

#[tokio::test]
async fn rejects_local_file_urls_with_read_file_guidance() {
    let tool = WebFetchTool::new();
    let result = execute_json(
        &tool,
        json!({
            "url": "file:///tmp/checkpoint.json",
            "prompt": "Read the file"
        }),
    )
    .await;
    let error = error_text(&result).unwrap_or("");
    assert!(error.contains("remote HTTP(S) URLs"));
    assert_eq!(result["error_type"], "local_resource");
    assert!(result["next_action"].as_str().unwrap_or("").contains("read_file"));
}

#[tokio::test]
async fn rejects_local_file_urls_before_markdown_backend_routing() {
    let tool = WebFetchTool::new();
    let result = execute_json(
        &tool,
        json!({
            "url": "file:///tmp/checkpoint.json",
            "format": "markdown",
            "prompt": "Read the file"
        }),
    )
    .await;

    assert_eq!(result["error_type"], "local_resource");
    assert!(error_text(&result).unwrap_or("").contains("remote HTTP(S) URLs"));
    assert!(result["next_action"].as_str().unwrap_or("").contains("read_file"));
}

#[tokio::test]
async fn rejects_relative_urls_before_markdown_backend_routing() {
    let tool = WebFetchTool::new();
    let result = execute_json(
        &tool,
        json!({
            "url": "README.md",
            "format": "markdown",
            "prompt": "Read the file"
        }),
    )
    .await;

    assert_eq!(result["error_type"], "local_resource");
    assert!(error_text(&result).unwrap_or("").contains("remote HTTP(S) URLs"));
    assert!(result["next_action"].as_str().unwrap_or("").contains("unified_file"));
}

#[tokio::test]
async fn rejects_relative_urls_before_normal_fetch_routing() {
    let tool = WebFetchTool::new();
    let result = execute_json(
        &tool,
        json!({
            "url": "README.md",
            "prompt": "Read the file"
        }),
    )
    .await;

    assert_eq!(result["error_type"], "local_resource");
    assert!(error_text(&result).unwrap_or("").contains("remote HTTP(S) URLs"));
    assert!(result["next_action"].as_str().unwrap_or("").contains("unified_file"));
}

#[tokio::test]
async fn allows_http_when_https_disabled() {
    let tool = WebFetchTool::with_config(
        vtcode_config::WebFetchMode::Restricted,
        Vec::new(),
        Vec::new(),
        Vec::new(),
        false, // strict_https_only = false
    );
    let result = execute_json(
        &tool,
        json!({
            "url": "http://example.com",
            "prompt": "Extract the main content"
        }),
    )
    .await;
    if let Some(error) = error_text(&result) {
        assert!(!error.contains("Only HTTPS URLs are allowed"));
    }
}

#[tokio::test]
async fn rejects_localhost_urls() {
    let tool = WebFetchTool::new();
    let result = execute_json(
        &tool,
        json!({
            "url": "https://localhost:8080",
            "prompt": "Extract the main content"
        }),
    )
    .await;
    let error = error_text(&result).unwrap_or("");
    assert!(error.contains("local/private networks"));
}

#[tokio::test]
async fn requires_both_url_and_prompt() {
    let tool = WebFetchTool::new();
    let result = execute_json(
        &tool,
        json!({
            "url": "http://example.com"
        }),
    )
    .await;
    // Prompt should be auto-filled; URL then fails HTTPS policy.
    let error = error_text(&result).unwrap_or("");
    assert!(error.contains("Only HTTPS URLs are allowed"));
}

#[tokio::test]
async fn rejects_sensitive_banking_domains() {
    let tool = WebFetchTool::new();
    let result = execute_json(
        &tool,
        json!({
            "url": "https://paypal.com/login",
            "prompt": "Extract the main content"
        }),
    )
    .await;
    let error = error_text(&result).unwrap_or("");
    assert!(error.contains("blocked for privacy and security reasons"));
}

#[tokio::test]
async fn rejects_sensitive_auth_domains() {
    let tool = WebFetchTool::new();
    let result = execute_json(
        &tool,
        json!({
            "url": "https://accounts.google.com",
            "prompt": "Extract the main content"
        }),
    )
    .await;
    let error = error_text(&result).unwrap_or("");
    assert!(error.contains("blocked for privacy and security reasons"));
}

#[tokio::test]
async fn rejects_urls_with_credentials() {
    let tool = WebFetchTool::new();
    let result = execute_json(
        &tool,
        json!({
            "url": "https://example.com?password=secret123",
            "prompt": "Extract the main content"
        }),
    )
    .await;
    let error = error_text(&result).unwrap_or("");
    assert!(error.contains("sensitive pattern"));
}

#[tokio::test]
async fn rejects_urls_with_api_keys() {
    let tool = WebFetchTool::new();
    let result = execute_json(
        &tool,
        json!({
            "url": "https://api.example.com?api_key=sk_live_123456",
            "prompt": "Extract the main content"
        }),
    )
    .await;
    let error = error_text(&result).unwrap_or("");
    assert!(error.contains("sensitive pattern"));
}

#[tokio::test]
async fn rejects_urls_with_tokens() {
    let tool = WebFetchTool::new();
    let result = execute_json(
        &tool,
        json!({
            "url": "https://example.com?token=xyz123",
            "prompt": "Extract the main content"
        }),
    )
    .await;
    let error = error_text(&result).unwrap_or("");
    assert!(error.contains("sensitive pattern"));
}

#[tokio::test]
async fn rejects_malicious_url_patterns() {
    let tool = WebFetchTool::new();
    let result = execute_json(
        &tool,
        json!({
            "url": "https://example.com/malware.exe\"",
            "prompt": "Extract the main content"
        }),
    )
    .await;
    let error = error_text(&result).unwrap_or("");
    assert!(error.contains("potentially malicious pattern"));
}

#[tokio::test]
async fn rejects_typosquatting_domains() {
    let tool = WebFetchTool::new();
    let result = execute_json(
        &tool,
        json!({
            "url": "https://g00gle.com",
            "prompt": "Extract the main content"
        }),
    )
    .await;
    let error = error_text(&result).unwrap_or("");
    assert!(error.contains("potentially malicious pattern"));
}

#[tokio::test]
async fn rejects_url_shorteners() {
    let tool = WebFetchTool::new();
    let result = execute_json(
        &tool,
        json!({
            "url": "https://bit.ly/xyz123",
            "prompt": "Extract the main content"
        }),
    )
    .await;
    let error = error_text(&result).unwrap_or("");
    assert!(error.contains("potentially malicious pattern"));
}

#[tokio::test]
async fn whitelist_mode_requires_allowed_domains() {
    let tool = WebFetchTool::with_config(
        vtcode_config::WebFetchMode::Whitelist,
        Vec::new(),
        Vec::new(),
        Vec::new(), // No allowed domains
        true,
    );
    let result = execute_json(
        &tool,
        json!({
            "url": "https://example.com",
            "prompt": "Extract the main content"
        }),
    )
    .await;
    let error = error_text(&result).unwrap_or("").to_string();
    assert!(error.contains("whitelist") || error.contains("whitelisted"));
}

#[tokio::test]
async fn whitelist_mode_allows_whitelisted_domains() {
    let tool = WebFetchTool::with_config(
        vtcode_config::WebFetchMode::Whitelist,
        Vec::new(),
        Vec::new(),
        vec!["example.com".to_string()], // Only example.com allowed
        true,
    );
    let result = execute_json(
        &tool,
        json!({
            "url": "https://example.com/path",
            "prompt": "Extract the main content"
        }),
    )
    .await;
    if let Some(error) = error_text(&result) {
        assert!(!error.contains("not in the whitelist"));
    }
}

#[tokio::test]
async fn whitelist_mode_rejects_non_whitelisted_domains() {
    let tool = WebFetchTool::with_config(
        vtcode_config::WebFetchMode::Whitelist,
        Vec::new(),
        Vec::new(),
        vec!["allowed.com".to_string()],
        true,
    );
    let result = execute_json(
        &tool,
        json!({
            "url": "https://notallowed.com",
            "prompt": "Extract the main content"
        }),
    )
    .await;
    let error = error_text(&result).unwrap_or("").to_string();
    assert!(error.contains("not in the whitelist"));
}

#[tokio::test]
async fn restricted_mode_allows_exemptions() {
    let tool = WebFetchTool::with_config(
        vtcode_config::WebFetchMode::Restricted,
        Vec::new(),
        Vec::new(),
        vec!["paypal.com".to_string()], // Exempt from blocklist
        true,
    );
    let result = execute_json(
        &tool,
        json!({
            "url": "https://paypal.com/login",
            "prompt": "Extract the main content"
        }),
    )
    .await;
    if let Some(error) = error_text(&result) {
        assert!(!error.contains("blocked for privacy"));
    }
}

#[tokio::test]
async fn custom_blocked_domains_work() {
    let tool = WebFetchTool::with_config(
        vtcode_config::WebFetchMode::Restricted,
        vec!["custom-blocked.com".to_string()], // Custom blocked domain
        Vec::new(),
        Vec::new(),
        true,
    );
    let result = execute_json(
        &tool,
        json!({
            "url": "https://custom-blocked.com/page",
            "prompt": "Extract the main content"
        }),
    )
    .await;
    let error = error_text(&result).unwrap_or("").to_string();
    assert!(error.contains("blocked for privacy and security reasons"));
}

#[tokio::test]
async fn custom_blocked_patterns_work() {
    let tool = WebFetchTool::with_config(
        vtcode_config::WebFetchMode::Restricted,
        Vec::new(),
        vec!["custom_secret=".to_string()], // Custom pattern
        Vec::new(),
        true,
    );
    let result = execute_json(
        &tool,
        json!({
            "url": "https://example.com?custom_secret=abc123",
            "prompt": "Extract the main content"
        }),
    )
    .await;
    let error = error_text(&result).unwrap_or("").to_string();
    assert!(error.contains("sensitive pattern"));
}

#[test]
fn default_headers_contain_text_markdown_accept() {
    let headers = WebFetchTool::default_headers();
    assert!(headers.contains_key(ACCEPT));
    let val = headers.get(ACCEPT).unwrap().to_str().unwrap();
    assert!(val.contains("text/markdown"));
}

#[test]
fn oversized_body_is_truncated_instead_of_rejected() {
    let fetched = fetched_content_from_bytes("αβγ".as_bytes(), 3).expect("valid utf-8 prefix should be returned");

    assert_eq!(fetched.content, "α");
    assert!(fetched.truncated_by_max_bytes);
    assert_eq!(fetched.source_size_bytes, "αβγ".len());
}

#[test]
fn truncation_backs_off_to_utf8_boundary_in_one_pass() {
    // Regression: the old implementation decremented `end` byte-by-byte,
    // which is O(n^2) on bodies whose prefix ends mid-multibyte-char. A 1MB
    // body of multibyte chars must truncate via `valid_up_to()` without
    // producing invalid UTF-8.
    let body = "界".repeat(500_000); // 3 bytes per char, 1.5MB total
    let max_bytes = 1_000_000; // cuts mid-char (not multiple of 3)
    let fetched = fetched_content_from_bytes(body.as_bytes(), max_bytes).expect("valid utf-8 prefix");
    assert!(fetched.truncated_by_max_bytes);
    assert!(fetched.content.len() <= max_bytes);
    assert!(
        fetched.content.len().is_multiple_of(3),
        "must end on a char boundary, got {}",
        fetched.content.len()
    );
}

#[test]
fn body_ending_exactly_at_cap_is_still_truncated() {
    // A body that is exactly `max_bytes` long is NOT truncated; one that
    // ends precisely at the cap after streaming must not be reported as
    // untruncated by the stream path. `fetched_content_from_bytes` alone
    // sees bytes.len() == max_bytes, so callers override the flag.
    let exact = fetched_content_from_bytes("abcde".as_bytes(), 5).expect("utf-8");
    assert!(!exact.truncated_by_max_bytes);

    // Multi-char exact fit that lands on a boundary is also not truncated.
    let fit = fetched_content_from_bytes("αβγ".as_bytes(), 6).expect("utf-8");
    assert!(!fit.truncated_by_max_bytes);
    assert_eq!(fit.content, "αβγ");
}

#[test]
fn description_guides_agents_to_try_llms_txt_first() {
    let tool = WebFetchTool::new();
    let description = tool.description();

    assert!(description.contains("/llms.txt"));
    assert!(description.contains("abc.com"));
    assert!(description.contains("https://abc.com/llms.txt"));
    assert!(description.contains("traverse"));
}

#[test]
fn max_bytes_and_timeout_are_clamped_to_hard_caps() {
    const _: () = {
        assert!(MAX_ALLOWED_BYTES >= MAX_CONTENT_SIZE);
        assert!(MAX_ALLOWED_TIMEOUT_SECS >= DEFAULT_TIMEOUT_SECS);
        assert!(MAX_ALLOWED_BYTES <= 10_000_000); // sanity: under 10MB
        assert!(MAX_ALLOWED_TIMEOUT_SECS <= 300); // sanity: under 5 minutes
    };
}

#[test]
fn ipv4_mapped_ipv6_loopback_is_private() {
    // ::ffff:127.0.0.1 should be caught as loopback
    assert!(is_private_host("::ffff:127.0.0.1"));
}

#[test]
fn ipv4_mapped_ipv6_private_is_private() {
    // ::ffff:10.0.0.1 should be caught as private (class A)
    assert!(is_private_host("::ffff:10.0.0.1"));
    // ::ffff:192.168.1.1 should be caught as private (class C)
    assert!(is_private_host("::ffff:192.168.1.1"));
    // ::ffff:172.16.0.1 should be caught as private (class B)
    assert!(is_private_host("::ffff:172.16.0.1"));
}

#[test]
fn ipv4_mapped_ipv6_link_local_is_private() {
    // ::ffff:169.254.0.1 should be caught as link-local
    assert!(is_private_host("::ffff:169.254.0.1"));
}

#[test]
fn ipv4_compatible_ipv6_loopback_is_private() {
    // ::127.0.0.1 (IPv4-compatible) should be caught
    assert!(is_private_host("::127.0.0.1"));
}

#[test]
fn ipv6_loopback_is_private() {
    assert!(is_private_host("::1"));
}

#[test]
fn ipv6_unique_local_is_private() {
    assert!(is_private_host("fd00::1"));
}

#[test]
fn ipv6_link_local_is_private() {
    assert!(is_private_host("fe80::1"));
}

#[test]
fn ipv4_mapped_ipv6_public_is_not_private() {
    // ::ffff:8.8.8.8 (Google DNS) should NOT be blocked
    assert!(!is_private_host("::ffff:8.8.8.8"));
}

#[tokio::test]
async fn rejects_localhost_tld() {
    let tool = WebFetchTool::new();
    let result = execute_json(
        &tool,
        json!({
            "url": "https://myapp.localhost/api",
            "prompt": "Extract content"
        }),
    )
    .await;
    let error = error_text(&result).unwrap_or("");
    assert!(error.contains("local/private networks"));
}

/// Regression test for the "who is vinhnx?" case. The default
/// `WebFetchConfig` seeds `allowed_domains` with common developer sites
/// (github.com, npmjs.com, crates.io, etc.). The default `Restricted`
/// mode honors those exemptions, so URLs on those hosts must pass
/// `validate_url` without touching the network.
#[test]
fn default_restricted_mode_allows_common_dev_sites() {
    let tool = WebFetchTool::from_config(&vtcode_config::WebFetchConfig::default());
    // After H1: the defaults come from the TOML allowlist filtered
    // to web-fetch-relevant categories. `docs.rs` and `www.npmjs.com`
    // are not in the TOML and are no longer in defaults; replace
    // them with registry hosts that ARE in the TOML.
    for url in [
        "https://github.com/vinhnx",
        "https://github.com/vinhnx?tab=repositories",
        "https://api.github.com/users/vinhnx",
        "https://api.github.com/users/vinhnx/repos?sort=stars&per_page=20",
        "https://registry.npmjs.org/vinhnx",
        "https://crates.io/users/vinhnx",
        "https://raw.githubusercontent.com/rust-lang/rust/master/README.md",
        "https://en.wikipedia.org/wiki/Rust_(programming_language)",
        "https://pypi.org/project/requests/",
        "https://r.jina.ai/https://example.com",
    ] {
        tool.validate_url(url)
            .unwrap_or_else(|e| panic!("default restricted mode should allow {url}: {e}"));
    }
}

/// Whitelist mode is a strict allowlist: only the user's explicit
/// `allowed_domains` apply, even if the defaults include the host.
/// This test constructs a tool with `Whitelist` mode and an empty
/// user allow list to assert the strict contract.
#[test]
fn whitelist_mode_is_strict_when_user_allow_list_is_empty() {
    let tool = WebFetchTool::from_config(&vtcode_config::WebFetchConfig {
        mode: vtcode_config::WebFetchMode::Whitelist,
        allowed_domains: Vec::new(),
        ..vtcode_config::WebFetchConfig::default()
    });
    let err = tool
        .validate_url("https://github.com/vinhnx")
        .expect_err("whitelist mode must reject domains not in the user allow list");
    assert!(err.to_string().contains("whitelist"), "unexpected error: {err}");
}

#[test]
fn domain_matches_allowed_handles_wildcards() {
    // Apex + subdomains under a wildcard.
    assert!(domain_matches_allowed("example.com", "*.example.com"));
    assert!(domain_matches_allowed("api.example.com", "*.example.com"));
    assert!(domain_matches_allowed("deep.nested.api.example.com", "*.example.com"));
    // Negative matches — wildcard must not be a substring match.
    assert!(!domain_matches_allowed("evilexample.com", "*.example.com"));
    assert!(!domain_matches_allowed("example.com.evil.tld", "*.example.com"));
    // Apex match is also accepted.
    assert!(domain_matches_allowed("example.com", "example.com"));
    assert!(domain_matches_allowed("api.example.com", "example.com"));
}

#[test]
fn domain_matches_allowed_rejects_single_label_wildcards() {
    // L10 (review): a wildcard like `*.com` would match every `.com`
    // host. That's almost certainly a misconfiguration; refuse it
    // outright so an editor who fat-fingers the allowlist doesn't
    // accidentally open the whole TLD.
    assert!(!domain_matches_allowed("example.com", "*.com"));
    assert!(!domain_matches_allowed("api.example.com", "*.com"));
    assert!(!domain_matches_allowed("co.uk", "*.uk"));
    // A multi-label suffix is still allowed.
    assert!(domain_matches_allowed("example.co.uk", "*.co.uk"));
}

#[test]
fn default_allowlist_includes_wildcard_categories() {
    // After H1: the web_fetch defaults are filtered to web-relevant
    // categories. The TOML currently has no `*.foo` wildcards in
    // those categories, so the defaults should not contain any.
    // The matcher itself is still tested for wildcards via
    // `domain_matches_allowed_handles_wildcards` above.
    let config = vtcode_config::WebFetchConfig::default();
    let wildcards: Vec<&str> = config
        .allowed_domains
        .iter()
        .map(|s| s.as_str())
        .filter(|s| s.starts_with("*."))
        .collect();
    assert!(
        wildcards.is_empty(),
        "expected no wildcards in default web_fetch allowlist (TOML has them only in auth/dev_infra which are excluded); got {wildcards:?}"
    );
}

#[test]
fn default_restricted_mode_lets_through_wildcard_hosts() {
    // Regression for the broader TOML allowlist: a URL on a wildcard
    // host should pass `validate_url` under the default `Restricted`
    // mode without any per-session config. The TOML only has wildcards
    // in `auth` / `dev_infra` (excluded by H1), so we exercise the
    // matcher with a user-supplied wildcard instead.
    let tool = WebFetchTool::from_config(&vtcode_config::WebFetchConfig {
        allowed_domains: vec!["*.example.com".to_string()],
        ..vtcode_config::WebFetchConfig::default()
    });
    for url in [
        "https://acme.example.com/authorize",
        "https://deep.nested.api.example.com/api",
    ] {
        tool.validate_url(url)
            .unwrap_or_else(|e| panic!("wildcard allow should accept {url}: {e}"));
    }
}

#[tokio::test]
async fn rejects_test_tld() {
    let tool = WebFetchTool::new();
    let result = execute_json(
        &tool,
        json!({
            "url": "https://example.test/page",
            "prompt": "Extract content"
        }),
    )
    .await;
    let error = error_text(&result).unwrap_or("");
    assert!(error.contains("local/private networks"));
}

#[tokio::test]
async fn rejects_invalid_tld() {
    let tool = WebFetchTool::new();
    let result = execute_json(
        &tool,
        json!({
            "url": "https://example.invalid/page",
            "prompt": "Extract content"
        }),
    )
    .await;
    let error = error_text(&result).unwrap_or("");
    assert!(error.contains("local/private networks"));
}

#[tokio::test]
async fn rejects_home_arpa_tld() {
    let tool = WebFetchTool::new();
    let result = execute_json(
        &tool,
        json!({
            "url": "https://myhost.home.arpa/page",
            "prompt": "Extract content"
        }),
    )
    .await;
    let error = error_text(&result).unwrap_or("");
    assert!(error.contains("local/private networks"));
}

/// Regression test for turn_578: a real HTTP 403 from npmjs.com
/// (server-side bot block) must come back as `error_type=http_error`
/// with `http_status=403`, NOT as a "policy_blocked" or "PermissionDenied"
/// response. The agent in turn_578 reported the host as "blocked by the
/// sandbox" because the error format only carried a flat string.
#[test]
fn classifies_http_403_as_server_block_not_policy() {
    let err = anyhow::anyhow!("HTTP request failed with status: 403 Forbidden");
    let response = web_fetch_error_response("https://www.npmjs.com/~vinhnx", 262144, 30, &err);
    assert_eq!(response["error_type"], "http_error");
    assert_eq!(response["http_status"], 403);
    let action = response["next_action"].as_str().unwrap_or("");
    assert!(
        action.contains("anti-bot") || action.contains("rate-limited"),
        "next_action should hint at anti-bot / rate-limit, got: {action}"
    );
}

/// Regression test for turn_578: a real HTTP 404 from crates.io (the
/// user does not exist) must come back as `error_type=http_error`
/// with `http_status=404` and a "the resource does not exist" hint.
#[test]
fn classifies_http_404_with_verify_url_hint() {
    let err = anyhow::anyhow!("HTTP request failed with status: 404 Not Found");
    let response = web_fetch_error_response("https://crates.io/users/vinhnx", 262144, 30, &err);
    assert_eq!(response["error_type"], "http_error");
    assert_eq!(response["http_status"], 404);
    let action = response["next_action"].as_str().unwrap_or("");
    assert!(action.contains("does not exist"), "next_action was: {action}");
}

/// 5xx responses are transient — the hint should nudge the agent to
/// either retry briefly or use web_search as a fallback.
#[test]
fn classifies_http_5xx_as_transient() {
    let err = anyhow::anyhow!("HTTP request failed with status: 503 Service Unavailable");
    let response = web_fetch_error_response("https://example.com", 262144, 30, &err);
    assert_eq!(response["error_type"], "http_error");
    assert_eq!(response["http_status"], 503);
    let action = response["next_action"].as_str().unwrap_or("");
    assert!(action.contains("retry") || action.contains("search"), "got: {action}");
}

/// Network-level errors (timeout, DNS, TLS) should be classified
/// distinctly from HTTP errors so the agent can pick the right
/// fallback.
#[test]
fn classifies_timeout_as_network_error() {
    let err = anyhow::anyhow!("request timed out");
    let response = web_fetch_error_response("https://example.com", 262144, 30, &err);
    assert_eq!(response["error_type"], "network_error");
    assert!(response.get("http_status").is_none() || response["http_status"].is_null());
}

#[test]
fn classifies_tls_error_separately() {
    let err = anyhow::anyhow!("TLS handshake failed: certificate verify failed");
    let response = web_fetch_error_response("https://example.com", 262144, 30, &err);
    assert_eq!(response["error_type"], "tls_error");
}

/// The error response must always carry a `next_action` so the agent
/// loop can decide what to do without re-parsing the free-form message.
#[test]
fn every_error_response_has_next_action() {
    let samples = [
        anyhow::anyhow!("HTTP request failed with status: 418 I'm a teapot"),
        anyhow::anyhow!("dns error: no such host"),
        anyhow::anyhow!("something completely unexpected"),
    ];
    for err in &samples {
        let response = web_fetch_error_response("https://example.com", 262144, 30, err);
        let action = response["next_action"].as_str().unwrap_or("");
        assert!(!action.is_empty(), "next_action must be non-empty; got response {response}");
    }
}

#[test]
fn contextual_policy_denial_preserves_reason_and_approval_guidance() {
    let err =
        anyhow::anyhow!("domain docs.example is blocked by configured allowlist").context("request validation failed");
    let response = web_fetch_error_response("https://docs.example/guide", 1000, 5, &err);
    assert_eq!(response["error_type"], "policy_blocked");
    assert!(
        response["error"]
            .as_str()
            .unwrap()
            .contains("domain docs.example is blocked by configured allowlist")
    );
    let action = response["next_action"].as_str().unwrap();
    assert!(action.contains("web_fetch approval and domain restrictions"));
    assert!(action.contains("do not bypass"));
    assert!(response.get("http_status").is_none());
}

#[test]
fn extract_http_status_delegates_to_classify_helpers() {
    // The local function delegates to `classify_helpers::extract_http_status`,
    // which is the canonical parser. These cases mirror the helpers'
    // own tests so a regression in the delegate is caught here too.
    assert_eq!(extract_http_status("status: 403 Forbidden"), Some(403));
    assert_eq!(
        extract_http_status("HTTP status server error (503 Service Unavailable) for url (https://example.com/)"),
        Some(503)
    );
    assert_eq!(extract_http_status("status:  500 Internal"), Some(500));
    assert_eq!(extract_http_status("no status here"), None);
}

#[test]
fn http_retry_after_survives_model_error_conversion() {
    let failure = errors::HttpFetchFailure { status: 429, retry_after_ms: Some(9000) };
    let output = web_fetch_error_response("https://example.com/", 1000, 30, &failure.into());
    let error = crate::tools::registry::ToolExecutionError::from_tool_output(&output).unwrap();
    let wire = error.to_json_value();
    assert_eq!(wire["error"]["http_status"], 429);
    assert_eq!(wire["error"]["retry_after_ms"], 9000);
    assert!(
        wire["error"]["recovery_suggestions"][0]
            .as_str()
            .unwrap()
            .contains("At most two")
    );
    assert_eq!(errors::retry_after_ms("6"), Some(6000));
    assert_eq!(errors::retry_after_ms("18446744073709551615"), Some(u64::MAX));
    assert_eq!(errors::retry_after_ms("Wed, 21 Oct 2015 07:28:00 GMT"), Some(0));
    assert_eq!(errors::retry_after_ms("not a delay"), None);
}

#[tokio::test]
async fn redirected_aliases_and_direct_fetches_share_the_response_origins_budget() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let target_sends = Arc::new(AtomicUsize::new(0));
    let alias_sends = Arc::new(AtomicUsize::new(0));
    let target_count = target_sends.clone();
    let alias_count = alias_sends.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut bytes = [0_u8; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut bytes).await.unwrap();
                if count == 0 {
                    break;
                }
                request.extend_from_slice(&bytes[..count]);
            }
            let request = String::from_utf8_lossy(&request);
            let response = if request.to_ascii_lowercase().contains("host: limited.example:") {
                target_count.fetch_add(1, Ordering::SeqCst);
                "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 0\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    .to_owned()
            } else {
                alias_count.fetch_add(1, Ordering::SeqCst);
                format!(
                    "HTTP/1.1 302 Found\r\nLocation: http://limited.example:{}/resource\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    address.port()
                )
            };
            stream.write_all(response.as_bytes()).await.unwrap();
        }
    });
    let shared = Arc::new(request_budget::FetchRequestBudget::default());
    let budget = request_budget::RequestBudget::new(shared.clone(), Instant::now() + Duration::from_secs(5));
    let redirect_budget = budget.clone();
    // Real HTTP transport with fixed test-only DNS mappings; production URL
    // policy remains exercised separately by the rejection tests above.
    let client = reqwest::Client::builder()
        .no_proxy()
        .resolve("first.example", address)
        .resolve("second.example", address)
        .resolve("third.example", address)
        .resolve("limited.example", address)
        .timeout(Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::custom(move |attempt| {
            follow_with_budget(attempt, Some(&redirect_budget))
        }))
        .build()
        .unwrap();
    let fetch = |host: &str| format!("http://{host}:{}/resource", address.port());
    let first = fetch_response(&client, &fetch("first.example"), 100, 5, Some(&budget))
        .await
        .unwrap_err();
    let error = web_fetch_error_response("first.example", 100, 5, &first);
    assert_eq!(error["error"]["http_status"], 429);
    assert!(
        error["error"]["debug_context"]["metadata"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| { entry[0] == "effective_origin" && entry[1].as_str().unwrap().contains("limited.example") })
    );
    let second = fetch_response(&client, &fetch("second.example"), 100, 5, Some(&budget))
        .await
        .unwrap_err();
    assert_eq!(web_fetch_error_response("second.example", 100, 5, &second)["error"]["http_status"], 429);
    let third = fetch("third.example");
    let direct = fetch("limited.example");
    let (alias, direct) = tokio::join!(
        fetch_response(&client, &third, 100, 5, Some(&budget)),
        fetch_response(&client, &direct, 100, 5, Some(&budget)),
    );
    for failure in [alias.unwrap_err(), direct.unwrap_err()] {
        let output = web_fetch_error_response("test", 100, 5, &failure);
        assert_eq!(output["error"]["http_status"], 429);
        assert_eq!(output["error"]["retryable"], false);
    }
    assert_eq!(target_sends.load(Ordering::SeqCst), 2, "only two requests may reach the limiting server");
    assert_eq!(alias_sends.load(Ordering::SeqCst), 3, "redirect sources retain independent accounting");
    shared.clear();
    assert!(
        fetch_response(&client, &fetch("limited.example"), 100, 5, Some(&budget))
            .await
            .is_err()
    );
    assert_eq!(target_sends.load(Ordering::SeqCst), 3, "a genuine request reset admits a fresh send");
    server.abort();
}
