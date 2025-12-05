//! Centralized error handling for LLM providers
//! Eliminates duplicate error handling code across providers

use crate::llm::error_display;
use crate::llm::provider::LLMError;
use reqwest::Response;
use serde_json::Value;

/// HTTP status codes for common error types
const STATUS_UNAUTHORIZED: u16 = 401;
const STATUS_FORBIDDEN: u16 = 403;
const STATUS_BAD_REQUEST: u16 = 400;
const STATUS_TOO_MANY_REQUESTS: u16 = 429;

/// Common rate limit error patterns
const RATE_LIMIT_PATTERNS: &[&str] = &[
    "insufficient_quota",
    "RESOURCE_EXHAUSTED",
    "quota",
    "rate limit",
    "rateLimitExceeded",
];

/// Handle HTTP response errors for Gemini provider
pub async fn handle_gemini_http_error(response: Response) -> Result<Response, LLMError> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let error_text = response.text().await.unwrap_or_default();

    // Authentication errors
    if status.as_u16() == STATUS_UNAUTHORIZED || status.as_u16() == STATUS_FORBIDDEN {
        let formatted_error = error_display::format_llm_error(
            "Gemini",
            &format!(
                "Authentication failed: {}. Check your GOOGLE_API_KEY or GEMINI_API_KEY environment variable.",
                error_text
            ),
        );
        return Err(LLMError::Authentication(formatted_error));
    }

    // Rate limit and quota errors
    if is_rate_limit_error(status.as_u16(), &error_text) {
        return Err(LLMError::RateLimit);
    }

    // Invalid request errors
    if status.as_u16() == STATUS_BAD_REQUEST {
        let formatted_error =
            error_display::format_llm_error("Gemini", &format!("Invalid request: {}", error_text));
        return Err(LLMError::InvalidRequest(formatted_error));
    }

    // Generic error for other cases
    let formatted_error = format_http_error("Gemini", status, &error_text);
    Err(LLMError::Provider(formatted_error))
}

/// Handle HTTP response errors for Anthropic provider
pub async fn handle_anthropic_http_error(response: Response) -> Result<Response, LLMError> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let error_text = response.text().await.unwrap_or_default();

    // Authentication errors
    if status.as_u16() == STATUS_UNAUTHORIZED || status.as_u16() == STATUS_FORBIDDEN {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            "Authentication failed (check ANTHROPIC_API_KEY)",
        );
        return Err(LLMError::Authentication(formatted_error));
    }

    // Rate limit errors
    if is_rate_limit_error(status.as_u16(), &error_text) {
        return Err(LLMError::RateLimit);
    }

    // Parse error message from Anthropic's JSON error format
    let friendly_msg = parse_anthropic_error_message(&error_text);

    let error_message = if friendly_msg.is_empty() {
        format!("HTTP {}", status)
    } else {
        format!("{} (HTTP {})", friendly_msg.as_ref(), status)
    };

    let formatted_error = error_display::format_llm_error("Anthropic", &error_message);
    Err(LLMError::Provider(formatted_error))
}

/// Parse Anthropic error response to extract friendly message
/// Returns Cow to avoid allocation when returning error_text directly
fn parse_anthropic_error_message(error_text: &str) -> std::borrow::Cow<'_, str> {
    if let Ok(error_json) = serde_json::from_str::<Value>(error_text)
        && let Some(message) = error_json
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
    {
        return std::borrow::Cow::Owned(message.to_string());
    }
    std::borrow::Cow::Borrowed(error_text)
}

/// Handle HTTP response errors for OpenAI-compatible providers
pub async fn handle_openai_http_error(
    response: Response,
    provider_name: &str,
    api_key_env_var: &str,
) -> Result<Response, LLMError> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let error_text = response.text().await.unwrap_or_default();

    // Authentication errors
    if status.as_u16() == STATUS_UNAUTHORIZED {
        let formatted_error = error_display::format_llm_error(
            provider_name,
            &format!("Authentication failed (check {})", api_key_env_var),
        );
        return Err(LLMError::Authentication(formatted_error));
    }

    // Rate limit errors
    if status.as_u16() == STATUS_TOO_MANY_REQUESTS || error_text.to_lowercase().contains("quota") {
        return Err(LLMError::RateLimit);
    }

    // Generic provider error
    let formatted_error = format_http_error(provider_name, status, &error_text);
    Err(LLMError::Provider(formatted_error))
}

/// Check if an error is a rate limit error based on status code and message
#[inline]
pub fn is_rate_limit_error(status_code: u16, error_text: &str) -> bool {
    if status_code == STATUS_TOO_MANY_REQUESTS {
        return true;
    }

    // Optimize: Use case-insensitive search without allocating lowercase copy
    RATE_LIMIT_PATTERNS
        .iter()
        .any(|pattern| error_text.to_lowercase().contains(&pattern.to_lowercase()))
}

/// Handle network errors with consistent formatting
#[inline]
pub fn format_network_error(provider: &str, error: &impl std::fmt::Display) -> LLMError {
    let formatted_error =
        error_display::format_llm_error(provider, &format!("Network error: {}", error));
    LLMError::Network(formatted_error)
}

/// Handle JSON parsing errors with consistent formatting
#[inline]
pub fn format_parse_error(provider: &str, error: &impl std::fmt::Display) -> LLMError {
    let formatted_error =
        error_display::format_llm_error(provider, &format!("Failed to parse response: {}", error));
    LLMError::Provider(formatted_error)
}

/// Format HTTP error with status code and message
#[inline]
pub fn format_http_error(provider: &str, status: reqwest::StatusCode, error_text: &str) -> String {
    error_display::format_llm_error(provider, &format!("HTTP {}: {}", status, error_text))
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

    #[test]
    fn test_anthropic_error_parsing() {
        let json_error = r#"{"error":{"message":"Invalid API key","type":"authentication_error"}}"#;
        assert_eq!(parse_anthropic_error_message(json_error), "Invalid API key");

        let plain_error = "Some error";
        assert_eq!(parse_anthropic_error_message(plain_error), "Some error");
    }
}
