//! Sandbox permissions for fine-grained access control.

use std::path::PathBuf;

use anyhow::anyhow;
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

    /// Request additional per-command sandbox permissions.
    WithAdditionalPermissions,

    /// Bypass the sandbox entirely (requires explicit approval).
    BypassSandbox,
}

impl SandboxPermissions {
    /// Canonical error when `with_additional_permissions` carries no paths.
    pub const MISSING_ADDITIONAL_PERMISSIONS_MESSAGE: &str = "missing `additional_permissions`; provide `fs_read` and/or `fs_write` when using `with_additional_permissions`";
    /// Canonical error when escalated/bypass requests lack justification.
    pub const MISSING_ESCALATION_JUSTIFICATION_MESSAGE: &str =
        "missing `justification`; provide a short approval question when using `sandbox_permissions=require_escalated`";
    /// Canonical error when `additional_permissions` is paired with the wrong mode.
    pub const ADDITIONAL_PERMISSIONS_MODE_MESSAGE: &str =
        "`additional_permissions` requires `sandbox_permissions` set to `with_additional_permissions`";

    /// Normalize an implicit additional-permission request to its explicit
    /// sandboxed wire mode. Escalated and bypass modes are left unchanged so
    /// their conflict checks remain fail-closed at the execution boundary.
    #[must_use]
    pub fn normalized_for(self, additional_permissions: Option<&AdditionalPermissions>) -> Self {
        if self == Self::UseDefault && additional_permissions.is_some_and(|permissions| !permissions.is_empty()) {
            Self::WithAdditionalPermissions
        } else {
            self
        }
    }

    /// Resolve and validate a raw sandbox request into a proven request.
    ///
    /// Modular proof: the permission impl itself establishes that the
    /// supertrait-like obligations hold (mode/additional combination is
    /// legal, escalated modes carry justification), so callers can trust the
    /// returned [`ValidatedSandboxRequest`] instead of re-checking the
    /// combination at every use site (external proof).
    ///
    /// Empty `additional_permissions` is treated as absent, `use_default`
    /// plus non-empty additional normalizes to `with_additional_permissions`,
    /// and escalated/bypass modes combined with additional permissions fail
    /// closed rather than silently downgrading.
    pub fn resolve_request(
        self,
        additional_permissions: Option<AdditionalPermissions>,
        justification: Option<&str>,
    ) -> anyhow::Result<ValidatedSandboxRequest> {
        let additional_permissions = additional_permissions.filter(|permissions| !permissions.is_empty());
        let normalized = self.normalized_for(additional_permissions.as_ref());

        if normalized.requires_escalated_permissions() {
            let justification = justification.map(str::trim).unwrap_or_default();
            if justification.is_empty() {
                return Err(anyhow!(Self::MISSING_ESCALATION_JUSTIFICATION_MESSAGE));
            }
        }

        if normalized.uses_additional_permissions() {
            let Some(additional_permissions) = additional_permissions else {
                return Err(anyhow!(Self::MISSING_ADDITIONAL_PERMISSIONS_MESSAGE));
            };
            Ok(ValidatedSandboxRequest {
                permissions: normalized,
                additional: Some(additional_permissions),
            })
        } else {
            if additional_permissions.is_some() {
                return Err(anyhow!(Self::ADDITIONAL_PERMISSIONS_MODE_MESSAGE));
            }
            Ok(ValidatedSandboxRequest { permissions: normalized, additional: None })
        }
    }

    /// Check if this permission requests full unsandboxed execution.
    pub fn requires_escalated_permissions(&self) -> bool {
        matches!(self, Self::RequireEscalated | Self::BypassSandbox)
    }

    /// Check if this permission requests additional sandboxed permissions.
    pub fn uses_additional_permissions(&self) -> bool {
        matches!(self, Self::WithAdditionalPermissions)
    }
}

/// A sandbox request proven valid by [`SandboxPermissions::resolve_request`].
///
/// Callers can trust the mode/additional combination without re-checking it:
/// the impl established the obligations once, at construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatedSandboxRequest {
    permissions: SandboxPermissions,
    additional: Option<AdditionalPermissions>,
}

impl ValidatedSandboxRequest {
    /// The normalized permission mode.
    #[must_use]
    pub fn permissions(&self) -> SandboxPermissions {
        self.permissions
    }

    /// The validated additional permissions, if any.
    #[must_use]
    pub fn additional(&self) -> Option<&AdditionalPermissions> {
        self.additional.as_ref()
    }

    /// Decompose into the trusted mode/additional pair.
    #[must_use]
    pub fn into_parts(self) -> (SandboxPermissions, Option<AdditionalPermissions>) {
        (self.permissions, self.additional)
    }
}

/// Additional per-command filesystem permissions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct AdditionalPermissions {
    /// Additional filesystem paths to grant read access.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fs_read: Vec<PathBuf>,
    /// Additional filesystem paths to grant write access.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fs_write: Vec<PathBuf>,
}

impl AdditionalPermissions {
    pub fn is_empty(&self) -> bool {
        self.fs_read.is_empty() && self.fs_write.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_permissions() {
        let perm = SandboxPermissions::default();
        assert_eq!(perm, SandboxPermissions::UseDefault);
        assert!(!perm.requires_escalated_permissions());
        assert!(!perm.uses_additional_permissions());
    }

    #[test]
    fn test_escalated_permissions() {
        let perm = SandboxPermissions::RequireEscalated;
        assert!(perm.requires_escalated_permissions());
    }

    #[test]
    fn test_bypass_permissions() {
        let perm = SandboxPermissions::BypassSandbox;
        assert!(perm.requires_escalated_permissions());
    }

    #[test]
    fn test_with_additional_permissions() {
        let perm = SandboxPermissions::WithAdditionalPermissions;
        assert!(perm.uses_additional_permissions());
        assert!(!perm.requires_escalated_permissions());
    }

    #[test]
    fn default_normalizes_for_non_empty_additional_permissions() {
        let additional = AdditionalPermissions {
            fs_read: vec![PathBuf::from("/tmp/reference")],
            fs_write: Vec::new(),
        };

        assert_eq!(
            SandboxPermissions::UseDefault.normalized_for(Some(&additional)),
            SandboxPermissions::WithAdditionalPermissions
        );
        assert_eq!(
            SandboxPermissions::RequireEscalated.normalized_for(Some(&additional)),
            SandboxPermissions::RequireEscalated
        );
        assert_eq!(
            SandboxPermissions::UseDefault.normalized_for(Some(&AdditionalPermissions::default())),
            SandboxPermissions::UseDefault
        );
    }

    fn non_empty_additional() -> AdditionalPermissions {
        AdditionalPermissions {
            fs_read: vec![PathBuf::from("/tmp/reference")],
            fs_write: Vec::new(),
        }
    }

    #[test]
    fn resolve_request_normalizes_default_with_additional() {
        let validated = SandboxPermissions::UseDefault
            .resolve_request(Some(non_empty_additional()), None)
            .expect("use_default plus additional should normalize");
        let (permissions, additional) = validated.into_parts();
        assert_eq!(permissions, SandboxPermissions::WithAdditionalPermissions);
        assert!(additional.is_some_and(|permissions| !permissions.is_empty()));
    }

    #[test]
    fn resolve_request_treats_empty_additional_as_absent() {
        let validated = SandboxPermissions::UseDefault
            .resolve_request(Some(AdditionalPermissions::default()), None)
            .expect("empty additional should be ignored");
        let (permissions, additional) = validated.into_parts();
        assert_eq!(permissions, SandboxPermissions::UseDefault);
        assert!(additional.is_none());
    }

    #[test]
    fn resolve_request_rejects_with_additional_without_paths() {
        let err = SandboxPermissions::WithAdditionalPermissions
            .resolve_request(None, None)
            .expect_err("with_additional_permissions without paths should fail");
        assert!(err.to_string().contains("missing `additional_permissions`"));
    }

    #[test]
    fn resolve_request_rejects_escalated_without_justification() {
        for mode in [SandboxPermissions::RequireEscalated, SandboxPermissions::BypassSandbox] {
            let err = mode
                .resolve_request(None, None)
                .expect_err("escalated modes require justification");
            assert!(err.to_string().contains("missing `justification`"));

            let err = mode
                .resolve_request(None, Some("   "))
                .expect_err("blank justification should fail");
            assert!(err.to_string().contains("missing `justification`"));
        }
    }

    #[test]
    fn resolve_request_rejects_escalated_with_additional_permissions() {
        for mode in [SandboxPermissions::RequireEscalated, SandboxPermissions::BypassSandbox] {
            let err = mode
                .resolve_request(Some(non_empty_additional()), Some("operator approved"))
                .expect_err("escalated modes must not combine additional permissions");
            assert!(
                err.to_string()
                    .contains("requires `sandbox_permissions` set to `with_additional_permissions`")
            );
        }
    }

    #[test]
    fn resolve_request_accepts_justified_escalated_request() {
        let validated = SandboxPermissions::RequireEscalated
            .resolve_request(None, Some("operator approved"))
            .expect("justified escalated request should resolve");
        let (permissions, additional) = validated.into_parts();
        assert_eq!(permissions, SandboxPermissions::RequireEscalated);
        assert!(additional.is_none());
    }
}
