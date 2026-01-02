//! Sandbox permissions for fine-grained access control.

use serde::{Deserialize, Serialize};

/// Fine-grained permissions for sandbox operations.
///
/// These permissions allow individual tool calls to request specific
/// capabilities beyond the base sandbox policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxPermissions {
    /// Use the default sandbox permissions from the policy.
    #[default]
    UseDefault,

    /// Request escalated permissions (requires approval).
    RequireEscalated,

    /// Bypass the sandbox entirely (requires explicit approval).
    BypassSandbox,
}

impl SandboxPermissions {
    /// Check if this permission requires approval.
    pub fn requires_approval(&self) -> bool {
        matches!(self, Self::RequireEscalated | Self::BypassSandbox)
    }

    /// Check if this permission bypasses the sandbox.
    pub fn bypasses_sandbox(&self) -> bool {
        matches!(self, Self::BypassSandbox)
    }

    /// Merge with another permission, taking the more permissive one.
    pub fn merge(&self, other: &Self) -> Self {
        use SandboxPermissions::*;
        match (self, other) {
            (BypassSandbox, _) | (_, BypassSandbox) => BypassSandbox,
            (RequireEscalated, _) | (_, RequireEscalated) => RequireEscalated,
            (UseDefault, UseDefault) => UseDefault,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_permissions() {
        let perm = SandboxPermissions::default();
        assert!(!perm.requires_approval());
        assert!(!perm.bypasses_sandbox());
    }

    #[test]
    fn test_escalated_permissions() {
        let perm = SandboxPermissions::RequireEscalated;
        assert!(perm.requires_approval());
        assert!(!perm.bypasses_sandbox());
    }

    #[test]
    fn test_bypass_permissions() {
        let perm = SandboxPermissions::BypassSandbox;
        assert!(perm.requires_approval());
        assert!(perm.bypasses_sandbox());
    }

    #[test]
    fn test_merge_permissions() {
        use SandboxPermissions::*;

        assert_eq!(UseDefault.merge(&UseDefault), UseDefault);
        assert_eq!(UseDefault.merge(&RequireEscalated), RequireEscalated);
        assert_eq!(RequireEscalated.merge(&BypassSandbox), BypassSandbox);
    }
}
