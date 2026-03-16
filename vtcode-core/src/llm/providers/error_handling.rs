//! Centralized error handling for LLM providers
//! Eliminates duplicate error handling code across providers

use crate::llm::error_display;
use crate::llm::provider::{LLMError, LLMErrorMetadata};
use reqwest::Response;
use serde_json::Value;

#[derive(Debug, Clone, Default)]
struct ApiResponseMetadata {
    request_id: Option<String>,
    organization_id: Option<String>,
    retry_after: Option<String>,
}

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
    let metadata = extract_response_metadata(&response);
    let error_text = response.text().await.unwrap_or_default();
    Err(parse_api_error_with_metadata(
        "Gemini",
        status,
        &error_text,
        metadata,
    ))
}

/// Handle HTTP response errors for Anthropic provider
pub async fn handle_anthropic_http_error(response: Response) -> Result<Response, LLMError> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let metadata = extract_response_metadata(&response);
    let error_text = response.text().await.unwrap_or_default();
    Err(parse_api_error_with_metadata(
        "Anthropic",
        status,
        &error_text,
        metadata,
    ))
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
    let metadata = extract_response_metadata(&response);
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

    Err(parse_api_error_with_metadata(
        provider_name,
        status,
        &error_text,
        metadata,
    ))
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
    parse_api_error_with_metadata(provider_name, status, body, ApiResponseMetadata::default())
}

fn parse_api_error_with_metadata(
    provider_name: &'static str,
    status: reqwest::StatusCode,
    body: &str,
    response_metadata: ApiResponseMetadata,
) -> LLMError {
    // Try to extract a meaningful error message from JSON
    let error_message = extract_human_error_message(body);

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
                response_metadata.request_id.clone(),
                response_metadata.organization_id.clone(),
                response_metadata.retry_after.clone(),
                Some(body.to_string()),
            )),
        },
        429 => LLMError::RateLimit {
            metadata: Some(LLMErrorMetadata::new(
                provider_name,
                Some(status_code),
                Some("rate_limit_error".to_string()),
                response_metadata.request_id.clone(),
                response_metadata.organization_id.clone(),
                response_metadata.retry_after.clone(),
                Some(error_message),
            )),
        },
        400 if is_rate_limit_error(status_code, body) => LLMError::RateLimit {
            metadata: Some(LLMErrorMetadata::new(
                provider_name,
                Some(status_code),
                Some("quota_exceeded".to_string()),
                response_metadata.request_id.clone(),
                response_metadata.organization_id.clone(),
                response_metadata.retry_after.clone(),
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
                response_metadata.request_id.clone(),
                response_metadata.organization_id.clone(),
                response_metadata.retry_after.clone(),
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
                response_metadata.request_id,
                response_metadata.organization_id,
                response_metadata.retry_after,
                Some(body.to_string()),
            )),
        },
    }
}

/// Extract the most human-readable error message from a provider's JSON error body.
///
/// Handles all known provider response schemas:
/// - OpenAI/DeepSeek/ZAI/Anthropic: `{"error": {"message": "..."}}`
/// - HuggingFace: `{"error": "..."}`
/// - Gemini: `{"error": {"status": "..."}}`
/// - FastAPI / OpenAI alternate: `{"detail": "..."}`
/// - Generic: `{"message": "..."}`
///
/// Falls back to the raw body if no known field is found.
pub fn extract_human_error_message(body: &str) -> String {
    let Ok(json) = serde_json::from_str::<Value>(body) else {
        return body.to_string();
    };

    // OpenAI/DeepSeek/ZAI/Anthropic: {"error": {"message": "..."}}
    if let Some(msg) = json
        .get("error")
        .and_then(|e| e.get("message"))
        .and_then(|m| m.as_str())
        .filter(|s| !s.trim().is_empty())
    {
        return msg.to_string();
    }
    // HuggingFace simple: {"error": "..."}
    if let Some(msg) = json
        .get("error")
        .and_then(|e| e.as_str())
        .filter(|s| !s.trim().is_empty())
    {
        return msg.to_string();
    }
    // FastAPI / OpenAI alternate: {"detail": "..."}
    if let Some(msg) = json
        .get("detail")
        .and_then(|d| d.as_str())
        .filter(|s| !s.trim().is_empty())
    {
        return msg.to_string();
    }
    // Gemini: {"error": {"status": "..."}}
    if let Some(msg) = json
        .get("error")
        .and_then(|e| e.get("status"))
        .and_then(|s| s.as_str())
        .filter(|s| !s.trim().is_empty())
    {
        return msg.to_string();
    }
    // Top-level message: {"message": "..."}
    if let Some(msg) = json
        .get("message")
        .and_then(|m| m.as_str())
        .filter(|s| !s.trim().is_empty())
    {
        return msg.to_string();
    }

    body.to_string()
}

fn extract_response_metadata(response: &Response) -> ApiResponseMetadata {
    ApiResponseMetadata {
        request_id: extract_header(
            response,
            &["request-id", "x-request-id", "openai-request-id"],
        ),
        organization_id: extract_header(
            response,
            &[
                "anthropic-organization-id",
                "openai-organization",
                "x-organization-id",
            ],
        ),
        retry_after: extract_header(response, &["retry-after"]),
    }
}

fn extract_header(response: &Response, names: &[&str]) -> Option<String> {
    names.iter().find_map(|name| {
        response
            .headers()
            .get(*name)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned)
    })
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
    fn parse_openai_rate_limit_error_preserves_provider_message() {
        let error = parse_api_error(
            "OpenAI",
            reqwest::StatusCode::TOO_MANY_REQUESTS,
            r#"{"error":{"message":"Project rate limit exceeded for this model.","type":"rate_limit_error"}}"#,
        );

        match error {
            LLMError::RateLimit { metadata } => {
                assert_eq!(
                    metadata.as_ref().and_then(|meta| meta.message.as_deref()),
                    Some("Project rate limit exceeded for this model.")
                );
            }
            other => panic!("expected rate limit error, got {other:?}"),
        }
    }

    #[test]
    fn extract_openai_error_message() {
        let body = r#"{"error":{"message":"Model not found","type":"invalid_request_error"}}"#;
        assert_eq!(extract_human_error_message(body), "Model not found");
    }

    #[test]
    fn extract_detail_field() {
        let body = r#"{"detail":"The 'gpt-5.4' model is not supported with this method."}"#;
        assert_eq!(
            extract_human_error_message(body),
            "The 'gpt-5.4' model is not supported with this method."
        );
    }

    #[test]
    fn extract_huggingface_error_string() {
        let body = r#"{"error":"Model is currently loading"}"#;
        assert_eq!(
            extract_human_error_message(body),
            "Model is currently loading"
        );
    }

    #[test]
    fn extract_top_level_message() {
        let body = r#"{"message":"Unauthorized access"}"#;
        assert_eq!(
            extract_human_error_message(body),
            "Unauthorized access"
        );
    }

    #[test]
    fn extract_gemini_status() {
        let body = r#"{"error":{"status":"PERMISSION_DENIED","code":403}}"#;
        assert_eq!(
            extract_human_error_message(body),
            "PERMISSION_DENIED"
        );
    }

    #[test]
    fn extract_falls_back_to_raw_body() {
        let body = "Internal Server Error";
        assert_eq!(extract_human_error_message(body), body);
    }

    #[test]
    fn extract_falls_back_for_unknown_json_schema() {
        let body = r#"{"code":500,"status":"error"}"#;
        assert_eq!(extract_human_error_message(body), body);
    }
}
