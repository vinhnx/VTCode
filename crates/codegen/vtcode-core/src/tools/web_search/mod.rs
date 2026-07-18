//! WebSearch tool: query -> ranked web results (title, url, snippet).
//!
//! Lightweight, keyless, single-provider. We only target DuckDuckGo's HTML
//! endpoint (`https://html.duckduckgo.com/html/?q=...`), which is the only
//! path that returns real web results without an API key. This keeps the
//! tool simple and avoids any API-key plumbing.
//!
//! Safety/rate-limit guard rails:
//! - A **cooldown** between consecutive network requests (default 3s) prevents
//!   hammering DDG and triggering anti-bot challenges.
//! - A short **result cache** (default 5min TTL) means repeated identical
//!   queries are answered from memory without any network call.
//! - A **session-wide cap** (default 12 requests) ensures the tool cannot
//!   leak past DDG's soft quotas even with varied queries.
//!
//! All three are configurable via `WebSearchConfig` and turned off in tests.

use super::traits::Tool;
use crate::config::constants::tools;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use regex::Regex;
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue, USER_AGENT};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};
use url::Url;
use vtcode_config::WebSearchConfig;

const MAX_TIMEOUT_SECS: u64 = 60;
const MAX_RESULTS_CAP: usize = 20;
const MAX_TITLE_CHARS: usize = 200;
const MAX_SNIPPET_CHARS: usize = 400;

/// Browser-like user agent. DuckDuckGo's HTML endpoint blocks obvious bots, so a
/// realistic UA reduces (but does not eliminate) the chance of being challenged.
const BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

pub(crate) const WEB_SEARCH_DESCRIPTION: &str = "Searches the web for a query and returns a ranked list of results (title, url, snippet) inline. Accepts: { query: string, max_results?: number }. Uses the keyless DuckDuckGo HTML endpoint (best-effort, may be rate-limited). Results are cached for a few minutes to avoid repeat hits. Use web_fetch on the most promising result URL to read full content. Returns { query, provider: \"duckduckgo\", count, cached, results: [{ title, url, snippet }] }.";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WebSearchArgs {
    /// Search query.
    query: String,
    #[serde(default)]
    max_results: Option<usize>,
}

pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

impl SearchResult {
    pub fn new(title: String, url: String, snippet: String) -> Self {
        Self { title, url, snippet }
    }
}

/// Lightweight in-process state shared between calls on the same tool
/// instance. Owns the cooldown clock, the request counter, and the result
/// cache.
#[derive(Default)]
struct SessionState {
    last_request_at: Option<Instant>,
    requests_made: u32,
    cache: HashMap<String, CachedResults>,
}

struct CachedResults {
    stored_at: Instant,
    payload: Value,
}

impl SessionState {
    fn cache_get(&self, key: &str, ttl: Duration) -> Option<Value> {
        let entry = self.cache.get(key)?;
        if entry.stored_at.elapsed() > ttl {
            return None;
        }
        Some(entry.payload.clone())
    }

    fn cache_put(&mut self, key: String, payload: Value) {
        self.cache.insert(key, CachedResults { stored_at: Instant::now(), payload });
    }
}

/// WebSearch tool. Stateless over the network (DDG) but tracks per-instance
/// cooldown, request cap, and short-lived result cache to avoid hammering
/// DDG's HTML endpoint and triggering anti-bot challenges.
#[derive(Clone, Default)]
pub struct WebSearchTool {
    config: Arc<Mutex<WebSearchConfig>>,
    state: Arc<Mutex<SessionState>>,
}

impl WebSearchTool {
    pub fn new() -> Self {
        Self {
            config: Arc::new(Mutex::new(WebSearchConfig::default())),
            state: Arc::new(Mutex::new(SessionState::default())),
        }
    }

    /// Construct a tool with an explicit configuration. The config drives the
    /// result-count default, request timeout, cooldown, cache TTL, and
    /// session-wide request cap.
    pub fn with_config(config: WebSearchConfig) -> Self {
        Self {
            config: Arc::new(Mutex::new(config)),
            state: Arc::new(Mutex::new(SessionState::default())),
        }
    }

    /// Apply a new configuration (e.g., after `vtcode.toml` reload).
    pub fn set_config(&self, config: WebSearchConfig) {
        if let Ok(mut guard) = self.config.lock() {
            *guard = config;
        }
    }

    /// Reset the in-process state (cooldown, counter, cache). Tests use this.
    pub fn reset(&self) {
        if let Ok(mut guard) = self.state.lock() {
            *guard = SessionState::default();
        }
    }

    fn snapshot_config(&self) -> WebSearchConfig {
        self.config.lock().map(|guard| guard.clone()).unwrap_or_default()
    }

    async fn run(&self, raw_args: Value) -> Result<Value> {
        let args: WebSearchArgs =
            serde_json::from_value(raw_args).context("Invalid arguments for web_search. Provide a 'query' string.")?;

        let query = args.query.trim().to_string();
        if query.is_empty() {
            return Err(anyhow!("web_search requires a non-empty 'query'"));
        }

        let snapshot = self.snapshot_config();
        let default_max = snapshot.max_results.clamp(1, MAX_RESULTS_CAP);
        let max_results = args.max_results.unwrap_or(default_max).clamp(1, MAX_RESULTS_CAP);
        let cooldown = Duration::from_millis(snapshot.cooldown_ms);
        let cache_ttl = Duration::from_secs(snapshot.cache_ttl_secs);
        let session_cap = snapshot.session_max_requests;

        // Include `max_results` in the key so a repeat query with a
        // different cap gets a fresh, larger or smaller result set. With the
        // hard 20-result cap this is rarely observable, but it keeps the
        // cache semantics tight: the value the agent sees was produced for
        // the same `max_results` it asked for.
        let cache_key = format!("{max_results}::{query}");

        // Fast path: cache hit. Avoid the network entirely.
        if let Some(cached) = self.state.lock().ok().and_then(|guard| guard.cache_get(&cache_key, cache_ttl)) {
            return Ok(mark_cached(cached));
        }

        // Enforce session-wide request cap before touching the network.
        {
            let state = self.state.lock().map_err(|e| anyhow!("web_search state lock poisoned: {e}"))?;
            if state.requests_made >= session_cap {
                return Ok(session_cap_reached_response(&query, session_cap));
            }
            if let Some(last) = state.last_request_at {
                let elapsed = last.elapsed();
                if elapsed < cooldown {
                    return Ok(cooldown_response(&query, cooldown.checked_sub(elapsed).unwrap()));
                }
            }
        }

        let results = duckduckgo_search(&query, max_results, snapshot.timeout_secs).await;

        // By this point we have either short-circuited above (cache / cap /
        // cooldown) or attempted a real network call. Record the request so
        // the cooldown and session cap kick in for subsequent calls.
        if let Ok(mut state) = self.state.lock() {
            state.last_request_at = Some(Instant::now());
            state.requests_made = state.requests_made.saturating_add(1);
        }

        match results {
            Ok(results) if results.is_empty() => {
                let payload = json!({
                    "query": query,
                    "provider": "duckduckgo",
                    "count": 0,
                    "results": [],
                    "warning": "No results were returned. DuckDuckGo may have rate-limited the request or matched nothing. Try a different query, or wait a few seconds and try again."
                });
                self.cache_put(&cache_key, &payload);
                Ok(payload)
            }
            Ok(results) => {
                let payload = json!({
                    "query": query,
                    "provider": "duckduckgo",
                    "count": results.len(),
                    "results": results
                        .into_iter()
                        .map(|r| json!({ "title": r.title, "url": r.url, "snippet": r.snippet }))
                        .collect::<Vec<_>>(),
                });
                self.cache_put(&cache_key, &payload);
                Ok(payload)
            }
            Err(e) => {
                // Categorize the DDG error so the agent can act on it. Common
                // cases:
                //   - HTTP 202 / anti-bot challenge  -> network/IP-level block
                //     from DDG; the agent should not retry this turn.
                //   - Any other reqwest error         -> likely transient; retry.
                let (error_type, next_action) = classify_search_error(&e.to_string());
                Ok(json!({
                    "error": format!("web_search failed: {e}"),
                    "query": query,
                    "provider": "duckduckgo",
                    "error_type": error_type,
                    "next_action": next_action,
                }))
            }
        }
    }

    fn cache_put(&self, key: &str, payload: &Value) {
        if let Ok(mut state) = self.state.lock() {
            state.cache_put(key.to_string(), payload.clone());
        }
    }
}

/// Classify a DuckDuckGo error into a `(error_type, next_action)` pair. The
/// returned strings are stable so the agent loop can branch on them.
fn classify_search_error(message: &str) -> (&'static str, &'static str) {
    let lower = message.to_lowercase();
    // 5xx from the upstream search service is transient but should not be
    // retried the same way as a network error — the cause is server-side.
    if let Some(status) = crate::tools::web_fetch::classify_helpers::extract_http_status(&lower) {
        if (500..=599).contains(&status) {
            return (
                "upstream_error",
                "The search service is currently unavailable. Retry after a short delay, or use web_fetch on a known URL as a fallback.",
            );
        }
    }
    if message.contains("HTTP 202") || lower.contains("anti-bot") || lower.contains("challenge") {
        (
            "antiban_blocked",
            "DuckDuckGo declined this request (likely an anti-bot challenge for this network). Do NOT retry immediately; pick a result URL from this session's earlier searches and use web_fetch on it instead, or ask the user to confirm a different search provider.",
        )
    } else if lower.contains("timeout") {
        (
            "network_error",
            "DuckDuckGo timed out. Retry after a short delay, or use web_fetch on a known URL as a fallback.",
        )
    } else {
        (
            "network_error",
            "Wait a few seconds and retry, or use web_fetch directly if you already know a relevant URL.",
        )
    }
}

fn mark_cached(mut payload: Value) -> Value {
    if let Value::Object(map) = &mut payload {
        map.insert("cached".to_string(), Value::Bool(true));
    }
    payload
}

fn cooldown_response(query: &str, wait: Duration) -> Value {
    json!({
        "error": "web_search cooldown active",
        "query": query,
        "provider": "duckduckgo",
        "retry_after_ms": wait.as_millis() as u64,
        "next_action": format!("Wait at least {} ms before the next web search to avoid being rate-limited.", wait.as_millis())
    })
}

fn session_cap_reached_response(query: &str, cap: u32) -> Value {
    json!({
        "error": "web_search session request cap reached",
        "query": query,
        "provider": "duckduckgo",
        "session_max_requests": cap,
        "next_action": format!("This session has used its {cap} web searches. Use web_fetch on a known URL or restart the session to search again.")
    })
}

fn build_client(timeout_secs: u64) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs.min(MAX_TIMEOUT_SECS)))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .context("failed to build HTTP client for web_search")
}

// ---------------------------------------------------------------------------
// DuckDuckGo (keyless, best-effort HTML scraping)
// ---------------------------------------------------------------------------

static DDG_RESULT_ANCHOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)<a[^>]*class="result__a"[^>]*href="([^"]*)"[^>]*>(.*?)</a>"#).expect("valid DDG anchor regex")
});
static DDG_SNIPPET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"(?s)class="result__snippet"[^>]*>(.*?)</a>"#).expect("valid DDG snippet regex"));

async fn duckduckgo_search(query: &str, max_results: usize, timeout_secs: u64) -> Result<Vec<SearchResult>> {
    let client = build_client(timeout_secs)?;
    let mut headers = HeaderMap::new();
    headers.insert(USER_AGENT, HeaderValue::from_static(BROWSER_USER_AGENT));
    headers.insert(ACCEPT, HeaderValue::from_static("text/html,application/xhtml+xml"));

    // The HTML endpoint expects the query as a POST form field `q`.
    let response = client
        .post("https://html.duckduckgo.com/html/")
        .headers(headers)
        .form(&[("q", query)])
        .send()
        .await
        .context("DuckDuckGo request failed")?;

    let status = response.status();
    // HTTP 202 from the HTML endpoint is DuckDuckGo's anti-bot challenge page
    // (it returns the homepage shell with no results). Treat it, and any other
    // non-200, as a challenge rather than letting parsing silently yield zero
    // results (which previously caused agent retry loops).
    if status.as_u16() == 202 || !status.is_success() {
        return Err(anyhow!(
            "DuckDuckGo declined the request (HTTP {status}), likely an anti-bot challenge for this network. Wait a few seconds and retry."
        ));
    }

    let body = response.text().await.context("failed to read DuckDuckGo response body")?;

    Ok(parse_duckduckgo_html(&body, max_results))
}

/// Extract ranked results from a DuckDuckGo HTML body. Pure function so it can
/// be exercised in unit tests against a local fixture (no live network).
pub fn parse_duckduckgo_html(body: &str, max_results: usize) -> Vec<SearchResult> {
    let snippets: Vec<String> = DDG_SNIPPET.captures_iter(body).map(|c| clean_html(&c[1])).collect();

    let mut results = Vec::new();
    for (idx, caps) in DDG_RESULT_ANCHOR.captures_iter(body).enumerate() {
        if results.len() >= max_results {
            break;
        }
        let raw_href = &caps[1];
        let title = clean_html(&caps[2]);
        let Some(url) = normalize_ddg_url(raw_href) else {
            continue;
        };
        if title.is_empty() {
            continue;
        }
        let snippet = snippets.get(idx).cloned().unwrap_or_default();
        results.push(SearchResult {
            title: truncate_chars(&title, MAX_TITLE_CHARS),
            url,
            snippet: truncate_chars(&snippet, MAX_SNIPPET_CHARS),
        });
    }

    results
}

/// Resolve a DuckDuckGo result href into a real https/http URL.
///
/// DDG wraps targets in a redirector like `//duckduckgo.com/l/?uddg=<encoded>`.
/// Returns `None` for non-http(s) schemes (defends against `javascript:` etc).
fn normalize_ddg_url(href: &str) -> Option<String> {
    let absolute = if let Some(stripped) = href.strip_prefix("//") {
        format!("https://{stripped}")
    } else {
        href.to_string()
    };

    let parsed = Url::parse(&absolute).ok()?;
    if let Some((_, target)) = parsed.query_pairs().find(|(k, _)| k == "uddg") {
        let target = target.into_owned();
        return validate_result_url(&target);
    }
    validate_result_url(&absolute)
}

/// Only allow http(s) result URLs with a host.
fn validate_result_url(url: &str) -> Option<String> {
    let parsed = Url::parse(url).ok()?;
    match parsed.scheme() {
        "http" | "https" => {}
        _ => return None,
    }
    if parsed.host_str().is_none_or(str::is_empty) {
        return None;
    }
    Some(url.to_string())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

static HTML_TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").expect("valid html tag regex"));

/// Strip HTML tags and decode common entities from a snippet/title fragment.
fn clean_html(input: &str) -> String {
    let without_tags = HTML_TAG.replace_all(input, "");
    decode_html_entities(without_tags.trim())
}

fn decode_html_entities(input: &str) -> String {
    input
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&#x2F;", "/")
        .replace("&nbsp;", " ")
        .replace("&hellip;", "…")
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    let trimmed = input.trim();
    if trimmed.chars().count() <= max_chars {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(max_chars).collect();
    out.push('…');
    out
}

#[async_trait]
impl Tool for WebSearchTool {
    async fn execute(&self, args: Value) -> Result<Value> {
        self.run(args).await
    }

    fn name(&self) -> &str {
        tools::WEB_SEARCH
    }

    fn description(&self) -> &str {
        WEB_SEARCH_DESCRIPTION
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_html_strips_tags_and_decodes_entities() {
        assert_eq!(clean_html("<b>Rust</b> &amp; <i>Cargo</i> &#39;build&#39;"), "Rust & Cargo 'build'");
    }

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
        assert!(validate_result_url("https://example.com").is_some());
    }

    #[test]
    fn missing_query_is_rejected() {
        let tool = WebSearchTool::new();
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool.run(json!({ "max_results": 5 })));
        assert!(result.is_err());
    }

    #[test]
    fn pattern_is_rejected_as_an_unknown_field() {
        let result = serde_json::from_value::<WebSearchArgs>(json!({
            "query": "vinhnx",
            "pattern": "legacy"
        }));
        assert!(result.is_err());
    }

    #[test]
    fn canonical_query_and_max_results_are_accepted() {
        let args = serde_json::from_value::<WebSearchArgs>(json!({
            "query": "vinhnx",
            "max_results": 5
        }))
        .expect("canonical web_search arguments");
        assert_eq!(args.query, "vinhnx");
        assert_eq!(args.max_results, Some(5));
    }

    #[test]
    fn truncate_chars_appends_ellipsis() {
        assert_eq!(truncate_chars("hello world", 5), "hello…");
        assert_eq!(truncate_chars("hi", 5), "hi");
    }

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
        // panic or fabricate results; the higher-level dispatcher then decides
        // how to surface this to the agent.
        let challenge = r#"<html><body>Anomaly detected.</body></html>"#;
        assert!(parse_duckduckgo_html(challenge, 10).is_empty());
    }

    #[test]
    fn session_cap_short_circuits_with_structured_error() {
        let config = WebSearchConfig {
            provider: Default::default(),
            max_results: 5,
            timeout_secs: 20,
            cooldown_ms: 0,
            cache_ttl_secs: 300,
            session_max_requests: 2,
        };
        let tool = WebSearchTool::with_config(config);
        // Bump the counter without going to the network.
        {
            let mut state = tool.state.lock().unwrap();
            state.requests_made = 2;
            state.last_request_at = Some(Instant::now());
        }
        let payload = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool.run(json!({ "query": "rust" })))
            .expect("cap should be a structured JSON, not a runtime error");
        assert_eq!(payload["error"], "web_search session request cap reached");
        assert_eq!(payload["session_max_requests"], 2);
    }

    #[test]
    fn cooldown_short_circuits_with_retry_after() {
        let config = WebSearchConfig {
            provider: Default::default(),
            max_results: 5,
            timeout_secs: 20,
            cooldown_ms: 5_000,
            cache_ttl_secs: 300,
            session_max_requests: 100,
        };
        let tool = WebSearchTool::with_config(config);
        {
            let mut state = tool.state.lock().unwrap();
            state.requests_made = 0;
            state.last_request_at = Some(Instant::now());
        }
        let payload = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool.run(json!({ "query": "rust" })))
            .expect("cooldown should be a structured JSON");
        assert_eq!(payload["error"], "web_search cooldown active");
        assert!(payload["retry_after_ms"].as_u64().unwrap() > 0);
    }

    #[test]
    fn cache_serves_repeat_queries_without_network() {
        // Pin max_results so the cache key matches the one the tool builds.
        let tool = WebSearchTool::with_config(WebSearchConfig {
            provider: Default::default(),
            max_results: 5,
            ..WebSearchConfig::default()
        });
        let cached_payload = json!({
            "query": "rust",
            "provider": "duckduckgo",
            "count": 1,
            "results": [{
                "title": "Cached Result",
                "url": "https://example.com/cached",
                "snippet": "from cache"
            }]
        });
        {
            let mut state = tool.state.lock().unwrap();
            state.cache_put("5::rust".to_string(), cached_payload.clone());
        }
        let payload = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool.run(json!({ "query": "rust" })))
            .expect("cache hit must not error");
        assert_eq!(payload["cached"], json!(true));
        assert_eq!(payload["count"], 1);
        assert_eq!(payload["results"][0]["title"], "Cached Result");
    }

    #[test]
    fn classify_search_error_flags_antiban_block() {
        // DDG's HTML endpoint returns 202 (or 200 with a challenge page) on
        // a bot block. The classifier must route that to `antiban_blocked`
        // and tell the agent to NOT retry, because the same turn will
        // almost certainly hit the same block.
        let (kind, action) = classify_search_error(
            "DuckDuckGo declined the request (HTTP 202), likely an anti-bot challenge for this network.",
        );
        assert_eq!(kind, "antiban_blocked");
        assert!(action.contains("Do NOT retry"), "action should discourage immediate retry; got: {action}");
    }

    #[test]
    fn classify_search_error_flags_timeout_as_network_error() {
        let (kind, action) = classify_search_error("request timed out after 20s");
        assert_eq!(kind, "network_error");
        assert!(
            action.contains("retry") || action.contains("web_fetch"),
            "action should suggest retry or web_fetch; got: {action}"
        );
    }
}
