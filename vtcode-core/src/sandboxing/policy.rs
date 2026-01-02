//! Sandbox policy definitions
//!
//! Defines the isolation levels for command execution, following the Codex model.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// A root directory that may be written to under the sandbox policy.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WritableRoot {
    /// Absolute path to the writable directory.
    pub root: PathBuf,
}

impl WritableRoot {
    /// Create a new writable root from a path.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { root: path.into() }
    }
}

/// Sandbox policy determining what operations are permitted during execution.
///
/// This follows the Codex sandboxing model with three main variants:
/// - **ReadOnly**: Only read operations allowed (safe for viewing files)
/// - **WorkspaceWrite**: Can write within specified directories
/// - **DangerFullAccess**: No restrictions (dangerous, requires explicit approval)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SandboxPolicy {
    /// No write access to the filesystem; network access may be disabled.
    ReadOnly,

    /// Write access limited to the specified roots; network optionally disabled.
    WorkspaceWrite {
        /// Directories where write access is permitted.
        writable_roots: Vec<WritableRoot>,

        /// Whether network access is allowed.
        #[serde(default)]
        network_access: bool,

        /// Exclude the TMPDIR environment variable from writable roots.
        #[serde(default)]
        exclude_tmpdir_env_var: bool,

        /// Exclude /tmp from writable roots.
        #[serde(default)]
        exclude_slash_tmp: bool,
    },

    /// Full access - no sandbox restrictions applied.
    /// Use with extreme caution.
    DangerFullAccess,

    /// External sandbox - the caller is responsible for sandbox setup.
    ExternalSandbox {
        /// Description of the external sandbox mechanism.
        description: String,
    },
}

impl Default for SandboxPolicy {
    fn default() -> Self {
        Self::ReadOnly
    }
}

impl SandboxPolicy {
    /// Create a read-only policy.
    pub fn read_only() -> Self {
        Self::ReadOnly
    }

    /// Create a new read-only policy (alias for backwards compatibility).
    pub fn new_read_only_policy() -> Self {
        Self::ReadOnly
    }

    /// Create a workspace-write policy with specified roots.
    pub fn workspace_write(writable_roots: Vec<PathBuf>) -> Self {
        Self::WorkspaceWrite {
            writable_roots: writable_roots.into_iter().map(WritableRoot::new).collect(),
            network_access: false,
            exclude_tmpdir_env_var: true,
            exclude_slash_tmp: true,
        }
    }

    /// Create a full-access policy (dangerous).
    pub fn full_access() -> Self {
        Self::DangerFullAccess
    }

    /// Check if the policy allows full network access.
    pub fn has_full_network_access(&self) -> bool {
        match self {
            Self::ReadOnly => false,
            Self::WorkspaceWrite { network_access, .. } => *network_access,
            Self::DangerFullAccess | Self::ExternalSandbox { .. } => true,
        }
    }

    /// Check if the policy allows full disk write access.
    pub fn has_full_disk_write_access(&self) -> bool {
        matches!(self, Self::DangerFullAccess | Self::ExternalSandbox { .. })
    }

    /// Check if the policy allows full disk read access.
    pub fn has_full_disk_read_access(&self) -> bool {
        // All policies allow read access
        true
    }

    /// Get the list of writable roots including the current working directory.
    pub fn get_writable_roots_with_cwd(&self, cwd: &Path) -> Vec<WritableRoot> {
        match self {
            Self::ReadOnly => vec![],
            Self::WorkspaceWrite { writable_roots, .. } => {
                let mut roots = writable_roots.clone();
                // Add cwd if not already included
                let cwd_root = WritableRoot::new(cwd);
                if !roots.contains(&cwd_root) {
                    roots.push(cwd_root);
                }
                roots
            }
            Self::DangerFullAccess | Self::ExternalSandbox { .. } => {
                // Full access - return cwd as a formality
                vec![WritableRoot::new(cwd)]
            }
        }
    }

    /// Check if a path is writable under this policy.
    pub fn is_path_writable(&self, path: &Path, cwd: &Path) -> bool {
        match self {
            Self::ReadOnly => false,
            Self::WorkspaceWrite { .. } => {
                let writable = self.get_writable_roots_with_cwd(cwd);
                writable.iter().any(|root| path.starts_with(&root.root))
            }
            Self::DangerFullAccess | Self::ExternalSandbox { .. } => true,
        }
    }

    /// Validate that another policy can be set from this one.
    /// Used to enforce policy escalation restrictions.
    pub fn can_set(&self, new_policy: &SandboxPolicy) -> anyhow::Result<()> {
        use SandboxPolicy::*;

        match (self, new_policy) {
            // Can always downgrade
            (DangerFullAccess, _) => Ok(()),
            // Cannot escalate from ReadOnly to write-capable
            (ReadOnly, WorkspaceWrite { .. } | DangerFullAccess) => Err(anyhow::anyhow!(
                "cannot escalate from read-only to write-capable policy"
            )),
            // Other transitions are allowed
            _ => Ok(()),
        }
    }

    /// Get a human-readable description of the policy.
    pub fn description(&self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only access",
            Self::WorkspaceWrite { .. } => "workspace write access",
            Self::DangerFullAccess => "full access (dangerous)",
            Self::ExternalSandbox { .. } => "external sandbox",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_only_policy() {
        let policy = SandboxPolicy::read_only();
        assert!(!policy.has_full_network_access());
        assert!(!policy.has_full_disk_write_access());
        assert!(policy.has_full_disk_read_access());
    }

    #[test]
    fn test_workspace_write_policy() {
        let policy = SandboxPolicy::workspace_write(vec![PathBuf::from("/tmp/workspace")]);
        assert!(!policy.has_full_network_access());
        assert!(!policy.has_full_disk_write_access());

        let cwd = PathBuf::from("/tmp/workspace");
        assert!(policy.is_path_writable(&cwd, &cwd));
        assert!(!policy.is_path_writable(&PathBuf::from("/etc"), &cwd));
    }

    #[test]
    fn test_full_access_policy() {
        let policy = SandboxPolicy::full_access();
        assert!(policy.has_full_network_access());
        assert!(policy.has_full_disk_write_access());
    }

    #[test]
    fn test_policy_escalation() {
        let read_only = SandboxPolicy::read_only();
        let full = SandboxPolicy::full_access();

        // Cannot escalate from read-only
        assert!(read_only.can_set(&full).is_err());

        // Can downgrade from full
        assert!(full.can_set(&read_only).is_ok());
    }
}
