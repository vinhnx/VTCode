//! Per-MCP-server sandbox derivation.
//!
//! §18.4.4 of *The Hitchhiker's Guide to Agentic AI* calls for per-tool
//! sandboxing: every code-executing surface needs an isolation profile, an
//! audit trail, and resource limits. Today, MCP servers run as plain child
//! processes (`crates/codegen/vtcode-mcp/src/provider.rs::connect_stdio`) or as direct HTTP
//! clients (`crates/codegen/vtcode-mcp/src/rmcp_client.rs`) — only the general-purpose
//! `SandboxPolicy` protects the harness; MCP servers themselves have no
//! isolation beyond command / endpoint allow-lists.
//!
//! This module composes a derived [`SandboxPolicy`] for each MCP server from
//! the user-supplied parent policy and the per-server configuration. The
//! [`McpSandboxWrapper`] type describes the platform-specific command
//! prepending (Seatbelt on macOS, `sandbox-executable` on Linux) so the runloop
//! can apply it at spawn time.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::sandboxing::{NetworkAllowlistEntry, ResourceLimits, SandboxPolicy, WritableRoot};

/// Per-MCP-server sandbox settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpSandboxOverrides {
    /// When true, [`derive_mcp_sandbox_policy`] applies the conservative
    /// resource caps below regardless of the parent policy. Default `true`.
    #[serde(default = "default_true")]
    pub enforce_conservative_limits: bool,
    /// Memory cap (MB) applied to MCP child processes. Default `512`.
    #[serde(default = "default_memory_mb")]
    pub max_memory_mb: u64,
    /// Maximum number of processes / pids in the sandbox. Default `64`.
    #[serde(default = "default_max_pids")]
    pub max_pids: u64,
    /// CPU time budget in seconds. Default `60`.
    #[serde(default = "default_cpu_secs")]
    pub cpu_time_secs: u64,
    /// Wall-clock timeout in seconds. Default `120`.
    #[serde(default = "default_timeout_secs")]
    pub timeout_secs: u64,
    /// Optional writable root. When set, the sandbox restricts writes to this
    /// directory (plus the workspace, if the parent policy permits workspace
    /// writes). When unset, the sandbox inherits the parent's writable set.
    #[serde(default)]
    pub writable_root: Option<PathBuf>,
    /// Network allow-list override. When set, MCP servers may only talk to
    /// these hosts — even when the parent policy would otherwise allow egress.
    #[serde(default)]
    pub allowed_endpoints: Vec<NetworkAllowlistEntry>,
}

impl Default for McpSandboxOverrides {
    fn default() -> Self {
        Self {
            enforce_conservative_limits: true,
            max_memory_mb: 512,
            max_pids: 64,
            cpu_time_secs: 60,
            timeout_secs: 120,
            writable_root: None,
            allowed_endpoints: Vec::new(),
        }
    }
}

fn default_true() -> bool {
    true
}
fn default_memory_mb() -> u64 {
    512
}
fn default_max_pids() -> u64 {
    64
}
fn default_cpu_secs() -> u64 {
    60
}
fn default_timeout_secs() -> u64 {
    120
}

/// Derive a [`SandboxPolicy`] for a single MCP server from the parent policy
/// and per-server overrides.
///
/// The derived policy:
///
/// - inherits the parent's sensitive-path denials and write roots,
/// - narrows the writable set to `writable_root` when set,
/// - restricts outbound network to `allowed_endpoints` when non-empty,
/// - applies conservative resource caps when `enforce_conservative_limits` is
///   true.
///
/// `DangerFullAccess` parents are downgraded to `WorkspaceWrite` around the
/// server's writable root; `ReadOnly` parents are left unchanged.
#[must_use]
pub fn derive_mcp_sandbox_policy(
    parent: &SandboxPolicy,
    overrides: &McpSandboxOverrides,
) -> SandboxPolicy {
    let mut derived = parent.clone();

    if let Some(root) = &overrides.writable_root {
        // Replace the writable set with the override root. We coerce into a
        // `WorkspaceWrite` if the parent was `DangerFullAccess` so we still
        // have a place to hang the restrictions.
        derived = match derived {
            SandboxPolicy::WorkspaceWrite {
                writable_roots: _,
                network_access,
                network_allowlist,
                sensitive_paths,
                mut resource_limits,
                seccomp_profile,
                exclude_tmpdir_env_var,
                exclude_slash_tmp,
            } => {
                if overrides.enforce_conservative_limits {
                    resource_limits = conservative_resource_limits(overrides);
                }
                SandboxPolicy::WorkspaceWrite {
                    writable_roots: vec![WritableRoot::new(root.clone())],
                    network_access,
                    network_allowlist: if overrides.allowed_endpoints.is_empty() {
                        network_allowlist
                    } else {
                        overrides.allowed_endpoints.clone()
                    },
                    sensitive_paths,
                    resource_limits,
                    seccomp_profile,
                    exclude_tmpdir_env_var,
                    exclude_slash_tmp,
                }
            }
            SandboxPolicy::ReadOnly { mut network_allowlist, .. } => {
                if !overrides.allowed_endpoints.is_empty() {
                    network_allowlist = overrides.allowed_endpoints.clone();
                }
                SandboxPolicy::ReadOnly {
                    network_access: !network_allowlist.is_empty(),
                    network_allowlist,
                }
            }
            SandboxPolicy::DangerFullAccess => {
                let mut resource_limits = ResourceLimits::default();
                if overrides.enforce_conservative_limits {
                    resource_limits = conservative_resource_limits(overrides);
                }
                SandboxPolicy::WorkspaceWrite {
                    writable_roots: vec![WritableRoot::new(root.clone())],
                    network_access: false,
                    network_allowlist: overrides.allowed_endpoints.clone(),
                    sensitive_paths: None,
                    resource_limits,
                    seccomp_profile: Default::default(),
                    exclude_tmpdir_env_var: false,
                    exclude_slash_tmp: false,
                }
            }
            SandboxPolicy::ExternalSandbox { description } => {
                SandboxPolicy::ExternalSandbox { description }
            }
        };
    } else if !overrides.allowed_endpoints.is_empty() {
        // No writable_root override but the network allow-list should still
        // be applied.
        derived = match derived {
            SandboxPolicy::WorkspaceWrite {
                writable_roots,
                network_access,
                sensitive_paths,
                mut resource_limits,
                seccomp_profile,
                exclude_tmpdir_env_var,
                exclude_slash_tmp,
                ..
            } => {
                if overrides.enforce_conservative_limits {
                    resource_limits = conservative_resource_limits(overrides);
                }
                SandboxPolicy::WorkspaceWrite {
                    writable_roots,
                    network_access,
                    network_allowlist: overrides.allowed_endpoints.clone(),
                    sensitive_paths,
                    resource_limits,
                    seccomp_profile,
                    exclude_tmpdir_env_var,
                    exclude_slash_tmp,
                }
            }
            SandboxPolicy::ReadOnly { .. } => SandboxPolicy::ReadOnly {
                network_access: true,
                network_allowlist: overrides.allowed_endpoints.clone(),
            },
            other => other,
        };
    }

    if overrides.enforce_conservative_limits
        && let SandboxPolicy::WorkspaceWrite { ref mut resource_limits, .. } = derived
    {
        *resource_limits = conservative_resource_limits(overrides);
    }

    derived
}

fn conservative_resource_limits(overrides: &McpSandboxOverrides) -> ResourceLimits {
    ResourceLimits {
        max_memory_mb: overrides.max_memory_mb,
        max_pids: u32::try_from(overrides.max_pids).unwrap_or(u32::MAX),
        max_disk_mb: 1024,
        cpu_time_secs: overrides.cpu_time_secs,
        timeout_secs: overrides.timeout_secs,
    }
}

/// Description of the per-MCP-server sandbox profile that will be applied at
/// launch time.
#[derive(Debug, Clone)]
pub struct McpSandboxWrapper {
    /// Platform-specific wrapper binary (e.g. `/usr/bin/sandbox-exec` on macOS).
    pub wrapper_binary: PathBuf,
    /// Argument(s) needed to point the wrapper at the policy (e.g. `-p
    /// <profile>` on macOS or `--sandbox-policy <json>` on Linux).
    pub policy_argv: Vec<String>,
}

impl McpSandboxWrapper {
    /// Build the wrapper for the current platform.
    ///
    /// - **macOS**: uses `/usr/bin/sandbox-exec -p <profile>` with a serialized
    ///   Seatbelt policy summary.
    /// - **Linux**: emits `--sandbox-policy <json>` so the `sandbox-executable`
    ///   helper can apply Landlock + seccomp from the same JSON shape the rest
    ///   of VTCode uses (`ResourceLimits::to_json`).
    /// - **Windows**: returns `None` (sandboxing is stubbed; see
    ///   `crates/codegen/vtcode-safety/src/sandboxing/manager.rs`).
    #[must_use]
    pub fn for_current_platform(policy: &SandboxPolicy) -> Option<Self> {
        #[cfg(target_os = "macos")]
        {
            // macOS Seatbelt profiles are out of scope for this helper — the
            // caller is expected to call `crate::sandboxing::manager::SandboxManager`
            // to render the full profile. We emit a summary token here so the
            // wrapper still functions when the runloop hasn't built the full
            // Seatbelt profile (tests, MCP dry-run mode).
            let summary = match policy {
                SandboxPolicy::ReadOnly { network_allowlist, .. } => format!(
                    "(version 1) (allow default) (deny file-write*) (allow network* (remote ip {network_allowlist:?}))",
                ),
                SandboxPolicy::WorkspaceWrite { writable_roots, network_allowlist, .. } => format!(
                    "(version 1) (allow default) (allow file-write* (subpath {writable_roots:?})) (allow network* (remote ip {network_allowlist:?}))",
                ),
                SandboxPolicy::DangerFullAccess => "(version 1) (allow default)".to_owned(),
                SandboxPolicy::ExternalSandbox { description } => description.clone(),
            };
            Some(Self {
                wrapper_binary: PathBuf::from("/usr/bin/sandbox-exec"),
                policy_argv: vec!["-p".to_owned(), summary],
            })
        }
        #[cfg(target_os = "linux")]
        {
            let payload = policy_to_json(policy);
            Some(Self {
                wrapper_binary: PathBuf::from("sandbox-executable"),
                policy_argv: vec!["--sandbox-policy".to_owned(), payload],
            })
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let _ = policy;
            tracing::warn!("MCP server sandboxing is not supported on this platform");
            None
        }
    }
}

#[cfg(target_os = "linux")]
fn policy_to_json(policy: &SandboxPolicy) -> String {
    // The Linux helper consumes the same JSON shape the harness uses for
    // SeccompProfile + ResourceLimits. Falling back to the Debug representation
    // when serialization isn't available keeps the wrapper informative even if
    // the helper can't fully parse it.
    serde_json::to_string(policy).unwrap_or_else(|_| format!("{policy:?}"))
}

/// Apply the per-server sandbox wrapper to a stdio command.
///
/// On macOS the wrapper invokes `/usr/bin/sandbox-exec -p <profile>`; on
/// Linux it prepends `sandbox-executable --sandbox-policy <json>`; on Windows
/// the wrapper is a no-op (with a `tracing::warn!` from
/// [`McpSandboxWrapper::for_current_platform`]).
///
/// stdio configuration is inherited by default — callers that need to
/// redirect stdin/stdout/stderr should configure the returned command after
/// this call.
#[must_use]
pub fn wrap_stdio_command(
    command: std::process::Command,
    sandbox_policy: &SandboxPolicy,
) -> std::process::Command {
    let Some(wrapper) = McpSandboxWrapper::for_current_platform(sandbox_policy) else {
        return command;
    };

    let original_program = command.get_program().to_owned();
    let original_args: Vec<_> = command.get_args().map(|s| s.to_owned()).collect();
    let current_dir = command.get_current_dir().map(|p| p.to_owned());
    let envs: Vec<_> = command
        .get_envs()
        .filter_map(|(k, v)| v.map(|val| (k.to_owned(), val.to_owned())))
        .collect();

    let mut new_command = std::process::Command::new(&wrapper.wrapper_binary);
    for arg in &wrapper.policy_argv {
        new_command.arg(arg);
    }
    new_command.arg(&original_program);
    for arg in &original_args {
        new_command.arg(arg);
    }
    if let Some(dir) = current_dir {
        new_command.current_dir(dir);
    }
    for (key, val) in envs {
        new_command.env(key, val);
    }
    new_command
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandboxing::SeccompProfile;

    fn workspace_parent() -> SandboxPolicy {
        SandboxPolicy::WorkspaceWrite {
            writable_roots: vec![WritableRoot::new(PathBuf::from("/workspace"))],
            network_access: true,
            network_allowlist: vec![NetworkAllowlistEntry::https("api.example.com")],
            sensitive_paths: None,
            resource_limits: ResourceLimits::unlimited(),
            seccomp_profile: SeccompProfile::permissive(),
            exclude_tmpdir_env_var: false,
            exclude_slash_tmp: false,
        }
    }

    #[test]
    fn derive_narrows_writable_root_when_set() {
        let parent = workspace_parent();
        let overrides = McpSandboxOverrides {
            writable_root: Some(PathBuf::from("/tmp/mcp-scratch")),
            ..McpSandboxOverrides::default()
        };
        let derived = derive_mcp_sandbox_policy(&parent, &overrides);
        match derived {
            SandboxPolicy::WorkspaceWrite { writable_roots, .. } => {
                assert_eq!(writable_roots.len(), 1);
                assert_eq!(writable_roots[0].root, PathBuf::from("/tmp/mcp-scratch"));
            }
            other => panic!("expected WorkspaceWrite, got {other:?}"),
        }
    }

    #[test]
    fn derive_keeps_parent_roots_when_override_missing() {
        let parent = workspace_parent();
        let overrides = McpSandboxOverrides::default();
        let derived = derive_mcp_sandbox_policy(&parent, &overrides);
        match derived {
            SandboxPolicy::WorkspaceWrite { writable_roots, .. } => {
                assert_eq!(writable_roots.len(), 1);
                assert_eq!(writable_roots[0].root, PathBuf::from("/workspace"));
            }
            other => panic!("expected WorkspaceWrite, got {other:?}"),
        }
    }

    #[test]
    fn derive_restricts_network_to_allowed_endpoints() {
        let parent = workspace_parent();
        let overrides = McpSandboxOverrides {
            allowed_endpoints: vec![NetworkAllowlistEntry::https("mcp.internal")],
            ..McpSandboxOverrides::default()
        };
        let derived = derive_mcp_sandbox_policy(&parent, &overrides);
        match derived {
            SandboxPolicy::WorkspaceWrite { network_allowlist, .. } => {
                assert_eq!(network_allowlist.len(), 1);
                assert_eq!(network_allowlist[0].domain, "mcp.internal");
            }
            other => panic!("expected WorkspaceWrite, got {other:?}"),
        }
    }

    #[test]
    fn derive_applies_conservative_resource_caps_by_default() {
        let parent = workspace_parent();
        let overrides = McpSandboxOverrides::default();
        let derived = derive_mcp_sandbox_policy(&parent, &overrides);
        match derived {
            SandboxPolicy::WorkspaceWrite { resource_limits, .. } => {
                assert_eq!(resource_limits.max_memory_mb, 512);
                assert_eq!(resource_limits.max_pids, 64);
                assert_eq!(resource_limits.cpu_time_secs, 60);
                assert_eq!(resource_limits.timeout_secs, 120);
            }
            other => panic!("expected WorkspaceWrite, got {other:?}"),
        }
    }

    #[test]
    fn derive_respects_disabled_conservative_caps() {
        let parent = workspace_parent();
        let overrides = McpSandboxOverrides {
            enforce_conservative_limits: false,
            ..McpSandboxOverrides::default()
        };
        let derived = derive_mcp_sandbox_policy(&parent, &overrides);
        match derived {
            SandboxPolicy::WorkspaceWrite { resource_limits, .. } => {
                // Parent had `ResourceLimits::unlimited()` which is all zeros.
                assert_eq!(resource_limits.max_memory_mb, 0);
            }
            other => panic!("expected WorkspaceWrite, got {other:?}"),
        }
    }

    #[test]
    fn derive_downgrades_danger_full_access_with_writable_root() {
        let overrides = McpSandboxOverrides {
            writable_root: Some(PathBuf::from("/tmp/mcp-scratch")),
            ..McpSandboxOverrides::default()
        };
        let derived = derive_mcp_sandbox_policy(&SandboxPolicy::DangerFullAccess, &overrides);
        match derived {
            SandboxPolicy::WorkspaceWrite { writable_roots, network_access, .. } => {
                assert_eq!(writable_roots.len(), 1);
                assert!(!network_access);
            }
            other => panic!("expected WorkspaceWrite downgrade, got {other:?}"),
        }
    }

    #[test]
    fn overrides_default_is_conservative() {
        let defaults = McpSandboxOverrides::default();
        assert!(defaults.enforce_conservative_limits);
        assert_eq!(defaults.max_memory_mb, 512);
        assert!(defaults.allowed_endpoints.is_empty());
        assert!(defaults.writable_root.is_none());
    }

    #[test]
    fn wrapper_for_current_platform_handles_unsupported_targets() {
        let policy = workspace_parent();
        // We can't predict the platform from here, but the function must
        // either return Some wrapper (macOS / Linux) or None (Windows /
        // others) without panicking.
        let _ = McpSandboxWrapper::for_current_platform(&policy);
    }
}
