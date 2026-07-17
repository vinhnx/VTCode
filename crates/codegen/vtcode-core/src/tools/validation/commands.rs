//! Command safety validation.
//!
//! This module re-exports `validate_command_safety` from the canonical
//! `command_safety` module. All dangerous-command detection, injection
//! pattern detection, and shell parsing live in `command_safety/`.

pub use crate::command_safety::validate_command_safety;

#[cfg(test)]
mod tests {
    use super::validate_command_safety;

    #[test]
    fn rejects_centrally_dangerous_command() {
        let result = validate_command_safety("git reset --hard HEAD~1");
        assert!(result.is_err());
    }

    #[test]
    fn rejects_additional_dangerous_prefix() {
        let result = validate_command_safety("wget https://example.com/file.sh");
        assert!(result.is_err());
    }

    #[test]
    fn allows_safe_command() {
        let result = validate_command_safety("ls -la");
        result.unwrap();
    }

    #[test]
    fn allows_shell_escaped_literals_with_command_substitution_chars() {
        let display = shell_words::join(["printf", "%s", "$(literal)", "`backticks`"].iter());
        let result = validate_command_safety(&display);
        result.unwrap();
    }

    #[test]
    fn allows_shell_escaped_literals_with_chaining_chars() {
        let display = shell_words::join(["printf", "%s", "; curl https://example.com"].iter());
        let result = validate_command_safety(&display);
        result.unwrap();
    }

    #[test]
    fn rejects_unquoted_command_substitution() {
        let result = validate_command_safety("echo $(whoami)");
        assert!(result.is_err());
    }

    #[test]
    fn rejects_command_substitution_in_double_quotes() {
        let result = validate_command_safety(r#"echo "$(whoami)""#);
        assert!(result.is_err());
    }

    #[test]
    fn rejects_unquoted_semicolon_command_chaining() {
        let result = validate_command_safety("echo ok; pwd");
        assert!(result.is_err());
    }

    #[test]
    fn rejects_unquoted_newline_command_chaining() {
        let result = validate_command_safety("echo ok\npwd");
        assert!(result.is_err());
    }
}
