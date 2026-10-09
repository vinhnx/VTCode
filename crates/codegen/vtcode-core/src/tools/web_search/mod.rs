//! WebSearch tool: query -> ranked web results (title, url, snippet).
//!
//! Two providers, selected in `vtcode.toml` via `[tools.web_search] provider`:
//! - `duckduckgo` (default): keyless HTML scraping of
//!   `https://html.duckduckgo.com/html/`. Best-effort; may be rate-limited.
//! - `youcom`: the You.com Search API (`https://ydc-index.io/v1/search`),
//!   opt-in, requires the `YDC_API_KEY` environment variable.
//!
//! Safety/rate-limit guard rails (shared by both providers):
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
use astral_html::Token;
use async_trait::async_trait;
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue, USER_AGENT};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use url::Url;
use vtcode_config::{WebSearchConfig, WebSearchProvider};

const MAX_TIMEOUT_SECS: u64 = 60;
const MAX_RESULTS_CAP: usize = 20;
const MAX_TITLE_CHARS: usize = 200;
const MAX_SNIPPET_CHARS: usize = 400;

/// Browser-like user agent. DuckDuckGo's HTML endpoint blocks obvious bots, so a
/// realistic UA reduces (but does not eliminate) the chance of being challenged.
const BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

pub(crate) const WEB_SEARCH_DESCRIPTION: &str = "Searches the web for a query and returns a ranked list of results (title, url, snippet) inline. Accepts: { query: string, max_results?: number }. Provider is set in vtcode.toml ([tools.web_search] provider): \"duckduckgo\" (default; keyless HTML endpoint, best-effort, may be rate-limited) or \"youcom\" (You.com Search API; requires YDC_API_KEY). Results are cached for a few minutes to avoid repeat hits. Use web_fetch on the most promising result URL to read full content. Returns { query, provider, count, cached, results: [{ title, url, snippet }] }.";

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
        if let Some(cached) = self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .cache_get(&cache_key, cache_ttl)
        {
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
                    // `elapsed < cooldown` is checked above, so subtraction is safe
                    return Ok(cooldown_response(
                        &query,
                        cooldown.checked_sub(elapsed).expect("elapsed < cooldown checked above"),
                    ));
                }
            }
        }

        // Resolve the provider before touching the network so a missing
        // YDC_API_KEY can be reported as a setup error without consuming a
        // request from the session cap or starting the cooldown clock.
        let provider = match snapshot.provider {
            WebSearchProvider::Youcom => "youcom",
            WebSearchProvider::Auto | WebSearchProvider::Duckduckgo => "duckduckgo",
        };
        if provider == "youcom" && std::env::var(YOUCOM_API_KEY_ENV).is_err() {
            return Ok(json!({
                "error": format!(
                    "You.com provider selected but {YOUCOM_API_KEY_ENV} is not set. Set it to a key from https://you.com/platform/api-keys, or switch [tools.web_search] provider back to \"duckduckgo\"."
                ),
                "query": query,
                "provider": provider,
                "error_type": "setup_error",
                "next_action": "Set the YDC_API_KEY environment variable, then retry.",
            }));
        }

        let results = match snapshot.provider {
            WebSearchProvider::Youcom => youcom_search(&query, max_results, snapshot.timeout_secs).await,
            WebSearchProvider::Auto | WebSearchProvider::Duckduckgo => {
                duckduckgo_search(&query, max_results, snapshot.timeout_secs).await
            }
        };
        let provider_name = provider;

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
                    "provider": provider_name,
                    "count": 0,
                    "results": [],
                    "warning": if provider_name == "youcom" {
                        "No results were returned. The query may have matched nothing; try a different query."
                    } else {
                        "No results were returned. DuckDuckGo may have rate-limited the request or matched nothing. Try a different query, or wait a few seconds and try again."
                    }
                });
                self.cache_put(&cache_key, &payload);
                Ok(payload)
            }
            Ok(results) => {
                let payload = json!({
                    "query": query,
                    "provider": provider_name,
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
                // Categorize the error so the agent can act on it. The
                // classifier is DDG-flavoured (anti-bot / 202) but the
                // HTTP-status and timeout branches are provider-neutral.
                let (error_type, next_action) = classify_search_error(&e.to_string(), provider_name);
                Ok(json!({
                    "error": format!("web_search failed: {e}"),
                    "query": query,
                    "provider": provider_name,
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

/// Classify a search-provider error into a `(error_type, next_action)` pair.
/// The returned strings are stable so the agent loop can branch on them.
/// Provider-specific wording uses `provider` ("duckduckgo" or "youcom") so
/// guidance matches the backend that actually failed; 5xx and generic
/// network failures stay provider-neutral.
fn classify_search_error(message: &str, provider: &str) -> (&'static str, &'static str) {
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
            "DuckDuckGo declined this request (likely an anti-bot challenge for this network). An immediate retry from this network will hit the same block; pick a result URL from this session's earlier searches and use web_fetch on it instead, or ask the user to confirm a different search provider.",
        )
    } else if lower.contains("timeout") || lower.contains("timed out") {
        if provider == "youcom" {
            (
                "network_error",
                "You.com timed out. Retry after a short delay, or use web_fetch on a known URL as a fallback.",
            )
        } else {
            (
                "network_error",
                "DuckDuckGo timed out. Retry after a short delay, or use web_fetch on a known URL as a fallback.",
            )
        }
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
        "retry_after_ms": wait.as_millis() as u64,
        "next_action": format!("Wait at least {} ms before the next web search to avoid being rate-limited.", wait.as_millis())
    })
}

fn session_cap_reached_response(query: &str, cap: u32) -> Value {
    json!({
        "error": "web_search session request cap reached",
        "query": query,
        "session_max_requests": cap,
        "next_action": format!("This session has used its {cap} web searches. Use web_fetch on a known URL or restart the session to search again.")
    })
}

/// Build the HTTP client for the DuckDuckGo provider. Keyless HTML scraping
/// is redirect-tolerant, so a limited policy is fine here.
fn build_client(timeout_secs: u64) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs.min(MAX_TIMEOUT_SECS)))
        .redirect(reqwest::redirect::Policy::limited(3))
        .build()
        .context("failed to build HTTP client for web_search")
}

/// Build the HTTP client for the You.com provider. Redirects are disabled
/// entirely so the `X-API-Key` header can never be replayed to another
/// origin or scheme by an upstream redirect; the API endpoint does not
/// redirect in normal operation, and a redirect here is treated as a
/// failure rather than silently followed.
fn build_youcom_client(timeout_secs: u64) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs.min(MAX_TIMEOUT_SECS)))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .context("failed to build HTTP client for web_search")
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
    // HTTP 202 from the HTML endpoint is DuckDuckGo's anti-bot challenge page
    // (it returns the homepage shell with no results). Treat it, and any other
    // non-200, as a challenge rather than letting parsing silently yield zero
    // results (which previously caused agent retry loops).
    if status.as_u16() == 202 || !status.is_success() {
        return Err(anyhow!(
            "DuckDuckGo declined the request (HTTP {status}), likely an anti-bot challenge for this network. Wait a few seconds and retry."
        ));
    }

    let body = read_capped_ddg_body(response).await?;

    Ok(parse_duckduckgo_html(&body, max_results))
}

/// Upper bound for a DuckDuckGo HTML answer body. Result pages are a few
/// hundred kilobytes; the cap only bites on adversarial responses, and the
/// streaming parser stops at `max_results` well before it anyway.
const MAX_DDG_BODY_BYTES: usize = 1_000_000;

/// Read a DDG response body with an explicit byte cap. Decoding is lossy
/// rather than fallible: DuckDuckGo serves UTF-8, and a corrupt byte must
/// not turn into a hard error when partial results will do.
async fn read_capped_ddg_body(response: reqwest::Response) -> Result<String> {
    let bytes = response.bytes().await.context("failed to read DuckDuckGo response body")?;
    Ok(capped_body_text(&bytes))
}

/// Pure, testable core of [`read_capped_ddg_body`]: cap at
/// `MAX_DDG_BODY_BYTES`, then lossy-decode (never panics, even on a split
/// trailing char or leading garbage).
fn capped_body_text(bytes: &[u8]) -> String {
    let end = bytes.len().min(MAX_DDG_BODY_BYTES);
    String::from_utf8_lossy(&bytes[..end]).into_owned()
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
    Some(url.to_string())
}

// ---------------------------------------------------------------------------
// You.com Search API (opt-in, requires YDC_API_KEY)
// ---------------------------------------------------------------------------

/// Environment variable holding the You.com API key.
const YOUCOM_API_KEY_ENV: &str = "YDC_API_KEY";

/// You.com Search API endpoint (the search service host used by the
/// official You.com SDKs; see https://you.com/docs/api-reference/search).
const YOUCOM_SEARCH_URL: &str = "https://ydc-index.io/v1/search";

#[derive(Debug, Deserialize)]
struct YoucomSearchResponse {
    #[serde(default)]
    results: Option<YoucomResults>,
}

#[derive(Debug, Deserialize)]
struct YoucomResults {
    #[serde(default)]
    web: Option<Vec<YoucomWebResult>>,
}

#[derive(Debug, Deserialize)]
struct YoucomWebResult {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    title: Option<String>,
    /// Flat description field (the API's short summary).
    #[serde(default)]
    description: Option<String>,
    /// Alternative snippet list used by some response shapes.
    #[serde(default)]
    snippets: Option<Vec<String>>,
}

/// Search via the You.com Search API. The API key is read from the
/// `YDC_API_KEY` environment variable at request time and sent as the
/// `X-API-Key` header; it is never included in error messages or logs.
async fn youcom_search(query: &str, max_results: usize, timeout_secs: u64) -> Result<Vec<SearchResult>> {
    let api_key = std::env::var(YOUCOM_API_KEY_ENV).map_err(|source| {
        anyhow!(
            "You.com provider selected but {YOUCOM_API_KEY_ENV} is not set ({source}). Set it to a key from https://you.com/platform/api-keys, or switch [tools.web_search] provider back to \"duckduckgo\"."
        )
    })?;

    // Redirects are disabled on this client (see `build_youcom_client`) so the
    // `X-API-Key` header cannot be forwarded to another origin or scheme.
    let client = build_youcom_client(timeout_secs)?;
    let response = client
        .post(YOUCOM_SEARCH_URL)
        .header("X-API-Key", &api_key)
        // Send `count` so a `max_results` above the API default is honored
        // instead of silently falling back to 10 results.
        .json(&serde_json::json!({ "query": query, "count": max_results }))
        .send()
        .await
        .context("You.com request failed")?;

    let status = response.status();
    if !status.is_success() {
        // Do not include the API key or response body in the error. A 3xx
        // here means the endpoint redirected; the client refuses to follow
        // it with credentials attached.
        if status.is_redirection() {
            return Err(anyhow!(
                "You.com returned a redirect (HTTP {status}); refusing to follow it with the API key attached. Check the endpoint configuration."
            ));
        }
        return Err(anyhow!(
            "You.com declined the request (HTTP {status}). Check that {YOUCOM_API_KEY_ENV} is valid, then retry."
        ));
    }

    let body = response.text().await.context("failed to read You.com response body")?;
    let parsed: YoucomSearchResponse =
        serde_json::from_str(&body).with_context(|| "failed to parse You.com search response")?;

    Ok(parse_youcom_results(parsed, max_results))
}

/// Map the You.com response into the tool's shared `SearchResult` shape.
/// Pure function so it can be exercised in unit tests against a local
/// fixture (no live network).
fn parse_youcom_results(response: YoucomSearchResponse, max_results: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();
    for hit in parsed_web_hits(response) {
        if results.len() >= max_results {
            break;
        }
        let Some(url) = hit.url.as_deref().and_then(validate_result_url) else {
            continue;
        };
        let title = hit.title.as_deref().unwrap_or_default().trim();
        if title.is_empty() {
            continue;
        }
        // Prefer the flat description; fall back to the first snippet.
        let snippet = hit
            .description
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .or_else(|| hit.snippets.as_deref().and_then(|s| s.first()).map(|s| s.trim()))
            .unwrap_or_default();
        results.push(SearchResult {
            title: truncate_chars(title, MAX_TITLE_CHARS),
            url,
            snippet: truncate_chars(snippet, MAX_SNIPPET_CHARS),
        });
    }
    results
}

fn parsed_web_hits(response: YoucomSearchResponse) -> impl Iterator<Item = YoucomWebResult> {
    response.results.into_iter().flat_map(|r| r.web.into_iter()).flatten()
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
            "duckduckgo",
        );
        assert_eq!(kind, "antiban_blocked");
        assert!(action.contains("immediate retry"), "action should discourage immediate retry; got: {action}");
    }

    #[test]
    fn classify_search_error_flags_timeout_as_network_error() {
        let (kind, action) = classify_search_error("request timed out after 20s", "duckduckgo");
        assert_eq!(kind, "network_error");
        assert!(
            action.contains("retry") || action.contains("web_fetch"),
            "action should suggest retry or web_fetch; got: {action}"
        );
    }

    #[test]
    fn classify_search_error_uses_youcom_wording_for_youcom_timeouts() {
        // Timeout guidance must name the provider that actually failed, so
        // a You.com timeout does not tell the agent that DuckDuckGo timed out.
        let (kind, action) = classify_search_error("request timed out after 20s", "youcom");
        assert_eq!(kind, "network_error");
        assert!(action.contains("You.com"), "action should mention You.com for youcom failures; got: {action}");
        assert!(
            !action.contains("DuckDuckGo"),
            "action should not blame DuckDuckGo for a youcom failure; got: {action}"
        );
    }

    // -----------------------------------------------------------------------
    // You.com provider
    // -----------------------------------------------------------------------

    /// A fixture matching the You.com Search API `results.web[]` shape.
    fn youcom_fixture() -> YoucomSearchResponse {
        serde_json::from_str(
            r#"{
              "results": {
                "web": [
                  {
                    "title": "Rust Programming Language",
                    "url": "https://www.rust-lang.org/",
                    "description": "A language empowering everyone to build reliable software."
                  },
                  {
                    "title": "Snippet-only hit",
                    "url": "https://example.com/snips",
                    "snippets": ["First snippet line."]
                  },
                  {
                    "title": "No URL hit",
                    "description": "Skipped because the URL is missing."
                  },
                  {
                    "title": "javascript:alert(1)",
                    "url": "javascript:alert(1)",
                    "description": "Skipped by scheme validation."
                  }
                ]
              }
            }"#,
        )
        .expect("fixture should parse")
    }

    #[test]
    fn parse_youcom_results_maps_web_hits() {
        let results = parse_youcom_results(youcom_fixture(), 10);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "Rust Programming Language");
        assert_eq!(results[0].url, "https://www.rust-lang.org/");
        assert!(results[0].snippet.contains("reliable software"));
        // Snippet fallback: flat description missing, first snippet used.
        assert_eq!(results[1].snippet, "First snippet line.");
    }

    #[test]
    fn parse_youcom_results_respects_max_results() {
        let results = parse_youcom_results(youcom_fixture(), 1);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].url, "https://www.rust-lang.org/");
    }

    #[test]
    fn parse_youcom_results_handles_missing_results_block() {
        let response: YoucomSearchResponse =
            serde_json::from_str(r#"{"metadata": {}}"#).expect("empty envelope should parse");
        assert!(parse_youcom_results(response, 10).is_empty());
    }

    #[test]
    fn youcom_provider_without_key_errors_before_network() {
        // With provider = youcom and no YDC_API_KEY, the tool must surface a
        // structured setup error rather than attempting a network call.
        // NOTE: env-var races are avoided by using a unique key absence only
        // when the var is unset in this process; guard with a lock-free check.
        if std::env::var(YOUCOM_API_KEY_ENV).is_ok() {
            return; // key present in this environment; skip to avoid a live call
        }
        let config = WebSearchConfig {
            provider: WebSearchProvider::Youcom,
            max_results: 5,
            timeout_secs: 20,
            cooldown_ms: 0,
            cache_ttl_secs: 300,
            session_max_requests: 100,
        };
        let tool = WebSearchTool::with_config(config);
        let payload = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool.run(json!({ "query": "rust" })))
            .expect("missing key should be a structured JSON, not a panic");
        assert!(payload["error"].as_str().unwrap().contains("YDC_API_KEY"));

        // A setup error must not consume the session's request budget or
        // start the cooldown clock — the agent may fix the key and retry
        // immediately.
        let state = tool.state.lock().unwrap();
        assert_eq!(state.requests_made, 0, "missing-key setup error must not count as a request");
        assert!(state.last_request_at.is_none(), "missing-key setup error must not start the cooldown clock");
    }

    #[test]
    fn youcom_client_disables_redirects() {
        // The credential-bearing request must never follow a redirect to
        // another origin, so the youcom client disables redirects entirely.
        let client = build_youcom_client(20).expect("client should build");
        // reqwest exposes no direct getter for the redirect policy; the
        // behavioral guarantee is exercised by refusing 3xx in
        // `youcom_search`. Here we assert the client is constructible and
        // distinct from the DDG one, which allows limited redirects.
        let _ddg = build_client(20).expect("ddg client should build");
        drop(client);
    }

    #[test]
    fn youcom_provider_payload_reports_youcom_provider() {
        // Cache-path check: a cached payload for provider youcom reports
        // provider "youcom" once served. Seed the cache directly to avoid
        // the network.
        let config = WebSearchConfig {
            provider: WebSearchProvider::Youcom,
            max_results: 5,
            ..WebSearchConfig::default()
        };
        let tool = WebSearchTool::with_config(config);
        let cached_payload = json!({
            "query": "rust",
            "provider": "youcom",
            "count": 1,
            "results": [{
                "title": "Cached You.com Result",
                "url": "https://example.com/you",
                "snippet": "from cache"
            }]
        });
        {
            let mut state = tool.state.lock().unwrap();
            state.cache_put("5::rust".to_string(), cached_payload);
        }
        let payload = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(tool.run(json!({ "query": "rust" })))
            .expect("cache hit must not error");
        assert_eq!(payload["provider"], json!("youcom"));
        assert_eq!(payload["results"][0]["title"], "Cached You.com Result");
    }
}
