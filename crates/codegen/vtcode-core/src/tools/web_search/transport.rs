//! Bounded DuckDuckGo HTTP requests and same-host redirect policy.

use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use futures::stream::StreamExt;
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue, USER_AGENT};
use url::Url;

use super::SearchResult;
use super::parser::parse_duckduckgo_response;

const MIN_TIMEOUT_SECS: u64 = 1;
const MAX_TIMEOUT_SECS: u64 = 60;

/// Browser-like user agent. DuckDuckGo's HTML endpoint blocks obvious bots, so a
/// realistic UA reduces (but does not eliminate) the chance of being challenged.
const BROWSER_USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0 Safari/537.36";

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

pub(super) async fn duckduckgo_search(query: &str, max_results: usize, timeout_secs: u64) -> Result<Vec<SearchResult>> {
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

#[cfg(test)]
mod tests;
