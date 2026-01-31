//! Dotfile protection configuration.
//!
//! Provides comprehensive protection for hidden configuration files (dotfiles)
//! to prevent automatic or implicit modifications by AI agents or automated tools.

use indexmap::IndexSet;
use serde::{Deserialize, Serialize};

/// Dotfile protection configuration.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DotfileProtectionConfig {
    /// Enable dotfile protection globally.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Require explicit user confirmation for any dotfile modification.
    #[serde(default = "default_true")]
    pub require_explicit_confirmation: bool,

    /// Enable immutable audit logging of all dotfile access attempts.
    #[serde(default = "default_true")]
    pub audit_logging_enabled: bool,

    /// Path to the audit log file.
    #[serde(default = "default_audit_log_path")]
    pub audit_log_path: String,

    /// Prevent cascading modifications (one dotfile change triggering others).
    #[serde(default = "default_true")]
    pub prevent_cascading_modifications: bool,

    /// Create backup before any permitted modification.
    #[serde(default = "default_true")]
    pub create_backups: bool,

    /// Directory for storing dotfile backups.
    #[serde(default = "default_backup_dir")]
    pub backup_directory: String,

    /// Maximum number of backups to retain per file.
    #[serde(default = "default_max_backups")]
    pub max_backups_per_file: usize,

    /// Preserve original file permissions and ownership.
    #[serde(default = "default_true")]
    pub preserve_permissions: bool,

    /// Whitelisted dotfiles that can be modified (after secondary confirmation).
    #[serde(default)]
    pub whitelist: IndexSet<String>,

    /// Additional dotfile patterns to protect (beyond defaults).
    #[serde(default)]
    pub additional_protected_patterns: Vec<String>,

    /// Block modifications during automated operations.
    #[serde(default = "default_true")]
    pub block_during_automation: bool,

    /// Operations that trigger extra protection.
    #[serde(default = "default_blocked_operations")]
    pub blocked_operations: Vec<String>,

    /// Secondary authentication required for whitelisted files.
    #[serde(default = "default_true")]
    pub require_secondary_auth_for_whitelist: bool,
}

impl Default for DotfileProtectionConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            require_explicit_confirmation: default_true(),
            audit_logging_enabled: default_true(),
            audit_log_path: default_audit_log_path(),
            prevent_cascading_modifications: default_true(),
            create_backups: default_true(),
            backup_directory: default_backup_dir(),
            max_backups_per_file: default_max_backups(),
            preserve_permissions: default_true(),
            whitelist: IndexSet::new(),
            additional_protected_patterns: Vec::new(),
            block_during_automation: default_true(),
            blocked_operations: default_blocked_operations(),
            require_secondary_auth_for_whitelist: default_true(),
        }
    }
}

/// Default protected dotfile patterns.
///
/// These patterns match common configuration files that should never be
/// modified automatically by AI agents or automated tools.
pub const DEFAULT_PROTECTED_DOTFILES: &[&str] = &[
    // Git configuration
    ".gitignore",
    ".gitattributes",
    ".gitmodules",
    ".gitconfig",
    ".git-credentials",
    // Editor configuration
    ".editorconfig",
    ".vscode/*",
    ".idea/*",
    ".cursor/*",
    // Environment files
    ".env",
    ".env.local",
    ".env.development",
    ".env.production",
    ".env.test",
    ".env.*",
    // Docker
    ".dockerignore",
    ".docker/*",
    // Node.js/JavaScript
    ".npmignore",
    ".npmrc",
    ".nvmrc",
    ".yarnrc",
    ".yarnrc.yml",
    ".pnpmrc",
    // Code formatting
    ".prettierrc",
    ".prettierrc.json",
    ".prettierrc.yml",
    ".prettierrc.yaml",
    ".prettierrc.js",
    ".prettierrc.cjs",
    ".prettierignore",
    // Linting
    ".eslintrc",
    ".eslintrc.json",
    ".eslintrc.yml",
    ".eslintrc.yaml",
    ".eslintrc.js",
    ".eslintrc.cjs",
    ".eslintignore",
    ".stylelintrc",
    ".stylelintrc.json",
    // Build tools
    ".babelrc",
    ".babelrc.json",
    ".babelrc.js",
    ".swcrc",
    ".tsbuildinfo",
    // Shell configuration
    ".zshrc",
    ".bashrc",
    ".bash_profile",
    ".bash_history",
    ".bash_logout",
    ".profile",
    ".zprofile",
    ".zshenv",
    ".zsh_history",
    ".shrc",
    ".kshrc",
    ".cshrc",
    ".tcshrc",
    ".fishrc",
    ".config/fish/*",
    // Editor configurations
    ".vimrc",
    ".vim/*",
    ".nvim/*",
    ".config/nvim/*",
    ".emacs",
    ".emacs.d/*",
    ".nanorc",
    // Terminal multiplexers
    ".tmux.conf",
    ".screenrc",
    // SSH and security
    ".ssh/*",
    ".ssh/config",
    ".ssh/known_hosts",
    ".ssh/authorized_keys",
    ".gnupg/*",
    ".gpg/*",
    // Cloud credentials
    ".aws/*",
    ".aws/config",
    ".aws/credentials",
    ".azure/*",
    ".config/gcloud/*",
    ".kube/*",
    ".kube/config",
    // Package managers and tools
    ".cargo/*",
    ".cargo/config.toml",
    ".cargo/credentials.toml",
    ".rustup/*",
    ".gem/*",
    ".bundle/*",
    ".pip/*",
    ".pypirc",
    ".poetry/*",
    ".pdm.toml",
    ".python-version",
    ".ruby-version",
    ".node-version",
    ".go-version",
    ".tool-versions",
    // Database
    ".pgpass",
    ".my.cnf",
    ".mongorc.js",
    ".rediscli_history",
    // Misc configuration
    ".netrc",
    ".curlrc",
    ".wgetrc",
    ".htaccess",
    ".htpasswd",
    // VT Code specific
    ".vtcode/*",
    ".vtcodegitignore",
    ".vtcode.toml",
    // Claude/AI
    ".claude/*",
    ".claude.json",
    ".agent/*",
    // Other common dotfiles
    ".inputrc",
    ".dircolors",
    ".mailrc",
    ".gitkeep",
    ".keep",
];

/// Operations that should never automatically modify dotfiles.
fn default_blocked_operations() -> Vec<String> {
    vec![
        "dependency_installation".into(),
        "code_formatting".into(),
        "git_operations".into(),
        "project_initialization".into(),
        "build_operations".into(),
        "test_execution".into(),
        "linting".into(),
        "auto_fix".into(),
    ]
}

#[inline]
const fn default_true() -> bool {
    true
}

#[inline]
fn default_audit_log_path() -> String {
    "~/.vtcode/dotfile_audit.log".into()
}

#[inline]
fn default_backup_dir() -> String {
    "~/.vtcode/dotfile_backups".into()
}

#[inline]
const fn default_max_backups() -> usize {
    10
}

impl DotfileProtectionConfig {
    /// Check if a file path matches a protected dotfile pattern.
    pub fn is_protected(&self, path: &str) -> bool {
        if !self.enabled {
            return false;
        }

        let filename = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path);

        // Check if it's a dotfile (starts with . or contains /. or is in a dotfile directory)
        let is_dotfile = filename.starts_with('.')
            || path.contains("/.")
            || path.starts_with('.')
            || Self::is_in_dotfile_directory(path);

        if !is_dotfile {
            return false;
        }

        // Check against default patterns
        for pattern in DEFAULT_PROTECTED_DOTFILES {
            if Self::matches_pattern(path, pattern) || Self::matches_pattern(filename, pattern) {
                return true;
            }
        }

        // Check against additional patterns
        for pattern in &self.additional_protected_patterns {
            if Self::matches_pattern(path, pattern) || Self::matches_pattern(filename, pattern) {
                return true;
            }
        }

        // Default: protect any file starting with .
        filename.starts_with('.') || Self::is_in_dotfile_directory(path)
    }

    /// Check if a path is inside a dotfile directory like .ssh, .aws, etc.
    fn is_in_dotfile_directory(path: &str) -> bool {
        let components: Vec<&str> = path.split('/').collect();
        for component in &components {
            if component.starts_with('.') && !component.is_empty() && *component != "." && *component != ".." {
                return true;
            }
        }
        false
    }

    /// Check if a file is in the whitelist.
    pub fn is_whitelisted(&self, path: &str) -> bool {
        let filename = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path);

        self.whitelist.contains(path) || self.whitelist.contains(filename)
    }

    /// Simple pattern matching with wildcard support.
    fn matches_pattern(path: &str, pattern: &str) -> bool {
        if pattern.contains('*') {
            // Handle wildcard patterns
            if pattern.ends_with("/*") {
                let prefix = &pattern[..pattern.len() - 2];
                path.starts_with(prefix) || path.contains(&format!("/{}/", prefix.trim_start_matches('.')))
            } else if pattern.ends_with(".*") {
                let prefix = &pattern[..pattern.len() - 1];
                path.starts_with(prefix)
            } else {
                // Simple glob matching
                let parts: Vec<&str> = pattern.split('*').collect();
                if parts.len() == 2 {
                    path.starts_with(parts[0]) && path.ends_with(parts[1])
                } else {
                    path == pattern
                }
            }
        } else {
            path == pattern || path.ends_with(&format!("/{}", pattern))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_protection() {
        let config = DotfileProtectionConfig::default();

        // Should be protected
        assert!(config.is_protected(".gitignore"));
        assert!(config.is_protected(".env"));
        assert!(config.is_protected(".env.local"));
        assert!(config.is_protected(".bashrc"));
        assert!(config.is_protected(".ssh/config"));
        assert!(config.is_protected("/home/user/.npmrc"));

        // Should not be protected (not dotfiles)
        assert!(!config.is_protected("README.md"));
        assert!(!config.is_protected("src/main.rs"));
    }

    #[test]
    fn test_whitelist() {
        let mut config = DotfileProtectionConfig::default();
        config.whitelist.insert(".gitignore".into());

        assert!(config.is_whitelisted(".gitignore"));
        assert!(!config.is_whitelisted(".env"));
    }

    #[test]
    fn test_disabled_protection() {
        let mut config = DotfileProtectionConfig::default();
        config.enabled = false;

        assert!(!config.is_protected(".gitignore"));
        assert!(!config.is_protected(".env"));
    }

    #[test]
    fn test_pattern_matching() {
        assert!(DotfileProtectionConfig::matches_pattern(".env.local", ".env.*"));
        assert!(DotfileProtectionConfig::matches_pattern(".env.production", ".env.*"));
        assert!(DotfileProtectionConfig::matches_pattern(".vscode/settings.json", ".vscode/*"));
    }
}
