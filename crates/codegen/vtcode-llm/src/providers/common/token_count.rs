//! Exact prompt token-count payloads, requests, and response parsing.

use super::read_provider_error_body;
use crate::error_display;
use crate::provider::LLMError;
use serde_json::{Value, json};

/// Remove generation-only controls from a payload before exact prompt-token counting.
/// Token-count endpoints generally require only prompt-side fields.
pub fn strip_generation_controls_for_token_count(payload: &mut Value) {
    let Some(root) = payload.as_object_mut() else {
        return;
    };

    for key in [
        "stream",
        "temperature",
        "top_p",
        "frequency_penalty",
        "presence_penalty",
        "stop",
        "max_tokens",
        "max_output_tokens",
        "n",
        "seed",
        "tool_choice",
        "parallel_tool_config",
        "response_format",
        "reasoning_effort",
        "metadata",
        "prompt_cache_key",
    ] {
        root.remove(key);
    }
}

#[inline]
fn parse_u32_value(value: &Value) -> Option<u32> {
    value
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .or_else(|| {
            value
                .as_i64()
                .and_then(|n| u64::try_from(n).ok())
                .and_then(|n| u32::try_from(n).ok())
        })
        .or_else(|| value.as_str().and_then(|s| s.parse::<u32>().ok()))
}

#[inline]
fn value_at_path<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    let mut cursor = value;
    for segment in path {
        cursor = cursor.get(*segment)?;
    }
    Some(cursor)
}

/// Parse prompt/input token counts from common response shapes.
pub fn parse_prompt_tokens_from_count_response(value: &Value) -> Option<u32> {
    const CANDIDATE_PATHS: &[&[&str]] = &[
        &["prompt_tokens"],
        &["input_tokens"],
        &["token_count"],
        &["usage", "prompt_tokens"],
        &["usage", "input_tokens"],
        &["data", "prompt_tokens"],
        &["data", "input_tokens"],
        &["data", "token_count"],
        &["usage", "total_tokens"],
        &["data", "total_tokens"],
        &["total_tokens"],
    ];

    for path in CANDIDATE_PATHS {
        if let Some(parsed) = value_at_path(value, path).and_then(parse_u32_value) {
            return Some(parsed);
        }
    }
    None
}

/// Execute an exact token-count request when provider endpoint is available.
/// Returns `Ok(None)` when endpoint appears unsupported.
pub async fn execute_token_count_request(
    request_builder: reqwest::RequestBuilder,
    payload: &Value,
    provider_name: &str,
) -> Result<Option<Value>, LLMError> {
    let response = request_builder.json(payload).send().await.map_err(|e| {
        let message = error_display::format_llm_error(provider_name, &format!("Token-count network error: {e}"));
        LLMError::Network { message, metadata: None }
    })?;

    let status = response.status();
    if matches!(
        status,
        reqwest::StatusCode::BAD_REQUEST
            | reqwest::StatusCode::UNPROCESSABLE_ENTITY
            | reqwest::StatusCode::NOT_FOUND
            | reqwest::StatusCode::METHOD_NOT_ALLOWED
            | reqwest::StatusCode::NOT_IMPLEMENTED
    ) {
        return Ok(None);
    }

    if !status.is_success() {
        let body = read_provider_error_body(response).await;
        let message =
            error_display::format_llm_error(provider_name, &format!("Token-count request failed ({status}): {body}"));
        return Err(LLMError::Provider { message, metadata: None });
    }

    let value = response.json::<Value>().await.map_err(|e| {
        let message =
            error_display::format_llm_error(provider_name, &format!("Failed to parse token-count response: {e}"));
        LLMError::Provider { message, metadata: None }
    })?;

    Ok(Some(value))
}
