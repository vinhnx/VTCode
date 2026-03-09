use serde_json::json;
use vtcode_core::config::constants::tools;

use super::classify::{
    contains_chained_instruction, looks_like_natural_language_request, looks_like_shell_command,
};
use super::normalize::normalize_for_shell_detection;
use super::paths::{normalize_path_operand, shell_quote_if_needed};

/// Detect if user input is an explicit "run <command>" request.
pub(crate) fn detect_explicit_run_command(input: &str) -> Option<(String, serde_json::Value)> {
    let trimmed = input.trim();

    if let Some(command) = detect_show_diff_command(trimmed) {
        return Some((
            tools::UNIFIED_EXEC.to_string(),
            json!({
                "command": command
            }),
        ));
    }

    let lower = trimmed.to_lowercase();
    if !lower.starts_with("run ") {
        return None;
    }

    let command_part = trimmed[4..].trim();
    if command_part.is_empty() {
        return None;
    }

    let normalized_command = normalize_for_shell_detection(command_part);
    if looks_like_natural_language_request(&normalized_command) {
        return None;
    }
    if contains_chained_instruction(&normalized_command) {
        return None;
    }
    if !looks_like_shell_command(&normalized_command) {
        return None;
    }

    Some((
        tools::UNIFIED_EXEC.to_string(),
        json!({
            "command": normalized_command
        }),
    ))
}

fn detect_show_diff_command(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let lower = trimmed.to_ascii_lowercase();
    if !lower.starts_with("show diff ") {
        return None;
    }

    let target = trimmed[10..].trim();
    if target.is_empty() {
        return None;
    }

    let target = normalize_path_operand(target);
    if target.is_empty() {
        return None;
    }

    Some(format!("git diff -- {}", shell_quote_if_needed(&target)))
}
