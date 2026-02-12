//! Command safety detection module
//!
//! Implements granular command safety evaluation based on subcommands and options,
//! following patterns from OpenAI's Codex project.
//!
//! Features:
//! - Safe-by-default subcommand allowlists (e.g., `git` only allows `branch|status|log`)
//! - Per-option blacklists (e.g., `find` forbids `-delete`, `-exec`)
//! - Shell chain parsing for `bash -lc "..."` scripts
//! - Windows/PowerShell-specific dangerous command detection
//! - Recursive dangerous command detection with `sudo` unwrapping
//! - Audit logging for compliance
//! - LRU caching for performance

pub mod audit;
pub mod cache;
pub mod command_db;
pub mod dangerous_commands;
pub mod safe_command_registry;
pub mod shell_parser;
pub mod unified;
#[cfg(windows)]
pub mod windows;
#[cfg(windows)]
pub mod windows_cmdlet_db;
#[cfg(windows)]
pub mod windows_com_analyzer;
#[cfg(windows)]
pub mod windows_enhanced;
#[cfg(windows)]
pub mod windows_registry_filter;

#[cfg(test)]
mod integration_tests;

pub use audit::{AuditEntry, SafetyAuditLogger};
pub use cache::SafetyDecisionCache;
pub use command_db::CommandDatabase;
pub use dangerous_commands::command_might_be_dangerous;
pub use safe_command_registry::{SafeCommandRegistry, SafetyDecision};
pub use shell_parser::parse_bash_lc_commands;
pub use unified::{
    EvaluationReason, EvaluationResult, PolicyAwareEvaluator, UnifiedCommandEvaluator,
};
#[cfg(windows)]
pub use windows_cmdlet_db::{CmdletCategory, CmdletDatabase, CmdletInfo, CmdletSeverity};
#[cfg(windows)]
pub use windows_com_analyzer::{ComObjectAnalyzer, ComObjectContext, ComObjectInfo, ComRiskLevel};
#[cfg(windows)]
pub use windows_enhanced::is_dangerous_windows_enhanced;
#[cfg(windows)]
pub use windows_registry_filter::{
    RegistryAccessFilter, RegistryAccessPattern, RegistryPathInfo, RegistryRiskLevel,
};

/// Evaluates if a command is safe to execute.
/// Returns true if the command passes all safety checks.
pub fn is_safe_command(registry: &SafeCommandRegistry, command: &[String]) -> bool {
    if command.is_empty() {
        return false;
    }

    // Check dangerous commands first
    if command_might_be_dangerous(command) {
        return false;
    }

    // Check safe command registry
    matches!(registry.is_safe(command), SafetyDecision::Allow)
}

/// Evaluate a shell command string by parsing it into subcommands and checking
/// each with the centralized dangerous-command detector.
///
/// Falls back to whitespace tokenization when structured parsing fails.
pub fn shell_string_might_be_dangerous(command: &str) -> bool {
    if let Ok(parsed_commands) = shell_parser::parse_shell_commands(command)
        && parsed_commands
            .iter()
            .any(|cmd| !cmd.is_empty() && command_might_be_dangerous(cmd))
    {
        return true;
    }

    let fallback_tokens: Vec<String> = command
        .split_whitespace()
        .map(ToString::to_string)
        .collect();
    !fallback_tokens.is_empty() && command_might_be_dangerous(&fallback_tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_command_is_not_safe() {
        let registry = SafeCommandRegistry::new();
        assert!(!is_safe_command(&registry, &[]));
    }

    #[test]
    fn shell_string_detects_dangerous_sequence() {
        assert!(shell_string_might_be_dangerous(
            "echo ok && git reset --hard HEAD~1"
        ));
    }
}
