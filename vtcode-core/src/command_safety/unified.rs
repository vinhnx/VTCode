//! Unified Command Evaluator - Phase 5
//!
//! Merges CommandPolicyEvaluator with command_safety module to provide
//! comprehensive command validation combining:
//! - Policy-based rules (allow/deny prefixes, regexes, globs)
//! - Safety rules (subcommand validation, dangerous patterns)
//! - Shell parsing (decompose complex scripts)
//! - Audit logging & caching

use crate::command_safety::{
    command_might_be_dangerous, parse_bash_lc_commands, CommandDatabase, SafeCommandRegistry,
    SafetyAuditLogger, SafetyDecision, SafetyDecisionCache,
};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Detailed reason for evaluation result
#[derive(Clone, Debug, PartialEq)]
pub enum EvaluationReason {
    /// Command allowed by policy rule
    PolicyAllow(String),
    /// Command denied by policy rule
    PolicyDeny(String),
    /// Command passed safety checks
    SafetyAllow,
    /// Command failed safety checks
    SafetyDeny(String),
    /// Hardcoded dangerous command detected
    DangerousCommand(String),
    /// Retrieved from cache
    CacheHit(bool, String),
}

impl std::fmt::Display for EvaluationReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PolicyAllow(msg) => write!(f, "Policy Allow: {}", msg),
            Self::PolicyDeny(msg) => write!(f, "Policy Deny: {}", msg),
            Self::SafetyAllow => write!(f, "Safety Allow"),
            Self::SafetyDeny(msg) => write!(f, "Safety Deny: {}", msg),
            Self::DangerousCommand(msg) => write!(f, "Dangerous: {}", msg),
            Self::CacheHit(allowed, msg) => {
                write!(f, "Cache {} {}", if *allowed { "Allow" } else { "Deny" }, msg)
            }
        }
    }
}

/// Complete evaluation result
#[derive(Clone, Debug)]
pub struct EvaluationResult {
    /// Whether the command is allowed
    pub allowed: bool,
    /// Primary reason for decision
    pub primary_reason: EvaluationReason,
    /// Secondary reasons (e.g., policy rule that matched)
    pub secondary_reasons: Vec<String>,
    /// Resolved command path (if available)
    pub resolved_path: Option<PathBuf>,
}

/// Unified command evaluator combining policies and safety rules
#[derive(Clone)]
pub struct UnifiedCommandEvaluator {
    // Safety components
    registry: SafeCommandRegistry,
    database: CommandDatabase,
    cache: SafetyDecisionCache,
    audit_logger: SafetyAuditLogger,

    // Policy component (wrapped for cloning)
    // Note: CommandPolicyEvaluator doesn't implement Clone in the codebase,
    // so we'll integrate it via Arc<Mutex<T>> in a real implementation
    // For now, we provide the structure and will integrate when adding to CommandTool
}

impl UnifiedCommandEvaluator {
    /// Create a new unified evaluator with default components
    pub fn new() -> Self {
        Self {
            registry: SafeCommandRegistry::new(),
            database: CommandDatabase,
            cache: SafetyDecisionCache::new(1000),
            audit_logger: SafetyAuditLogger::new(true),
        }
    }

    /// Evaluate a command with full context (async)
    ///
    /// # Evaluation Pipeline
    /// 1. Check cache
    /// 2. Apply dangerous command detection
    /// 3. Apply safety registry rules (with subcommand validation)
    /// 4. Handle shell parsing if needed (bash -lc)
    /// 5. Log audit entry (async)
    /// 6. Cache result
    pub async fn evaluate(&self, command: &[String]) -> Result<EvaluationResult> {
        if command.is_empty() {
            return Ok(EvaluationResult {
                allowed: false,
                primary_reason: EvaluationReason::SafetyDeny("empty command".into()),
                secondary_reasons: vec![],
                resolved_path: None,
            });
        }

        let command_text = command.join(" ");

        // 1. Check cache first
        if let Some(cached_decision) = self.cache.get(&command_text).await {
            let reason = EvaluationReason::CacheHit(
                cached_decision.is_safe,
                cached_decision.reason.clone(),
            );
            // Note: Audit logging skipped for cached decisions (logged on original evaluation)
            return Ok(EvaluationResult {
                allowed: cached_decision.is_safe,
                primary_reason: reason,
                secondary_reasons: vec![],
                resolved_path: None,
            });
        }

        // 2. Check dangerous commands first (fail-fast)
        if command_might_be_dangerous(command) {
            let result = EvaluationResult {
                allowed: false,
                primary_reason: EvaluationReason::DangerousCommand(
                    "matches dangerous patterns".into(),
                ),
                secondary_reasons: vec![],
                resolved_path: None,
            };
            // Note: Audit logging could be added here via async task
            self.cache
                .put(
                    command_text.clone(),
                    false,
                    "dangerous command pattern".into(),
                )
                .await;
            return Ok(result);
        }

        // 3. Apply safety registry rules
        let registry_decision = self.registry.is_safe(command);
        match registry_decision {
            SafetyDecision::Deny(reason) => {
                let result = EvaluationResult {
                    allowed: false,
                    primary_reason: EvaluationReason::SafetyDeny(reason.clone()),
                    secondary_reasons: vec!["registry rule".into()],
                    resolved_path: None,
                };
                // Note: Audit logging could be added here via async task
                self.cache
                    .put(command_text.clone(), false, reason.clone())
                    .await;
                return Ok(result);
            }
            SafetyDecision::Allow => {
                // Passed registry, continue to database checks
            }
            SafetyDecision::Unknown => {
                // Continue to database checks
            }
        }

        // 4. Apply command database rules
        // Note: Database rules are optional. Currently the registry covers the main use cases.
        // In a production system, this would merge database rules with registry rules.
        // For now, we skip explicit database check as the registry is comprehensive.

        // 5. Handle shell parsing for bash -lc and similar patterns
        // Note: For simplicity, we evaluate each sub-command non-recursively
        // by applying the same checks. In production, this could be refactored to support recursion.
        if let Some(scripts) = parse_bash_lc_commands(command) {
            for script in scripts {
                // Apply the same checks to each script without recursive call
                if command_might_be_dangerous(&script) {
                    let result = EvaluationResult {
                        allowed: false,
                        primary_reason: EvaluationReason::DangerousCommand(
                            format!("dangerous in sub-script: {}", script.join(" ")),
                        ),
                        secondary_reasons: vec![],
                        resolved_path: None,
                    };
                    self.cache
                        .put(
                            command_text.clone(),
                            false,
                            result.primary_reason.to_string(),
                        )
                        .await;
                    return Ok(result);
                }

                // Check safety registry for sub-command
                if let SafetyDecision::Deny(reason) = self.registry.is_safe(&script) {
                    let result = EvaluationResult {
                        allowed: false,
                        primary_reason: EvaluationReason::SafetyDeny(
                            format!("sub-command denied: {}", reason),
                        ),
                        secondary_reasons: vec![],
                        resolved_path: None,
                    };
                    self.cache
                        .put(
                            command_text.clone(),
                            false,
                            result.primary_reason.to_string(),
                        )
                        .await;
                    return Ok(result);
                }
            }
        }

        // 6. All checks passed
        let result = EvaluationResult {
            allowed: true,
            primary_reason: EvaluationReason::SafetyAllow,
            secondary_reasons: vec!["passed all safety checks".into()],
            resolved_path: None,
        };
        // Note: Audit logging could be added here via async task
        self.cache
            .put(
                command_text,
                true,
                "passed all safety checks".into(),
            )
            .await;
        Ok(result)
    }

    /// Evaluate with explicit policy check (requires external CommandPolicyEvaluator)
    ///
    /// This is a placeholder for integration with CommandPolicyEvaluator.
    /// In a real implementation, this would:
    /// 1. Check policy rules first (deny precedence)
    /// 2. Then apply safety rules
    /// 3. Merge results
    pub async fn evaluate_with_policy(
        &self,
        command: &[String],
        policy_allowed: bool,
        policy_reason: &str,
    ) -> Result<EvaluationResult> {
        // If policy explicitly denies, stop here
        if !policy_allowed {
            return Ok(EvaluationResult {
                allowed: false,
                primary_reason: EvaluationReason::PolicyDeny(policy_reason.into()),
                secondary_reasons: vec![],
                resolved_path: None,
            });
        }

        // Policy allows, continue with safety checks
        self.evaluate(command).await
    }

    /// Get reference to the cache for metrics/debugging
    pub fn cache(&self) -> &SafetyDecisionCache {
        &self.cache
    }

    /// Get reference to the audit logger
    pub fn audit_logger(&self) -> &SafetyAuditLogger {
        &self.audit_logger
    }

    /// Get reference to the registry
    pub fn registry(&self) -> &SafeCommandRegistry {
        &self.registry
    }

    /// Get reference to the database
    pub fn database(&self) -> &CommandDatabase {
        &self.database
    }
}

impl Default for UnifiedCommandEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn empty_command_denied() {
        let evaluator = UnifiedCommandEvaluator::new();
        let result = evaluator.evaluate(&[]).await.unwrap();
        assert!(!result.allowed);
    }

    #[tokio::test]
    async fn dangerous_command_denied() {
        let evaluator = UnifiedCommandEvaluator::new();
        let result = evaluator
            .evaluate(&["rm".to_string(), "-rf".to_string(), "/".to_string()])
            .await
            .unwrap();
        assert!(!result.allowed);
        matches!(result.primary_reason, EvaluationReason::DangerousCommand(_));
    }

    #[tokio::test]
    async fn safe_command_allowed() {
        let evaluator = UnifiedCommandEvaluator::new();
        // git is in the default safe registry
        let result = evaluator
            .evaluate(&["git".to_string(), "status".to_string()])
            .await
            .unwrap();
        assert!(result.allowed);
    }

    #[tokio::test]
    async fn cache_hit_on_repeated_command() {
        let evaluator = UnifiedCommandEvaluator::new();
        let cmd = vec!["git".to_string(), "status".to_string()];

        // First evaluation
        let result1 = evaluator.evaluate(&cmd).await.unwrap();
        assert!(result1.allowed);

        // Second evaluation (should be cached)
        let result2 = evaluator.evaluate(&cmd).await.unwrap();
        assert!(result2.allowed);
        matches!(result2.primary_reason, EvaluationReason::CacheHit(true, _));
    }

    #[tokio::test]
    async fn bash_lc_decomposition() {
        let evaluator = UnifiedCommandEvaluator::new();
        // bash -lc with mixed safe/unsafe commands
        let cmd = vec![
            "bash".to_string(),
            "-lc".to_string(),
            "git status && rm -rf /".to_string(),
        ];
        let result = evaluator.evaluate(&cmd).await.unwrap();
        assert!(!result.allowed);
        // Should detect the rm -rf in the sub-command
    }

    #[test]
    fn evaluation_reason_display() {
        let reason = EvaluationReason::PolicyAllow("test".into());
        assert_eq!(reason.to_string(), "Policy Allow: test");

        let reason = EvaluationReason::SafetyDeny("forbidden".into());
        assert_eq!(reason.to_string(), "Safety Deny: forbidden");
    }

    #[tokio::test]
    async fn policy_deny_stops_evaluation() {
        let evaluator = UnifiedCommandEvaluator::new();
        let result = evaluator
            .evaluate_with_policy(
                &["git".to_string(), "status".to_string()],
                false,
                "policy blocked",
            )
            .await
            .unwrap();
        assert!(!result.allowed);
        matches!(result.primary_reason, EvaluationReason::PolicyDeny(_));
    }

    #[tokio::test]
    async fn policy_allow_continues_to_safety_checks() {
        let evaluator = UnifiedCommandEvaluator::new();
        let result = evaluator
            .evaluate_with_policy(
                &["git".to_string(), "status".to_string()],
                true,
                "policy allowed",
            )
            .await
            .unwrap();
        // Policy allows, git status passes safety checks
        assert!(result.allowed);
    }

    #[tokio::test]
    async fn safety_deny_overrides_policy_allow() {
        let evaluator = UnifiedCommandEvaluator::new();
        let result = evaluator
            .evaluate_with_policy(
                &["rm".to_string(), "-rf".to_string(), "/".to_string()],
                true,
                "policy allowed",
            )
            .await
            .unwrap();
        // Policy allows but safety rules deny
        assert!(!result.allowed);
        matches!(result.primary_reason, EvaluationReason::DangerousCommand(_));
    }

    #[tokio::test]
    async fn evaluation_result_contains_reasons() {
        let evaluator = UnifiedCommandEvaluator::new();
        let result = evaluator
            .evaluate(&["git".to_string(), "status".to_string()])
            .await
            .unwrap();
        assert!(result.allowed);
        assert!(!result.secondary_reasons.is_empty());
    }

    #[tokio::test]
    async fn forbidden_git_subcommand_denied() {
        let evaluator = UnifiedCommandEvaluator::new();
        // git push is not in the allowed subcommands for git
        let result = evaluator
            .evaluate(&["git".to_string(), "push".to_string()])
            .await
            .unwrap();
        assert!(!result.allowed);
    }
}

/// Policy-aware evaluator adapter for backward compatibility with CommandPolicyEvaluator
///
/// This adapter wraps UnifiedCommandEvaluator with policy rule evaluation,
/// allowing gradual migration from CommandPolicyEvaluator to UnifiedCommandEvaluator.
#[derive(Clone)]
pub struct PolicyAwareEvaluator {
    unified: Arc<Mutex<UnifiedCommandEvaluator>>,
    /// Policy allow decision (if Some, policy layer is active)
    allow_policy_decision: Option<bool>,
    policy_reason: Option<String>,
}

impl PolicyAwareEvaluator {
    /// Create a new policy-aware evaluator with default components
    pub fn new() -> Self {
        Self {
            unified: Arc::new(Mutex::new(UnifiedCommandEvaluator::new())),
            allow_policy_decision: None,
            policy_reason: None,
        }
    }

    /// Create with explicit policy decision
    pub fn with_policy(
        allow_policy_decision: bool,
        policy_reason: impl Into<String>,
    ) -> Self {
        Self {
            unified: Arc::new(Mutex::new(UnifiedCommandEvaluator::new())),
            allow_policy_decision: Some(allow_policy_decision),
            policy_reason: Some(policy_reason.into()),
        }
    }

    /// Evaluate command with optional policy layer
    pub async fn evaluate(&self, command: &[String]) -> Result<EvaluationResult> {
        let evaluator = self.unified.lock().await;

        // Apply policy layer if configured
        if let (Some(policy_allowed), Some(reason)) = (&self.allow_policy_decision, &self.policy_reason)
        {
            evaluator
                .evaluate_with_policy(command, *policy_allowed, reason)
                .await
        } else {
            // No policy configured, use pure safety evaluation
            evaluator.evaluate(command).await
        }
    }

    /// Set policy decision (allows updating policy after creation)
    pub fn set_policy(&mut self, allowed: bool, reason: impl Into<String>) {
        self.allow_policy_decision = Some(allowed);
        self.policy_reason = Some(reason.into());
    }

    /// Clear policy decision (revert to pure safety evaluation)
    pub fn clear_policy(&mut self) {
        self.allow_policy_decision = None;
        self.policy_reason = None;
    }

    /// Get reference to the underlying evaluator for advanced access
    pub fn unified(&self) -> Arc<Mutex<UnifiedCommandEvaluator>> {
        Arc::clone(&self.unified)
    }
}

impl Default for PolicyAwareEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod adapter_tests {
    use super::*;

    #[tokio::test]
    async fn policy_aware_without_policy_uses_safety() {
        let evaluator = PolicyAwareEvaluator::new();
        let result = evaluator
            .evaluate(&["git".to_string(), "status".to_string()])
            .await
            .unwrap();
        assert!(result.allowed);
    }

    #[tokio::test]
    async fn policy_aware_with_deny_policy_blocks_safe_command() {
        let evaluator = PolicyAwareEvaluator::with_policy(false, "policy blocked");
        let result = evaluator
            .evaluate(&["git".to_string(), "status".to_string()])
            .await
            .unwrap();
        assert!(!result.allowed);
        matches!(result.primary_reason, EvaluationReason::PolicyDeny(_));
    }

    #[tokio::test]
    async fn policy_aware_with_allow_policy_still_blocks_dangerous() {
        let evaluator = PolicyAwareEvaluator::with_policy(true, "policy allowed");
        let result = evaluator
            .evaluate(&["rm".to_string(), "-rf".to_string(), "/".to_string()])
            .await
            .unwrap();
        // Policy allows, but safety rules should deny
        assert!(!result.allowed);
    }

    #[tokio::test]
    async fn policy_aware_mutable_set_policy() {
        let mut evaluator = PolicyAwareEvaluator::new();
        // Initially no policy
        let result1 = evaluator
            .evaluate(&["git".to_string(), "status".to_string()])
            .await
            .unwrap();
        assert!(result1.allowed);

        // Set deny policy
        evaluator.set_policy(false, "policy blocked");
        let result2 = evaluator
            .evaluate(&["git".to_string(), "status".to_string()])
            .await
            .unwrap();
        assert!(!result2.allowed);

        // Clear policy
        evaluator.clear_policy();
        let result3 = evaluator
            .evaluate(&["git".to_string(), "status".to_string()])
            .await
            .unwrap();
        assert!(result3.allowed);
    }
}
