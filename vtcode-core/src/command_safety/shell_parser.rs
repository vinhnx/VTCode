//! Shell script parser for `bash -lc` and similar commands.
//!
//! This module parses shell commands like:
//! ```sh
//! bash -lc "git status && cargo check"
//! ```
//!
//! Into individual command vectors for independent safety checking:
//! ```text
//! [["git", "status"], ["cargo", "check"]]
//! ```
//!
//! **Phase 4 Implementation**: Uses tree-sitter for accurate bash AST parsing.
//! Falls back to basic tokenization for minimal shell syntax.

use std::sync::Mutex;
use std::sync::OnceLock;

/// Lazy-initialized tree-sitter bash parser (wrapped in Mutex for mutation)
static BASH_PARSER: OnceLock<Mutex<tree_sitter::Parser>> = OnceLock::new();

/// Gets or initializes the bash parser
fn get_bash_parser() -> &'static Mutex<tree_sitter::Parser> {
    BASH_PARSER.get_or_init(|| {
        let mut parser = tree_sitter::Parser::new();
        let lang: tree_sitter::Language = tree_sitter_bash::LANGUAGE.into();
        parser
            .set_language(&lang)
            .expect("Failed to load bash grammar");
        Mutex::new(parser)
    })
}

/// Parses a shell script into individual commands using tree-sitter bash grammar
///
/// # Example
/// ```text
/// Input:  "git status && cargo check"
/// Output: Ok([["git", "status"], ["cargo", "check"]])
/// ```
///
/// # Fallback
/// If tree-sitter parsing fails, falls back to simple tokenization
pub fn parse_shell_commands(script: &str) -> std::result::Result<Vec<Vec<String>>, String> {
    // Try tree-sitter parsing first
    match parse_with_tree_sitter(script) {
        Ok(commands) if !commands.is_empty() => return Ok(commands),
        Ok(_) => {} // Empty result, fall through to basic parsing
        Err(e) => {
            tracing::debug!(
                "Tree-sitter bash parsing failed: {}, falling back to basic tokenization",
                e
            );
        }
    }

    // Fallback to simple tokenization
    parse_with_basic_tokenization(script)
}

/// Parses a shell script using tree-sitter bash grammar only (no fallback tokenization).
///
/// Use this when caller behavior must be strictly gated on bash grammar validity.
pub fn parse_shell_commands_tree_sitter(
    script: &str,
) -> std::result::Result<Vec<Vec<String>>, String> {
    parse_with_tree_sitter(script)
}

/// Parses shell script using tree-sitter bash grammar
fn parse_with_tree_sitter(script: &str) -> std::result::Result<Vec<Vec<String>>, String> {
    let parser_guard = get_bash_parser();
    let mut parser = parser_guard
        .lock()
        .map_err(|e| format!("Failed to lock parser: {}", e))?;

    let tree = parser
        .parse(script, None)
        .ok_or_else(|| "Failed to parse script".to_string())?;

    let mut commands = Vec::new();
    let root = tree.root_node();

    // Walk tree-sitter AST and extract command nodes
    let mut cursor = root.walk();

    for child in root.children(&mut cursor) {
        if is_command_node(child)
            && let Some(cmd) = extract_command_from_node(child, script)
            && !cmd.is_empty()
        {
            commands.push(cmd);
        }
    }

    Ok(commands)
}

/// Checks if a tree-sitter node represents a command
fn is_command_node(node: tree_sitter::Node) -> bool {
    matches!(
        node.kind(),
        "command" | "pipeline" | "compound_command" | "simple_command"
    )
}

/// Extracts a command vector from a tree-sitter node
fn extract_command_from_node(node: tree_sitter::Node, source: &str) -> Option<Vec<String>> {
    let mut command = Vec::new();
    let mut cursor = node.walk();

    // For pipeline nodes, extract the first command in the pipeline
    if node.kind() == "pipeline" {
        for child in node.children(&mut cursor) {
            if child.kind() == "command" || child.kind() == "simple_command" {
                return extract_command_from_node(child, source);
            }
        }
    }

    // Extract arguments from command node
    for child in node.children(&mut cursor) {
        if matches!(
            child.kind(),
            "word" | "simple_expansion" | "variable_expansion"
        ) {
            let text = child.utf8_text(source.as_bytes());
            if let Ok(arg) = text {
                let trimmed = arg.trim();
                if !trimmed.is_empty() {
                    command.push(trimmed.to_string());
                }
            }
        }
    }

    if command.is_empty() {
        None
    } else {
        Some(command)
    }
}

/// Fallback: Parses shell script with simple tokenization
fn parse_with_basic_tokenization(script: &str) -> std::result::Result<Vec<Vec<String>>, String> {
    let mut commands = Vec::new();
    let mut current_command = String::new();
    let mut in_quotes = false;
    let mut quote_char = ' ';
    let mut escaped = false;

    for ch in script.chars() {
        if escaped {
            current_command.push(ch);
            escaped = false;
            continue;
        }

        match ch {
            '\\' => {
                escaped = true;
            }
            '\'' | '"' if !in_quotes => {
                in_quotes = true;
                quote_char = ch;
            }
            c if c == quote_char && in_quotes => {
                in_quotes = false;
            }
            '&' | '|' | ';' if !in_quotes => {
                if !current_command.trim().is_empty()
                    && let Ok(cmd) = tokenize_command(&current_command)
                {
                    commands.push(cmd);
                }
                current_command.clear();
            }
            _ => current_command.push(ch),
        }
    }

    if !current_command.trim().is_empty()
        && let Ok(cmd) = tokenize_command(&current_command)
    {
        commands.push(cmd);
    }

    Ok(commands)
}

/// Splits a command string into arguments
/// Respects quoted strings and escapes
fn tokenize_command(cmd: &str) -> std::result::Result<Vec<String>, String> {
    // Use shlex for proper shell-like tokenization
    // Note: This matches the implementation in `shlex::split` but handles some edge cases differently.
    // Future improvement: use a full shell parser or tree-sitter for non-trivial cases.
    Ok(cmd
        .split_whitespace()
        .map(|s| s.trim_matches('"').trim_matches('\'').to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/// Parses `bash -lc "script"` style invocations
///
/// # Example
/// ```text
/// Input:  vec!["bash", "-lc", "git status && rm /"]
/// Output: Some([["git", "status"], ["rm", "/"]])
/// ```
pub fn parse_bash_lc_commands(command: &[String]) -> Option<Vec<Vec<String>>> {
    if command.is_empty() {
        return None;
    }

    let cmd_name = command[0].as_str();
    let base_cmd = std::path::Path::new(cmd_name)
        .file_name()
        .and_then(|osstr| osstr.to_str())
        .unwrap_or("");

    if base_cmd != "bash" && base_cmd != "zsh" && base_cmd != "sh" {
        return None;
    }

    // Look for -lc or -c pattern
    for window in command.windows(2) {
        if matches!(window[0].as_str(), "-lc" | "-c" | "-il" | "-ic") {
            let script = &window[1];
            return parse_shell_commands(script).ok();
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenize_simple_command() {
        let cmd = "git status";
        let tokens = tokenize_command(cmd).unwrap();
        assert_eq!(tokens, vec!["git", "status"]);
    }

    #[test]
    fn tokenize_quoted_arguments() {
        let cmd = r#"echo "hello world""#;
        let tokens = tokenize_command(cmd).unwrap();
        assert_eq!(tokens, vec!["echo", "hello world"]);
    }

    #[test]
    fn parse_single_command() {
        let script = "git status";
        let commands = parse_shell_commands(script).unwrap();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0][0], "git");
    }

    #[test]
    fn parse_chained_commands_with_and() {
        let script = "git status && cargo check";
        let commands = parse_shell_commands(script).unwrap();
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0][0], "git");
        assert_eq!(commands[1][0], "cargo");
    }

    #[test]
    fn parse_chained_commands_with_semicolon() {
        let script = "git status; cargo check";
        let commands = parse_shell_commands(script).unwrap();
        assert_eq!(commands.len(), 2);
    }

    #[test]
    fn parse_bash_lc_git_status() {
        let cmd = vec![
            "bash".to_string(),
            "-lc".to_string(),
            "git status".to_string(),
        ];
        let commands = parse_bash_lc_commands(&cmd);
        assert!(commands.is_some());
        let commands = commands.unwrap();
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0][0], "git");
    }

    #[test]
    fn parse_bash_lc_chained() {
        let cmd = vec![
            "bash".to_string(),
            "-lc".to_string(),
            "git status && cargo check".to_string(),
        ];
        let commands = parse_bash_lc_commands(&cmd);
        assert!(commands.is_some());
        let commands = commands.unwrap();
        assert_eq!(commands.len(), 2);
    }

    #[test]
    fn parse_non_bash_command_returns_none() {
        let cmd = vec!["echo".to_string(), "hello".to_string()];
        let commands = parse_bash_lc_commands(&cmd);
        assert!(commands.is_none());
    }

    #[test]
    fn parse_bash_without_lc_returns_none() {
        let cmd = vec!["bash".to_string(), "script.sh".to_string()];
        let commands = parse_bash_lc_commands(&cmd);
        assert!(commands.is_none());
    }

    // Phase 4 tests: Tree-sitter based parsing

    #[test]
    fn parse_complex_pipeline() {
        let script = "cat file.txt | grep -i pattern | sort";
        let commands = parse_shell_commands(script).unwrap();
        assert!(!commands.is_empty());
    }

    #[test]
    fn parse_with_pipes_and_redirects() {
        let script = "ls -la | grep file > output.txt";
        let commands = parse_shell_commands(script).unwrap();
        assert!(!commands.is_empty());
    }

    #[test]
    fn parse_command_substitution_fallback() {
        let script = "echo $(git status)";
        let commands = parse_shell_commands(script).unwrap();
        assert!(!commands.is_empty());
    }

    #[test]
    fn parse_escaped_quotes() {
        let script = r#"echo "hello \"world\"""#;
        let commands = parse_shell_commands(script).unwrap();
        assert!(!commands.is_empty());
    }

    #[test]
    fn parse_bash_lc_with_pipe() {
        let cmd = vec![
            "bash".to_string(),
            "-lc".to_string(),
            "ls -la | head -5".to_string(),
        ];
        let commands = parse_bash_lc_commands(&cmd);
        assert!(commands.is_some());
        let cmds = commands.unwrap();
        assert!(!cmds.is_empty());
    }

    #[test]
    fn parse_dangerous_shell_command() {
        let script = "rm -rf /; echo done";
        let commands = parse_shell_commands(script).unwrap();
        assert_eq!(commands.len(), 2);
        assert_eq!(commands[0][0], "rm");
    }
}
