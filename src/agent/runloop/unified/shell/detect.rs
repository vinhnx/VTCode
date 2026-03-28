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

    let (prefix, command_part) = split_prefix_and_command(trimmed)?;
    if !is_explicit_run_prefix(prefix, command_part) {
        return None;
    }
    if looks_like_subagent_shortcut(command_part) {
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

fn looks_like_subagent_shortcut(command_part: &str) -> bool {
    let Ok(tokens) = shell_words::split(command_part) else {
        return false;
    };

    match tokens.as_slice() {
        [_, kind] => kind.eq_ignore_ascii_case("agent") || kind.eq_ignore_ascii_case("subagent"),
        [article, _, kind] => {
            article.eq_ignore_ascii_case("the")
                && (kind.eq_ignore_ascii_case("agent") || kind.eq_ignore_ascii_case("subagent"))
        }
        _ => false,
    }
}

fn split_prefix_and_command(input: &str) -> Option<(&str, &str)> {
    let mut parts = input.splitn(2, char::is_whitespace);
    let prefix = parts.next()?;
    let command = parts.next()?.trim();
    if command.is_empty() {
        return None;
    }
    Some((prefix, command))
}

fn is_explicit_run_prefix(prefix: &str, command_part: &str) -> bool {
    let lower = prefix.to_ascii_lowercase();
    lower == "run" || is_likely_run_typo(&lower, command_part)
}

fn is_likely_run_typo(prefix: &str, command_part: &str) -> bool {
    if prefix.len() != 3 || !prefix.chars().all(|ch| ch.is_ascii_alphabetic()) {
        return false;
    }

    let Some(next_token) = command_part.split_whitespace().next() else {
        return false;
    };
    if !is_known_shell_starter(next_token) {
        return false;
    }

    let chars: Vec<char> = prefix.chars().collect();
    let target = ['r', 'u', 'n'];
    let mismatches = chars
        .iter()
        .zip(target.iter())
        .filter(|(actual, expected)| actual != expected)
        .count();
    mismatches == 1 || chars == ['u', 'r', 'n'] || chars == ['r', 'n', 'u']
}

fn is_known_shell_starter(token: &str) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "cargo"
            | "git"
            | "npm"
            | "pnpm"
            | "pytest"
            | "python"
            | "python3"
            | "node"
            | "go"
            | "make"
            | "cmake"
            | "docker"
            | "kubectl"
            | "ls"
            | "cat"
            | "head"
            | "tail"
            | "wc"
            | "du"
            | "df"
            | "tree"
            | "stat"
            | "file"
            | "bat"
            | "less"
            | "more"
            | "rg"
            | "grep"
    )
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
