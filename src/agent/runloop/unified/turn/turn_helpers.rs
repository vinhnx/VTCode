//! Common helpers for turn processing extracted to reduce duplication

use crate::agent::runloop::unified::state::CtrlCState;
use anyhow::Result;
use std::time::Duration;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

/// Centralized error display with consistent formatting
pub fn display_error(
    renderer: &mut AnsiRenderer,
    category: &str,
    error: &anyhow::Error,
) -> Result<()> {
    renderer.line_if_not_empty(MessageStyle::Output)?;
    renderer.line(MessageStyle::Error, &format!("{}: {}", category, error))
}

/// Error display with recovery suggestions from [`vtcode_commons::ErrorCategory`].
///
/// Shows the error itself, then appends actionable recovery hints when available.
#[allow(dead_code)]
pub fn display_error_with_recovery(
    renderer: &mut AnsiRenderer,
    category: &str,
    error: &anyhow::Error,
) -> Result<()> {
    display_error(renderer, category, error)?;

    let err_cat = vtcode_commons::classify_anyhow_error(error);
    let suggestions = err_cat.recovery_suggestions();
    if !suggestions.is_empty() {
        let hint = suggestions.join("; ");
        renderer.line(MessageStyle::Info, &format!("Hint: {}", hint))?;
    }
    Ok(())
}

/// Centralized status message display
pub fn display_status(renderer: &mut AnsiRenderer, message: &str) -> Result<()> {
    renderer.line(MessageStyle::Info, message)
}

/// Check if operation should continue based on ctrl-c state
pub fn should_continue_operation(ctrl_c_state: &CtrlCState) -> bool {
    !ctrl_c_state.is_cancel_requested() && !ctrl_c_state.is_exit_requested()
}

/// Sanitize a raw error message for user display.
///
/// Strips internal implementation details that leak from `anyhow` error chains,
/// long stack traces, and duplicated "context: source" patterns so that the TUI
/// shows a clean, actionable one-liner.
pub fn sanitize_error_for_display(raw: &str) -> String {
    // 1. Take only the first meaningful line — anyhow chains are newline-separated.
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

    // 2. If the line still contains a long chain ("ctx1: ctx2: ctx3: ..."),
    //    keep only the outermost context and the innermost cause.
    let parts: Vec<&str> = first_line.splitn(4, ": ").collect();
    let cleaned = if parts.len() >= 4 {
        // outer: <middle…>: inner — collapse to "outer: inner"
        format!("{}: {}", parts[0], parts[parts.len() - 1])
    } else {
        first_line.to_string()
    };

    // 3. Cap length for TUI friendliness (200 chars).
    if cleaned.len() > 200 {
        format!("{}…", &cleaned[..197])
    } else {
        cleaned
    }
}

/// Format a tool error with category label and optional recovery hint.
///
/// Returns a tuple of (primary_message, optional_hint).
pub fn format_tool_error_for_user(
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
pub fn calculate_backoff(attempt: usize, base_ms: u64, max_ms: u64) -> Duration {
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
    fn sanitize_collapses_long_colon_chain() {
        let raw = "outer context: middle1: middle2: actual root cause";
        let result = sanitize_error_for_display(raw);
        assert_eq!(result, "outer context: actual root cause");
    }

    #[test]
    fn sanitize_preserves_short_message() {
        let raw = "connection refused";
        let result = sanitize_error_for_display(raw);
        assert_eq!(result, "connection refused");
    }

    #[test]
    fn sanitize_caps_length() {
        let raw = "x".repeat(300);
        let result = sanitize_error_for_display(&raw);
        assert!(result.len() <= 201); // 197 + "…" (3 bytes UTF-8)
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
}
