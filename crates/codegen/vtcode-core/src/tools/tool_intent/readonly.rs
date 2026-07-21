use crate::tools::command_args::{is_readonly_command_string, raw_command_text};
use serde_json::Value;

/// Conservative allow-list of read-only inspection commands used by
/// `command_session`. Any command that could write, move, or delete must be
/// rejected so it is not cached or parallelized as read-only.
const READONLY_UNIFIED_EXEC_COMMANDS: &[&str] = &[
    "rg", "ls", "cat", "diff", "find", "wc", "grep", "egrep", "fgrep", "head", "tail", "sort", "uniq", "awk", "sed",
    "cut", "tr", "ast-grep", "sg", "echo", "pwd", "printf", "true", "false", "test",
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
        if !is_readonly_pipeline_segments(args) {
            return false;
        }
        // For `&&`-chained commands, every segment must start with an
        // allow-listed command. This allows harmless exploration like
        // `ls -la && echo '---' && ls -la crates/` in plan mode while
        // blocking `ls -la && rm foo.txt` (checkpoint turn_726).
        return is_readonly_chained_segments(args);
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

/// For `&&`-chained commands, ensure every segment begins with an allow-listed
/// read-only command. This prevents read-only classification of constructs like
/// `ls -la && rm foo.txt` or `cat a.txt && tee b.txt`. Mirrors the pipeline
/// segment check but for `&&` chaining (checkpoint turn_726: plan mode blocked
/// `ls -la && echo '---' && ls -la crates/` because `&&` was rejected outright,
/// forcing the model to fall back to `request_user_input` which was also denied).
pub fn is_readonly_chained_segments(args: &Value) -> bool {
    let Some(raw) = raw_command_text(args) else {
        return false;
    };

    // First split by `&&` to get chain segments, then each segment may also
    // contain `|` pipelines — the pipeline check is handled separately by
    // `is_readonly_pipeline_segments`, so here we only need to verify the
    // first command of each `&&` segment is allow-listed.
    let segments: Vec<&str> = raw.split("&&").map(str::trim).collect();
    if segments.len() <= 1 {
        return true;
    }

    for segment in &segments {
        if segment.is_empty() {
            return false;
        }
        // A `&&` segment may itself be a pipeline (e.g. `ls -la | head -5`).
        // Check the first command of the first pipeline sub-segment.
        let first_part = segment.split('|').next().unwrap_or(segment).trim();
        let first_command = first_part
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn and_chain_allows_readonly_segments() {
        assert!(is_readonly_chained_segments(&json!({"command": "ls -la && echo '---' && ls -la crates/"})));
        assert!(is_readonly_chained_segments(&json!({"command": "pwd && ls src/"})));
        assert!(is_readonly_chained_segments(&json!({"command": "cat foo.txt && grep bar"})));
    }

    #[test]
    fn and_chain_rejects_destructive_segments() {
        assert!(!is_readonly_chained_segments(&json!({"command": "ls -la && rm foo.txt"})));
        assert!(!is_readonly_chained_segments(&json!({"command": "cat x && mv a b"})));
        assert!(!is_readonly_chained_segments(&json!({"command": "true && cp a b"})));
    }

    #[test]
    fn and_chain_rejects_non_allowlisted_segments() {
        assert!(!is_readonly_chained_segments(&json!({"command": "ls -la && python script.py"})));
        assert!(!is_readonly_chained_segments(&json!({"command": "ls -la && cargo build"})));
    }

    #[test]
    fn and_chain_allows_pipeline_within_segment() {
        // A `&&` segment may itself be a pipeline — only the first command
        // of each `&&` segment needs to be allow-listed here; the pipeline
        // segment check is handled by `is_readonly_pipeline_segments`.
        assert!(is_readonly_chained_segments(&json!({"command": "ls -la | head -5 && echo done"})));
    }

    #[test]
    fn and_chain_single_command_passes() {
        assert!(is_readonly_chained_segments(&json!({"command": "ls -la"})));
        assert!(is_readonly_chained_segments(&json!({"command": "echo hi"})));
    }

    #[test]
    fn readonly_command_session_allows_and_chain() {
        // The exact pattern from checkpoint turn_726 that was blocked.
        assert!(is_readonly_command_session_command(&json!({
            "action": "run",
            "command": "ls -la /path/ && echo '---' && ls -la /path/crates/"
        })));
    }

    #[test]
    fn readonly_command_session_rejects_destructive_and_chain() {
        assert!(!is_readonly_command_session_command(&json!({
            "action": "run",
            "command": "ls -la && rm foo.txt"
        })));
    }
}
