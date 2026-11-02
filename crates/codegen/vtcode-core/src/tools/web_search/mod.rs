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
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use vtcode_config::WebSearchConfig;

mod parser;
mod transport;

pub use parser::parse_duckduckgo_html;
use transport::duckduckgo_search;

const MAX_QUERY_CHARS: usize = 500;
const MAX_RESULTS_CAP: usize = 20;

pub(crate) const WEB_SEARCH_DESCRIPTION: &str = "Searches the web with keyless DuckDuckGo HTML search and returns ranked results (title, url, snippet). Accepts { query: string (up to 500 characters), max_results?: number }. Results are cached briefly to reduce rate limiting; cooldown and session caps bound requests. Use web_fetch on a promising result URL to read full content, subject to separate approval. Returns { query, provider, count, cached, results: [{ title, url, snippet }] } or a structured error with error_type, retryable, and next_action.";

/// Shared declaration schema for both builtin registration paths.
pub(crate) fn web_search_parameter_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "A nonblank search query (a topic, question, or keywords), up to 500 characters.",
                "minLength": 1,
                "maxLength": MAX_QUERY_CHARS
            },
            "max_results": {
                "type": "integer",
                "description": "Maximum number of results to return (default: 8, max: 20)."
            }
        },
        "required": ["query"],
        "additionalProperties": false
    })
}

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
mod tests;
