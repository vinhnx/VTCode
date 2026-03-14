//! Approval requirement types for execution policy.

use serde::{Deserialize, Serialize};

/// Fine-grained rejection controls for approval prompts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RejectConfig {
    /// Reject approval prompts related to sandbox escalation.
    pub sandbox_approval: bool,
    /// Reject prompts triggered by policy `prompt` rules.
    pub rules: bool,
    /// Reject approval prompts related to built-in permission requests.
    pub request_permissions: bool,
    /// Reject MCP elicitation prompts.
    pub mcp_elicitations: bool,
}

impl RejectConfig {
    pub const fn rejects_sandbox_approval(self) -> bool {
        self.sandbox_approval
    }

    pub const fn rejects_rules_approval(self) -> bool {
        self.rules
    }

    pub const fn rejects_request_permissions(self) -> bool {
        self.request_permissions
    }

    pub const fn rejects_mcp_elicitations(self) -> bool {
        self.mcp_elicitations
    }
}

/// Policy for when to ask for approval before executing commands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskForApproval {
    /// Never ask for approval (autonomous mode).
    Never,

    /// Ask only when explicitly requested by policy.
    OnRequest,

    /// Ask unless the command is in the trusted list.
    #[default]
    UnlessTrusted,

    /// Ask only on failure (retry with approval).
    OnFailure,

    /// Fine-grained rejection controls for approval prompts.
    Reject(RejectConfig),
}

impl AskForApproval {
    /// Check if this policy requires asking for unknown commands.
    pub fn requires_approval_for_unknown(&self) -> bool {
        matches!(
            self,
            Self::UnlessTrusted | Self::OnRequest | Self::Reject(_)
        )
    }

    /// Check whether rule-triggered approval prompts are rejected.
    pub const fn rejects_rule_prompt(self) -> bool {
        match self {
            Self::Never => true,
            Self::Reject(reject_config) => reject_config.rejects_rules_approval(),
            Self::OnFailure | Self::OnRequest | Self::UnlessTrusted => false,
        }
    }

    /// Check whether sandbox-related approval prompts are rejected.
    pub const fn rejects_sandbox_prompt(self) -> bool {
        match self {
            Self::Never => true,
            Self::Reject(reject_config) => reject_config.rejects_sandbox_approval(),
            Self::OnFailure | Self::OnRequest | Self::UnlessTrusted => false,
        }
    }

    /// Check whether MCP elicitation prompts are rejected.
    pub const fn rejects_mcp_elicitation(self) -> bool {
        match self {
            Self::Never => true,
            Self::Reject(reject_config) => reject_config.rejects_mcp_elicitations(),
            Self::OnFailure | Self::OnRequest | Self::UnlessTrusted => false,
        }
    }
}

/// Compute the default approval requirement for a tool invocation.
///
/// `requires_sandbox_approval_prompt` should be `true` when the selected
/// sandbox mode still requires an approval prompt, and `false` when the tool is
/// already running unsandboxed or under an external sandbox that should not
/// trigger an additional sandbox approval prompt.
#[must_use]
pub fn default_exec_approval_requirement(
    policy: AskForApproval,
    requires_sandbox_approval_prompt: bool,
) -> ExecApprovalRequirement {
    let needs_approval = match policy {
        AskForApproval::Never | AskForApproval::OnFailure => false,
        AskForApproval::OnRequest | AskForApproval::Reject(_) => requires_sandbox_approval_prompt,
        AskForApproval::UnlessTrusted => true,
    };

    if needs_approval && policy.rejects_sandbox_prompt() {
        ExecApprovalRequirement::forbidden("approval policy rejected sandbox approval prompt")
    } else if needs_approval {
        ExecApprovalRequirement::NeedsApproval {
            reason: None,
            proposed_execpolicy_amendment: None,
        }
    } else {
        ExecApprovalRequirement::skip()
    }
}

/// A proposed amendment to the execution policy.
///
/// When a command requires approval but isn't explicitly forbidden,
/// this amendment can be proposed to allow similar commands in the future.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecPolicyAmendment {
    /// The command pattern to add to the policy.
    pub pattern: Vec<String>,
}

impl ExecPolicyAmendment {
    /// Create a new policy amendment.
    pub fn new(pattern: Vec<String>) -> Self {
        Self { pattern }
    }

    /// Create from a single command prefix.
    pub fn from_prefix(prefix: impl Into<String>) -> Self {
        Self {
            pattern: vec![prefix.into()],
        }
    }

    /// Check if a command matches this amendment pattern.
    pub fn matches(&self, command: &[String]) -> bool {
        if command.len() < self.pattern.len() {
            return false;
        }
        self.pattern
            .iter()
            .zip(command.iter())
            .all(|(pattern, cmd)| pattern == cmd)
    }

    /// Convert the amendment to a policy rule string.
    pub fn to_rule_string(&self) -> String {
        let pattern_json = serde_json::to_string(&self.pattern).unwrap_or_default();
        format!("prefix_rule(pattern={}, decision=\"allow\")", pattern_json)
    }

    /// Get the command pattern for Codex compatibility.
    pub fn command_pattern(&self) -> &[String] {
        &self.pattern
    }
}

/// Requirement for approval before executing a command.
///
/// This enum represents the outcome of evaluating a command against the
/// execution policy, indicating whether the command can proceed, needs
/// approval, or is forbidden.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecApprovalRequirement {
    /// Command can be executed without approval.
    Skip {
        /// Whether to bypass the sandbox for this command.
        bypass_sandbox: bool,
        /// Proposed policy amendment if the user wants to trust this command.
        proposed_execpolicy_amendment: Option<ExecPolicyAmendment>,
    },

    /// Command requires user approval before execution.
    NeedsApproval {
        /// Reason for requiring approval.
        reason: Option<String>,
        /// Proposed policy amendment to skip future approvals.
        proposed_execpolicy_amendment: Option<ExecPolicyAmendment>,
    },

    /// Command is forbidden by policy and cannot be executed.
    Forbidden {
        /// Reason for forbidding the command.
        reason: String,
    },
}

impl ExecApprovalRequirement {
    /// Create a skip requirement.
    pub fn skip() -> Self {
        Self::Skip {
            bypass_sandbox: false,
            proposed_execpolicy_amendment: None,
        }
    }

    /// Create a skip requirement with sandbox bypass.
    pub fn skip_with_bypass() -> Self {
        Self::Skip {
            bypass_sandbox: true,
            proposed_execpolicy_amendment: None,
        }
    }

    /// Create an approval requirement.
    pub fn needs_approval(reason: impl Into<String>) -> Self {
        Self::NeedsApproval {
            reason: Some(reason.into()),
            proposed_execpolicy_amendment: None,
        }
    }

    /// Create a needs approval requirement with an amendment.
    pub fn needs_approval_with_amendment(
        reason: Option<String>,
        amendment: ExecPolicyAmendment,
    ) -> Self {
        Self::NeedsApproval {
            reason,
            proposed_execpolicy_amendment: Some(amendment),
        }
    }

    /// Create a forbidden requirement.
    pub fn forbidden(reason: impl Into<String>) -> Self {
        Self::Forbidden {
            reason: reason.into(),
        }
    }

    /// Check if approval is needed.
    pub fn requires_approval(&self) -> bool {
        matches!(self, Self::NeedsApproval { .. })
    }

    /// Check if the command is forbidden.
    pub fn is_forbidden(&self) -> bool {
        matches!(self, Self::Forbidden { .. })
    }

    /// Check if the command can proceed (skip or approved).
    pub fn can_proceed(&self) -> bool {
        matches!(self, Self::Skip { .. })
    }

    /// Get the proposed amendment, if any.
    pub fn get_amendment(&self) -> Option<&ExecPolicyAmendment> {
        match self {
            Self::Skip {
                proposed_execpolicy_amendment,
                ..
            } => proposed_execpolicy_amendment.as_ref(),
            Self::NeedsApproval {
                proposed_execpolicy_amendment,
                ..
            } => proposed_execpolicy_amendment.as_ref(),
            Self::Forbidden { .. } => None,
        }
    }

    /// Get the proposed exec policy amendment if any (Codex-compatible name).
    pub fn proposed_execpolicy_amendment(&self) -> Option<&ExecPolicyAmendment> {
        self.get_amendment()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_skip_requirement() {
        let req = ExecApprovalRequirement::skip();
        assert!(req.can_proceed());
        assert!(!req.requires_approval());
        assert!(!req.is_forbidden());
    }

    #[test]
    fn test_needs_approval_requirement() {
        let req = ExecApprovalRequirement::needs_approval("dangerous command");
        assert!(!req.can_proceed());
        assert!(req.requires_approval());
        assert!(!req.is_forbidden());
    }

    #[test]
    fn test_forbidden_requirement() {
        let req = ExecApprovalRequirement::forbidden("policy violation");
        assert!(!req.can_proceed());
        assert!(!req.requires_approval());
        assert!(req.is_forbidden());
    }

    #[test]
    fn test_amendment() {
        let amendment = ExecPolicyAmendment::new(vec!["cargo".to_string(), "build".to_string()]);
        let rule = amendment.to_rule_string();
        assert!(rule.contains("cargo"));
        assert!(rule.contains("build"));
        assert!(rule.contains("allow"));
    }

    #[test]
    fn test_reject_config_helpers() {
        let config = RejectConfig {
            sandbox_approval: true,
            rules: false,
            request_permissions: false,
            mcp_elicitations: true,
        };
        assert!(config.rejects_sandbox_approval());
        assert!(!config.rejects_rules_approval());
        assert!(!config.rejects_request_permissions());
        assert!(config.rejects_mcp_elicitations());
    }

    #[test]
    fn test_ask_for_approval_rejection_helpers() {
        assert!(AskForApproval::Never.rejects_rule_prompt());
        assert!(AskForApproval::Never.rejects_sandbox_prompt());
        assert!(AskForApproval::Never.rejects_mcp_elicitation());

        assert!(!AskForApproval::OnRequest.rejects_rule_prompt());
        assert!(!AskForApproval::OnRequest.rejects_sandbox_prompt());
        assert!(!AskForApproval::OnRequest.rejects_mcp_elicitation());

        let reject_policy = AskForApproval::Reject(RejectConfig {
            sandbox_approval: true,
            rules: false,
            request_permissions: false,
            mcp_elicitations: true,
        });
        assert!(!reject_policy.rejects_rule_prompt());
        assert!(reject_policy.rejects_sandbox_prompt());
        assert!(reject_policy.rejects_mcp_elicitation());
    }

    #[test]
    fn test_reject_policy_serde_roundtrip() {
        let value = json!({
            "reject": {
                "sandbox_approval": true,
                "rules": false,
                "mcp_elicitations": true
            }
        });
        let policy: AskForApproval = serde_json::from_value(value).expect("deserialize policy");
        assert_eq!(
            policy,
            AskForApproval::Reject(RejectConfig {
                sandbox_approval: true,
                rules: false,
                request_permissions: false,
                mcp_elicitations: true,
            })
        );

        let serialized = serde_json::to_value(policy).expect("serialize policy");
        assert_eq!(
            serialized,
            json!({
                "reject": {
                    "sandbox_approval": true,
                    "rules": false,
                    "request_permissions": false,
                    "mcp_elicitations": true
                }
            })
        );
    }

    #[test]
    fn test_reject_policy_defaults_missing_request_permissions_to_false() {
        let policy: AskForApproval = serde_json::from_value(json!({
            "reject": {
                "sandbox_approval": true,
                "rules": false,
                "mcp_elicitations": true
            }
        }))
        .expect("deserialize legacy reject policy");

        assert_eq!(
            policy,
            AskForApproval::Reject(RejectConfig {
                sandbox_approval: true,
                rules: false,
                request_permissions: false,
                mcp_elicitations: true,
            })
        );
    }

    #[test]
    fn default_exec_approval_requirement_skips_for_never() {
        let requirement = default_exec_approval_requirement(AskForApproval::Never, true);

        assert_eq!(requirement, ExecApprovalRequirement::skip());
    }

    #[test]
    fn default_exec_approval_requirement_skips_for_on_failure() {
        let requirement = default_exec_approval_requirement(AskForApproval::OnFailure, true);

        assert_eq!(requirement, ExecApprovalRequirement::skip());
    }

    #[test]
    fn default_exec_approval_requirement_requires_approval_for_on_request() {
        let requirement = default_exec_approval_requirement(AskForApproval::OnRequest, true);

        assert_eq!(
            requirement,
            ExecApprovalRequirement::NeedsApproval {
                reason: None,
                proposed_execpolicy_amendment: None,
            }
        );
    }

    #[test]
    fn default_exec_approval_requirement_skips_on_request_without_prompt() {
        let requirement = default_exec_approval_requirement(AskForApproval::OnRequest, false);

        assert_eq!(requirement, ExecApprovalRequirement::skip());
    }

    #[test]
    fn default_exec_approval_requirement_requires_approval_for_unless_trusted() {
        let requirement = default_exec_approval_requirement(AskForApproval::UnlessTrusted, false);

        assert_eq!(
            requirement,
            ExecApprovalRequirement::NeedsApproval {
                reason: None,
                proposed_execpolicy_amendment: None,
            }
        );
    }

    #[test]
    fn default_exec_approval_requirement_rejects_sandbox_prompt_when_configured() {
        let policy = AskForApproval::Reject(RejectConfig {
            sandbox_approval: true,
            rules: false,
            request_permissions: false,
            mcp_elicitations: false,
        });

        let requirement = default_exec_approval_requirement(policy, true);

        assert_eq!(
            requirement,
            ExecApprovalRequirement::Forbidden {
                reason: "approval policy rejected sandbox approval prompt".to_string(),
            }
        );
    }

    #[test]
    fn default_exec_approval_requirement_keeps_prompt_when_rejection_disabled() {
        let policy = AskForApproval::Reject(RejectConfig {
            sandbox_approval: false,
            rules: true,
            request_permissions: false,
            mcp_elicitations: true,
        });

        let requirement = default_exec_approval_requirement(policy, true);

        assert_eq!(
            requirement,
            ExecApprovalRequirement::NeedsApproval {
                reason: None,
                proposed_execpolicy_amendment: None,
            }
        );
    }
}
