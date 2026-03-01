//! Centralized error handling for LLM providers
//! Eliminates duplicate error handling code across providers

use crate::llm::error_display;
use crate::llm::provider::{LLMError, LLMErrorMetadata};
use reqwest::Response;
use serde_json::Value;

/// HTTP status codes for common error types
pub const STATUS_UNAUTHORIZED: u16 = 401;
pub const STATUS_FORBIDDEN: u16 = 403;
pub const STATUS_BAD_REQUEST: u16 = 400;
pub const STATUS_TOO_MANY_REQUESTS: u16 = 429;

/// Common rate limit error patterns (pre-lowercased for efficient matching)
const RATE_LIMIT_PATTERNS: &[&str] = &[
    "insufficient_quota",
    "resource_exhausted",
    "quota",
    "rate limit",
    "rate_limit",
    "ratelimit",
    "ratelimitexceeded",
    "concurrency",
    "frequency",
    "usage limit",
    "too many requests",
    "daily call limit",
    "package has expired",
];

/// Handle HTTP response errors for Gemini provider
pub async fn handle_gemini_http_error(response: Response) -> Result<Response, LLMError> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let error_text = response.text().await.unwrap_or_default();
    Err(parse_api_error("Gemini", status, &error_text))
}

/// Handle HTTP response errors for Anthropic provider
pub async fn handle_anthropic_http_error(response: Response) -> Result<Response, LLMError> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let error_text = response.text().await.unwrap_or_default();
    Err(parse_api_error("Anthropic", status, &error_text))
}

/// Handle HTTP response errors for OpenAI-compatible providers
pub async fn handle_openai_http_error(
    response: Response,
    provider_name: &'static str,
    _api_key_env_var: &str,
) -> Result<Response, LLMError> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let error_text = response.text().await.unwrap_or_default();

    // Universal diagnostic logging — helps debug post-tool follow-up failures
    // and transient API issues across all OpenAI-compatible providers.
    tracing::warn!(
        provider = provider_name,
        status = %status,
        body = %error_text,
        "{} HTTP error",
        provider_name
    );

    Err(parse_api_error(provider_name, status, &error_text))
}

/// Check if an error is a rate limit error based on status code and message
#[inline]
pub fn is_rate_limit_error(status_code: u16, error_text: &str) -> bool {
    if status_code == STATUS_TOO_MANY_REQUESTS {
        return true;
    }

    // Optimize: Lowercase once and use pre-lowercased patterns
    let lower = error_text.to_lowercase();
    RATE_LIMIT_PATTERNS
        .iter()
        .any(|pattern| lower.contains(pattern))
}

/// Handle network errors with consistent formatting
#[inline]
pub fn format_network_error(provider: &str, error: &impl std::fmt::Display) -> LLMError {
    let formatted_error =
        error_display::format_llm_error(provider, &format!("Network error: {}", error));
    LLMError::Network {
        message: formatted_error,
        metadata: None,
    }
}

/// Handle JSON parsing errors with consistent formatting
#[inline]
pub fn format_parse_error(provider: &str, error: &impl std::fmt::Display) -> LLMError {
    let formatted_error =
        error_display::format_llm_error(provider, &format!("Failed to parse response: {}", error));
    LLMError::Provider {
        message: formatted_error,
        metadata: None,
    }
}

/// Format HTTP error with status code and message
#[inline]
pub fn format_http_error(provider: &str, status: reqwest::StatusCode, error_text: &str) -> String {
    error_display::format_llm_error(provider, &format!("HTTP {}: {}", status, error_text))
}

/// Parse standard API error response body into LLMError.
///
/// Handles multiple provider error formats:
/// - OpenAI/DeepSeek/ZAI: `{"error": {"message": "..."}}`
/// - Anthropic: `{"type": "error", "error": {"message": "..."}}`
/// - Gemini: `{"error": {"message": "...", "status": "..."}}`
/// - HuggingFace: `{"error": "..."}`
///
/// Falls back to raw body if JSON parsing fails.
pub fn parse_api_error(
    provider_name: &'static str,
    status: reqwest::StatusCode,
    body: &str,
) -> LLMError {
    // Try to extract a meaningful error message from JSON
    let error_message = if let Ok(json) = serde_json::from_str::<Value>(body) {
        // OpenAI/DeepSeek/ZAI/Anthropic format: {"error": {"message": "..."}}
        if let Some(msg) = json
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
        {
            msg.to_string()
        }
        // HuggingFace simple format: {"error": "..."}
        else if let Some(msg) = json.get("error").and_then(|e| e.as_str()) {
            msg.to_string()
        }
        // Gemini alternate format: {"error": {"status": "...", "code": ...}}
        else if let Some(status_msg) = json
            .get("error")
            .and_then(|e| e.get("status"))
            .and_then(|s| s.as_str())
        {
            status_msg.to_string()
        }
        // Fallback to raw body
        else {
            body.to_string()
        }
    } else {
        body.to_string()
    };

    // Categorize by status code
    let status_code = status.as_u16();

    match status_code {
        401 | 403 => LLMError::Authentication {
            message: error_display::format_llm_error(
                provider_name,
                &format!("Authentication failed: {}", error_message),
            ),
            metadata: Some(LLMErrorMetadata::new(
                provider_name,
                Some(status_code),
                Some("authentication_error".to_string()),
                None,
                None, // organization_id
                None,
                Some(body.to_string()),
            )),
        },
        429 => LLMError::RateLimit {
            metadata: Some(LLMErrorMetadata::new(
                provider_name,
                Some(status_code),
                Some("rate_limit_error".to_string()),
                None,
                None, // organization_id
                None,
                Some(error_message),
            )),
        },
        400 if is_rate_limit_error(status_code, body) => LLMError::RateLimit {
            metadata: Some(LLMErrorMetadata::new(
                provider_name,
                Some(status_code),
                Some("quota_exceeded".to_string()),
                None,
                None, // organization_id
                None,
                Some(error_message),
            )),
        },
        400 => LLMError::InvalidRequest {
            message: error_display::format_llm_error(
                provider_name,
                &format!("Invalid request: {}", error_message),
            ),
            metadata: Some(LLMErrorMetadata::new(
                provider_name,
                Some(status_code),
                Some("invalid_request".to_string()),
                None,
                None, // organization_id
                None,
                Some(body.to_string()),
            )),
        },
        _ => LLMError::Provider {
            message: error_display::format_llm_error(
                provider_name,
                &format!("HTTP {}: {}", status, error_message),
            ),
            metadata: Some(LLMErrorMetadata::new(
                provider_name,
                Some(status_code),
                None,
                None,
                None, // organization_id
                None,
                Some(body.to_string()),
            )),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_detection() {
        assert!(is_rate_limit_error(429, ""));
        assert!(is_rate_limit_error(400, "insufficient_quota"));
        assert!(is_rate_limit_error(400, "RESOURCE_EXHAUSTED"));
        assert!(is_rate_limit_error(400, "rate limit exceeded"));
        assert!(!is_rate_limit_error(400, "invalid request"));
        assert!(!is_rate_limit_error(200, ""));
    }

    #[test]
    fn test_status_codes() {
        assert_eq!(STATUS_UNAUTHORIZED, 401);
        assert_eq!(STATUS_FORBIDDEN, 403);
        assert_eq!(STATUS_BAD_REQUEST, 400);
        assert_eq!(STATUS_TOO_MANY_REQUESTS, 429);
    }
}
