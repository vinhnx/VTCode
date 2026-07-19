use crate::tools::command_args::{is_readonly_command_string, raw_command_text};
use serde_json::Value;

/// Conservative allow-list of read-only inspection commands used by
/// `command_session`. Any command that could write, move, or delete must be
/// rejected so it is not cached or parallelized as read-only.
const READONLY_UNIFIED_EXEC_COMMANDS: &[&str] = &[
    "rg", "ls", "cat", "diff", "find", "wc", "grep", "egrep", "fgrep", "head", "tail", "sort", "uniq", "awk", "sed",
    "cut", "tr", "ast-grep", "sg",
];

pub fn is_readonly_base_command(command: &str) -> bool {
    READONLY_UNIFIED_EXEC_COMMANDS.contains(&command)
}

pub fn is_readonly_command_session_command(args: &Value) -> bool {
    let Ok(Some(parts)) = crate::tools::command_args::command_words(args) else {
        return false;
    };

    if parts.iter().any(|part| part == "--dry-run") {
        return true;
    }

    let Some(command) = parts.first().map(String::as_str) else {
        return false;
    };

    if is_readonly_base_command(command) {
        // Verify the raw command has no redirections, command substitutions, or
        // destructive subcommands (e.g. `find -delete`, `-exec rm`).
        if !is_readonly_command_string(args) {
            return false;
        }
        // For pipelines, every segment must start with an allow-listed command.
        return is_readonly_pipeline_segments(args);
    }

    match command {
        "git" => matches!(parts.get(1).map(String::as_str), Some("status")),
        "cargo" => matches!(parts.get(1).map(String::as_str), Some("check" | "test")),
        "npm" | "pnpm" | "yarn" => match parts.get(1).map(String::as_str) {
            Some("test") => true,
            Some("run") => matches!(parts.get(2).map(String::as_str), Some("test")),
            _ => false,
        },
        _ => false,
    }
}

/// For pipelined commands, ensure every segment begins with an allow-listed
/// read-only command. This prevents read-only caching of constructs like
/// `cat a.txt | tee b.txt` or `grep x | rm`.
pub fn is_readonly_pipeline_segments(args: &Value) -> bool {
    let Some(raw) = raw_command_text(args) else {
        return false;
    };

    let segments: Vec<&str> = raw.split('|').map(str::trim).collect();
    if segments.len() <= 1 {
        return true;
    }

    for segment in segments {
        if segment.is_empty() {
            return false;
        }
        let first_command = segment
            .split_whitespace()
            .find(|token| !token.starts_with('-') && !token.contains('='))
            .map(|token| token.to_ascii_lowercase());
        let Some(first_command) = first_command else {
            return false;
        };
        if !is_readonly_base_command(&first_command) {
            return false;
        }
    }

    true
}
