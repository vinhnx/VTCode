use anyhow::{Result, anyhow};
use regex::Regex;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tracing::warn;

#[derive(Clone, Debug)]
pub struct ShellPolicyCacheEntry {
    pub signature: u64,
    pub deny_regexes: Vec<(String, Regex)>,
    pub deny_globs: Vec<(String, Regex)>,
}

pub struct ShellPolicyChecker {
    cache: Option<ShellPolicyCacheEntry>,
    commands_config: Option<crate::config::CommandsConfig>,
}

impl ShellPolicyChecker {
    pub fn new() -> Self {
        Self {
            cache: None,
            commands_config: None,
        }
    }
}

impl Default for ShellPolicyChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellPolicyChecker {
    pub fn set_commands_config(&mut self, commands_config: &crate::config::CommandsConfig) {
        self.commands_config = Some(commands_config.clone());
        self.reset_cache();
    }

    pub fn commands_config(&self) -> Option<&crate::config::CommandsConfig> {
        self.commands_config.as_ref()
    }

    pub fn check_command(
        &mut self,
        command: &str,
        agent_type: &str,
        deny_regex_patterns: &[String],
        deny_glob_patterns: &[String],
    ) -> Result<()> {
        let mut hasher = DefaultHasher::new();
        deny_regex_patterns.hash(&mut hasher);
        deny_glob_patterns.hash(&mut hasher);
        let signature = hasher.finish();

        let entry = if let Some(ref entry) = self.cache
            && entry.signature == signature
        {
            entry
        } else {
            let compiled_regexes = deny_regex_patterns
                .iter()
                .filter_map(|pattern| {
                    if pattern.is_empty() { return None; }
                    match Regex::new(pattern) {
                        Ok(re) => Some((pattern.clone(), re)),
                        Err(err) => {
                            warn!(agent = agent_type, pattern, error = %err, "Invalid deny regex pattern skipped");
                            None
                        }
                    }
                })
                .collect::<Vec<_>>();

            let compiled_globs = deny_glob_patterns
                .iter()
                .filter_map(|pattern| {
                    if pattern.is_empty() { return None; }
                    let re_pattern = format!("^{}$", regex::escape(pattern).replace(r"\*", ".*").replace(r"\?", "."));
                    match Regex::new(&re_pattern) {
                        Ok(re) => Some((pattern.clone(), re)),
                        Err(err) => {
                            warn!(agent = agent_type, pattern, error = %err, "Invalid deny glob pattern skipped");
                            None
                        }
                    }
                })
                .collect::<Vec<_>>();

            let new_entry = ShellPolicyCacheEntry {
                signature,
                deny_regexes: compiled_regexes,
                deny_globs: compiled_globs,
            };
            self.cache = Some(new_entry);
            self.cache
                .as_ref()
                .ok_or_else(|| anyhow!("Failed to initialize shell policy cache entry"))?
        };

        // Split compound commands on shell operators (&&, ||, ;) and validate
        // each sub-command independently. This prevents a denied sub-command
        // (e.g. `rm -f /tmp/file`) from blocking the entire compound command
        // when paired with a safe sub-command (e.g. `sg run ...`).
        for sub_command in split_compound_command(command) {
            let sub = sub_command.trim();
            if sub.is_empty() {
                continue;
            }

            for (pattern, compiled) in &entry.deny_regexes {
                if compiled.is_match(sub) {
                    return Err(anyhow!(
                        "Shell command denied by agent regex policy: {}",
                        pattern
                    ));
                }
            }

            for (pattern, compiled) in &entry.deny_globs {
                if compiled.is_match(sub) {
                    return Err(anyhow!(
                        "Shell command denied by agent glob policy: {}",
                        pattern
                    ));
                }
            }
        }

        Ok(())
    }

    pub fn reset_cache(&mut self) {
        self.cache = None;
    }
}

/// Split a compound shell command on `&&`, `||`, and `;` operators.
///
/// Returns individual sub-commands so each can be validated independently
/// against the deny policy. This prevents a denied sub-command in a compound
/// expression (e.g. `rm -f /tmp/file && sg run ...`) from blocking the entire
/// command when the other sub-commands are safe.
///
/// Note: This is a simple split that does not handle nested subshells,
/// quoted strings containing operators, or pipeline operators (`|`).
/// Pipeline operators are intentionally left unsplit because the individual
/// segments of a pipe are typically all executed together and should be
/// validated as a unit.
fn split_compound_command(command: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut current_start: usize = 0;
    // Use byte-level iteration since all operators (`&&`, `||`, `;`) are ASCII,
    // and Rust string slicing requires byte offsets, not char indices.
    let bytes = command.as_bytes();
    let len = bytes.len();

    let mut i: usize = 0;
    while i < len {
        if bytes[i] == b'&' && i + 1 < len && bytes[i + 1] == b'&' {
            // Found && — emit the sub-command before it
            parts.push(&command[current_start..i]);
            i += 2;
            // Skip whitespace after &&
            while i < len && bytes[i] == b' ' {
                i += 1;
            }
            current_start = i;
        } else if bytes[i] == b'|' && i + 1 < len && bytes[i + 1] == b'|' {
            // Found || — emit the sub-command before it
            parts.push(&command[current_start..i]);
            i += 2;
            while i < len && bytes[i] == b' ' {
                i += 1;
            }
            current_start = i;
        } else if bytes[i] == b';' {
            // Found ; — emit the sub-command before it
            parts.push(&command[current_start..i]);
            i += 1;
            while i < len && bytes[i] == b' ' {
                i += 1;
            }
            current_start = i;
        } else {
            i += 1;
        }
    }

    // Emit the final sub-command
    if current_start < len {
        parts.push(&command[current_start..]);
    }

    parts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_compound_and_operator() {
        let parts = split_compound_command("echo a && echo b");
        // Whitespace after && is skipped by the splitter.
        assert_eq!(parts, vec!["echo a ", "echo b"]);
    }

    #[test]
    fn split_compound_or_operator() {
        let parts = split_compound_command("echo a || echo b");
        assert_eq!(parts, vec!["echo a ", "echo b"]);
    }

    #[test]
    fn split_compound_semicolon() {
        let parts = split_compound_command("echo a; echo b");
        assert_eq!(parts, vec!["echo a", "echo b"]);
    }

    #[test]
    fn split_compound_mixed_operators() {
        let parts = split_compound_command("echo a && echo b || echo c; echo d");
        assert_eq!(parts, vec!["echo a ", "echo b ", "echo c", "echo d"]);
    }

    #[test]
    fn split_compound_leading_and() {
        let parts = split_compound_command("&& echo a");
        // Leading empty fragment before && is emitted but skipped by check_command.
        assert_eq!(parts, vec!["", "echo a"]);
    }

    #[test]
    fn split_compound_trailing_and() {
        let parts = split_compound_command("echo a &&");
        assert_eq!(parts, vec!["echo a "]);
    }

    #[test]
    fn split_compound_no_operators() {
        let parts = split_compound_command("echo hello world");
        assert_eq!(parts, vec!["echo hello world"]);
    }

    #[test]
    fn split_compound_pipe_not_split() {
        // Pipes are intentionally NOT split -- pipe segments execute as a unit.
        let parts = split_compound_command("echo a | grep b");
        assert_eq!(parts, vec!["echo a | grep b"]);
    }

    #[test]
    fn split_compound_empty_string() {
        let parts = split_compound_command("");
        // Empty input produces no parts (the final emit skips because current_start == len).
        assert!(parts.is_empty());
    }

    #[test]
    fn split_compound_non_ascii() {
        // Multi-byte UTF-8 characters before an operator must not cause a panic.
        // Before the byte-offset fix, this panicked because char indices != byte offsets.
        let parts = split_compound_command("echo 日本語 && echo test");
        assert_eq!(parts, vec!["echo 日本語 ", "echo test"]);
    }

    #[test]
    fn glob_star_matches_command() {
        let mut checker = ShellPolicyChecker::new();
        let globs = vec!["curl*".to_string()];
        // `curl https://example.com` should be denied by `curl*` glob.
        assert!(
            checker
                .check_command("curl https://example.com", "test", &[], &globs)
                .is_err()
        );
    }

    #[test]
    fn glob_question_mark_matches_single_char() {
        let mut checker = ShellPolicyChecker::new();
        let globs = vec!["rm ?".to_string()];
        // `rm f` matches `rm ?` (single char).
        assert!(checker.check_command("rm f", "test", &[], &globs).is_err());
        // `rm foo` does NOT match `rm ?` (multiple chars).
        assert!(checker.check_command("rm foo", "test", &[], &globs).is_ok());
    }

    #[test]
    fn glob_does_not_match_unrelated_command() {
        let mut checker = ShellPolicyChecker::new();
        let globs = vec!["curl*".to_string()];
        assert!(
            checker
                .check_command("echo hello", "test", &[], &globs)
                .is_ok()
        );
    }

    #[test]
    fn deny_regex_blocks_sub_command_after_split() {
        let mut checker = ShellPolicyChecker::new();
        let regexes = vec![r"\brm\b".to_string()];
        // `echo hello && rm -rf /tmp` -- the `rm` sub-command should be denied.
        assert!(
            checker
                .check_command("echo hello && rm -rf /tmp", "test", &regexes, &[])
                .is_err()
        );
        // `echo hello && echo world` -- both sub-commands are safe.
        assert!(
            checker
                .check_command("echo hello && echo world", "test", &regexes, &[])
                .is_ok()
        );
    }
}
