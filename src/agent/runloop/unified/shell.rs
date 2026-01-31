use serde_json::json;
use shell_words::split as shell_split;
use vtcode_core::config::constants::tools;

/// Detect if user input is an explicit "run <command>" request.
/// Returns Some((tool_name, args)) if the input matches the pattern.
///
/// This function intercepts user prompts like:
/// - "run ls -a"
/// - "run git status"
/// - "run cargo build"
///
/// And converts them directly to run_pty_cmd tool calls, bypassing LLM interpretation.
pub(crate) fn detect_explicit_run_command(input: &str) -> Option<(String, serde_json::Value)> {
    let trimmed = input.trim();

    // Check for "run " prefix (case-insensitive)
    let lower = trimmed.to_lowercase();
    if !lower.starts_with("run ") {
        return None;
    }

    // Extract the command after "run "
    let command_part = trimmed[4..].trim();
    if command_part.is_empty() {
        return None;
    }

    // Don't intercept if it looks like a natural language request
    // e.g., "run the tests" or "run all unit tests"
    let natural_language_indicators = [
        "the ", "all ", "some ", "this ", "that ", "my ", "our ", "a ", "an ",
    ];
    let command_lower = command_part.to_lowercase();
    for indicator in natural_language_indicators {
        if command_lower.starts_with(indicator) {
            return None;
        }
    }

    if contains_chained_instruction(command_part) {
        return None;
    }

    // Build the tool call arguments
    let args = json!({
        "command": command_part
    });

    Some((tools::RUN_PTY_CMD.to_string(), args))
}

fn contains_chained_instruction(command_part: &str) -> bool {
    let tokens = shell_split(command_part).unwrap_or_else(|_| {
        command_part
            .split_whitespace()
            .map(|s| s.to_string())
            .collect()
    });
    if tokens.len() < 2 {
        return false;
    }

    let separators = ["and", "then", "after", "before", "also", "next"];
    for (idx, token) in tokens.iter().enumerate() {
        let lowered = token.to_ascii_lowercase();
        if separators.contains(&lowered.as_str()) && idx + 1 < tokens.len() {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_explicit_run_command_basic() {
        let result = detect_explicit_run_command("run ls -a");
        assert!(result.is_some());
        let (tool_name, args) = result.unwrap();
        assert_eq!(tool_name, "run_pty_cmd");
        assert_eq!(args["command"], "ls -a");
    }

    #[test]
    fn test_detect_explicit_run_command_git() {
        let result = detect_explicit_run_command("run git status");
        assert!(result.is_some());
        let (tool_name, args) = result.unwrap();
        assert_eq!(tool_name, "run_pty_cmd");
        assert_eq!(args["command"], "git status");
    }

    #[test]
    fn test_detect_explicit_run_command_cargo() {
        let result = detect_explicit_run_command("run cargo build --release");
        assert!(result.is_some());
        let (tool_name, args) = result.unwrap();
        assert_eq!(tool_name, "run_pty_cmd");
        assert_eq!(args["command"], "cargo build --release");
    }

    #[test]
    fn test_detect_explicit_run_command_case_insensitive() {
        let result = detect_explicit_run_command("Run npm install");
        assert!(result.is_some());
        let (tool_name, args) = result.unwrap();
        assert_eq!(tool_name, "run_pty_cmd");
        assert_eq!(args["command"], "npm install");
    }

    #[test]
    fn test_detect_explicit_run_command_natural_language_rejected() {
        // These should NOT be intercepted - they're natural language
        assert!(detect_explicit_run_command("run the tests").is_none());
        assert!(detect_explicit_run_command("run all unit tests").is_none());
        assert!(detect_explicit_run_command("run some commands").is_none());
        assert!(detect_explicit_run_command("run this script").is_none());
        assert!(detect_explicit_run_command("run a quick check").is_none());
    }

    #[test]
    fn test_detect_explicit_run_command_rejects_chained_instruction() {
        assert!(detect_explicit_run_command("run cargo clippy and fix issue").is_none());
        assert!(detect_explicit_run_command("run cargo test then analyze failures").is_none());
    }

    #[test]
    fn test_detect_explicit_run_command_allows_quoted_and() {
        let result = detect_explicit_run_command("run echo \"fish and chips\"");
        assert!(result.is_some());
    }

    #[test]
    fn test_detect_explicit_run_command_not_run_prefix() {
        // These should NOT be intercepted
        assert!(detect_explicit_run_command("ls -a").is_none());
        assert!(detect_explicit_run_command("please run ls").is_none());
        assert!(detect_explicit_run_command("can you run git status").is_none());
    }

    #[test]
    fn test_detect_explicit_run_command_empty() {
        assert!(detect_explicit_run_command("run").is_none());
        assert!(detect_explicit_run_command("run ").is_none());
        assert!(detect_explicit_run_command("run  ").is_none());
    }
}
