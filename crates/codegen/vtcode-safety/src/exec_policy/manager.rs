//! Execution policy manager.
//!
//! Coordinates policy evaluation, approval requirements, and sandbox enforcement.
//! Inspired by Codex's ExecPolicyManager pattern.

use super::{
    approval::{AskForApproval, ExecApprovalRequirement, ExecPolicyAmendment},
    policy::{Decision, Policy, PolicyEvaluation, RuleMatch},
};
use crate::command_safety::command_might_be_dangerous;
use crate::sandboxing::SandboxPolicy;
use anyhow::{Context, Result};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Arc,
};
use tokio::sync::RwLock;
use vtcode_commons::VtCodePaths;

const PROMPT_CONFLICT_REASON: &str = "approval required by policy, but AskForApproval is set to Never";
const REJECT_SANDBOX_APPROVAL_REASON: &str =
    "approval required by policy, but AskForApproval::Reject.sandbox_approval is set";
const REJECT_RULES_APPROVAL_REASON: &str = "approval required by policy rule, but AskForApproval::Reject.rules is set";

fn prompt_is_rejected_by_policy(approval_policy: AskForApproval, prompt_is_rule: bool) -> Option<&'static str> {
    if prompt_is_rule {
        if !approval_policy.rejects_rule_prompt() {
            return None;
        }

        return Some(if matches!(approval_policy, AskForApproval::Never) {
            PROMPT_CONFLICT_REASON
        } else {
            REJECT_RULES_APPROVAL_REASON
        });
    }

    if !approval_policy.rejects_sandbox_prompt() {
        return None;
    }

    Some(if matches!(approval_policy, AskForApproval::Never) {
        PROMPT_CONFLICT_REASON
    } else {
        REJECT_SANDBOX_APPROVAL_REASON
    })
}

/// Configuration for the execution policy manager.
#[derive(Debug, Clone)]
pub struct ExecPolicyConfig {
    /// Default sandbox policy for commands.
    default_sandbox_policy: SandboxPolicy,

    /// Default approval behavior.
    default_approval: AskForApproval,

    /// Whether to apply heuristics for unknown commands.
    use_heuristics: bool,
}

impl Default for ExecPolicyConfig {
    fn default() -> Self {
        Self {
            default_sandbox_policy: SandboxPolicy::read_only(),
            default_approval: AskForApproval::UnlessTrusted,
            use_heuristics: true,
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
    #[expect(
        dead_code,
        reason = "The workspace root is retained for the policy-manager constructor ABI and future path-aware policy checks."
    )]
    workspace_root: PathBuf,

    /// Commands that have been pre-approved this session.
    session_approved: RwLock<HashSet<Vec<String>>>,
}

impl ExecPolicyManager {
    /// Create a new policy manager.
    fn new(workspace_root: PathBuf, config: ExecPolicyConfig) -> Self {
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
    fn with_defaults(workspace_root: PathBuf) -> Self {
        Self::new(workspace_root, ExecPolicyConfig::default())
    }

    /// Load policy from a file.
    pub async fn load_policy(&self, path: &Path) -> Result<()> {
        let parser = super::parser::PolicyParser::new();
        let loaded_policy = parser.load_file(path).await.context("Failed to load policy file")?;

        let mut policy = self.policy.write().await;
        *policy = loaded_policy;
        Ok(())
    }

    /// Load rule files as precedence layers; `layers[0]` wins over later
    /// entries when both define the same pattern. Missing files are skipped.
    /// Returns the number of files actually loaded.
    pub async fn load_policy_layers(&self, layers: &[PathBuf]) -> Result<usize> {
        let parser = super::parser::PolicyParser::new();
        let mut merged = Policy::empty();
        let mut loaded = 0usize;
        for layer in layers {
            if !layer.is_file() {
                continue;
            }
            let layer_policy = parser
                .load_file(layer)
                .await
                .with_context(|| format!("Failed to load policy layer {}", layer.display()))?;
            merged.prepend_layer(layer_policy.rules().iter().cloned());
            loaded += 1;
        }
        let mut policy = self.policy.write().await;
        *policy = merged;
        Ok(loaded)
    }

    /// Add a prefix rule to the policy.
    async fn add_prefix_rule(&self, pattern: &[String], decision: Decision) -> Result<()> {
        let mut policy = self.policy.write().await;
        policy.add_prefix_rule(pattern, decision)
    }

    /// Add a trusted pattern amendment.
    async fn add_trusted_pattern(&self, amendment: ExecPolicyAmendment) {
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
    async fn check_approval(&self, command: &[String]) -> ExecApprovalRequirement {
        // Check if already approved this session
        {
            let approved = self.session_approved.read().await;
            if approved.contains(command) {
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
            Decision::Prompt => {
                let prompt_is_rule =
                    matches!(rule_match, RuleMatch::PrefixRuleMatch { decision: Decision::Prompt, .. });

                match prompt_is_rejected_by_policy(self.config.default_approval, prompt_is_rule) {
                    Some(reason) => ExecApprovalRequirement::forbidden(reason),
                    None => ExecApprovalRequirement::needs_approval(self.format_approval_reason(command, &rule_match)),
                }
            }
            Decision::Forbidden => {
                ExecApprovalRequirement::forbidden(self.format_forbidden_reason(command, &rule_match))
            }
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
                if let ExecApprovalRequirement::NeedsApproval { reason: Some(r), .. } = &approval {
                    reasons.push(r.clone());
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
    async fn approve_command(&self, command: &[String]) {
        let mut approved = self.session_approved.write().await;
        let _inserted = approved.insert(command.to_vec());
    }

    /// Clear all session approvals.
    async fn clear_session_approvals(&self) {
        let mut approved = self.session_approved.write().await;
        approved.clear();
    }

    /// Evaluate a command against the full policy stack.
    pub async fn evaluate(&self, command: &[String]) -> PolicyEvaluation {
        let policy = self.policy.read().await;
        policy.check_multiple(std::iter::once(command), &|cmd| self.heuristics_decision(cmd))
    }

    /// Apply heuristics to determine decision for unknown commands.
    ///
    /// Uses the centralized `command_safety` module for dangerous command detection.
    fn heuristics_decision(&self, command: &[String]) -> Decision {
        if !self.config.use_heuristics {
            return Decision::Prompt;
        }

        if command.is_empty() {
            return Decision::Prompt;
        }

        let Some(cmd) = command.first() else {
            return Decision::Prompt;
        };

        // Known safe read-only commands that can proceed without approval
        let safe_commands = [
            "ls", "cat", "head", "tail", "grep", "find", "echo", "pwd", "which", "type", "less", "more", "wc", "sort",
            "uniq", "diff", "env", "printenv", "hostname", "uname", "date", "whoami", "id", "file", "stat", "tree",
            "df", "du", "uptime",
        ];

        if safe_commands.contains(&cmd.as_str()) {
            return Decision::Allow;
        }

        // Check dangerous commands using centralized logic
        if command_might_be_dangerous(command) {
            // Check for --dry-run flag to allow prompting instead of forbidding
            if command.iter().any(|arg| arg == "--dry-run" || arg == "-n") {
                return Decision::Prompt;
            }
            return Decision::Forbidden;
        }

        // For all other commands, default to prompting for approval
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
                format!("Command '{}' requires confirmation (no explicit policy rule)", command.join(" "))
            }
        }
    }

    /// Format the reason for forbidding a command.
    fn format_forbidden_reason(&self, command: &[String], rule_match: &RuleMatch) -> String {
        match rule_match {
            RuleMatch::PrefixRuleMatch { rule, .. } => {
                format!("Command '{}' is forbidden by rule '{}'", command.join(" "), rule.pattern.join(" "))
            }
            RuleMatch::HeuristicsRuleMatch { .. } => {
                format!("Command '{}' is forbidden by safety heuristics", command.join(" "))
            }
        }
    }
}

/// Shared reference to an ExecPolicyManager.
pub type SharedExecPolicyManager = Arc<ExecPolicyManager>;

/// Canonical rule-file layers for `workspace_root`, highest precedence first:
/// `<workspace>/.vtcode/rules/*.rules` (project rules), then user rules from
/// the canonical config directory's `rules/`, then legacy `~/.vtcode/rules/`.
/// Files within a directory load in sorted order; missing directories yield no
/// layers. Both user roots stay in the list so installations that have not
/// migrated to the native/XDG layout keep loading their personal rules.
pub fn rules_layers(workspace_root: &Path) -> Vec<PathBuf> {
    match VtCodePaths::resolve() {
        Ok(paths) => rules_layers_for_paths(workspace_root, &paths),
        Err(_) => {
            // Path resolution failure must not silently drop user rules.
            let mut user_roots = Vec::new();
            if let Some(home) = dirs::home_dir() {
                user_roots.push(home.join(".vtcode").join("rules"));
            }
            rules_layers_from(workspace_root, &user_roots)
        }
    }
}

/// Resolve the user rule roots from one path policy snapshot. Pure over the
/// snapshot so the canonical-config wiring is testable without process env.
fn rules_layers_for_paths(workspace_root: &Path, paths: &VtCodePaths) -> Vec<PathBuf> {
    let user_roots = [paths.config_dir().join("rules"), paths.legacy_dir().join("rules")];
    rules_layers_from(workspace_root, &user_roots)
}

/// Pure core of [`rules_layers`]: enumerate layers from explicit roots so the
/// precedence contract is testable without mutating process-global environment.
fn rules_layers_from(workspace_root: &Path, user_roots: &[PathBuf]) -> Vec<PathBuf> {
    fn dir_layers(dir: PathBuf) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("rules"))
            .collect();
        files.sort();
        files
    }

    let mut layers = dir_layers(workspace_root.join(".vtcode").join("rules"));
    let mut seen: HashSet<PathBuf> = layers.iter().cloned().collect();
    for root in user_roots {
        for path in dir_layers(root.clone()) {
            // An override can make the canonical root coincide with the
            // legacy root. Keep the first occurrence; `Vec::dedup` would miss
            // non-adjacent duplicates once a directory holds several files.
            if seen.insert(path.clone()) {
                layers.push(path);
            }
        }
    }
    layers
}

/// Create a shared manager with the default rule layers for `workspace_root`
/// auto-loaded (see [`rules_layers`]). Missing rule directories are fine;
/// errors from unreadable or malformed files propagate.
pub async fn shared_exec_policy_manager_with_rules(workspace_root: &Path) -> Result<SharedExecPolicyManager> {
    let manager = Arc::new(ExecPolicyManager::with_defaults(workspace_root.to_path_buf()));
    let _loaded = manager.load_policy_layers(&rules_layers(workspace_root)).await?;
    Ok(manager)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn invalid_reload_preserves_active_policy() {
        let dir = tempdir().unwrap();
        let manager = ExecPolicyManager::with_defaults(dir.path().to_path_buf());
        let active = dir.path().join("active.rules");
        let valid = dir.path().join("valid.rules");
        let invalid = dir.path().join("invalid.rules");
        std::fs::write(&active, "forbidden: git\nallow: custom").unwrap();
        std::fs::write(&valid, "allow: git\nforbidden: custom").unwrap();
        std::fs::write(&invalid, "allow: echo\nallow: ").unwrap();
        manager.load_policy(&active).await.unwrap();
        let before = manager.policy.read().await.rules().to_vec();
        assert!(manager.load_policy(&invalid).await.is_err());
        assert_eq!(manager.policy.read().await.rules(), before);
        assert!(manager.load_policy_layers(&[valid, invalid]).await.is_err());
        assert_eq!(manager.policy.read().await.rules(), before);
        assert!(manager.check_approval(&["git".into()]).await.is_forbidden());
        assert!(manager.check_approval(&["custom".into()]).await.can_proceed());
    }

    #[tokio::test]
    async fn empty_trusted_amendment_cannot_bypass_policy() {
        let dir = tempdir().unwrap();
        let manager = ExecPolicyManager::with_defaults(dir.path().to_path_buf());
        let amendment = serde_json::from_str(r#"{"pattern":[]}"#).unwrap();
        manager.add_trusted_pattern(amendment).await;
        manager.add_prefix_rule(&["blocked".into()], Decision::Forbidden).await.unwrap();
        assert!(manager.check_approval(&[]).await.requires_approval());
        assert!(manager.check_approval(&["unknown".into()]).await.requires_approval());
        assert!(manager.check_approval(&["blocked".into()]).await.is_forbidden());
    }

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
        let result = manager.check_approval(&["cargo".to_string(), "build".to_string()]).await;
        assert!(result.can_proceed());

        // Unknown command should need approval
        let result = manager.check_approval(&["unknown".to_string(), "command".to_string()]).await;
        assert!(result.requires_approval());
    }

    #[tokio::test]
    async fn load_policy_layers_first_layer_wins_on_duplicate_patterns() {
        let workspace = tempdir().unwrap();
        let ws_rules = workspace.path().join(".vtcode").join("rules");
        std::fs::create_dir_all(&ws_rules).unwrap();
        std::fs::write(ws_rules.join("project.rules"), "allow: git push\n").unwrap();

        let user = tempdir().unwrap();
        let user_rules = user.path().join("rules");
        std::fs::create_dir_all(&user_rules).unwrap();
        std::fs::write(user_rules.join("user.rules"), "prompt: git push\nforbidden: rm\n").unwrap();

        let manager = ExecPolicyManager::with_defaults(workspace.path().to_path_buf());
        let layers = vec![ws_rules.join("project.rules"), user_rules.join("user.rules")];
        let loaded = manager.load_policy_layers(&layers).await.unwrap();
        assert_eq!(loaded, 2);

        // The workspace layer's `allow` wins over the user layer's `prompt`
        // for the same pattern.
        let result = manager.check_approval(&["git".to_string(), "push".to_string()]).await;
        assert!(result.can_proceed());

        // Patterns only present in the lower-precedence layer still apply.
        let result = manager
            .check_approval(&["rm".to_string(), "-rf".to_string(), "/".to_string()])
            .await;
        assert!(!result.can_proceed());
    }

    #[tokio::test]
    async fn load_policy_layers_skips_missing_files() {
        let dir = tempdir().unwrap();
        let manager = ExecPolicyManager::with_defaults(dir.path().to_path_buf());
        let loaded = manager
            .load_policy_layers(&[dir.path().join("does-not-exist.rules")])
            .await
            .unwrap();
        assert_eq!(loaded, 0);
    }

    #[test]
    fn rules_layers_lists_workspace_files_sorted_before_user_layers() {
        let workspace = tempdir().unwrap();
        let ws_rules = workspace.path().join(".vtcode").join("rules");
        std::fs::create_dir_all(&ws_rules).unwrap();
        std::fs::write(ws_rules.join("b.rules"), "").unwrap();
        std::fs::write(ws_rules.join("a.rules"), "").unwrap();
        std::fs::write(ws_rules.join("ignored.txt"), "").unwrap();

        let layers = rules_layers(workspace.path());
        // Workspace rules come first (highest precedence), sorted; non-`.rules`
        // files are excluded. User layers (if any exist under $HOME) come after.
        let workspace_count = layers
            .iter()
            .take_while(|p| p.starts_with(ws_rules.join("a.rules").parent().unwrap()))
            .count();
        assert!(workspace_count >= 2, "expected workspace rules layers, got {layers:?}");
        assert_eq!(layers[0].file_name().unwrap(), "a.rules");
        assert_eq!(layers[1].file_name().unwrap(), "b.rules");
        assert!(layers.iter().all(|p| p.extension().and_then(|e| e.to_str()) == Some("rules")));
    }

    #[test]
    fn rules_layers_for_paths_wires_canonical_config_before_legacy() {
        // Build a path policy from explicit values so the production wrapper's
        // config-dir wiring is verified without mutating process-global env.
        let canonical = tempdir().unwrap();
        let legacy = tempdir().unwrap();
        let paths = VtCodePaths::from_environment(&[
            ("VTCODE_CONFIG", canonical.path().to_str().unwrap()),
            ("VTCODE_HOME", legacy.path().to_str().unwrap()),
        ])
        .expect("resolve explicit path policy");

        let canonical_rules = canonical.path().join("rules");
        let legacy_rules = legacy.path().join("rules");
        std::fs::create_dir_all(&canonical_rules).unwrap();
        std::fs::create_dir_all(&legacy_rules).unwrap();
        std::fs::write(canonical_rules.join("canonical.rules"), "").unwrap();
        std::fs::write(legacy_rules.join("legacy.rules"), "").unwrap();

        let workspace = tempdir().unwrap();
        let layers = rules_layers_for_paths(workspace.path(), &paths);
        let names: Vec<_> = layers
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["canonical.rules", "legacy.rules"]);
    }

    #[test]
    fn rules_layers_load_canonical_user_rules_and_keep_legacy_fallback() {
        let workspace = tempdir().unwrap();
        let ws_rules = workspace.path().join(".vtcode").join("rules");
        let canonical = tempdir().unwrap();
        let canonical_rules = canonical.path().join("rules");
        let legacy = tempdir().unwrap();
        let legacy_rules = legacy.path().join("rules");

        for (dir, file) in [
            (&ws_rules, "project.rules"),
            (&canonical_rules, "canonical.rules"),
            (&legacy_rules, "legacy.rules"),
        ] {
            std::fs::create_dir_all(dir).unwrap();
            std::fs::write(dir.join(file), "").unwrap();
        }

        let layers = rules_layers_from(workspace.path(), &[canonical_rules.clone(), legacy_rules.clone()]);
        let names: Vec<_> = layers
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["project.rules", "canonical.rules", "legacy.rules"]);

        // An override can make the canonical root coincide with the legacy
        // one. Two files per root produce a non-adjacent duplicate sequence
        // (a, b, a, b), which `Vec::dedup` would leave untouched.
        std::fs::write(canonical_rules.join("second.rules"), "").unwrap();
        let deduped = rules_layers_from(workspace.path(), &[canonical_rules.clone(), canonical_rules]);
        let deduped_names: Vec<_> = deduped
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(deduped_names, vec!["project.rules", "canonical.rules", "second.rules"]);
    }

    #[tokio::test]
    async fn test_prompt_conflict_with_never_policy_forbids() {
        let dir = tempdir().unwrap();
        let manager = ExecPolicyManager::new(
            dir.path().to_path_buf(),
            ExecPolicyConfig {
                default_approval: AskForApproval::Never,
                ..ExecPolicyConfig::default()
            },
        );

        let result = manager.check_approval(&["unknown".to_string(), "command".to_string()]).await;
        assert_eq!(result, ExecApprovalRequirement::forbidden(PROMPT_CONFLICT_REASON));
    }

    #[tokio::test]
    async fn test_reject_rules_policy_forbids_rule_prompt() {
        let dir = tempdir().unwrap();
        let manager = ExecPolicyManager::new(
            dir.path().to_path_buf(),
            ExecPolicyConfig {
                default_approval: AskForApproval::Reject(crate::exec_policy::RejectConfig {
                    sandbox_approval: false,
                    rules: true,
                    request_permissions: false,
                    mcp_elicitations: false,
                }),
                ..ExecPolicyConfig::default()
            },
        );
        manager
            .add_prefix_rule(&["git".to_string()], Decision::Prompt)
            .await
            .expect("add prompt rule");

        let result = manager.check_approval(&["git".to_string()]).await;
        assert_eq!(result, ExecApprovalRequirement::forbidden(REJECT_RULES_APPROVAL_REASON));
    }

    #[tokio::test]
    async fn test_reject_sandbox_policy_forbids_non_rule_prompt() {
        let dir = tempdir().unwrap();
        let manager = ExecPolicyManager::new(
            dir.path().to_path_buf(),
            ExecPolicyConfig {
                default_approval: AskForApproval::Reject(crate::exec_policy::RejectConfig {
                    sandbox_approval: true,
                    rules: false,
                    request_permissions: false,
                    mcp_elicitations: false,
                }),
                ..ExecPolicyConfig::default()
            },
        );

        let result = manager.check_approval(&["unknown".to_string(), "command".to_string()]).await;
        assert_eq!(result, ExecApprovalRequirement::forbidden(REJECT_SANDBOX_APPROVAL_REASON));
    }

    #[tokio::test]
    async fn test_trusted_patterns() {
        let dir = tempdir().unwrap();
        let manager = ExecPolicyManager::with_defaults(dir.path().to_path_buf());

        // Add trusted pattern
        let amendment = ExecPolicyAmendment::from_prefix("cargo");
        manager.add_trusted_pattern(amendment).await;

        // Check any cargo command
        let result = manager.check_approval(&["cargo".to_string(), "test".to_string()]).await;
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
    async fn session_approval_preserves_argument_boundaries() {
        let dir = tempdir().unwrap();
        let manager = ExecPolicyManager::with_defaults(dir.path().to_path_buf());
        let approved = vec!["unknown".to_string(), "one two".to_string()];
        let distinct = vec!["unknown".to_string(), "one".to_string(), "two".to_string()];

        manager.approve_command(&approved).await;
        assert!(manager.check_approval(&approved).await.can_proceed());
        assert!(manager.check_approval(&distinct).await.requires_approval());
    }

    #[tokio::test]
    async fn test_heuristics() {
        let dir = tempdir().unwrap();
        let manager = ExecPolicyManager::with_defaults(dir.path().to_path_buf());

        // Safe command
        let result = manager.check_approval(&["ls".to_string()]).await;
        assert!(result.can_proceed());

        // Dangerous command (rm)
        let result = manager.check_approval(&["rm".to_string(), "-rf".to_string()]).await;
        assert!(result.is_forbidden());
    }
}
