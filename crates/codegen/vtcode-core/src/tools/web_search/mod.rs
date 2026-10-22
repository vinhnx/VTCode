//! Octen web search: ranked excerpts, with bounded paid requests and no full-page fetch.
use super::traits::Tool;
use crate::config::constants::tools;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use reqwest::header::HeaderValue;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use url::Url;
use vtcode_auth::AuthCredentialsStoreMode;
use vtcode_config::WebSearchConfig;
use vtcode_config::api_keys::{ResolvedCredential, resolve_octen_credential_with_mode};

const SEARCH_URL: &str = "https://api.octen.ai/search";
const MAX_TIMEOUT_SECS: u64 = 60;
const MAX_RESULTS_CAP: usize = 20;
const MAX_TITLE_CHARS: usize = 200;
const MAX_SNIPPET_CHARS: usize = 400;
const MAX_BODY_BYTES: usize = 1_000_000;

pub(crate) const WEB_SEARCH_DESCRIPTION: &str = "Search the web with Octen and return ranked excerpts (title, url, snippet). Accepts { query: string, max_results?: number }. Requires /secret add octen or OCTEN_API_KEY. Results are cached briefly; paid requests have a cooldown and session cap. Results are excerpts only; full-page web_fetch requires separate authorization. Returns { query, provider: octen, count, cached, results: [{ title, url, snippet }] }.";

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

/// Shared per-session cache and outbound quota. Credentials are resolved anew per search.
#[derive(Clone, Default)]
pub struct WebSearchTool {
    config: Arc<Mutex<WebSearchConfig>>,
    state: Arc<Mutex<SessionState>>,
    storage_mode: AuthCredentialsStoreMode,
}

impl WebSearchTool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(config: WebSearchConfig) -> Self {
        Self::with_config_and_storage(config, AuthCredentialsStoreMode::default())
    }

    pub fn with_config_and_storage(config: WebSearchConfig, storage_mode: AuthCredentialsStoreMode) -> Self {
        Self {
            config: Arc::new(Mutex::new(config)),
            state: Arc::default(),
            storage_mode,
        }
    }

    pub fn set_config(&self, config: WebSearchConfig) {
        if let Ok(mut guard) = self.config.lock() {
            *guard = config;
        }
    }

    pub fn reset(&self) {
        if let Ok(mut guard) = self.state.lock() {
            *guard = SessionState::default();
        }
    }

    async fn run(&self, raw_args: Value) -> Result<Value> {
        self.run_with_credentials(raw_args, || resolve_octen_credential_with_mode(self.storage_mode))
            .await
    }

    async fn run_with_credentials(
        &self,
        raw_args: Value,
        resolve: impl FnOnce() -> Result<Option<ResolvedCredential>>,
    ) -> Result<Value> {
        let args: WebSearchArgs =
            serde_json::from_value(raw_args).context("Invalid arguments for web_search. Provide a 'query' string.")?;
        let query = args.query.trim();
        if query.is_empty() || query.chars().count() > 500 {
            return Err(anyhow!("web_search requires a nonblank query of at most 500 characters"));
        }
        let snapshot = self
            .config
            .lock()
            .map_err(|error| anyhow!("web_search config lock poisoned: {error}"))?
            .clone();
        let max_results = args.max_results.unwrap_or(snapshot.max_results).clamp(1, MAX_RESULTS_CAP);
        let cache_key = format!("{max_results}::{query}");
        if let Some(mut cached) = self
            .state
            .lock()
            .map_err(|error| anyhow!("web_search state lock poisoned: {error}"))?
            .cache_get(&cache_key, Duration::from_secs(snapshot.cache_ttl_secs))
        {
            cached["cached"] = json!(true);
            return Ok(cached);
        }
        // No prompt, migration, or paid credential validation at this boundary.
        let credential = match resolve() {
            Ok(Some(credential)) => credential,
            Ok(None) => {
                return Ok(search_error(
                    query,
                    "setup_error",
                    "Octen credentials are missing",
                    "Run /secret add octen or set OCTEN_API_KEY.",
                ));
            }
            Err(_) => {
                return Ok(search_error(
                    query,
                    "credential_storage_error",
                    "Cannot read Octen credentials from secure storage",
                    "Check the configured credential storage backend and /secret status octen.",
                ));
            }
        };
        let client = build_client(snapshot.timeout_secs)?;
        let Some(key) = credential.secret.as_deref() else {
            return Ok(search_error(
                query,
                "setup_error",
                "Octen credentials are missing",
                "Run /secret add octen or set OCTEN_API_KEY.",
            ));
        };
        let request = match build_request(&client, SEARCH_URL, query, max_results, key) {
            Ok(request) => request,
            Err(_) => {
                return Ok(search_error(
                    query,
                    "setup_error",
                    "Invalid Octen credential format",
                    "Replace the key with /secret add octen or set OCTEN_API_KEY.",
                ));
            }
        };
        self.send_search(request, query, max_results, &cache_key, &snapshot).await
    }

    async fn send_search(
        &self,
        request: reqwest::RequestBuilder,
        query: &str,
        max_results: usize,
        cache_key: &str,
        config: &WebSearchConfig,
    ) -> Result<Value> {
        // Reserve immediately before send, atomically with admission. Concurrent calls,
        // cancellations, and failed sends cannot exceed the cap or escape accounting.
        {
            let mut state = self
                .state
                .lock()
                .map_err(|error| anyhow!("web_search state lock poisoned: {error}"))?;
            if state.requests_made >= config.session_max_requests {
                return Ok(json!({"error": "web_search session request cap reached", "query": query,
                    "session_max_requests": config.session_max_requests,
                    "next_action": "The session search budget is exhausted. Full-page fetch still requires separate authorization."}));
            }
            if let Some(last) = state.last_request_at {
                let wait = Duration::from_millis(config.cooldown_ms).saturating_sub(last.elapsed());
                if !wait.is_zero() {
                    return Ok(json!({"error": "web_search cooldown active", "query": query,
                        "retry_after_ms": wait.as_millis() as u64,
                        "next_action": "Wait for the cooldown before another search."}));
                }
            }
            state.requests_made += 1;
            state.last_request_at = Some(Instant::now());
        }
        match request_results(request, max_results).await {
            Ok(results) => {
                let payload = json!({"query": query, "provider": "octen", "count": results.len(), "cached": false,
                    "results": results.into_iter().map(|r| json!({"title": r.title, "url": r.url, "snippet": r.snippet})).collect::<Vec<_>>()});
                if payload["count"].as_u64().is_some_and(|count| count > 0) {
                    self.state
                        .lock()
                        .map_err(|error| anyhow!("web_search state lock poisoned: {error}"))?
                        .cache_put(cache_key.to_owned(), payload.clone());
                }
                Ok(payload)
            }
            Err(failure) => Ok(search_error(query, failure.kind, failure.message, failure.action)),
        }
    }
}

fn search_error(query: &str, kind: &str, message: &str, action: &str) -> Value {
    json!({"error": message, "query": query, "provider": "octen", "error_type": kind, "next_action": action})
}

fn build_client(timeout_secs: u64) -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .timeout(Duration::from_secs(timeout_secs.clamp(1, MAX_TIMEOUT_SECS)))
        .build()
        .context("failed to build Octen search client")
}

fn build_request(
    client: &reqwest::Client,
    endpoint: &str,
    query: &str,
    count: usize,
    key: &str,
) -> Result<reqwest::RequestBuilder> {
    let mut header = HeaderValue::from_str(key)
        .map_err(|_invalid_header| anyhow!("Invalid Octen credential format; replace it with /secret add octen"))?;
    header.set_sensitive(true);
    Ok(client.post(endpoint).header("x-api-key", header).json(&json!({
        "query": query, "count": count, "highlight": {"enable": true}, "full_content": {"enable": false}, "format": "text"
    })))
}

#[derive(Debug)]
struct SearchFailure {
    kind: &'static str,
    message: &'static str,
    action: &'static str,
}

impl SearchFailure {
    fn malformed() -> Self {
        Self {
            kind: "malformed_response",
            message: "Octen returned a malformed search response",
            action: "Report the incompatible response; no automatic retry was made.",
        }
    }
    fn network(error: reqwest::Error) -> Self {
        // Never emit transport debug values, upstream bodies, headers, or echoed credentials.
        Self {
            kind: if error.is_timeout() { "timeout" } else { "network_error" },
            message: if error.is_timeout() {
                "Octen search timed out"
            } else {
                "Octen network request failed"
            },
            action: "Check connectivity before explicitly retrying; each outbound attempt consumes session quota.",
        }
    }
    fn status(code: i64) -> Self {
        match code {
            401 | 403 => Self {
                kind: "credentials_rejected",
                message: "Octen rejected authorization or account access",
                action: "Check /secret status octen, the key, and account balance. Replace the key with /secret add octen if needed.",
            },
            429 => Self {
                kind: "rate_limited",
                message: "Octen rate limit reached",
                action: "Wait before explicitly retrying.",
            },
            300..=399 => Self {
                kind: "redirect_rejected",
                message: "Octen returned a redirect; credentials were not forwarded",
                action: "Report the endpoint redirect; no fallback was attempted.",
            },
            _ => Self {
                kind: "api_error",
                message: "Octen reported an API failure",
                action: "Check Octen service/account status before explicitly retrying.",
            },
        }
    }
}

#[derive(Deserialize)]
struct OctenResponse {
    code: i64,
    data: Option<OctenData>,
}
#[derive(Deserialize)]
struct OctenData {
    results: Vec<OctenHit>,
}
#[derive(Deserialize)]
struct OctenHit {
    title: String,
    url: String,
    #[serde(default)]
    highlight: String,
}

async fn request_results(
    request: reqwest::RequestBuilder,
    max_results: usize,
) -> std::result::Result<Vec<SearchResult>, SearchFailure> {
    let mut response = request.send().await.map_err(SearchFailure::network)?;
    if !response.status().is_success() {
        return Err(SearchFailure::status(i64::from(response.status().as_u16())));
    }
    let oversized = || SearchFailure {
        kind: "response_too_large",
        message: "Octen response exceeded the size limit",
        action: "Use a narrower query; no partial response was cached.",
    };
    if response.content_length().is_some_and(|n| n > MAX_BODY_BYTES as u64) {
        return Err(oversized());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(SearchFailure::network)? {
        if chunk.len() > MAX_BODY_BYTES.saturating_sub(body.len()) {
            return Err(oversized());
        }
        body.extend_from_slice(&chunk);
    }
    parse_response(&body, max_results)
}

fn parse_response(body: &[u8], max_results: usize) -> std::result::Result<Vec<SearchResult>, SearchFailure> {
    let parsed: OctenResponse =
        serde_json::from_slice(body).map_err(|_malformed_payload| SearchFailure::malformed())?;
    if parsed.code != 0 {
        return Err(SearchFailure::status(parsed.code));
    }
    let data = parsed.data.ok_or_else(SearchFailure::malformed)?;
    let had_hits = !data.results.is_empty();
    let mut results = Vec::new();
    for hit in data.results {
        let Some(url) = validate_result_url(&hit.url) else {
            continue;
        };
        if hit.title.trim().is_empty() {
            continue;
        }
        results.push(SearchResult::new(
            truncate_chars(&hit.title, MAX_TITLE_CHARS),
            url,
            truncate_chars(&hit.highlight, MAX_SNIPPET_CHARS),
        ));
        if results.len() >= max_results {
            break;
        }
    }
    if had_hits && results.is_empty() {
        return Err(SearchFailure::malformed());
    }
    Ok(results)
}

fn validate_result_url(value: &str) -> Option<String> {
    let parsed = Url::parse(value).ok()?;
    if !matches!(parsed.scheme(), "http" | "https")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
    {
        return None;
    }
    Some(parsed.to_string())
}

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
