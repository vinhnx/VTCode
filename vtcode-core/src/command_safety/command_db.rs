//! Command database: comprehensive safe command rules organized by category.
//!
//! This module organizes commands into semantic categories and provides
//! helper functions for building command rules at scale.
//!
//! Categories:
//! - File operations (read-only)
//! - Source control (read-only)
//! - Build systems
//! - Version managers
//! - Development tools
//! - Text processing
//! - System utilities (read-only)

use super::safe_command_registry::CommandRule;
use std::collections::HashMap;

/// Database of command rules by category
#[derive(Clone)]
pub struct CommandDatabase;

impl CommandDatabase {
    /// Returns all built-in command rules
    pub fn all_rules() -> HashMap<String, CommandRule> {
        let mut rules = HashMap::new();

        // File operations (read-only)
        for cmd in Self::file_operations() {
            rules.insert(cmd, CommandRule::safe_readonly());
        }

        // Source control (read-only safe + dangerous operations)
        for cmd in Self::source_control() {
            rules.insert(
                cmd,
                CommandRule::with_allowed_subcommands(vec![
                    "branch",
                    "status",
                    "log",
                    "diff",
                    "show",
                    "rev-parse",
                ]),
            );
        }

        // Build systems
        for cmd in Self::build_systems() {
            rules.insert(cmd, CommandRule::safe_readonly());
        }

        // Version managers (read-only)
        for cmd in Self::version_managers() {
            rules.insert(cmd, CommandRule::safe_readonly());
        }

        // Development tools
        for cmd in Self::development_tools() {
            rules.insert(cmd, CommandRule::safe_readonly());
        }

        // Text processing
        for cmd in Self::text_processing() {
            rules.insert(cmd, CommandRule::safe_readonly());
        }

        rules
    }

    /// File operations (read-only commands)
    fn file_operations() -> Vec<String> {
        vec![
            "cat", "head", "tail", "wc", "file", "stat", "ls", "find", "locate", "which",
            "whereis", "tree", "du", "df",
        ]
        .into_iter()
        .map(|s| s.to_string())
        .collect()
    }

    /// Source control commands
    fn source_control() -> Vec<String> {
        vec!["git", "hg", "svn", "bzr"]
            .into_iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// Build system commands
    fn build_systems() -> Vec<String> {
        vec!["cargo", "make", "cmake", "ninja", "gradle", "mvn", "ant"]
            .into_iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// Version manager commands
    fn version_managers() -> Vec<String> {
        vec!["rustup", "rbenv", "nvm", "pyenv", "jenv", "sdkman"]
            .into_iter()
            .map(|s| s.to_string())
            .collect()
    }

    /// Development tool commands
    fn development_tools() -> Vec<String> {
        vec![
            "node", "python", "ruby", "go", "java", "javac", "gcc", "g++", "clang", "rustc",
        ]
        .into_iter()
        .map(|s| s.to_string())
        .collect()
    }

    /// Text processing commands
    fn text_processing() -> Vec<String> {
        vec![
            "grep", "sed", "awk", "cut", "paste", "sort", "uniq", "tr", "rev", "expand",
            "unexpand", "fmt", "pr",
        ]
        .into_iter()
        .map(|s| s.to_string())
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn database_loads_without_error() {
        let rules = CommandDatabase::all_rules();
        assert!(!rules.is_empty());
    }

    #[test]
    fn database_includes_file_operations() {
        let rules = CommandDatabase::all_rules();
        assert!(rules.contains_key("cat"));
        assert!(rules.contains_key("grep"));
    }

    #[test]
    fn database_includes_source_control() {
        let rules = CommandDatabase::all_rules();
        assert!(rules.contains_key("git"));
        assert!(rules.contains_key("svn"));
    }

    #[test]
    fn database_includes_build_systems() {
        let rules = CommandDatabase::all_rules();
        assert!(rules.contains_key("cargo"));
        assert!(rules.contains_key("make"));
    }
}
