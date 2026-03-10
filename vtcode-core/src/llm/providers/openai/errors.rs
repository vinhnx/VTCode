//! OpenAI provider error handling and formatting utilities.
//!
//! This module contains error detection, formatting, and recovery logic
//! for OpenAI API interactions.

use reqwest::StatusCode;
use reqwest::header::HeaderMap;

use crate::config::constants::models;

/// Detect if an OpenAI API error indicates the model was not found or is inaccessible.
pub fn is_model_not_found(status: StatusCode, error_text: &str) -> bool {
    if !matches!(
        status,
        StatusCode::NOT_FOUND | StatusCode::BAD_REQUEST | StatusCode::UNPROCESSABLE_ENTITY
    ) {
        return false;
    }

    let lower = error_text.to_ascii_lowercase();
    lower.contains("model_not_found")
        || (lower.contains("model") && lower.contains("does not exist"))
        || (lower.contains("model") && lower.contains("not found"))
        || lower.contains("unknown model")
}

/// Provide a fallback model when the requested model is unavailable.
pub fn fallback_model_if_not_found(model: &str) -> Option<String> {
    match model {
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
    let lower = body.to_ascii_lowercase();

    (status == StatusCode::NOT_FOUND && lower.trim().is_empty())
        || lower.contains("not enabled for the responses api")
        || lower.contains("responses api")
            && (lower.contains("unsupported") || lower.contains("not supported"))
        || lower.contains("invalid api parameter")
        || lower.contains("unsupported parameter")
        || lower.contains("1210")
        || lower.contains("invalid_request_error")
}

#[cfg(test)]
mod tests {
    use super::{is_model_not_found, is_responses_api_unsupported};
    use reqwest::StatusCode;

    #[test]
    fn model_not_found_requires_model_specific_body() {
        assert!(!is_model_not_found(StatusCode::NOT_FOUND, ""));
        assert!(is_model_not_found(StatusCode::NOT_FOUND, "model_not_found"));
        assert!(is_model_not_found(
            StatusCode::BAD_REQUEST,
            "The requested model does not exist"
        ));
    }

    #[test]
    fn responses_api_unsupported_keeps_blank_404_fallback() {
        assert!(is_responses_api_unsupported(StatusCode::NOT_FOUND, ""));
        assert!(is_responses_api_unsupported(
            StatusCode::BAD_REQUEST,
            "This endpoint is not enabled for the Responses API"
        ));
        assert!(!is_responses_api_unsupported(
            StatusCode::NOT_FOUND,
            "model_not_found"
        ));
    }
}
