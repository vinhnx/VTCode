//! OpenAI provider error handling and formatting utilities.
//!
//! This module contains error detection, formatting, and recovery logic
//! for OpenAI API interactions.

use reqwest::StatusCode;
use reqwest::header::HeaderMap;

use crate::config::constants::models;

/// Detect if an OpenAI API error indicates the model was not found or is inaccessible.
pub fn is_model_not_found(status: StatusCode, error_text: &str) -> bool {
    status == StatusCode::NOT_FOUND
        || error_text.contains("model_not_found")
        || (error_text.to_ascii_lowercase().contains("does not exist")
            && error_text.to_ascii_lowercase().contains("model"))
}

/// Provide a fallback model when the requested model is unavailable.
pub fn fallback_model_if_not_found(model: &str) -> Option<String> {
    match model {
        m if m == models::openai::GPT_5_2 || m == models::openai::GPT_5_2_ALIAS => {
            Some(models::openai::GPT_5.to_string())
        }
        m if m == models::openai::GPT_5_MINI => Some(models::openai::GPT_5.to_string()),
        m if m == models::openai::GPT_5_NANO => Some(models::openai::GPT_5_MINI.to_string()),
        _ => Some(models::openai::DEFAULT_MODEL.to_string()),
    }
}

/// Format an OpenAI API error with request metadata.
pub fn format_openai_error(
    status: StatusCode,
    body: &str,
    headers: &HeaderMap,
    context: &str,
) -> String {
    let request_id = headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<none>");
    let trimmed_body: String = body.chars().take(2_000).collect();
    if trimmed_body.is_empty() {
        format!(
            "{} (status {}) [request_id={}]",
            context, status, request_id
        )
    } else {
        format!(
            "{} (status {}) [request_id={}] Body: {}",
            context, status, request_id, trimmed_body
        )
    }
}

/// Detect if an error indicates Responses API is not supported for this model/endpoint.
pub fn is_responses_api_unsupported(status: StatusCode, body: &str) -> bool {
    matches!(
        status,
        StatusCode::NOT_FOUND | StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
    ) || body.contains("model does not exist")
        || body.contains("model not found")
        || body.contains("not enabled for the Responses API")
        || body.contains("Invalid API parameter")
        || body.contains("1210")
        || body.contains("invalid_request_error")
}
