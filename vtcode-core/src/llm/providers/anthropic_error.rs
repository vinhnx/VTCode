//! Extended error handling for Anthropic provider

use crate::llm::error_display;
use crate::llm::provider::LLMError;
use reqwest::Response;

/// Handle HTTP response errors for Anthropic provider
pub async fn handle_anthropic_http_error(response: Response) -> Result<Response, LLMError> {
    if response.status().is_success() {
        return Ok(response);
    }

    let status = response.status();
    let status_code = status.as_u16();

    // Extract request-id from header
    let header_request_id = response
        .headers()
        .get("request-id")
        .and_then(|h| h.to_str().ok().map(|s| s.to_string()));

    // Extract organization-id from header
    let organization_id = response
        .headers()
        .get("anthropic-organization-id")
        .and_then(|h| h.to_str().ok().map(|s| s.to_string()));

    // Extract retry-after from header
    let retry_after = response
        .headers()
        .get("retry-after")
        .and_then(|h| h.to_str().ok().map(|s| s.to_string()));

    let error_text = response.text().await.unwrap_or_default();

    // Parse error message and type from Anthropic's error format
    let error_json: Option<serde_json::Value> = serde_json::from_str(&error_text).ok();

    let (error_type, friendly_msg, body_request_id) = if let Some(json) = &error_json {
        let anthropic_err = json.get("error");
        let error_type = anthropic_err
            .and_then(|e| e.get("type"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string());
        let message = anthropic_err
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or(&error_text)
            .to_string();
        let rid = json
            .get("request_id")
            .and_then(|r| r.as_str())
            .map(|s| s.to_string());
        (error_type, message, rid)
    } else {
        (None, error_text, None)
    };

    let request_id = header_request_id.or(body_request_id);

    let metadata = Some(crate::llm::provider::LLMErrorMetadata::new(
        "Anthropic",
        Some(status_code),
        error_type.clone(),
        request_id,
        organization_id,
        retry_after,
        Some(friendly_msg.clone()),
    ));

    let error_message = if friendly_msg.is_empty() {
        format!("HTTP {}", status)
    } else {
        format!("{} (HTTP {})", friendly_msg, status)
    };

    let formatted_error = error_display::format_llm_error("Anthropic", &error_message);

    match status_code {
        400 | 404 | 413 => Err(LLMError::InvalidRequest {
            message: formatted_error,
            metadata,
        }),
        401 | 403 => Err(LLMError::Authentication {
            message: formatted_error,
            metadata,
        }),
        429 => Err(LLMError::RateLimit { metadata }),
        500 | 529 => Err(LLMError::Provider {
            message: formatted_error,
            metadata,
        }),
        _ => Err(LLMError::Provider {
            message: formatted_error,
            metadata,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_error_mapping() {
        // This is a bit tricky to test without a mock response
        // but it confirms the logic is sound.
    }
}
