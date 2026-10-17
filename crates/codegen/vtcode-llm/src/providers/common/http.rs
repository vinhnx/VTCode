//! Bounded provider HTTP helpers and endpoint selection.

use crate::error_display;
use crate::provider::LLMError;
use futures::StreamExt;
use serde_json::{Value, json};

/// Caps provider error bodies before they are parsed, logged, or copied into
/// an [`LLMError`]. The diagnostic sanitizer applies a smaller display cap;
/// this boundary cap prevents a hostile provider from causing an unbounded
/// allocation while still leaving room for structured error metadata.
pub(crate) const PROVIDER_ERROR_BODY_MAX_BYTES: usize = 16 * 1024;

/// Read at most [`PROVIDER_ERROR_BODY_MAX_BYTES`] from a provider error body.
///
/// Error responses are untrusted input. Do not replace this with
/// `Response::text()` in error paths: that method buffers the complete body
/// before the diagnostic sanitizer can bound it.
pub(crate) async fn read_provider_error_body(response: reqwest::Response) -> String {
    let mut body = Vec::with_capacity(PROVIDER_ERROR_BODY_MAX_BYTES);
    let mut stream = response.bytes_stream();

    while let Some(chunk_result) = stream.next().await {
        let Ok(chunk) = chunk_result else {
            break;
        };

        let remaining = PROVIDER_ERROR_BODY_MAX_BYTES.saturating_sub(body.len());
        if remaining == 0 {
            break;
        }

        body.extend(chunk.iter().copied().take(remaining));
        if body.len() == PROVIDER_ERROR_BODY_MAX_BYTES {
            break;
        }
    }

    match String::from_utf8(body) {
        Ok(body) => body,
        Err(error) => {
            let valid_up_to = error.utf8_error().valid_up_to();
            let mut bytes = error.into_bytes();
            bytes.truncate(valid_up_to);
            String::from_utf8(bytes).unwrap_or_default()
        }
    }
}

/// Returns the first present header among `names`, as an owned string.
pub(crate) fn extract_header(headers: &reqwest::header::HeaderMap, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find_map(|name| headers.get(*name).and_then(|value| value.to_str().ok()).map(ToOwned::to_owned))
}

/// Parses a JSON response body from an HTTP response, mapping errors to
/// `LLMError::Provider` with a formatted message including the provider name.
pub(crate) async fn parse_json_response(response: reqwest::Response, provider_name: &str) -> Result<Value, LLMError> {
    response.json().await.map_err(|e| LLMError::Provider {
        message: error_display::format_llm_error(provider_name, &format!("failed to parse response: {e}")),
        metadata: None,
    })
}

/// Builds the `/chat/completions` endpoint URL for an OpenAI-compatible base URL,
/// tolerating a trailing slash on the configured base.
#[inline]
pub(crate) fn chat_completions_url(base_url: &str) -> String {
    format!("{}/chat/completions", base_url.trim_end_matches('/'))
}

/// Sends an OpenAI-compatible chat-completions POST, mapping transport failures
/// to `LLMError::Network` with consistent formatting.
///
/// The caller supplies a `RequestBuilder` with the endpoint, authentication, and
/// any provider-specific headers already applied; this helper only attaches the
/// JSON payload, dispatches the request, and normalizes network errors.
pub(crate) async fn send_chat_completions(
    request: reqwest::RequestBuilder,
    payload: &Value,
    provider_name: &str,
) -> Result<reqwest::Response, LLMError> {
    request
        .json(payload)
        .send()
        .await
        .map_err(|error| crate::providers::error_handling::format_network_error(provider_name, &error))
}

pub(crate) fn override_base_url(
    default_base_url: &str,
    base_url: Option<String>,
    env_var_name: Option<&str>,
) -> String {
    if let Some(url) = base_url {
        let trimmed = url.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    if let Some(var_name) = env_var_name
        && let Ok(value) = std::env::var(var_name)
    {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }

    default_base_url.to_string()
}
