//! Approval requirement types for execution policy.

use serde::{Deserialize, Serialize};

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
}

impl AskForApproval {
	/// Check if this policy requires asking for unknown commands.
	pub fn requires_approval_for_unknown(&self) -> bool {
		matches!(self, Self::UnlessTrusted | Self::OnRequest)
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
}

#[cfg(test)]
mod tests {
	use super::*;

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
}
