//! Safe command registry: defines which commands and subcommands are safe to execute.
//!
//! This module implements the "safe-by-subcommand" pattern from Codex:
//! Instead of blocking entire commands, we maintain granular allowlists
//! of safe subcommands and forbid specific dangerous options.
//!
//! Example:
//! ```text
//! git branch     ✓ safe (read-only)
//! git reset      ✗ dangerous (destructive)
//! git status     ✓ safe (read-only)
//!
//! find .         ✓ safe
//! find . -delete ✗ dangerous (has -delete option)
//!
//! cargo check    ✓ safe (read-only check)
//! cargo clean    ✗ dangerous (destructive)
//! ```

use std::collections::HashMap;

/// Result of a command safety check
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SafetyDecision {
    /// Command is safe to execute
    Allow,
    /// Command is dangerous and should be blocked
    Deny(String),
    /// Safety status unknown; defer to policy evaluator
    Unknown,
}

/// Registry of safe commands and their safe subcommands/options
#[derive(Clone)]
pub struct SafeCommandRegistry {
    rules: HashMap<String, CommandRule>,
}

/// A rule for when a command is safe
#[derive(Clone)]
pub struct CommandRule {
    /// If Some, only these subcommands are allowed
    safe_subcommands: Option<Vec<String>>,
    /// These options make a command unsafe (e.g., "-delete" for find)
    forbidden_options: Vec<String>,
    /// Custom validation function for complex logic
    custom_check: Option<fn(&[String]) -> SafetyDecision>,
}

impl CommandRule {
    /// Creates a read-only safe command rule
    pub fn safe_readonly() -> Self {
        Self {
            safe_subcommands: None,
            forbidden_options: vec![],
            custom_check: None,
        }
    }

    /// Creates a rule with allowed subcommands
    pub fn with_allowed_subcommands(subcommands: Vec<&str>) -> Self {
        Self {
            safe_subcommands: Some(subcommands.into_iter().map(|s| s.to_string()).collect()),
            forbidden_options: vec![],
            custom_check: None,
        }
    }

    /// Creates a rule with forbidden options
    pub fn with_forbidden_options(options: Vec<&str>) -> Self {
        Self {
            safe_subcommands: None,
            forbidden_options: options.into_iter().map(|s| s.to_string()).collect(),
            custom_check: None,
        }
    }
}

impl SafeCommandRegistry {
    /// Creates a new empty registry
    pub fn new() -> Self {
        Self {
            rules: Self::default_rules(),
        }
    }

    /// Builds the default safe command rules (Codex patterns + VT Code extensions)
    fn default_rules() -> HashMap<String, CommandRule> {
        let mut rules = HashMap::new();

        // ──── Git (safe: status, log, diff, show; branch is conditionally safe) ────
        // Note: git branch is NOT in the safe list because branch deletion (-d/-D/--delete)
        // is destructive. Only read-only branch operations (--show-current, --list) are safe.
        rules.insert(
            "git".to_string(),
            CommandRule {
                safe_subcommands: Some(vec![
                    "status".to_string(),
                    "log".to_string(),
                    "diff".to_string(),
                    "show".to_string(),
                ]),
                forbidden_options: vec![],
                custom_check: Some(Self::check_git),
            },
        );

        // ──── Cargo (safe: check, build, clippy, fmt --check) ────
        rules.insert(
            "cargo".to_string(),
            CommandRule {
                safe_subcommands: Some(vec![
                    "check".to_string(),
                    "build".to_string(),
                    "clippy".to_string(),
                ]),
                forbidden_options: vec![],
                custom_check: Some(Self::check_cargo),
            },
        );

        // ──── Find (forbid: -exec, -delete, -fls, -fprint*, -fprintf) ────
        rules.insert(
            "find".to_string(),
            CommandRule {
                safe_subcommands: None,
                forbidden_options: vec![
                    "-exec".to_string(),
                    "-execdir".to_string(),
                    "-ok".to_string(),
                    "-okdir".to_string(),
                    "-delete".to_string(),
                    "-fls".to_string(),
                    "-fprint".to_string(),
                    "-fprint0".to_string(),
                    "-fprintf".to_string(),
                ],
                custom_check: None,
            },
        );

        // ──── Base64 (forbid: -o, --output) ────
        rules.insert(
            "base64".to_string(),
            CommandRule {
                safe_subcommands: None,
                forbidden_options: vec!["-o".to_string(), "--output".to_string()],
                custom_check: Some(Self::check_base64),
            },
        );

        // ──── Sed (only allow -n {N|M,N}p pattern) ────
        rules.insert(
            "sed".to_string(),
            CommandRule {
                safe_subcommands: None,
                forbidden_options: vec![],
                custom_check: Some(Self::check_sed),
            },
        );

        // ──── Ripgrep (forbid: --pre, --hostname-bin, -z, --search-zip) ────
        rules.insert(
            "rg".to_string(),
            CommandRule {
                safe_subcommands: None,
                forbidden_options: vec![
                    "--pre".to_string(),
                    "--hostname-bin".to_string(),
                    "--search-zip".to_string(),
                    "-z".to_string(),
                ],
                custom_check: None,
            },
        );

        // ──── Safe read-only tools ────
        for cmd in &[
            "cat", "ls", "pwd", "echo", "grep", "head", "tail", "wc", "tr", "cut", "paste", "sort",
            "uniq", "rev", "seq", "expr", "uname", "whoami", "id", "stat", "which",
        ] {
            rules.insert(
                cmd.to_string(),
                CommandRule {
                    safe_subcommands: None,
                    forbidden_options: vec![],
                    custom_check: None,
                },
            );
        }

        rules
    }

    /// Checks if a command is safe
    pub fn is_safe(&self, command: &[String]) -> SafetyDecision {
        if command.is_empty() {
            return SafetyDecision::Unknown;
        }

        let cmd_name = Self::extract_command_name(&command[0]);
        let Some(rule) = self.rules.get(cmd_name) else {
            return SafetyDecision::Unknown;
        };

        // Run custom check if defined
        if let Some(check_fn) = rule.custom_check {
            let result = check_fn(command);
            if result != SafetyDecision::Unknown {
                return result;
            }
        }

        // Check safe subcommands (if restricted list exists)
        if let Some(ref safe_subs) = rule.safe_subcommands {
            if command.len() < 2 {
                return SafetyDecision::Deny(format!("Command {} requires a subcommand", cmd_name));
            }
            let subcommand = &command[1];
            if !safe_subs.contains(subcommand) {
                return SafetyDecision::Deny(format!(
                    "Subcommand {} not in safe list for {}",
                    subcommand, cmd_name
                ));
            }
        }

        // Check forbidden options
        for forbidden in &rule.forbidden_options {
            if command
                .iter()
                .any(|arg| arg == forbidden || arg.starts_with(&format!("{}=", forbidden)))
            {
                return SafetyDecision::Deny(format!(
                    "Option {} is not allowed for {}",
                    forbidden, cmd_name
                ));
            }
        }

        SafetyDecision::Allow
    }

    /// Extract base command name from full path (e.g., "/usr/bin/git" -> "git")
    fn extract_command_name(cmd: &str) -> &str {
        std::path::Path::new(cmd)
            .file_name()
            .and_then(|osstr| osstr.to_str())
            .unwrap_or(cmd)
    }

    // ──── Custom Checks ────

    /// Git: allow status, log, diff, show; branch only for read-only operations
    fn check_git(command: &[String]) -> SafetyDecision {
        if command.len() < 2 {
            return SafetyDecision::Unknown;
        }

        // Use the shared git subcommand finder to skip global options
        let subcommands = &["status", "log", "diff", "show", "branch"];
        let Some((idx, subcommand)) =
            crate::command_safety::dangerous_commands::find_git_subcommand(command, subcommands)
        else {
            return SafetyDecision::Unknown;
        };

        match subcommand {
            "status" | "log" | "diff" | "show" => SafetyDecision::Allow,
            "branch" => {
                // Only allow read-only branch operations
                let branch_args = &command[idx + 1..];
                let is_read_only = branch_args.iter().all(|arg| {
                    let arg = arg.as_str();
                    // Safe: --show-current, --list, -l (list), -v (verbose), -a (all), -r (remote)
                    // Unsafe: -d, -D, --delete, -m, -M, --move, -c, -C, --create
                    matches!(
                        arg,
                        "--show-current"
                            | "--list"
                            | "-l"
                            | "-v"
                            | "-vv"
                            | "-a"
                            | "-r"
                            | "--all"
                            | "--remote"
                            | "--verbose"
                            | "--format"
                    ) || arg.starts_with("--format=")
                        || arg.starts_with("--sort=")
                        || arg.starts_with("--contains=")
                        || arg.starts_with("--no-contains=")
                        || arg.starts_with("--merged=")
                        || arg.starts_with("--no-merged=")
                        || arg.starts_with("--points-at=")
                });

                // Also check for any delete/move/create flags
                let has_dangerous_flag = branch_args.iter().any(|arg| {
                    let arg = arg.as_str();
                    matches!(
                        arg,
                        "-d" | "-D"
                            | "--delete"
                            | "-m"
                            | "-M"
                            | "--move"
                            | "-c"
                            | "-C"
                            | "--create"
                            | "--set-upstream"
                            | "--set-upstream-to"
                            | "--unset-upstream"
                    ) || arg.starts_with("--delete=")
                        || arg.starts_with("--move=")
                        || arg.starts_with("--create=")
                        || arg.starts_with("--set-upstream-to=")
                });

                if has_dangerous_flag {
                    SafetyDecision::Deny(
                        "git branch with modification flags is not allowed".to_string(),
                    )
                } else if is_read_only || branch_args.is_empty() {
                    SafetyDecision::Allow
                } else {
                    // Unknown flags - be conservative
                    SafetyDecision::Deny(
                        "git branch with unknown flags requires approval".to_string(),
                    )
                }
            }
            _ => SafetyDecision::Unknown,
        }
    }

    /// Cargo: allow check, build, clippy
    fn check_cargo(command: &[String]) -> SafetyDecision {
        if command.len() < 2 {
            return SafetyDecision::Unknown;
        }
        match command[1].as_str() {
            "check" | "build" | "clippy" => SafetyDecision::Allow,
            "fmt" => {
                // cargo fmt --check is safe (read-only)
                if command.contains(&"--check".to_string()) {
                    SafetyDecision::Allow
                } else {
                    SafetyDecision::Deny("cargo fmt without --check is not allowed".to_string())
                }
            }
            _ => SafetyDecision::Deny(format!(
                "cargo {} is not in safe subcommand list",
                command[1]
            )),
        }
    }

    /// Base64: forbid output redirection
    fn check_base64(command: &[String]) -> SafetyDecision {
        const UNSAFE_OPTIONS: &[&str] = &["-o", "--output"];

        for arg in command.iter().skip(1) {
            if UNSAFE_OPTIONS.contains(&arg.as_str()) {
                return SafetyDecision::Deny(format!(
                    "base64 {} is not allowed (output redirection)",
                    arg
                ));
            }
            if arg.starts_with("--output=") || (arg.starts_with("-o") && arg != "-o") {
                return SafetyDecision::Deny(
                    "base64 output redirection is not allowed".to_string(),
                );
            }
        }
        SafetyDecision::Unknown
    }

    /// Sed: only allow `-n {N|M,N}p` pattern
    fn check_sed(command: &[String]) -> SafetyDecision {
        if command.len() <= 2 {
            return SafetyDecision::Unknown;
        }

        if command.len() <= 4
            && command.get(1).map(|s| s.as_str()) == Some("-n")
            && let Some(pattern) = command.get(2)
            && Self::is_valid_sed_n_arg(pattern)
        {
            return SafetyDecision::Allow;
        }

        SafetyDecision::Deny("sed only allows safe pattern: sed -n {N|M,N}p".to_string())
    }

    /// Helper: validate sed -n pattern
    fn is_valid_sed_n_arg(arg: &str) -> bool {
        // Pattern must end with 'p'
        let Some(core) = arg.strip_suffix('p') else {
            return false;
        };

        // Split on ',' and validate
        let parts: Vec<&str> = core.split(',').collect();
        match parts.as_slice() {
            // Single number: e.g., "10"
            [num] => !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()),
            // Range: e.g., "1,5"
            [a, b] => {
                !a.is_empty()
                    && !b.is_empty()
                    && a.chars().all(|c| c.is_ascii_digit())
                    && b.chars().all(|c| c.is_ascii_digit())
            }
            _ => false,
        }
    }
}

impl Default for SafeCommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_status_is_safe() {
        let registry = SafeCommandRegistry::new();
        let cmd = vec!["git".to_string(), "status".to_string()];
        assert_eq!(registry.is_safe(&cmd), SafetyDecision::Allow);
    }

    #[test]
    fn git_reset_is_dangerous() {
        let registry = SafeCommandRegistry::new();
        let cmd = vec!["git".to_string(), "reset".to_string()];
        assert!(matches!(registry.is_safe(&cmd), SafetyDecision::Deny(_)));
    }

    #[test]
    fn cargo_check_is_safe() {
        let registry = SafeCommandRegistry::new();
        let cmd = vec!["cargo".to_string(), "check".to_string()];
        assert_eq!(registry.is_safe(&cmd), SafetyDecision::Allow);
    }

    #[test]
    fn cargo_clean_is_dangerous() {
        let registry = SafeCommandRegistry::new();
        let cmd = vec!["cargo".to_string(), "clean".to_string()];
        assert!(matches!(registry.is_safe(&cmd), SafetyDecision::Deny(_)));
    }

    #[test]
    fn cargo_fmt_without_check_is_dangerous() {
        let registry = SafeCommandRegistry::new();
        let cmd = vec!["cargo".to_string(), "fmt".to_string()];
        assert!(matches!(registry.is_safe(&cmd), SafetyDecision::Deny(_)));
    }

    #[test]
    fn cargo_fmt_with_check_is_safe() {
        let registry = SafeCommandRegistry::new();
        let cmd = vec![
            "cargo".to_string(),
            "fmt".to_string(),
            "--check".to_string(),
        ];
        assert_eq!(registry.is_safe(&cmd), SafetyDecision::Allow);
    }

    #[test]
    fn find_without_dangerous_options_is_unknown() {
        let registry = SafeCommandRegistry::new();
        let cmd = vec!["find".to_string(), ".".to_string()];
        assert_eq!(registry.is_safe(&cmd), SafetyDecision::Unknown);
    }

    #[test]
    fn find_with_delete_is_dangerous() {
        let registry = SafeCommandRegistry::new();
        let cmd = vec!["find".to_string(), ".".to_string(), "-delete".to_string()];
        assert!(matches!(registry.is_safe(&cmd), SafetyDecision::Deny(_)));
    }

    #[test]
    fn find_with_exec_is_dangerous() {
        let registry = SafeCommandRegistry::new();
        let cmd = vec![
            "find".to_string(),
            ".".to_string(),
            "-exec".to_string(),
            "rm".to_string(),
        ];
        assert!(matches!(registry.is_safe(&cmd), SafetyDecision::Deny(_)));
    }

    #[test]
    fn base64_without_output_is_unknown() {
        let registry = SafeCommandRegistry::new();
        let cmd = vec!["base64".to_string(), "file.txt".to_string()];
        assert_eq!(registry.is_safe(&cmd), SafetyDecision::Unknown);
    }

    #[test]
    fn base64_with_output_is_dangerous() {
        let registry = SafeCommandRegistry::new();
        let cmd = vec![
            "base64".to_string(),
            "file.txt".to_string(),
            "-o".to_string(),
            "output.txt".to_string(),
        ];
        assert!(matches!(registry.is_safe(&cmd), SafetyDecision::Deny(_)));
    }

    #[test]
    fn sed_n_single_line_is_safe() {
        let registry = SafeCommandRegistry::new();
        let cmd = vec!["sed".to_string(), "-n".to_string(), "10p".to_string()];
        assert_eq!(registry.is_safe(&cmd), SafetyDecision::Allow);
    }

    #[test]
    fn sed_n_range_is_safe() {
        let registry = SafeCommandRegistry::new();
        let cmd = vec!["sed".to_string(), "-n".to_string(), "1,5p".to_string()];
        assert_eq!(registry.is_safe(&cmd), SafetyDecision::Allow);
    }

    #[test]
    fn sed_without_n_is_dangerous() {
        let registry = SafeCommandRegistry::new();
        let cmd = vec!["sed".to_string(), "s/foo/bar/g".to_string()];
        assert!(matches!(registry.is_safe(&cmd), SafetyDecision::Deny(_)));
    }

    #[test]
    fn rg_with_pre_is_dangerous() {
        let registry = SafeCommandRegistry::new();
        let cmd = vec![
            "rg".to_string(),
            "--pre".to_string(),
            "some_command".to_string(),
            "pattern".to_string(),
        ];
        assert!(matches!(registry.is_safe(&cmd), SafetyDecision::Deny(_)));
    }

    #[test]
    fn cat_is_always_safe() {
        let registry = SafeCommandRegistry::new();
        let cmd = vec!["cat".to_string(), "file.txt".to_string()];
        assert_eq!(registry.is_safe(&cmd), SafetyDecision::Allow);
    }

    #[test]
    fn extract_command_name_from_path() {
        assert_eq!(
            SafeCommandRegistry::extract_command_name("/usr/bin/git"),
            "git"
        );
        assert_eq!(
            SafeCommandRegistry::extract_command_name("/usr/local/bin/cargo"),
            "cargo"
        );
        assert_eq!(SafeCommandRegistry::extract_command_name("git"), "git");
    }
}
