//! Spooling utilities for large tool outputs.
//!
//! This module provides formatting utilities for spool notifications.
//! The actual spooling logic lives in `streams_helpers::spool_output_if_needed`.

use std::path::Path;

/// Format a spool notification string for use in follow-up hints.
pub fn format_spool_hint(log_path: &Path) -> String {
    format!(
        "Large output was spooled to \"{}\". Use exec_command with shell tools such as cat, sed, or rg to inspect details.",
        log_path.display()
    )
}

/// Format a spool notification message for display.
pub fn format_spool_notification(content: &str, log_path: &Path, title: Option<&str>) -> String {
    let bytes = content.len();
    let lines = content.lines().count();

    match title {
        Some(t) if !t.is_empty() => {
            format!(
                "[{}] {} bytes, {} lines — spooled to: {}",
                t.to_uppercase(),
                bytes,
                lines,
                log_path.display()
            )
        }
        _ => {
            format!(
                "Command output too large ({bytes} bytes, {lines} lines), spooled to: {}",
                log_path.display()
            )
        }
    }
}
