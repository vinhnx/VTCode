//! Extended error handling for Anthropic provider

use crate::llm::error_display;
use crate::llm::provider::LLMError;
use reqwest::Response;

/// HTTP status codes
const STATUS_UNAUTHORIZED: u16 = 401;
const STATUS_FORBIDDEN: u16 = 403;
const STATUS_TOO_MANY_REQUESTS: u16 = 429;

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
        return Err(LLMError::Authentication {
            message: formatted_error,
            metadata: None,
        });
    }

    // Rate limit errors
    if status.as_u16() == STATUS_TOO_MANY_REQUESTS {
        return Err(LLMError::RateLimit { metadata: None });
    }

    // Parse error message from Anthropic's error format
    let friendly_msg = if let Ok(error_json) = serde_json::from_str::<serde_json::Value>(&error_text) {
        error_json
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or(&error_text)
    } else {
        &error_text
    };

    let error_message = if friendly_msg.is_empty() {
        format!("HTTP {}", status)
    } else {
        format!("{} (HTTP {})", friendly_msg, status)
    };

    let formatted_error = error_display::format_llm_error("Anthropic", &error_message);
    Err(LLMError::Provider {
        message: formatted_error,
        metadata: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_codes() {
        assert_eq!(STATUS_UNAUTHORIZED, 401);
        assert_eq!(STATUS_FORBIDDEN, 403);
        assert_eq!(STATUS_TOO_MANY_REQUESTS, 429);
    }
}
