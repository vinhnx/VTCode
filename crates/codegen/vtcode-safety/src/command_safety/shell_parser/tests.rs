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
fn parse_loop_body_commands() {
    let script = "cd crates/codegen/vtcode-core/src/tools/registry && for f in *.rs; do echo \"=== $f ===\"; grep -nE '^(pub )?(struct|enum|fn)' \"$f\" | head -50; done";
    let commands = parse_shell_commands(script).unwrap();

    assert_eq!(commands[0], vec!["cd", "crates/codegen/vtcode-core/src/tools/registry"]);
    assert!(
        commands
            .iter()
            .any(|command| command.first().is_some_and(|name| name == "echo"))
    );
    assert!(
        commands
            .iter()
            .any(|command| command.first().is_some_and(|name| name == "grep"))
    );
    assert!(
        commands
            .iter()
            .any(|command| command.first().is_some_and(|name| name == "head"))
    );
}

#[test]
fn parse_chained_commands_with_semicolon() {
    let script = "git status; cargo check";
    let commands = parse_shell_commands(script).unwrap();
    assert_eq!(commands.len(), 2);
}

#[test]
fn parse_bash_lc_git_status() {
    let cmd = vec!["bash".to_string(), "-lc".to_string(), "git status".to_string()];
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
fn parse_tree_sitter_preserves_command_name_with_quoted_args() {
    let script = r#"echo "fish and chips""#;
    let commands = parse_shell_commands_tree_sitter(script).unwrap();
    assert!(!commands.is_empty());
    assert_eq!(commands[0][0], "echo");
}

#[test]
fn parse_tree_sitter_preserves_single_and_ansi_quoted_args() {
    let script = r#"printf '\n' && git diff '--output=out.txt' && printf $'\n'"#;
    let commands = parse_shell_commands_tree_sitter(script).unwrap();
    assert!(
        commands
            .iter()
            .any(|command| command.iter().any(|word| word.contains("--output=out.txt")))
    );
    assert!(commands.iter().any(|command| command.iter().any(|word| word.contains("\\n"))));
}

#[test]
fn dynamic_syntax_allows_literal_escapes_inside_double_quoted_arguments() {
    assert!(!contains_dynamic_shell_syntax(r#"rg -n "\[profile|lto|codegen-units|strip" Cargo.toml"#));
    assert!(!contains_dynamic_shell_syntax(r#"printf "\nTop-level:\n""#));
    assert!(!contains_dynamic_shell_syntax(r#"printf "quoted: \"value\"""#));
    assert!(contains_dynamic_shell_syntax(r#"echo "safe\"$(id)""#));
}

#[test]
fn dynamic_syntax_rejects_unquoted_escapes() {
    assert!(contains_dynamic_shell_syntax(r"rg -n \[profile Cargo.toml"));
}

#[test]
fn output_redirection_guard_rejects_input_and_heredoc_shapes() {
    assert!(has_only_output_redirections("cargo check > build.log 2>&1"));
    assert!(has_only_output_redirections("cargo check | head -40 > build.log"));
    assert!(has_only_output_redirections("cargo check &> build.log"));
    assert!(has_only_output_redirections("cargo check &>> build.log"));
    assert!(!has_only_output_redirections("cargo check < build-input.log"));
    assert!(!has_only_output_redirections("cargo check <<'EOF'\ninput\nEOF"));
    assert!(!has_only_output_redirections("cargo check > $(printf build.log)"));
    assert!(!has_only_output_redirections("cargo check > build.log &"));
    assert!(!has_only_output_redirections("cargo check 2>&-"));
}

#[test]
fn strict_tree_sitter_parser_rejects_incomplete_shell_syntax() {
    assert!(parse_shell_commands_tree_sitter("cargo check &&").is_err());
    assert!(parse_shell_commands_tree_sitter("echo '").is_err());
}

#[test]
fn parse_bash_lc_with_pipe() {
    let cmd = vec!["bash".to_string(), "-lc".to_string(), "ls -la | head -5".to_string()];
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

#[test]
fn prewarm_bash_parser_initializes_successfully() {
    prewarm_bash_parser().expect("bash parser should initialize");
}

#[test]
fn dynamic_find_syntax_is_detected_without_rejecting_quoted_globs() {
    assert!(contains_dynamic_find_syntax("find src -maxdepth 0 -exe$''c touch /tmp/VT_BYPASS_POC {} +"));
    assert!(!contains_dynamic_find_syntax("find src -type f -name '*.rs'"));
}

#[test]
fn cached_tree_hit_still_fails_closed_on_malformed_scripts() {
    // The tree cache is keyed by exact source text. The second call takes a
    // cache hit, so `has_error` must be preserved across the cache or a
    // malformed script would pass on the second parse after failing the
    // first — a fail-open regression.
    let malformed = "cargo check && ";
    assert!(parse_shell_commands_tree_sitter(malformed).is_err());
    assert!(
        parse_shell_commands_tree_sitter(malformed).is_err(),
        "cached tree must keep rejecting malformed scripts"
    );
}

#[test]
fn cached_tree_hit_preserves_extraction_across_repeated_and_mixed_calls() {
    // Cache hits must return the same grammar output for the same source,
    // across both strict and lenient entry points, so safety and approval
    // classification cannot diverge between the first and later parses.
    let script = "vtcode-cache-extraction-probe && grep -n pattern file | head -5";
    let first = parse_shell_commands(script).expect("initial parse");
    let second = parse_shell_commands(script).expect("cached parse");
    assert_eq!(first, second);
    assert_eq!(first.len(), 3);
    assert!(first.iter().any(|command| command.first().is_some_and(|name| name == "grep")));

    let strict = parse_shell_commands_tree_sitter(script).expect("strict parse of cached tree");
    assert_eq!(strict, first);
}
