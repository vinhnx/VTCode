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
//! stdio wrapper delegates to the canonical sandbox manager.

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};

use crate::sandboxing::{NetworkAllowlistEntry, ResourceLimits, SandboxPolicy, WritableRoot};

/// Per-MCP-server sandbox settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpSandboxOverrides {
    /// When true, [`derive_mcp_sandbox_policy`] applies the conservative
    /// resource caps below without relaxing tighter parent limits. Default `true`.
    #[serde(default = "default_true")]
    enforce_conservative_limits: bool,
    /// Memory cap (MB) applied to MCP child processes. Default `512`.
    #[serde(default = "default_memory_mb")]
    max_memory_mb: u64,
    /// Maximum number of processes / pids in the sandbox. Default `64`.
    #[serde(default = "default_max_pids")]
    max_pids: u64,
    /// CPU time budget in seconds. Default `60`.
    #[serde(default = "default_cpu_secs")]
    cpu_time_secs: u64,
    /// Wall-clock timeout in seconds. Default `120`.
    #[serde(default = "default_timeout_secs")]
    timeout_secs: u64,
    /// Optional writable root. With a workspace-write parent it must match
    /// every explicit parent root; the launch working directory remains writable.
    /// When unset, the sandbox inherits the parent's writable set.
    #[serde(default)]
    writable_root: Option<PathBuf>,
    /// Network allow-list override. With a restricted parent, entries must
    /// exactly match parent entries; otherwise derivation fails.
    #[serde(default)]
    allowed_endpoints: Vec<NetworkAllowlistEntry>,
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
/// Workspace-write policies also grant the launch working directory; the
/// caller must use the same working directory for the parent and derived policy.
/// Resource limits in the resulting policy are subject to platform enforcement.
///
/// # Errors
///
/// Returns an error when an override cannot be represented without potentially
/// widening the parent or when the policy cannot carry the requested limits.
pub fn derive_mcp_sandbox_policy(parent: &SandboxPolicy, overrides: &McpSandboxOverrides) -> Result<SandboxPolicy> {
    let mut derived = parent.clone();
    let has_restrictions = overrides.writable_root.is_some()
        || !overrides.allowed_endpoints.is_empty()
        || overrides.enforce_conservative_limits;
    if matches!(derived, SandboxPolicy::ExternalSandbox { .. }) && has_restrictions {
        return Err(anyhow!("MCP overrides cannot be applied to an external sandbox"));
    }
    if matches!(derived, SandboxPolicy::DangerFullAccess) && has_restrictions {
        derived = SandboxPolicy::WorkspaceWrite {
            writable_roots: overrides.writable_root.iter().cloned().map(WritableRoot::new).collect(),
            network_access: !overrides.allowed_endpoints.is_empty(),
            network_allowlist: Vec::new(),
            sensitive_paths: None,
            resource_limits: ResourceLimits::unlimited(),
            seccomp_profile: Default::default(),
            exclude_tmpdir_env_var: false,
            exclude_slash_tmp: false,
        };
    } else if let (Some(root), SandboxPolicy::WorkspaceWrite { writable_roots, .. }) =
        (&overrides.writable_root, &mut derived)
    {
        if writable_roots.is_empty() || writable_roots.iter().any(|parent_root| parent_root.root != *root) {
            return Err(anyhow!("MCP writable root must match every parent writable root"));
        }
    }
    match &mut derived {
        SandboxPolicy::WorkspaceWrite {
            network_access, network_allowlist, resource_limits, ..
        } => {
            narrow_network(network_access, network_allowlist, &overrides.allowed_endpoints)?;
            if overrides.enforce_conservative_limits {
                *resource_limits = intersect_resource_limits(resource_limits, &conservative_resource_limits(overrides));
            }
        }
        SandboxPolicy::ReadOnly { network_access, network_allowlist } => {
            if overrides.enforce_conservative_limits {
                return Err(anyhow!("read-only sandbox policy cannot carry MCP resource limits"));
            }
            narrow_network(network_access, network_allowlist, &overrides.allowed_endpoints)?;
        }
        _ => {}
    }
    Ok(derived)
}

fn narrow_network(
    access: &mut bool,
    parent: &mut Vec<NetworkAllowlistEntry>,
    requested: &[NetworkAllowlistEntry],
) -> Result<()> {
    if requested.is_empty() {
        return Ok(());
    }
    if parent.is_empty() {
        if !*access {
            return Err(anyhow!("MCP endpoints cannot enable network denied by the parent"));
        }
    } else if requested.iter().any(|entry| !parent.contains(entry)) {
        return Err(anyhow!("MCP endpoints must exactly match entries in the parent allowlist"));
    }
    *parent = requested.to_vec();
    *access = true;
    Ok(())
}

fn intersect_resource_limits(parent: &ResourceLimits, cap: &ResourceLimits) -> ResourceLimits {
    fn bounded<T: Ord + Default + Copy>(a: T, b: T) -> T {
        match (a == T::default(), b == T::default()) {
            (true, _) => b,
            (_, true) => a,
            _ => a.min(b),
        }
    }
    ResourceLimits {
        max_memory_mb: bounded(parent.max_memory_mb, cap.max_memory_mb),
        max_pids: bounded(parent.max_pids, cap.max_pids),
        max_disk_mb: bounded(parent.max_disk_mb, cap.max_disk_mb),
        cpu_time_secs: bounded(parent.cpu_time_secs, cap.cpu_time_secs),
        timeout_secs: (if parent.timeout_secs == 0 {
            300
        } else {
            parent.timeout_secs
        })
        .min(if cap.timeout_secs == 0 { 300 } else { cap.timeout_secs }),
    }
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

/// Apply the per-server sandbox wrapper to a stdio command.
///
/// This delegates to the same [`crate::sandboxing::SandboxManager`] used by
/// command execution. Unsupported platforms, missing Linux helpers, external
/// policies, workspace-write policies, and policies whose network rules cannot
/// be enforced exactly all return an error. A workspace-write policy always
/// carries a timeout (zero selects the default), which this command-only API
/// cannot enforce without owning the child lifecycle.
///
/// stdio configuration is inherited by default — callers that need to
/// redirect stdin/stdout/stderr should configure the returned command after
/// this call.
#[expect(
    unused_results,
    reason = "MCP command builder methods configure the owned command and return a fluent mutable reference."
)]
pub fn wrap_stdio_command(
    command: std::process::Command,
    sandbox_policy: &SandboxPolicy,
) -> Result<std::process::Command> {
    if matches!(sandbox_policy, SandboxPolicy::DangerFullAccess) {
        return Ok(command);
    }
    if matches!(sandbox_policy, SandboxPolicy::ExternalSandbox { .. }) {
        return Err(anyhow!("MCP stdio cannot use an external sandbox policy without an external launcher"));
    }
    if matches!(sandbox_policy, SandboxPolicy::WorkspaceWrite { .. }) {
        return Err(anyhow!("MCP stdio command wrapper cannot enforce workspace policy resource limits or timeout"));
    }

    let original_program = command
        .get_program()
        .to_str()
        .ok_or_else(|| anyhow!("MCP stdio program must be valid UTF-8"))?;
    let original_args: Vec<_> = command
        .get_args()
        .map(|arg| {
            arg.to_str()
                .map(str::to_owned)
                .ok_or_else(|| anyhow!("MCP stdio argument must be valid UTF-8"))
        })
        .collect::<Result<_>>()?;
    let current_dir = match command.get_current_dir() {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir().context("resolve MCP stdio working directory")?,
    };
    if current_dir.to_str().is_none() {
        return Err(anyhow!("MCP stdio working directory must be valid UTF-8"));
    }
    let envs: HashMap<_, _> = command
        .get_envs()
        .filter_map(|(key, value)| value.map(|value| (key, value)))
        .map(|(key, value)| {
            let key = key
                .to_str()
                .ok_or_else(|| anyhow!("MCP stdio environment name must be valid UTF-8"))?;
            let value = value
                .to_str()
                .ok_or_else(|| anyhow!("MCP stdio environment value must be valid UTF-8"))?;
            Ok((key.to_owned(), value.to_owned()))
        })
        .collect::<Result<_>>()?;

    let linux_launcher = crate::sandboxing::LinuxSandboxLauncher::resolve();
    let spec = crate::sandboxing::CommandSpec::new(original_program)
        .with_args(original_args)
        .with_cwd(current_dir.clone())
        .with_env(envs.into_iter().collect());
    let exec_env = crate::sandboxing::SandboxManager::new()
        .transform(spec, sandbox_policy, &current_dir, linux_launcher.as_ref())
        .context("transform MCP stdio command with the sandbox policy")?;

    let mut new_command = std::process::Command::new(exec_env.program);
    new_command.args(exec_env.args).current_dir(exec_env.cwd).env_clear();
    for (key, value) in exec_env.env {
        new_command.env(key, value);
    }
    Ok(new_command)
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
            writable_root: Some(PathBuf::from("/workspace")),
            ..McpSandboxOverrides::default()
        };
        let derived = derive_mcp_sandbox_policy(&parent, &overrides).unwrap();
        match derived {
            SandboxPolicy::WorkspaceWrite { writable_roots, .. } => {
                assert_eq!(writable_roots.len(), 1);
                assert_eq!(writable_roots[0].root, PathBuf::from("/workspace"));
            }
            other => panic!("expected WorkspaceWrite, got {other:?}"),
        }
    }

    #[test]
    fn derive_keeps_parent_roots_when_override_missing() {
        let parent = workspace_parent();
        let overrides = McpSandboxOverrides::default();
        let derived = derive_mcp_sandbox_policy(&parent, &overrides).unwrap();
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
            allowed_endpoints: vec![NetworkAllowlistEntry::https("api.example.com")],
            ..McpSandboxOverrides::default()
        };
        let derived = derive_mcp_sandbox_policy(&parent, &overrides).unwrap();
        match derived {
            SandboxPolicy::WorkspaceWrite { network_allowlist, .. } => {
                assert_eq!(network_allowlist.len(), 1);
                assert_eq!(network_allowlist[0].domain, "api.example.com");
            }
            other => panic!("expected WorkspaceWrite, got {other:?}"),
        }
    }

    #[test]
    fn derive_applies_conservative_resource_caps_by_default() {
        let parent = workspace_parent();
        let overrides = McpSandboxOverrides::default();
        let derived = derive_mcp_sandbox_policy(&parent, &overrides).unwrap();
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
        let derived = derive_mcp_sandbox_policy(&parent, &overrides).unwrap();
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
        let derived = derive_mcp_sandbox_policy(&SandboxPolicy::DangerFullAccess, &overrides).unwrap();
        match derived {
            SandboxPolicy::WorkspaceWrite { writable_roots, network_access, .. } => {
                assert_eq!(writable_roots.len(), 1);
                assert!(!network_access);
            }
            other => panic!("expected WorkspaceWrite downgrade, got {other:?}"),
        }
    }

    #[test]
    fn derive_rejects_roots_outside_parent_even_with_traversal() {
        for root in ["/other", "/workspace/../other", "/workspace/subdir"] {
            let overrides = McpSandboxOverrides {
                writable_root: Some(root.into()),
                ..McpSandboxOverrides::default()
            };
            assert!(derive_mcp_sandbox_policy(&workspace_parent(), &overrides).is_err(), "{root}");
        }
        let mut parent = workspace_parent();
        if let SandboxPolicy::WorkspaceWrite { ref mut writable_roots, .. } = parent {
            writable_roots.push(WritableRoot::new("/workspace/child"));
        }
        let overrides = McpSandboxOverrides {
            writable_root: Some("/workspace".into()),
            ..McpSandboxOverrides::default()
        };
        assert!(derive_mcp_sandbox_policy(&parent, &overrides).is_err());
    }

    #[test]
    fn derive_never_enables_denied_or_different_network() {
        let overrides = McpSandboxOverrides {
            allowed_endpoints: vec![NetworkAllowlistEntry::https("other.example.com")],
            enforce_conservative_limits: false,
            ..McpSandboxOverrides::default()
        };
        assert!(derive_mcp_sandbox_policy(&workspace_parent(), &overrides).is_err());
        assert!(derive_mcp_sandbox_policy(&SandboxPolicy::read_only(), &overrides).is_err());
        let full_network = SandboxPolicy::ReadOnly { network_access: true, network_allowlist: vec![] };
        let derived = derive_mcp_sandbox_policy(&full_network, &overrides).unwrap();
        assert!(derived.is_network_allowed("other.example.com", 443));
        assert!(!derived.is_network_allowed("unrelated.example.com", 443));
    }

    #[test]
    fn derive_keeps_tighter_parent_limits_and_default_timeout() {
        let mut parent = workspace_parent();
        if let SandboxPolicy::WorkspaceWrite { ref mut resource_limits, .. } = parent {
            *resource_limits = ResourceLimits {
                max_memory_mb: 128,
                max_pids: 0,
                max_disk_mb: 10,
                cpu_time_secs: 12,
                timeout_secs: 0,
            };
        }
        let overrides = McpSandboxOverrides {
            timeout_secs: 600,
            ..McpSandboxOverrides::default()
        };
        let derived = derive_mcp_sandbox_policy(&parent, &overrides).unwrap();
        assert_eq!(
            derived.resource_limits(),
            ResourceLimits {
                max_memory_mb: 128,
                max_pids: 64,
                max_disk_mb: 10,
                cpu_time_secs: 12,
                timeout_secs: 300,
            }
        );
    }

    #[test]
    fn derive_rejects_unrepresentable_limits_and_external_overrides() {
        assert!(derive_mcp_sandbox_policy(&SandboxPolicy::read_only(), &McpSandboxOverrides::default()).is_err());
        let external = SandboxPolicy::ExternalSandbox { description: "external".to_string() };
        assert!(derive_mcp_sandbox_policy(&external, &McpSandboxOverrides::default()).is_err());
        let no_overrides = McpSandboxOverrides {
            enforce_conservative_limits: false,
            ..McpSandboxOverrides::default()
        };
        assert!(matches!(
            derive_mcp_sandbox_policy(&external, &no_overrides),
            Ok(SandboxPolicy::ExternalSandbox { .. })
        ));
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
        let policy = SandboxPolicy::read_only_with_network(vec![NetworkAllowlistEntry::https("api.example.com")]);
        // A policy with a hostname allowlist must fail closed until the
        // platform can enforce the destination exactly.
        let command = std::process::Command::new("mcp-server");
        let result = wrap_stdio_command(command, &policy);
        assert!(result.is_err());
    }

    #[test]
    fn wrapper_rejects_workspace_limits_including_default_timeout() {
        for limits in [ResourceLimits::unlimited(), ResourceLimits::conservative()] {
            let mut policy = SandboxPolicy::workspace_write(vec![]);
            if let SandboxPolicy::WorkspaceWrite { ref mut resource_limits, .. } = policy {
                *resource_limits = limits;
            }
            let error = wrap_stdio_command(std::process::Command::new("mcp-server"), &policy).unwrap_err();
            assert!(error.to_string().contains("resource limits or timeout"));
        }
        assert!(wrap_stdio_command(std::process::Command::new("mcp-server"), &SandboxPolicy::full_access()).is_ok());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn wrapper_accepts_supported_read_only_policy() {
        let command = wrap_stdio_command(std::process::Command::new("/usr/bin/true"), &SandboxPolicy::read_only())
            .expect("read-only Seatbelt policy");
        assert_eq!(command.get_program(), "/usr/bin/sandbox-exec");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn wrapper_preserves_environment_removals_and_explicit_values() {
        let mut command = std::process::Command::new("/usr/bin/true");
        let _ = command.env_clear().env("PATH", "/explicit/bin").env_remove("HOME");
        let wrapped = wrap_stdio_command(command, &SandboxPolicy::read_only()).unwrap();
        let env: HashMap<_, _> = wrapped.get_envs().filter_map(|(k, v)| v.map(|v| (k, v))).collect();
        assert_eq!(env.get(std::ffi::OsStr::new("PATH")), Some(&std::ffi::OsStr::new("/explicit/bin")));
        assert!(!env.contains_key(std::ffi::OsStr::new("HOME")));
    }

    #[cfg(unix)]
    #[test]
    fn wrapper_rejects_non_utf8_command_data() {
        use std::os::unix::ffi::OsStringExt;
        let invalid = std::ffi::OsString::from_vec(vec![0xff]);
        let mut argument = std::process::Command::new("mcp-server");
        let _ = argument.arg(&invalid);
        assert!(
            wrap_stdio_command(argument, &SandboxPolicy::read_only())
                .unwrap_err()
                .to_string()
                .contains("argument")
        );
        let mut environment = std::process::Command::new("mcp-server");
        let _ = environment.env("PATH", &invalid);
        assert!(
            wrap_stdio_command(environment, &SandboxPolicy::read_only())
                .unwrap_err()
                .to_string()
                .contains("value")
        );
        let program = std::process::Command::new(&invalid);
        assert!(
            wrap_stdio_command(program, &SandboxPolicy::read_only())
                .unwrap_err()
                .to_string()
                .contains("program")
        );
        let mut name = std::process::Command::new("mcp-server");
        let _ = name.env(&invalid, "value");
        assert!(
            wrap_stdio_command(name, &SandboxPolicy::read_only())
                .unwrap_err()
                .to_string()
                .contains("name")
        );
        let mut cwd = std::process::Command::new("mcp-server");
        let _ = cwd.current_dir(&invalid);
        assert!(
            wrap_stdio_command(cwd, &SandboxPolicy::read_only())
                .unwrap_err()
                .to_string()
                .contains("working directory")
        );
    }
}
