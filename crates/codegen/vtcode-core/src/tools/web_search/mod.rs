//! WebSearch tool: query -> ranked web results (title, url, snippet).
//!
//! The keyless DuckDuckGo HTML endpoint provides best-effort ranked results and
//! may be rate-limited or challenged by anti-bot defenses.
//!
//! Safety/rate-limit guard rails:
//! - A **cooldown** between consecutive network requests (default 3s) prevents
//!   hammering DDG and triggering anti-bot challenges.
//! - A short **result cache** (default 5min TTL) means repeated identical
//!   queries are answered from memory without any network call.
//! - A **session-wide cap** (default 12 requests) ensures the tool cannot
//!   run away with varied queries. This is a conservative VTCode budget, not
//!   a DuckDuckGo-published quota.
//!
//! All three are configurable via `WebSearchConfig` and turned off in tests.

use super::traits::Tool;
use crate::config::constants::tools;
use anyhow::{Context, Result, anyhow};
use astral_html::Token;
use async_trait::async_trait;
use futures::stream::StreamExt;
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue, USER_AGENT};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use url::Url;
use vtcode_config::WebSearchConfig;

const MIN_TIMEOUT_SECS: u64 = 1;
const MAX_TIMEOUT_SECS: u64 = 60;
const MAX_QUERY_CHARS: usize = 500;
const MAX_RESULTS_CAP: usize = 20;
const MAX_TITLE_CHARS: usize = 200;
const MAX_SNIPPET_CHARS: usize = 400;

/// Browser-like user agent. DuckDuckGo's HTML endpoint blocks obvious bots, so a
/// realistic UA reduces (but does not eliminate) the chance of being challenged.
const BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

pub(crate) const WEB_SEARCH_DESCRIPTION: &str = "Searches the web with keyless DuckDuckGo HTML search and returns ranked results (title, url, snippet). Accepts { query: string (up to 500 characters), max_results?: number }. Results are cached briefly to reduce rate limiting; cooldown and session caps bound requests. Use web_fetch on a promising result URL to read full content, subject to separate approval. Returns { query, provider, count, cached, results: [{ title, url, snippet }] } or a structured error with error_type, retryable, and next_action.";

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

    fn snapshot_config(&self) -> Result<WebSearchConfig> {
        self.config
            .lock()
            .map(|guard| guard.clone())
            .map_err(|error| anyhow!("web_search config lock poisoned: {error}"))
    }

    async fn run(&self, raw_args: Value) -> Result<Value> {
        let args: WebSearchArgs =
            serde_json::from_value(raw_args).context("Invalid arguments for web_search. Provide a 'query' string.")?;

        let query = args.query.trim().to_string();
        if query.is_empty() || query.chars().count() > MAX_QUERY_CHARS {
            return Err(anyhow!("web_search requires a nonblank query of at most {MAX_QUERY_CHARS} characters"));
        }

        let snapshot = self.snapshot_config()?;
        let default_max = snapshot.max_results.clamp(1, MAX_RESULTS_CAP);
        let max_results = args.max_results.unwrap_or(default_max).clamp(1, MAX_RESULTS_CAP);
        let cooldown = Duration::from_millis(snapshot.cooldown_ms);
        let cache_ttl = Duration::from_secs(snapshot.cache_ttl_secs);
        let session_cap = snapshot.session_max_requests;
        let provider = snapshot.provider.backend_id();

        // Include `max_results` in the key so a repeat query with a
        // different cap gets a fresh, larger or smaller result set. With the
        // hard 20-result cap this is rarely observable, but it keeps the
        // cache semantics tight: the value the agent sees was produced for
        // the same `max_results` it asked for.
        let cache_key = format!("{max_results}::{query}");

        // Fast path: cache hit. Avoid the network entirely.
        let cached = self
            .state
            .lock()
            .map_err(|error| anyhow!("web_search state lock poisoned: {error}"))?
            .cache_get(&cache_key, cache_ttl);
        if let Some(cached) = cached {
            return Ok(mark_cached(cached, provider));
        }

        // Reserve the request before touching the network. The reservation is
        // atomic with the cap/cooldown check, so concurrent calls cannot all
        // pass admission and exceed the session budget.
        if let Some(response) = self.admit_request(&query, provider, cooldown, session_cap)? {
            return Ok(response);
        }

        let results = duckduckgo_search(&query, max_results, snapshot.timeout_secs).await;

        match results {
            Ok(results) => {
                let payload = results_response(&query, provider, results);
                self.cache_put(&cache_key, &payload)?;
                Ok(payload)
            }
            Err(e) => {
                // Categorize the error so the agent can act on it.
                Ok(search_error_response(&query, provider, &e.to_string()))
            }
        }
    }

    fn cache_put(&self, key: &str, payload: &Value) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|error| anyhow!("web_search state lock poisoned: {error}"))?;
        state.cache_put(key.to_string(), payload.clone());
        Ok(())
    }

    fn admit_request(
        &self,
        query: &str,
        provider: &str,
        cooldown: Duration,
        session_cap: u32,
    ) -> Result<Option<Value>> {
        let mut state = self.state.lock().map_err(|e| anyhow!("web_search state lock poisoned: {e}"))?;
        if state.requests_made >= session_cap {
            return Ok(Some(session_cap_reached_response(provider, query, session_cap)));
        }
        if let Some(last) = state.last_request_at {
            let elapsed = last.elapsed();
            if elapsed < cooldown {
                return Ok(Some(cooldown_response(provider, query, cooldown.saturating_sub(elapsed))));
            }
        }
        state.requests_made = state.requests_made.saturating_add(1);
        state.last_request_at = Some(Instant::now());
        Ok(None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SearchErrorClassification {
    error_type: &'static str,
    next_action: &'static str,
    retryable: bool,
}

/// Classify a search-provider error into the stable fields exposed to the
/// agent. Anti-bot blocks are deliberately non-retryable; ordinary transport
/// and upstream 5xx failures remain explicitly retryable.
fn classify_search_error(message: &str) -> SearchErrorClassification {
    let lower = message.to_lowercase();
    let status = crate::tools::web_fetch::classify_helpers::extract_http_status(&lower);
    if matches!(status, Some(202 | 403 | 429)) || lower.contains("anti-bot") || lower.contains("challenge") {
        return SearchErrorClassification {
            error_type: "antiban_blocked",
            next_action: "DuckDuckGo declined this request (likely an anti-bot challenge for this network). Do not retry immediately from this network; pick a result URL from this session's earlier searches and use web_fetch on it instead, or ask the user to confirm a different search provider.",
            retryable: false,
        };
    }
    if lower.contains("response exceeds") || lower.contains("body exceeded") {
        return SearchErrorClassification {
            error_type: "response_too_large",
            next_action: "DuckDuckGo returned an oversized response. Narrow the query before explicitly retrying.",
            retryable: false,
        };
    }
    if status.is_some_and(|status| (500..=599).contains(&status)) {
        return SearchErrorClassification {
            error_type: "upstream_error",
            next_action: "The search service is currently unavailable. Retry once after a short delay, or use web_fetch on a known URL as a fallback.",
            retryable: true,
        };
    }
    if lower.contains("timeout") || lower.contains("timed out") {
        return SearchErrorClassification {
            error_type: "network_error",
            next_action: "DuckDuckGo timed out. Retry once after a short delay, or use web_fetch on a known URL as a fallback.",
            retryable: true,
        };
    }
    SearchErrorClassification {
        error_type: "network_error",
        next_action: "Retry once after a short delay, or use web_fetch directly if you already know a relevant URL.",
        retryable: true,
    }
}

fn results_response(query: &str, provider: &str, results: Vec<SearchResult>) -> Value {
    let count = results.len();
    let mut payload = json!({
        "query": query,
        "provider": provider,
        "count": count,
        "cached": false,
        "results": results
            .into_iter()
            .map(|result| json!({ "title": result.title, "url": result.url, "snippet": result.snippet }))
            .collect::<Vec<_>>(),
    });

    if count == 0 {
        payload["warning"] = json!(
            "No results were returned. DuckDuckGo may have rate-limited the request or matched nothing. Try a different query, or wait a few seconds and try again."
        );
    }

    payload
}

fn search_error_response(query: &str, provider: &str, message: &str) -> Value {
    let classification = classify_search_error(message);
    json!({
        "error": format!("web_search failed: {message}"),
        "query": query,
        "provider": provider,
        "error_type": classification.error_type,
        "retryable": classification.retryable,
        "next_action": classification.next_action,
    })
}

fn mark_cached(mut payload: Value, provider: &str) -> Value {
    if let Value::Object(map) = &mut payload {
        map.insert("provider".to_string(), Value::String(provider.to_owned()));
        map.insert("cached".to_string(), Value::Bool(true));
    }
    payload
}

fn cooldown_response(provider: &str, query: &str, wait: Duration) -> Value {
    let retry_after_ms = wait.as_millis().clamp(1, u64::MAX as u128) as u64;
    json!({
        "error": "web_search cooldown active",
        "query": query,
        "provider": provider,
        "error_type": "cooldown",
        "retryable": true,
        "retry_after_ms": retry_after_ms,
        "next_action": format!("Wait at least {retry_after_ms} ms before the next web search to avoid being rate-limited.")
    })
}

fn session_cap_reached_response(provider: &str, query: &str, cap: u32) -> Value {
    json!({
        "error": "web_search session request cap reached",
        "query": query,
        "provider": provider,
        "error_type": "session_cap",
        "retryable": false,
        "session_max_requests": cap,
        "request_budget_remaining": 0,
        "next_action": format!("This session has used its {cap} web searches. Use web_fetch on a known URL or restart the session to search again.")
    })
}

/// Build the HTTP client for the DuckDuckGo provider. Follow only HTTPS
/// redirects that stay on the known DuckDuckGo HTML host; a compromised or
/// misconfigured upstream must not turn search into an SSRF relay.
fn build_client(timeout_secs: u64) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(effective_timeout_secs(timeout_secs)))
        .retry(reqwest::retry::never())
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 3 {
                return attempt.stop();
            }
            if is_allowed_ddg_redirect(attempt.url()) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .context("failed to build HTTP client for web_search")
}

fn effective_timeout_secs(timeout_secs: u64) -> u64 {
    timeout_secs.clamp(MIN_TIMEOUT_SECS, MAX_TIMEOUT_SECS)
}

fn is_allowed_ddg_redirect(url: &Url) -> bool {
    url.scheme() == "https" && url.host_str() == Some("html.duckduckgo.com")
}

// ---------------------------------------------------------------------------
// DuckDuckGo (keyless, best-effort HTML scraping)
//
// Result extraction is tokenized with `astral-html`'s streaming `Reader`
// (pinned pilot dependency, vtcode-core only): tag and attribute names
// arrive ASCII-lowercased, text in `script`/`style` stays raw text instead
// of spurious tags, and each snippet is collected from its own anchor's
// window so a missing snippet yields empty rather than the next result's.
// ---------------------------------------------------------------------------

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
    // HTTP 202, 403, and 429 are common anti-bot responses. Treat them as a
    // block rather than letting parsing or generic retry logic hide the cause.
    if matches!(status.as_u16(), 202 | 403 | 429) {
        return Err(anyhow!(
            "DuckDuckGo declined the request (HTTP {status}), likely an anti-bot challenge for this network. Wait a few seconds and retry."
        ));
    }
    if !status.is_success() {
        return Err(anyhow!("DuckDuckGo returned HTTP {status}"));
    }

    let body = read_capped_ddg_body(response).await?;

    parse_duckduckgo_response(&body, max_results)
}

/// Upper bound for a DuckDuckGo HTML answer body. Result pages are a few
/// hundred kilobytes; the cap only bites on adversarial responses, and the
/// streaming parser stops at `max_results` well before it anyway.
const MAX_DDG_BODY_BYTES: usize = 1_000_000;

/// Read a DDG response body with an explicit byte cap. Decoding is lossy
/// rather than fallible: DuckDuckGo serves UTF-8, and a corrupt byte must
/// not turn into a hard error when partial results will do.
async fn read_capped_ddg_body(response: reqwest::Response) -> Result<String> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_DDG_BODY_BYTES as u64)
    {
        return Err(anyhow!("DuckDuckGo response exceeds the {MAX_DDG_BODY_BYTES}-byte limit"));
    }

    let mut bytes = Vec::with_capacity(MAX_DDG_BODY_BYTES.min(64 * 1024));
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("failed to read DuckDuckGo response body")?;
        let remaining = MAX_DDG_BODY_BYTES.saturating_sub(bytes.len());
        if chunk.len() > remaining {
            return Err(anyhow!("DuckDuckGo response body exceeded the {MAX_DDG_BODY_BYTES}-byte limit"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(capped_body_text(&bytes))
}

/// Pure, testable core of [`read_capped_ddg_body`]: cap at
/// `MAX_DDG_BODY_BYTES`, then lossy-decode (never panics, even on a split
/// trailing char or leading garbage).
fn capped_body_text(bytes: &[u8]) -> String {
    let end = bytes.len().min(MAX_DDG_BODY_BYTES);
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

/// Parse one DDG response and reject known anti-bot pages before they can be
/// mistaken for a legitimate empty result set. A normal no-hit page has no
/// result anchors and no challenge markers, so it remains a valid empty
/// response and can be cached briefly.
fn parse_duckduckgo_response(body: &str, max_results: usize) -> Result<Vec<SearchResult>> {
    let results = parse_duckduckgo_html(body, max_results);
    if results.is_empty() && is_duckduckgo_challenge_page(body) {
        return Err(anyhow!(
            "DuckDuckGo returned an anti-bot challenge page instead of search results; wait before retrying from this network"
        ));
    }
    Ok(results)
}

fn is_duckduckgo_challenge_page(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    [
        "anomaly detected",
        "captcha",
        "challenge-form",
        "unusual traffic",
        "verify you are human",
        "are you a robot",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

/// Extract ranked results from a DuckDuckGo HTML body. Pure function so it can
/// be exercised in unit tests against a local fixture (no live network).
///
/// Single streaming pass: a result anchor opens title collection, its first
/// `</a>` completes it, and the following `result__snippet` element (before
/// the next result anchor) supplies the snippet. First-close-wins keeps
/// parity with the previous extractor on well-formed markup.
pub fn parse_duckduckgo_html(body: &str, max_results: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();
    let mut state = DdgSection::Idle;
    for token in astral_html::Reader::new(body) {
        if results.len() >= max_results {
            break;
        }
        match token {
            Token::StartTag(tag) => {
                if tag.name == "a" && has_ddg_class(&tag, "result__a") {
                    // A new result anchor ends the pending one (keeping an
                    // empty snippet) and drops any partial buffers.
                    if let DdgSection::AfterTitle { url, title } = std::mem::replace(&mut state, DdgSection::Idle) {
                        push_ddg_result(&mut results, url, title, String::new(), max_results);
                        if results.len() >= max_results {
                            break;
                        }
                    }
                    if !tag.self_closing {
                        if let Some(href) = ddg_attr(&tag, "href") {
                            state = DdgSection::InTitle { href: href.to_string(), buf: String::new() };
                        }
                    }
                } else if matches!(state, DdgSection::AfterTitle { .. }) && has_ddg_class(&tag, "result__snippet") {
                    if let DdgSection::AfterTitle { url, title } = std::mem::replace(&mut state, DdgSection::Idle) {
                        if tag.self_closing {
                            push_ddg_result(&mut results, url, title, String::new(), max_results);
                        } else {
                            state = DdgSection::InSnippet {
                                url,
                                title,
                                name: tag.name.to_string(),
                                buf: String::new(),
                            };
                        }
                    }
                }
                // All other tags are content or noise: nested formatting
                // text arrives as Text tokens; nothing to do here.
            }
            Token::Text(text) => match &mut state {
                DdgSection::InTitle { buf, .. } | DdgSection::InSnippet { buf, .. } => {
                    buf.push_str(&text);
                }
                DdgSection::Idle | DdgSection::AfterTitle { .. } => {}
            },
            Token::EndTag(tag) => {
                if tag.name == "a" && matches!(state, DdgSection::InTitle { .. }) {
                    if let DdgSection::InTitle { href, buf } = std::mem::replace(&mut state, DdgSection::Idle) {
                        let title = buf.trim().to_string();
                        if !title.is_empty() {
                            if let Some(url) = normalize_ddg_url(&href) {
                                state = DdgSection::AfterTitle { url, title };
                            }
                        }
                    }
                } else if matches!(state, DdgSection::InSnippet { .. }) {
                    let done = if let DdgSection::InSnippet { name, .. } = &state {
                        tag.name == *name
                    } else {
                        false
                    };
                    if done {
                        if let DdgSection::InSnippet { url, title, buf, .. } =
                            std::mem::replace(&mut state, DdgSection::Idle)
                        {
                            push_ddg_result(&mut results, url, title, buf, max_results);
                        }
                    }
                }
            }
            _ => {}
        }
    }
    // Trailing result without a snippet element keeps an empty snippet.
    if let DdgSection::AfterTitle { url, title } = state {
        push_ddg_result(&mut results, url, title, String::new(), max_results);
    }
    results
}

/// Tokenizer position while extracting one result: idle, collecting the
/// anchor title, holding a completed title, or collecting its snippet.
enum DdgSection {
    Idle,
    InTitle {
        href: String,
        buf: String,
    },
    AfterTitle {
        url: String,
        title: String,
    },
    InSnippet {
        url: String,
        title: String,
        name: String,
        buf: String,
    },
}

/// Decoded value of attribute `name` on `tag`, if present. Attribute names
/// are ASCII-lowercased by the tokenizer, so plain equality applies.
fn ddg_attr(tag: &astral_html::Tag<'_>, name: &str) -> Option<String> {
    tag.attributes
        .iter()
        .find(|attr| attr.name == name)
        .map(|attr| attr.value().to_string())
}

/// Whether the tag's `class` attribute carries `class` as a whitespace
/// separated token (ASCII case-insensitive, matching DDG's lowercase).
fn has_ddg_class(tag: &astral_html::Tag<'_>, class: &str) -> bool {
    tag.attributes
        .iter()
        .find(|attr| attr.name == "class")
        .is_some_and(|attr| attr.value().split_whitespace().any(|token| token.eq_ignore_ascii_case(class)))
}

/// Append one validated result unless the cap is reached. Callers validate
/// the URL and require a non-empty title before reaching `AfterTitle`, so
/// this only truncates and pushes.
fn push_ddg_result(results: &mut Vec<SearchResult>, url: String, title: String, snippet: String, max_results: usize) {
    if results.len() >= max_results {
        return;
    }
    results.push(SearchResult {
        title: truncate_chars(&title, MAX_TITLE_CHARS),
        url,
        snippet: truncate_chars(&snippet, MAX_SNIPPET_CHARS),
    });
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
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return None;
    }
    Some(url.to_string())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn truncate_chars(input: &str, max_chars: usize) -> String {
    vtcode_commons::formatting::truncate_text(input.trim(), max_chars, "…")
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
    use crate::tools::web_fetch::classify_helpers::decode_html_entities;
    use vtcode_config::WebSearchProvider;

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
    fn missing_query_is_rejected() {
        let tool = WebSearchTool::new();
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool.run(json!({ "max_results": 5 })));
        assert!(result.is_err());
    }

    #[test]
    fn oversized_query_is_rejected_before_network_admission() {
        let tool = WebSearchTool::new();
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool.run(json!({ "query": "x".repeat(MAX_QUERY_CHARS + 1) })));
        assert!(result.is_err());
        assert_eq!(tool.state.lock().unwrap().requests_made, 0);
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
        // panic or fabricate results; the response wrapper turns the marker
        // into a structured blocked-search error.
        let challenge = r#"<html><body>Anomaly detected.</body></html>"#;
        assert!(parse_duckduckgo_html(challenge, 10).is_empty());
    }

    #[test]
    fn challenge_response_is_rejected_before_empty_results_are_cached() {
        let challenge = r#"<html><body>Anomaly detected.</body></html>"#;
        let error = match parse_duckduckgo_response(challenge, 10) {
            Ok(_) => panic!("challenge pages must be rejected"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("anti-bot challenge"));

        let empty = "<html><body><p>No results found.</p></body></html>";
        assert!(parse_duckduckgo_response(empty, 10).unwrap().is_empty());
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
        assert_eq!(payload["provider"], WebSearchProvider::Auto.backend_id());
        assert_eq!(payload["error_type"], "session_cap");
        assert_eq!(payload["retryable"], false);
        assert_eq!(payload["session_max_requests"], 2);
        assert_eq!(payload["request_budget_remaining"], 0);
    }

    #[test]
    fn request_admission_reserves_before_network_and_enforces_cap() {
        let tool = WebSearchTool::new();
        let provider = WebSearchProvider::Auto.backend_id();
        assert!(tool.admit_request("first", provider, Duration::ZERO, 1).unwrap().is_none());

        let response = tool
            .admit_request("second", provider, Duration::ZERO, 1)
            .unwrap()
            .expect("the second concurrent admission must hit the cap");
        assert_eq!(response["error"], "web_search session request cap reached");
        assert_eq!(response["provider"], provider);
        assert_eq!(response["error_type"], "session_cap");
        assert_eq!(response["retryable"], false);
        assert_eq!(response["session_max_requests"], 1);
        assert_eq!(response["request_budget_remaining"], 0);
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
        assert_eq!(payload["provider"], WebSearchProvider::Auto.backend_id());
        assert_eq!(payload["error_type"], "cooldown");
        assert_eq!(payload["retryable"], true);
        assert!(payload["retry_after_ms"].as_u64().unwrap() > 0);
    }

    #[test]
    fn result_payloads_report_the_canonical_backend() {
        let provider = WebSearchProvider::Auto.backend_id();
        let success = results_response(
            "rust",
            provider,
            vec![SearchResult::new(
                "Rust Book".to_owned(),
                "https://doc.rust-lang.org/book/".to_owned(),
                "Ownership and borrowing".to_owned(),
            )],
        );
        let empty = results_response("no matching result", provider, Vec::new());

        assert_eq!(success["provider"], WebSearchProvider::Duckduckgo.backend_id());
        assert_eq!(success["count"], 1);
        assert!(success.get("warning").is_none());
        assert_eq!(empty["provider"], WebSearchProvider::Duckduckgo.backend_id());
        assert_eq!(empty["count"], 0);
        assert_eq!(empty["results"], json!([]));
        assert_eq!(
            empty["warning"],
            "No results were returned. DuckDuckGo may have rate-limited the request or matched nothing. Try a different query, or wait a few seconds and try again."
        );
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
            "provider": WebSearchProvider::Duckduckgo.backend_id(),
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
        assert_eq!(payload["provider"], WebSearchProvider::Auto.backend_id());
        assert_eq!(payload["count"], 1);
        assert_eq!(payload["results"][0]["title"], "Cached Result");
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

    #[test]
    fn classify_search_error_flags_antiban_statuses() {
        for status in [202, 403, 429] {
            let classification = classify_search_error(&format!(
                "DuckDuckGo declined the request (HTTP {status}), likely an anti-bot challenge for this network."
            ));
            assert_eq!(classification.error_type, "antiban_blocked");
            assert!(!classification.retryable);
            assert!(classification.next_action.contains("Do not retry immediately"));
        }
    }

    #[test]
    fn challenge_html_is_antiban_and_non_retryable() {
        let error = match parse_duckduckgo_response("<html><body><div class=\"captcha\">verify</div></body></html>", 10)
        {
            Ok(_) => panic!("challenge HTML must not become an empty success"),
            Err(error) => error,
        };
        let classification = classify_search_error(&error.to_string());
        assert_eq!(classification.error_type, "antiban_blocked");
        assert!(!classification.retryable);
    }

    #[test]
    fn classify_search_error_flags_timeout_as_network_error() {
        let classification = classify_search_error("request timed out after 20s");
        assert_eq!(classification.error_type, "network_error");
        assert!(classification.retryable);
        assert!(classification.next_action.contains("Retry once"));

        let payload =
            search_error_response("rust", WebSearchProvider::Auto.backend_id(), "request timed out after 20s");
        assert_eq!(payload["provider"], WebSearchProvider::Duckduckgo.backend_id());
        assert_eq!(payload["error_type"], "network_error");
        assert_eq!(payload["retryable"], true);
    }

    #[test]
    fn classify_search_error_flags_five_hundred_as_retryable_upstream_error() {
        let classification = classify_search_error("DuckDuckGo returned HTTP 503");
        assert_eq!(classification.error_type, "upstream_error");
        assert!(classification.retryable);
    }

    #[test]
    fn classify_search_error_flags_oversized_response() {
        let classification = classify_search_error("DuckDuckGo response body exceeded the 1000000-byte limit");
        assert_eq!(classification.error_type, "response_too_large");
        assert!(!classification.retryable);
        assert!(classification.next_action.contains("Narrow the query"));
    }
}
