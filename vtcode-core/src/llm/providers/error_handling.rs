//! Centralized error handling for LLM providers
//! Eliminates duplicate error handling code across providers

use crate::llm::error_display;
use crate::llm::provider::{LLMError, LLMErrorMetadata};
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
        return Err(LLMError::Authentication {
            message: formatted_error,
            metadata: None,
        });
    }

    // Rate limit and quota errors
    if is_rate_limit_error(status.as_u16(), &error_text) {
        return Err(LLMError::RateLimit { metadata: None });
    }

    // Invalid request errors
    if status.as_u16() == STATUS_BAD_REQUEST {
        let formatted_error =
            error_display::format_llm_error("Gemini", &format!("Invalid request: {}", error_text));
        return Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata: None,
        });
    }

    // Generic error for other cases
    let formatted_error = format_http_error("Gemini", status, &error_text);
    Err(LLMError::Provider {
        message: formatted_error,
        metadata: None,
    })
}

/// Handle HTTP response errors for Anthropic provider
pub async fn handle_anthropic_http_error(response: Response) -> Result<Response, LLMError> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let headers = response.headers();

    let retry_after = headers
        .get("retry-after")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    let request_id = headers
        .get("request-id")
        .or_else(|| headers.get("x-request-id"))
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    // Capture rate limit reset info for debugging/user feedback
    let mut rate_limit_info = Vec::new();
    if let Some(reset) = headers
        .get("anthropic-ratelimit-requests-reset")
        .and_then(|h| h.to_str().ok())
    {
        rate_limit_info.push(format!("Requests reset in {}", reset));
    }
    if let Some(reset) = headers
        .get("anthropic-ratelimit-tokens-reset")
        .and_then(|h| h.to_str().ok())
    {
        rate_limit_info.push(format!("Tokens reset in {}", reset));
    }
    let rate_limit_message = if rate_limit_info.is_empty() {
        None
    } else {
        Some(rate_limit_info.join("; "))
    };

    let error_text = response.text().await.unwrap_or_default();

    // Authentication errors
    if status.as_u16() == STATUS_UNAUTHORIZED || status.as_u16() == STATUS_FORBIDDEN {
        let formatted_error = error_display::format_llm_error(
            "Anthropic",
            "Authentication failed (check ANTHROPIC_API_KEY)",
        );
        return Err(LLMError::Authentication {
            message: formatted_error,
            metadata: Some(LLMErrorMetadata::new(
                "Anthropic",
                Some(status.as_u16()),
                Some("authentication_error".to_string()),
                request_id,
                None,
                Some(error_text),
            )),
        });
    }

    // Rate limit errors
    if is_rate_limit_error(status.as_u16(), &error_text) {
        return Err(LLMError::RateLimit {
            metadata: Some(LLMErrorMetadata::new(
                "Anthropic",
                Some(status.as_u16()),
                Some("rate_limit_error".to_string()),
                request_id,
                retry_after,
                rate_limit_message,
            )),
        });
    }

    // Parse error message from Anthropic's JSON error format
    let friendly_msg = parse_anthropic_error_message(&error_text);

    let error_message = if friendly_msg.is_empty() {
        format!("HTTP {}", status)
    } else {
        format!("{} (HTTP {})", friendly_msg.as_ref(), status)
    };

    let formatted_error = error_display::format_llm_error("Anthropic", &error_message);
    Err(LLMError::Provider {
        message: formatted_error,
        metadata: Some(LLMErrorMetadata::new(
            "Anthropic",
            Some(status.as_u16()),
            None,
            request_id,
            retry_after,
            Some(error_text),
        )),
    })
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
        return Err(LLMError::Authentication {
            message: formatted_error,
            metadata: None,
        });
    }

    // Rate limit errors
    if status.as_u16() == STATUS_TOO_MANY_REQUESTS || error_text.to_lowercase().contains("quota") {
        return Err(LLMError::RateLimit { metadata: None });
    }

    // Generic provider error
    let formatted_error = format_http_error(provider_name, status, &error_text);
    Err(LLMError::Provider {
        message: formatted_error,
        metadata: None,
    })
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

    #[test]
    fn test_anthropic_error_parsing() {
        let json_error = r#"{"error":{"message":"Invalid API key","type":"authentication_error"}}"#;
        assert_eq!(parse_anthropic_error_message(json_error), "Invalid API key");

        let plain_error = "Some error";
        assert_eq!(parse_anthropic_error_message(plain_error), "Some error");
    }
}
