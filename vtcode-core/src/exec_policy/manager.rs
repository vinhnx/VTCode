//! Execution policy manager.
//!
//! Coordinates policy evaluation, approval requirements, and sandbox enforcement.
//! Inspired by Codex's ExecPolicyManager pattern.

use super::{
    approval::{AskForApproval, ExecApprovalRequirement, ExecPolicyAmendment},
    policy::{Decision, Policy, PolicyEvaluation, RuleMatch},
};
use crate::sandboxing::SandboxPolicy;
use anyhow::{Context, Result};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::RwLock;

/// Configuration for the execution policy manager.
#[derive(Debug, Clone)]
pub struct ExecPolicyConfig {
    /// Default sandbox policy for commands.
    pub default_sandbox_policy: SandboxPolicy,

    /// Default approval behavior.
    pub default_approval: AskForApproval,

    /// Whether to apply heuristics for unknown commands.
    pub use_heuristics: bool,

    /// Maximum command length before requiring confirmation.
    pub max_auto_approve_length: usize,
}

impl Default for ExecPolicyConfig {
    fn default() -> Self {
        Self {
            default_sandbox_policy: SandboxPolicy::ReadOnly,
            default_approval: AskForApproval::UnlessTrusted,
            use_heuristics: true,
            max_auto_approve_length: 256,
        }
    }
}

/// Manages execution policies and authorization decisions.
pub struct ExecPolicyManager {
    /// The current policy.
    policy: RwLock<Policy>,

    /// Trusted command patterns.
    trusted_patterns: RwLock<Vec<ExecPolicyAmendment>>,

    /// Active sandbox policy.
    sandbox_policy: RwLock<SandboxPolicy>,

    /// Configuration.
    config: ExecPolicyConfig,

    /// Workspace root for path validation.
    #[allow(dead_code)]
    workspace_root: PathBuf,

    /// Commands that have been pre-approved this session.
    session_approved: RwLock<HashSet<String>>,
}

impl ExecPolicyManager {
    /// Create a new policy manager.
    pub fn new(workspace_root: PathBuf, config: ExecPolicyConfig) -> Self {
        Self {
            policy: RwLock::new(Policy::empty()),
            trusted_patterns: RwLock::new(Vec::new()),
            sandbox_policy: RwLock::new(config.default_sandbox_policy.clone()),
            config,
            workspace_root,
            session_approved: RwLock::new(HashSet::new()),
        }
    }

    /// Create with default configuration.
    pub fn with_defaults(workspace_root: PathBuf) -> Self {
        Self::new(workspace_root, ExecPolicyConfig::default())
    }

    /// Load policy from a file.
    pub async fn load_policy(&self, path: &Path) -> Result<()> {
        let parser = super::parser::PolicyParser::new();
        let loaded_policy = parser
            .load_file(path)
            .await
            .context("Failed to load policy file")?;

        let mut policy = self.policy.write().await;
        *policy = loaded_policy;
        Ok(())
    }

    /// Add a prefix rule to the policy.
    pub async fn add_prefix_rule(&self, pattern: &[String], decision: Decision) -> Result<()> {
        let mut policy = self.policy.write().await;
        policy.add_prefix_rule(pattern, decision)
    }

    /// Add a trusted pattern amendment.
    pub async fn add_trusted_pattern(&self, amendment: ExecPolicyAmendment) {
        let mut patterns = self.trusted_patterns.write().await;
        patterns.push(amendment);
    }

    /// Set the sandbox policy.
    pub async fn set_sandbox_policy(&self, policy: SandboxPolicy) {
        let mut sandbox = self.sandbox_policy.write().await;
        *sandbox = policy;
    }

    /// Get the current sandbox policy.
    pub async fn sandbox_policy(&self) -> SandboxPolicy {
        self.sandbox_policy.read().await.clone()
    }

    /// Check if a command requires approval.
    pub async fn check_approval(&self, command: &[String]) -> ExecApprovalRequirement {
        // Check if already approved this session
        let command_key = command.join(" ");
        {
            let approved = self.session_approved.read().await;
            if approved.contains(&command_key) {
                return ExecApprovalRequirement::skip();
            }
        }

        // Check trusted patterns
        {
            let patterns = self.trusted_patterns.read().await;
            for pattern in patterns.iter() {
                if pattern.matches(command) {
                    return ExecApprovalRequirement::skip();
                }
            }
        }

        // Check policy rules
        let policy = self.policy.read().await;
        let rule_match = policy.check(command);

        // Apply heuristics for non-policy matches
        let decision = match &rule_match {
            RuleMatch::PrefixRuleMatch { decision, .. } => *decision,
            RuleMatch::HeuristicsRuleMatch { .. } => self.heuristics_decision(command),
        };

        match decision {
            Decision::Allow => ExecApprovalRequirement::skip(),
            Decision::Prompt => ExecApprovalRequirement::needs_approval(
                self.format_approval_reason(command, &rule_match),
            ),
            Decision::Forbidden => ExecApprovalRequirement::forbidden(
                self.format_forbidden_reason(command, &rule_match),
            ),
        }
    }

    /// Check multiple commands and return combined approval requirement.
    pub async fn check_approval_batch(&self, commands: &[Vec<String>]) -> ExecApprovalRequirement {
        let mut needs_approval_flag = false;
        let mut reasons = Vec::new();

        for command in commands {
            let approval = self.check_approval(command).await;
            if approval.is_forbidden() {
                return approval;
            }
            if approval.requires_approval() {
                needs_approval_flag = true;
                if let ExecApprovalRequirement::NeedsApproval { reason, .. } = &approval {
                    if let Some(r) = reason {
                        reasons.push(r.clone());
                    }
                }
            }
        }

        if needs_approval_flag {
            ExecApprovalRequirement::needs_approval(reasons.join("; "))
        } else {
            ExecApprovalRequirement::skip()
        }
    }

    /// Mark a command as approved for this session.
    pub async fn approve_command(&self, command: &[String]) {
        let command_key = command.join(" ");
        let mut approved = self.session_approved.write().await;
        approved.insert(command_key);
    }

    /// Clear all session approvals.
    pub async fn clear_session_approvals(&self) {
        let mut approved = self.session_approved.write().await;
        approved.clear();
    }

    /// Evaluate a command against the full policy stack.
    pub async fn evaluate(&self, command: &[String]) -> PolicyEvaluation {
        let policy = self.policy.read().await;
        let commands = vec![command.to_vec()];
        policy.check_multiple(commands.iter(), &|cmd| self.heuristics_decision(cmd))
    }

    /// Apply heuristics to determine decision for unknown commands.
    fn heuristics_decision(&self, command: &[String]) -> Decision {
        if !self.config.use_heuristics {
            return Decision::Prompt;
        }

        if command.is_empty() {
            return Decision::Prompt;
        }

        let cmd = &command[0];

        // Known safe read-only commands
        let safe_commands = [
            "ls", "cat", "head", "tail", "grep", "find", "echo", "pwd", "which", "type", "less",
            "more", "wc", "sort", "uniq", "diff", "env", "printenv", "hostname", "uname", "date",
            "whoami", "id", "file", "stat", "tree", "df", "du", "uptime",
        ];

        if safe_commands.contains(&cmd.as_str()) {
            return Decision::Allow;
        }

        // Known dangerous commands
        let dangerous_commands = [
            "rm", "rmdir", "dd", "mkfs", "fdisk", "shutdown", "reboot", "halt", "poweroff", "init",
            "kill", "killall", "pkill", "chmod", "chown", "chgrp", "sudo", "su",
        ];

        if dangerous_commands.contains(&cmd.as_str()) {
            // Check for --dry-run flag
            if command.iter().any(|arg| arg == "--dry-run" || arg == "-n") {
                return Decision::Prompt;
            }
            return Decision::Forbidden;
        }

        // Commands with potentially dangerous flags
        if command.iter().any(|arg| {
            arg == "--force" || arg == "-f" || arg == "--hard" || arg == "-rf" || arg == "-fr"
        }) {
            return Decision::Prompt;
        }

        Decision::Prompt
    }

    /// Format the reason for requiring approval.
    fn format_approval_reason(&self, command: &[String], rule_match: &RuleMatch) -> String {
        match rule_match {
            RuleMatch::PrefixRuleMatch { rule, .. } => {
                format!(
                    "Command '{}' matched rule '{}' requiring confirmation",
                    command.join(" "),
                    rule.pattern.join(" ")
                )
            }
            RuleMatch::HeuristicsRuleMatch { .. } => {
                format!(
                    "Command '{}' requires confirmation (no explicit policy rule)",
                    command.join(" ")
                )
            }
        }
    }

    /// Format the reason for forbidding a command.
    fn format_forbidden_reason(&self, command: &[String], rule_match: &RuleMatch) -> String {
        match rule_match {
            RuleMatch::PrefixRuleMatch { rule, .. } => {
                format!(
                    "Command '{}' is forbidden by rule '{}'",
                    command.join(" "),
                    rule.pattern.join(" ")
                )
            }
            RuleMatch::HeuristicsRuleMatch { .. } => {
                format!(
                    "Command '{}' is forbidden by safety heuristics",
                    command.join(" ")
                )
            }
        }
    }
}

/// Shared reference to an ExecPolicyManager.
pub type SharedExecPolicyManager = Arc<ExecPolicyManager>;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_policy_manager_basic() {
        let dir = tempdir().unwrap();
        let manager = ExecPolicyManager::with_defaults(dir.path().to_path_buf());

        // Add a rule
        manager
            .add_prefix_rule(&["cargo".to_string(), "build".to_string()], Decision::Allow)
            .await
            .unwrap();

        // Check approval
        let result = manager
            .check_approval(&["cargo".to_string(), "build".to_string()])
            .await;
        assert!(result.can_proceed());

        // Unknown command should need approval
        let result = manager
            .check_approval(&["unknown".to_string(), "command".to_string()])
            .await;
        assert!(result.requires_approval());
    }

    #[tokio::test]
    async fn test_trusted_patterns() {
        let dir = tempdir().unwrap();
        let manager = ExecPolicyManager::with_defaults(dir.path().to_path_buf());

        // Add trusted pattern
        let amendment = ExecPolicyAmendment::from_prefix("cargo");
        manager.add_trusted_pattern(amendment).await;

        // Check any cargo command
        let result = manager
            .check_approval(&["cargo".to_string(), "test".to_string()])
            .await;
        assert!(result.can_proceed());
    }

    #[tokio::test]
    async fn test_session_approval() {
        let dir = tempdir().unwrap();
        let manager = ExecPolicyManager::with_defaults(dir.path().to_path_buf());

        let cmd = vec!["git".to_string(), "status".to_string()];

        // Initially needs approval
        let result = manager.check_approval(&cmd).await;
        assert!(result.requires_approval());

        // Approve it
        manager.approve_command(&cmd).await;

        // Now it should skip
        let result = manager.check_approval(&cmd).await;
        assert!(result.can_proceed());

        // Clear approvals
        manager.clear_session_approvals().await;

        // Needs approval again
        let result = manager.check_approval(&cmd).await;
        assert!(result.requires_approval());
    }

    #[tokio::test]
    async fn test_heuristics() {
        let dir = tempdir().unwrap();
        let manager = ExecPolicyManager::with_defaults(dir.path().to_path_buf());

        // Safe command
        let result = manager.check_approval(&["ls".to_string()]).await;
        assert!(result.can_proceed());

        // Dangerous command (rm)
        let result = manager
            .check_approval(&["rm".to_string(), "-rf".to_string()])
            .await;
        assert!(result.is_forbidden());
    }
}
