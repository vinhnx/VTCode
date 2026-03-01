use serde_json::json;
use shell_words::split as shell_split;
use vtcode_core::command_safety::shell_parser::parse_shell_commands_tree_sitter;
use vtcode_core::config::constants::tools;

const RUN_COMMAND_PREFIX_WRAPPERS: [&str; 9] = [
    "unix command ",
    "shell command ",
    "command ",
    "cmd ",
    "please ",
    "kindly ",
    "just ",
    "quickly ",
    "quick ",
];

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

    if let Some(command) = detect_show_diff_command(trimmed) {
        return Some((
            tools::RUN_PTY_CMD.to_string(),
            json!({
                "command": command
            }),
        ));
    }

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

    let normalized_command = normalize_natural_language_command(command_part);

    // Don't intercept if it still looks like a natural-language request
    // after normalization attempts.
    if looks_like_natural_language_request(&normalized_command) {
        return None;
    }

    if contains_chained_instruction(&normalized_command) {
        return None;
    }

    if !looks_like_shell_command(&normalized_command) {
        return None;
    }

    // Build the tool call arguments
    let args = json!({
        "command": normalized_command
    });

    Some((tools::RUN_PTY_CMD.to_string(), args))
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

    let target = normalize_show_diff_target(target);
    if target.is_empty() {
        return None;
    }

    // Use -- to force path interpretation and avoid accidental rev parsing.
    Some(format!("git diff -- {}", target))
}

fn normalize_show_diff_target(target: &str) -> &str {
    let mut normalized = target.trim();
    loop {
        let previous = normalized;
        normalized = normalized.trim();
        normalized = normalized.strip_prefix('"').unwrap_or(normalized);
        normalized = normalized.strip_suffix('"').unwrap_or(normalized);
        normalized = normalized.strip_prefix('\'').unwrap_or(normalized);
        normalized = normalized.strip_suffix('\'').unwrap_or(normalized);
        normalized = normalized
            .trim_end_matches(['.', ',', ';', '!', '?'])
            .trim();
        if normalized == previous {
            return normalized;
        }
    }
}

fn normalize_natural_language_command(command_part: &str) -> String {
    let extracted = extract_inline_backtick_command(command_part).unwrap_or(command_part);
    let cleaned = strip_run_command_prefixes(extracted);
    normalize_cargo_phrase(cleaned)
        .or_else(|| normalize_node_package_manager_phrase(cleaned))
        .or_else(|| normalize_pytest_phrase(cleaned))
        .or_else(|| normalize_unix_phrase(cleaned))
        .unwrap_or_else(|| cleaned.to_string())
}

pub(crate) fn strip_run_command_prefixes(command_part: &str) -> &str {
    let mut input = command_part.trim_start();
    loop {
        let lowered = input.to_ascii_lowercase();
        let stripped = RUN_COMMAND_PREFIX_WRAPPERS.iter().find_map(|prefix| {
            if lowered.starts_with(prefix) {
                input.get(prefix.len()..)
            } else {
                None
            }
        });
        let Some(rest) = stripped else {
            return input;
        };
        input = rest.trim_start();
    }
}

fn normalize_cargo_phrase(command_part: &str) -> Option<String> {
    let tokens: Vec<&str> = command_part.split_whitespace().collect();
    let lowered_tokens: Vec<String> = tokens
        .iter()
        .map(|token| token.to_ascii_lowercase())
        .collect();
    if lowered_tokens.len() < 2 || lowered_tokens.first()? != "cargo" {
        return None;
    }

    match lowered_tokens.get(1).map(String::as_str) {
        Some("test") => normalize_cargo_test_phrase(&tokens, &lowered_tokens),
        Some("check") => normalize_cargo_check_phrase(&tokens, &lowered_tokens),
        _ => None,
    }
}

fn normalize_cargo_test_phrase(tokens: &[&str], lowered_tokens: &[String]) -> Option<String> {
    // cargo test on <bin> bin for func <name>
    if lowered_tokens.len() >= 8
        && lowered_tokens[2] == "on"
        && lowered_tokens[4] == "bin"
        && lowered_tokens[5] == "for"
        && matches!(
            lowered_tokens[6].as_str(),
            "func" | "function" | "test" | "case"
        )
    {
        let bin_name = trim_token(tokens[3]);
        let test_name_joined = tokens[7..].join(" ");
        let test_name = trim_token(&test_name_joined);
        if !bin_name.is_empty() && !test_name.is_empty() {
            return Some(format!("cargo test --bin {} {}", bin_name, test_name));
        }
    }

    // cargo test on <pkg> package|crate
    if lowered_tokens.len() >= 5
        && lowered_tokens[2] == "on"
        && matches!(lowered_tokens[4].as_str(), "package" | "pkg" | "crate")
    {
        let package_name = trim_token(tokens[3]);
        if !package_name.is_empty() {
            return Some(format!("cargo test -p {}", package_name));
        }
    }

    None
}

fn normalize_cargo_check_phrase(tokens: &[&str], lowered_tokens: &[String]) -> Option<String> {
    // cargo check on <pkg> package|crate
    if lowered_tokens.len() >= 5
        && lowered_tokens[2] == "on"
        && matches!(lowered_tokens[4].as_str(), "package" | "pkg" | "crate")
    {
        let package_name = trim_token(tokens[3]);
        if !package_name.is_empty() {
            return Some(format!("cargo check -p {}", package_name));
        }
    }

    // cargo check on <bin> bin
    if lowered_tokens.len() >= 5 && lowered_tokens[2] == "on" && lowered_tokens[4] == "bin" {
        let bin_name = trim_token(tokens[3]);
        if !bin_name.is_empty() {
            return Some(format!("cargo check --bin {}", bin_name));
        }
    }

    None
}

fn normalize_node_package_manager_phrase(command_part: &str) -> Option<String> {
    let tokens: Vec<&str> = command_part.split_whitespace().collect();
    let lowered_tokens: Vec<String> = tokens
        .iter()
        .map(|token| token.to_ascii_lowercase())
        .collect();
    if lowered_tokens.len() < 2 {
        return None;
    }

    let pm = lowered_tokens.first()?.as_str();
    if !matches!(pm, "npm" | "pnpm") {
        return None;
    }

    match lowered_tokens.get(1).map(String::as_str) {
        // npm test on <workspace> workspace|package|project
        Some("test")
            if lowered_tokens.len() >= 5
                && lowered_tokens[2] == "on"
                && matches!(
                    lowered_tokens[4].as_str(),
                    "workspace" | "package" | "pkg" | "project"
                ) =>
        {
            let target = trim_token(tokens[3]);
            if target.is_empty() {
                return None;
            }
            Some(format!("{} test --workspace {}", pm, target))
        }
        // npm run <script> on <workspace> workspace|package|project
        Some("run")
            if lowered_tokens.len() >= 6
                && lowered_tokens[3] == "on"
                && matches!(
                    lowered_tokens[5].as_str(),
                    "workspace" | "package" | "pkg" | "project"
                ) =>
        {
            let script = trim_token(tokens[2]);
            let target = trim_token(tokens[4]);
            if script.is_empty() || target.is_empty() {
                return None;
            }
            Some(format!("{} run {} --workspace {}", pm, script, target))
        }
        _ => None,
    }
}

fn normalize_pytest_phrase(command_part: &str) -> Option<String> {
    let tokens: Vec<&str> = command_part.split_whitespace().collect();
    let lowered_tokens: Vec<String> = tokens
        .iter()
        .map(|token| token.to_ascii_lowercase())
        .collect();
    if lowered_tokens.len() < 2 || lowered_tokens.first()? != "pytest" {
        return None;
    }

    // pytest on <path> for func|function|test|case <name>
    if lowered_tokens.len() >= 6
        && lowered_tokens[1] == "on"
        && lowered_tokens[3] == "for"
        && matches!(
            lowered_tokens[4].as_str(),
            "func" | "function" | "test" | "case"
        )
    {
        let path = trim_token(tokens[2]);
        let test_name_joined = tokens[5..].join(" ");
        let test_name = trim_token(&test_name_joined);
        if path.is_empty() || test_name.is_empty() {
            return None;
        }
        return Some(format!("pytest {} -k {}", path, test_name));
    }

    // pytest on <path>
    if lowered_tokens.len() >= 3 && lowered_tokens[1] == "on" {
        let path_joined = tokens[2..].join(" ");
        let path = trim_token(&path_joined);
        if path.is_empty() {
            return None;
        }
        return Some(format!("pytest {}", path));
    }

    // pytest for func|function|test|case <name>
    if lowered_tokens.len() >= 4
        && lowered_tokens[1] == "for"
        && matches!(
            lowered_tokens[2].as_str(),
            "func" | "function" | "test" | "case"
        )
    {
        let test_name_joined = tokens[3..].join(" ");
        let test_name = trim_token(&test_name_joined);
        if test_name.is_empty() {
            return None;
        }
        return Some(format!("pytest -k {}", test_name));
    }

    None
}

fn normalize_unix_phrase(command_part: &str) -> Option<String> {
    let tokens: Vec<&str> = command_part.split_whitespace().collect();
    let lowered_tokens: Vec<String> = tokens
        .iter()
        .map(|token| token.to_ascii_lowercase())
        .collect();
    if lowered_tokens.len() < 2 {
        return None;
    }

    let cmd = lowered_tokens.first()?.as_str();

    // Generic unix/dev command pattern: "<cmd> on <target>" -> "<cmd> <target>"
    // Works for common commands where "on" naturally denotes target path/file.
    let on_compatible_commands = [
        "ls", "cat", "head", "tail", "wc", "du", "df", "tree", "stat", "file", "bat", "less",
        "more", "git", "cargo", "pytest", "npm", "pnpm", "node", "python", "python3", "go", "java",
        "javac", "rustc", "make", "cmake", "docker", "kubectl",
    ];
    if lowered_tokens[1] == "on" && on_compatible_commands.contains(&cmd) {
        let target_joined = tokens[2..].join(" ");
        let target = trim_token(&target_joined);
        if !target.is_empty() {
            return Some(format!("{} {}", tokens[0], target));
        }
    }

    // grep/rg natural phrase:
    // "grep for TODO on src" -> "grep TODO src"
    if matches!(cmd, "grep" | "rg")
        && lowered_tokens.len() >= 5
        && lowered_tokens[1] == "for"
        && let Some(on_idx) = lowered_tokens[2..].iter().position(|token| token == "on")
    {
        let on_idx = on_idx + 2;
        let pattern_joined = tokens[2..on_idx].join(" ");
        let pattern = trim_token(&pattern_joined);
        let target_joined = tokens[on_idx + 1..].join(" ");
        let target = trim_token(&target_joined);
        if !pattern.is_empty() && !target.is_empty() {
            return Some(format!("{} {} {}", tokens[0], pattern, target));
        }
    }

    // find natural phrase:
    // "find on src for *.rs" -> "find src -name *.rs"
    if cmd == "find"
        && lowered_tokens.len() >= 5
        && lowered_tokens[1] == "on"
        && let Some(for_idx) = lowered_tokens[2..].iter().position(|token| token == "for")
    {
        let for_idx = for_idx + 2;
        let base_joined = tokens[2..for_idx].join(" ");
        let base = trim_token(&base_joined);
        let pattern_joined = tokens[for_idx + 1..].join(" ");
        let pattern = trim_token(&pattern_joined);
        if !base.is_empty() && !pattern.is_empty() {
            return Some(format!("find {} -name {}", base, pattern));
        }
    }

    None
}

fn looks_like_natural_language_request(command_part: &str) -> bool {
    let natural_language_indicators = [
        "the ", "all ", "some ", "this ", "that ", "my ", "our ", "a ", "an ",
    ];
    let command_lower = command_part.to_ascii_lowercase();
    natural_language_indicators
        .iter()
        .any(|indicator| command_lower.starts_with(indicator))
}

fn looks_like_shell_command(command_part: &str) -> bool {
    parse_shell_commands_tree_sitter(command_part)
        .map(|commands| !commands.is_empty())
        .unwrap_or_else(|_| {
            shell_split(command_part)
                .map(|tokens| !tokens.is_empty())
                .unwrap_or(false)
        })
}

fn extract_inline_backtick_command(command_part: &str) -> Option<&str> {
    let start = command_part.find('`')?;
    let remainder = command_part.get(start + 1..)?;
    let end_rel = remainder.find('`')?;
    let extracted = remainder.get(..end_rel)?.trim();
    if extracted.is_empty() {
        return None;
    }
    Some(extracted)
}

fn trim_token(token: &str) -> &str {
    token.trim().trim_end_matches(['.', ',', ';', '!', '?'])
}

fn contains_chained_instruction(command_part: &str) -> bool {
    let Ok(tokens) = shell_split(command_part) else {
        return false;
    };
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
    fn test_detect_show_diff_direct_command() {
        let result = detect_explicit_run_command("show diff src/main.rs");
        assert!(result.is_some());
        let (tool_name, args) = result.expect("direct command expected");
        assert_eq!(tool_name, "run_pty_cmd");
        assert_eq!(args["command"], "git diff -- src/main.rs");
    }

    #[test]
    fn test_detect_show_diff_trims_quotes_and_punctuation() {
        let result = detect_explicit_run_command("show diff \"src/main.rs\".");
        assert!(result.is_some());
        let (_, args) = result.expect("direct command expected");
        assert_eq!(args["command"], "git diff -- src/main.rs");
    }

    #[test]
    fn test_detect_show_diff_allows_dot_prefixed_paths() {
        let result = detect_explicit_run_command("show diff .vtcode/tool-policy.json");
        assert!(result.is_some());
        let (_, args) = result.expect("direct command expected");
        assert_eq!(args["command"], "git diff -- .vtcode/tool-policy.json");
    }

    #[test]
    fn test_detect_explicit_run_command_empty() {
        assert!(detect_explicit_run_command("run").is_none());
        assert!(detect_explicit_run_command("run ").is_none());
        assert!(detect_explicit_run_command("run  ").is_none());
    }

    #[test]
    fn test_detect_explicit_run_command_normalizes_natural_cargo_test_phrase() {
        let result = detect_explicit_run_command(
            "run cargo test on vtcode bin for func highlights_run_prefix_user_input",
        );
        assert!(result.is_some());
        let (_, args) = result.expect("normalized command expected");
        assert_eq!(
            args["command"],
            "cargo test --bin vtcode highlights_run_prefix_user_input"
        );
    }

    #[test]
    fn test_detect_explicit_run_command_keeps_standard_cargo_test() {
        let result = detect_explicit_run_command("run cargo test --bin vtcode smoke_test");
        assert!(result.is_some());
        let (_, args) = result.expect("command expected");
        assert_eq!(args["command"], "cargo test --bin vtcode smoke_test");
    }

    #[test]
    fn test_detect_explicit_run_command_normalizes_cargo_check_package_phrase() {
        let result = detect_explicit_run_command("run cargo check on vtcode-core package");
        assert!(result.is_some());
        let (_, args) = result.expect("normalized command expected");
        assert_eq!(args["command"], "cargo check -p vtcode-core");
    }

    #[test]
    fn test_detect_explicit_run_command_strips_polite_prefixes() {
        let result = detect_explicit_run_command("run please cargo check");
        assert!(result.is_some());
        let (_, args) = result.expect("normalized command expected");
        assert_eq!(args["command"], "cargo check");
    }

    #[test]
    fn test_detect_explicit_run_command_strips_mixed_wrappers() {
        let result = detect_explicit_run_command("run command please cargo check");
        assert!(result.is_some());
        let (_, args) = result.expect("normalized command expected");
        assert_eq!(args["command"], "cargo check");
    }

    #[test]
    fn test_detect_explicit_run_command_extracts_backtick_command() {
        let result = detect_explicit_run_command(
            "run please use `cargo test --bin vtcode highlights_run_prefix_user_input`",
        );
        assert!(result.is_some());
        let (_, args) = result.expect("normalized command expected");
        assert_eq!(
            args["command"],
            "cargo test --bin vtcode highlights_run_prefix_user_input"
        );
    }

    #[test]
    fn test_detect_explicit_run_command_normalizes_npm_workspace_test_phrase() {
        let result = detect_explicit_run_command("run npm test on web workspace");
        assert!(result.is_some());
        let (_, args) = result.expect("normalized command expected");
        assert_eq!(args["command"], "npm test --workspace web");
    }

    #[test]
    fn test_detect_explicit_run_command_normalizes_pnpm_workspace_script_phrase() {
        let result = detect_explicit_run_command("run pnpm run lint on frontend package");
        assert!(result.is_some());
        let (_, args) = result.expect("normalized command expected");
        assert_eq!(args["command"], "pnpm run lint --workspace frontend");
    }

    #[test]
    fn test_detect_explicit_run_command_normalizes_pytest_path_phrase() {
        let result = detect_explicit_run_command("run pytest on tests/unit/test_shell.py");
        assert!(result.is_some());
        let (_, args) = result.expect("normalized command expected");
        assert_eq!(args["command"], "pytest tests/unit/test_shell.py");
    }

    #[test]
    fn test_detect_explicit_run_command_normalizes_pytest_function_phrase() {
        let result =
            detect_explicit_run_command("run pytest for func test_detect_explicit_run_command");
        assert!(result.is_some());
        let (_, args) = result.expect("normalized command expected");
        assert_eq!(
            args["command"],
            "pytest -k test_detect_explicit_run_command"
        );
    }

    #[test]
    fn test_detect_explicit_run_command_normalizes_pytest_path_and_function_phrase() {
        let result = detect_explicit_run_command(
            "run pytest on tests/unit/test_shell.py for func test_detect_explicit_run_command",
        );
        assert!(result.is_some());
        let (_, args) = result.expect("normalized command expected");
        assert_eq!(
            args["command"],
            "pytest tests/unit/test_shell.py -k test_detect_explicit_run_command"
        );
    }

    #[test]
    fn test_detect_explicit_run_command_strips_unix_command_wrapper() {
        let result = detect_explicit_run_command("run unix command ls -la");
        assert!(result.is_some());
        let (_, args) = result.expect("normalized command expected");
        assert_eq!(args["command"], "ls -la");
    }

    #[test]
    fn test_detect_explicit_run_command_normalizes_unix_on_pattern() {
        let result = detect_explicit_run_command("run ls on /tmp");
        assert!(result.is_some());
        let (_, args) = result.expect("normalized command expected");
        assert_eq!(args["command"], "ls /tmp");
    }

    #[test]
    fn test_detect_explicit_run_command_normalizes_grep_for_on_pattern() {
        let result = detect_explicit_run_command("run rg for TODO on src");
        assert!(result.is_some());
        let (_, args) = result.expect("normalized command expected");
        assert_eq!(args["command"], "rg TODO src");
    }

    #[test]
    fn test_detect_explicit_run_command_normalizes_find_on_for_pattern() {
        let result = detect_explicit_run_command("run find on src for *.rs");
        assert!(result.is_some());
        let (_, args) = result.expect("normalized command expected");
        assert_eq!(args["command"], "find src -name *.rs");
    }
}
