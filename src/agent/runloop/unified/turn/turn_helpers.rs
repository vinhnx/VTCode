//! Common helpers for turn processing extracted to reduce duplication

use crate::agent::runloop::unified::state::CtrlCState;
use anyhow::Result;
use std::time::Duration;
use vtcode_core::llm::provider::LLMError;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

/// Centralized error display with consistent formatting.
///
/// Shows two lines for LLM errors:
/// 1. Human-friendly error message (extracted from the JSON body)
/// 2. Full JSON response body for debugging
pub(crate) fn display_error(
    renderer: &mut AnsiRenderer,
    category: &str,
    error: &anyhow::Error,
) -> Result<()> {
    renderer.line_if_not_empty(MessageStyle::Output)?;
    renderer.line(
        MessageStyle::Error,
        &format!("{}: {}", category, error_message_for_user(error)),
    )?;
    // Show full JSON body for LLM errors when available and different from the message
    if let Some(llm_err) = error.downcast_ref::<LLMError>() {
        if let Some(raw_body) = llm_error_raw_body(llm_err) {
            let human = llm_error_human_message(llm_err);
            if raw_body != human {
                renderer.line(
                    MessageStyle::Error,
                    &format!("Full response: {}", raw_body),
                )?;
            }
        }
    }
    Ok(())
}

pub(crate) fn error_message_for_user(error: &anyhow::Error) -> String {
    let message = error
        .downcast_ref::<LLMError>()
        .map(llm_error_human_message)
        .unwrap_or_else(|| error.to_string());
    sanitize_error_for_display(&message)
}

/// Extract the human-friendly error message from an LLMError.
///
/// For errors with metadata containing a raw body, this tries to parse a
/// human-readable message from the JSON. For errors that already have a
/// formatted `message` field, it returns that directly.
fn llm_error_human_message(error: &LLMError) -> String {
    match error {
        LLMError::Authentication { metadata, .. }
        | LLMError::InvalidRequest { metadata, .. }
        | LLMError::Network { metadata, .. }
        | LLMError::Provider { metadata, .. } => {
            // Try to extract a clean message from the raw body stored in metadata
            if let Some(meta) = metadata {
                if let Some(raw) = &meta.message {
                    let human =
                        vtcode_core::llm::providers::error_handling::extract_human_error_message(
                            raw,
                        );
                    if human != *raw {
                        return human;
                    }
                }
            }
            // Fall back to the formatted message field
            match error {
                LLMError::Authentication { message, .. }
                | LLMError::InvalidRequest { message, .. }
                | LLMError::Network { message, .. }
                | LLMError::Provider { message, .. } => message.clone(),
                _ => error.to_string(),
            }
        }
        LLMError::RateLimit { metadata } => metadata
            .as_ref()
            .and_then(|meta| {
                meta.message.as_ref().map(|raw| {
                    vtcode_core::llm::providers::error_handling::extract_human_error_message(raw)
                })
            })
            .unwrap_or_else(|| error.to_string()),
    }
}

/// Extract the raw body string from LLMError metadata, if available.
fn llm_error_raw_body(error: &LLMError) -> Option<String> {
    let metadata = match error {
        LLMError::Authentication { metadata, .. }
        | LLMError::InvalidRequest { metadata, .. }
        | LLMError::Network { metadata, .. }
        | LLMError::Provider { metadata, .. }
        | LLMError::RateLimit { metadata, .. } => metadata.as_ref(),
    };
    metadata.and_then(|meta| meta.message.clone())
}

/// Centralized status message display
pub(crate) fn display_status(renderer: &mut AnsiRenderer, message: &str) -> Result<()> {
    renderer.line(MessageStyle::Info, message)
}

/// Providers that support Responses-style server-side continuity chaining.
pub(crate) fn supports_responses_chaining(provider_name: &str) -> bool {
    provider_name.eq_ignore_ascii_case("openai")
        || provider_name.eq_ignore_ascii_case("openresponses")
}

/// Check if operation should continue based on ctrl-c state
pub(crate) fn should_continue_operation(ctrl_c_state: &CtrlCState) -> bool {
    !ctrl_c_state.is_cancel_requested() && !ctrl_c_state.is_exit_requested()
}

/// Sanitize a raw error message for user display.
///
/// Strips internal implementation details that leak from `anyhow` error chains
/// and long stack traces, but preserves the full error content (including API
/// response bodies and detail fields) so users can see complete debugging info.
pub(crate) fn sanitize_error_for_display(raw: &str) -> String {
    // Strip anyhow chain noise (stack traces, "Caused by:" indented lines)
    // but keep the first meaningful line intact — it contains all the detail.
    let first_line = raw
        .lines()
        .find(|l| {
            let trimmed = l.trim();
            !trimmed.is_empty()
                && !trimmed.starts_with("Caused by:")
                && !trimmed.starts_with("Stack backtrace")
                && !trimmed.starts_with("   ")
        })
        .unwrap_or(raw)
        .trim();

    first_line.to_string()
}

/// Format a tool error with category label and optional recovery hint.
///
/// Returns a tuple of (primary_message, optional_hint).
pub(crate) fn format_tool_error_for_user(
    tool_name: &str,
    error_message: &str,
) -> (String, Option<String>) {
    let category = vtcode_commons::classify_error_message(error_message);
    let label = category.user_label();
    let sanitized = sanitize_error_for_display(error_message);

    let primary = format!("Tool '{}' failed ({}): {}", tool_name, label, sanitized);

    let suggestions = category.recovery_suggestions();
    let hint = if suggestions.is_empty() {
        None
    } else {
        Some(format!("Hint: {}", suggestions.join("; ")))
    };

    (primary, hint)
}

/// Exponential backoff calculation
pub(crate) fn calculate_backoff(attempt: usize, base_ms: u64, max_ms: u64) -> Duration {
    let exp = 2_u64.saturating_pow(attempt.min(4) as u32);
    let backoff_ms = base_ms.saturating_mul(exp);
    Duration::from_millis(backoff_ms.min(max_ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_anyhow_chain() {
        let raw = "Failed to read config\n\nCaused by:\n    0: IO error\n    1: file not found";
        let result = sanitize_error_for_display(raw);
        assert_eq!(result, "Failed to read config");
    }

    #[test]
    fn sanitize_preserves_full_colon_chain() {
        let raw = "outer context: middle1: middle2: actual root cause";
        let result = sanitize_error_for_display(raw);
        assert_eq!(result, raw);
    }

    #[test]
    fn sanitize_preserves_short_message() {
        let raw = "connection refused";
        let result = sanitize_error_for_display(raw);
        assert_eq!(result, "connection refused");
    }

    #[test]
    fn sanitize_preserves_long_error_body() {
        let raw = format!(
            "OpenAI Responses API error (status 400) Body: {{\"detail\":\"The 'gpt-5.4' model is not supported. {}\"}}",
            "x".repeat(300)
        );
        let result = sanitize_error_for_display(&raw);
        assert_eq!(result, raw);
    }

    #[test]
    fn format_tool_error_includes_category() {
        let (msg, hint) = format_tool_error_for_user("read_file", "connection timed out");
        assert!(msg.contains("read_file"));
        assert!(msg.contains("timed out")); // sanitized message
        // Category label should be present (timeout or network)
        assert!(msg.contains('(') && msg.contains(')'));
        // Timeout/network errors should have a hint
        assert!(hint.is_some());
    }

    #[test]
    fn format_tool_error_no_hint_for_generic() {
        let (msg, _hint) = format_tool_error_for_user("my_tool", "something went wrong");
        assert!(msg.contains("my_tool"));
        assert!(msg.contains("something went wrong"));
    }

    #[test]
    fn user_error_message_prefers_llm_rate_limit_metadata() {
        let error = anyhow::Error::new(LLMError::RateLimit {
            metadata: Some(vtcode_core::llm::provider::LLMErrorMetadata::new(
                "OpenAI",
                Some(429),
                Some("rate_limit_error".to_string()),
                Some("req_123".to_string()),
                None,
                None,
                Some("Project rate limit exceeded for model gpt-5.2.".to_string()),
            )),
        });

        assert_eq!(
            error_message_for_user(&error),
            "Project rate limit exceeded for model gpt-5.2."
        );
    }

    #[test]
    fn user_error_extracts_detail_field_from_json_body() {
        let body = r#"{"detail":"The 'gpt-5.4' model is not supported with this method."}"#;
        let error = anyhow::Error::new(LLMError::InvalidRequest {
            message: "Invalid request".to_string(),
            metadata: Some(vtcode_core::llm::provider::LLMErrorMetadata::new(
                "OpenAI",
                Some(400),
                Some("invalid_request".to_string()),
                None,
                None,
                None,
                Some(body.to_string()),
            )),
        });

        assert_eq!(
            error_message_for_user(&error),
            "The 'gpt-5.4' model is not supported with this method."
        );
    }

    #[test]
    fn user_error_extracts_nested_error_message_from_json_body() {
        let body = r#"{"error":{"message":"Model not found","type":"invalid_request_error","code":"model_not_found"}}"#;
        let error = anyhow::Error::new(LLMError::Provider {
            message: "Provider error".to_string(),
            metadata: Some(vtcode_core::llm::provider::LLMErrorMetadata::new(
                "OpenAI",
                Some(404),
                None,
                None,
                None,
                None,
                Some(body.to_string()),
            )),
        });

        assert_eq!(error_message_for_user(&error), "Model not found");
    }

    #[test]
    fn raw_body_extracted_from_metadata() {
        let body = r#"{"detail":"Some error detail"}"#;
        let llm_err = LLMError::InvalidRequest {
            message: "Invalid request".to_string(),
            metadata: Some(vtcode_core::llm::provider::LLMErrorMetadata::new(
                "OpenAI",
                Some(400),
                None,
                None,
                None,
                None,
                Some(body.to_string()),
            )),
        };

        let raw = llm_error_raw_body(&llm_err);
        assert_eq!(raw.as_deref(), Some(body));
    }

    #[test]
    fn raw_body_none_when_no_metadata() {
        let llm_err = LLMError::Provider {
            message: "some error".to_string(),
            metadata: None,
        };
        assert!(llm_error_raw_body(&llm_err).is_none());
    }
}
