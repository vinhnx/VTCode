mod classify;
mod detect;
mod normalize;
mod paths;

pub(crate) use detect::detect_explicit_run_command;
pub(crate) use normalize::strip_run_command_prefixes;
pub(crate) use paths::shell_quote_if_needed;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_explicit_run_command_basic() {
        let result = detect_explicit_run_command("run ls -a");
        assert!(result.is_some());
        let (tool_name, args) = result.expect("command expected");
        assert_eq!(tool_name, "unified_exec");
        assert_eq!(args["command"], "ls -a");
    }

    #[test]
    fn test_detect_explicit_run_command_git() {
        let result = detect_explicit_run_command("run git status");
        assert!(result.is_some());
        let (tool_name, args) = result.expect("command expected");
        assert_eq!(tool_name, "unified_exec");
        assert_eq!(args["command"], "git status");
    }

    #[test]
    fn test_detect_explicit_run_command_cargo() {
        let result = detect_explicit_run_command("run cargo build --release");
        assert!(result.is_some());
        let (tool_name, args) = result.expect("command expected");
        assert_eq!(tool_name, "unified_exec");
        assert_eq!(args["command"], "cargo build --release");
    }

    #[test]
    fn test_detect_explicit_run_command_case_insensitive() {
        let result = detect_explicit_run_command("Run npm install");
        assert!(result.is_some());
        let (tool_name, args) = result.expect("command expected");
        assert_eq!(tool_name, "unified_exec");
        assert_eq!(args["command"], "npm install");
    }

    #[test]
    fn test_detect_explicit_run_command_natural_language_rejected() {
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
        assert!(detect_explicit_run_command("ls -a").is_none());
        assert!(detect_explicit_run_command("please run ls").is_none());
        assert!(detect_explicit_run_command("can you run git status").is_none());
    }

    #[test]
    fn test_detect_show_diff_direct_command() {
        let result = detect_explicit_run_command("show diff src/main.rs");
        assert!(result.is_some());
        let (tool_name, args) = result.expect("direct command expected");
        assert_eq!(tool_name, "unified_exec");
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
    fn test_detect_show_diff_normalizes_on_prefixed_file_mentions() {
        let result =
            detect_explicit_run_command("show diff on @vtcode-core/src/tools/registry/policy.rs");
        assert!(result.is_some());
        let (_, args) = result.expect("direct command expected");
        assert_eq!(
            args["command"],
            "git diff -- vtcode-core/src/tools/registry/policy.rs"
        );
    }

    #[test]
    fn test_detect_show_diff_quotes_whitespace_paths_after_mention_normalization() {
        let result = detect_explicit_run_command("show diff on @\"docs/file with spaces.md\"");
        assert!(result.is_some());
        let (_, args) = result.expect("direct command expected");
        assert_eq!(args["command"], "git diff -- 'docs/file with spaces.md'");
    }

    #[test]
    fn test_detect_show_diff_quotes_shell_sensitive_paths() {
        let result = detect_explicit_run_command("show diff on @\"docs/file(1).md\"");
        assert!(result.is_some());
        let (_, args) = result.expect("direct command expected");
        assert_eq!(args["command"], "git diff -- 'docs/file(1).md'");
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
    fn test_detect_explicit_run_command_normalizes_unix_on_file_mention() {
        let result = detect_explicit_run_command("run ls on @src");
        assert!(result.is_some());
        let (_, args) = result.expect("normalized command expected");
        assert_eq!(args["command"], "ls src");
    }

    #[test]
    fn test_detect_explicit_run_command_normalizes_grep_for_on_pattern() {
        let result = detect_explicit_run_command("run rg for TODO on src");
        assert!(result.is_some());
        let (_, args) = result.expect("normalized command expected");
        assert_eq!(args["command"], "rg TODO src");
    }

    #[test]
    fn test_detect_explicit_run_command_normalizes_grep_target_file_mention() {
        let result = detect_explicit_run_command("run rg for TODO on @src");
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

    #[test]
    fn test_detect_explicit_run_command_normalizes_find_base_file_mention() {
        let result = detect_explicit_run_command("run find on @src for *.rs");
        assert!(result.is_some());
        let (_, args) = result.expect("normalized command expected");
        assert_eq!(args["command"], "find src -name *.rs");
    }

    #[test]
    fn test_detect_explicit_run_command_normalizes_pytest_file_mention_path() {
        let result = detect_explicit_run_command("run pytest on @tests/unit/test_shell.py");
        assert!(result.is_some());
        let (_, args) = result.expect("normalized command expected");
        assert_eq!(args["command"], "pytest tests/unit/test_shell.py");
    }
}
