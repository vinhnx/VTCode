//! Shared sandbox context for MCP stdio launches.

use anyhow::{Result, anyhow};
use hashbrown::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use vtcode_config::mcp::McpSandboxOverrideConfig;
use vtcode_safety::mcp_sandbox::{McpSandboxOverrides, derive_mcp_sandbox_policy};
use vtcode_safety::sandboxing::{CommandSpec, LinuxSandboxLauncher, SandboxManager, SandboxPolicy};

/// Sandbox policy and launch context inherited by MCP stdio providers.
#[derive(Debug, Clone)]
pub struct McpSandboxContext {
    policy: SandboxPolicy,
    sandbox_cwd: PathBuf,
    linux_sandbox_executable: Option<LinuxSandboxLauncher>,
}

impl McpSandboxContext {
    /// Create a context using the configured workspace as the sandbox cwd.
    #[must_use]
    pub fn new(policy: SandboxPolicy, sandbox_cwd: impl Into<PathBuf>) -> Self {
        Self {
            policy,
            sandbox_cwd: sandbox_cwd.into(),
            linux_sandbox_executable: LinuxSandboxLauncher::resolve(),
        }
    }

    /// Override the Linux helper path, primarily for embedding applications and tests.
    #[must_use]
    pub fn with_linux_sandbox_executable(mut self, executable: impl Into<PathBuf>) -> Self {
        self.linux_sandbox_executable = Some(LinuxSandboxLauncher::external(executable.into()));
        self
    }

    /// Return the policy carried by this context.
    #[must_use]
    pub fn policy(&self) -> &SandboxPolicy {
        &self.policy
    }

    /// Derive a per-server narrowed context from this session context.
    ///
    /// Unset override fields inherit the parent policy. Setting any resource
    /// limit opts into the conservative cap family for the unset siblings, so
    /// limits require a workspace-write session policy and fail otherwise.
    ///
    /// # Errors
    ///
    /// Returns an error when the overrides cannot narrow the parent safely.
    pub fn narrowed(&self, overrides: &McpSandboxOverrideConfig) -> Result<Self> {
        let has_limits = overrides.max_memory_mb.is_some()
            || overrides.max_pids.is_some()
            || overrides.cpu_time_secs.is_some()
            || overrides.timeout_secs.is_some();
        let defaults = McpSandboxOverrides::default();
        let narrowed = McpSandboxOverrides {
            enforce_conservative_limits: has_limits,
            max_memory_mb: overrides.max_memory_mb.unwrap_or(defaults.max_memory_mb),
            max_pids: overrides.max_pids.unwrap_or(defaults.max_pids),
            cpu_time_secs: overrides.cpu_time_secs.unwrap_or(defaults.cpu_time_secs),
            timeout_secs: overrides.timeout_secs.unwrap_or(defaults.timeout_secs),
            writable_root: overrides.writable_root.clone(),
            allowed_endpoints: Vec::new(),
        };
        let policy = derive_mcp_sandbox_policy(&self.policy, &narrowed)?;
        Ok(Self {
            policy,
            sandbox_cwd: self.sandbox_cwd.clone(),
            linux_sandbox_executable: self.linux_sandbox_executable.clone(),
        })
    }

    pub(crate) fn transform_stdio(
        &self,
        program: OsString,
        args: Vec<OsString>,
        working_dir: Option<&Path>,
        env: HashMap<OsString, OsString>,
    ) -> Result<SandboxedStdioCommand> {
        if matches!(self.policy, SandboxPolicy::ExternalSandbox { .. }) {
            return Err(anyhow!("MCP stdio cannot use an external sandbox policy without an external launcher"));
        }

        let cwd = working_dir.map(Path::to_path_buf).unwrap_or_else(|| self.sandbox_cwd.clone());
        let spec = CommandSpec::new(program.clone())
            .with_args(args.iter().map(|arg| arg.to_string_lossy().into_owned()))
            .with_cwd(cwd.clone())
            .with_env(
                env.iter()
                    .map(|(key, value)| (key.to_string_lossy().into_owned(), value.to_string_lossy().into_owned()))
                    .collect(),
            );
        let exec_env = SandboxManager::new()
            .transform(spec, &self.policy, &cwd, self.linux_sandbox_executable.as_ref())
            .map_err(|error| anyhow!("failed to apply MCP sandbox: {error}"))?;

        Ok(SandboxedStdioCommand {
            program: exec_env.program.into_os_string(),
            args: exec_env.args.into_iter().map(OsString::from).collect(),
            working_dir: exec_env.cwd,
            env: exec_env
                .env
                .into_iter()
                .map(|(key, value)| (OsString::from(key), OsString::from(value)))
                .collect(),
        })
    }
}

/// Resolve the effective sandbox context for one provider.
///
/// No overrides inherits the session context unchanged (including `None`).
/// Overrides without a session context are an error: silently ignoring
/// explicit sandbox requests would leave the server unsandboxed while
/// configured otherwise.
pub(crate) fn resolve_provider_sandbox_context(
    session: Option<McpSandboxContext>,
    overrides: Option<&McpSandboxOverrideConfig>,
) -> Result<Option<McpSandboxContext>> {
    match (session, overrides) {
        (session, None) => Ok(session),
        (Some(context), Some(overrides)) => Ok(Some(context.narrowed(overrides)?)),
        (None, Some(_)) => Err(anyhow!("MCP provider requests sandbox overrides but session sandboxing is disabled")),
    }
}

pub(crate) struct SandboxedStdioCommand {
    pub(crate) program: OsString,
    pub(crate) args: Vec<OsString>,
    pub(crate) working_dir: PathBuf,
    pub(crate) env: HashMap<OsString, OsString>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_context(policy: SandboxPolicy) -> McpSandboxContext {
        McpSandboxContext::new(policy, PathBuf::from("/tmp"))
    }

    #[test]
    fn narrowed_empty_overrides_inherits_parent() {
        let parent = session_context(SandboxPolicy::workspace_write(vec![]));
        let narrowed = parent
            .narrowed(&McpSandboxOverrideConfig::default())
            .expect("empty overrides inherit");
        assert_eq!(narrowed.policy().resource_limits(), parent.policy().resource_limits());
    }

    #[test]
    fn narrowed_applies_set_limit_and_conservative_siblings() {
        let parent = session_context(SandboxPolicy::workspace_write(vec![]));
        let overrides = McpSandboxOverrideConfig {
            max_memory_mb: Some(256),
            ..McpSandboxOverrideConfig::default()
        };
        let narrowed = parent.narrowed(&overrides).expect("limits narrow workspace-write parent");
        let limits = narrowed.policy().resource_limits();
        assert_eq!(limits.max_memory_mb, 256);
        assert_eq!(limits.max_pids, 64);
        assert_eq!(limits.timeout_secs, 120);
    }

    #[test]
    fn narrowed_rejects_limits_on_read_only_parent() {
        let parent = session_context(SandboxPolicy::read_only());
        let overrides = McpSandboxOverrideConfig {
            max_memory_mb: Some(256),
            ..McpSandboxOverrideConfig::default()
        };
        let err = parent.narrowed(&overrides).expect_err("read-only carries no limits");
        assert!(format!("{err:#}").contains("cannot carry"), "unexpected error: {err:#}");
    }

    #[test]
    fn resolve_absent_overrides_inherits_session_unchanged() {
        let session = session_context(SandboxPolicy::workspace_write(vec![]));
        let resolved = resolve_provider_sandbox_context(Some(session.clone()), None).expect("inherit");
        assert!(resolved.is_some());
        assert!(resolve_provider_sandbox_context(None, None).expect("none stays none").is_none());
    }

    #[test]
    fn resolve_overrides_without_session_context_fails_closed() {
        let overrides = McpSandboxOverrideConfig {
            max_memory_mb: Some(256),
            ..McpSandboxOverrideConfig::default()
        };
        let err =
            resolve_provider_sandbox_context(None, Some(&overrides)).expect_err("unsatisfiable overrides must fail");
        assert!(format!("{err:#}").contains("session sandboxing is disabled"), "unexpected error: {err:#}");
    }

    #[test]
    fn narrowed_writable_root_must_match_parent() {
        let parent = session_context(SandboxPolicy::workspace_write(vec![PathBuf::from("/work")]));
        let matching = McpSandboxOverrideConfig {
            writable_root: Some(PathBuf::from("/work")),
            ..McpSandboxOverrideConfig::default()
        };
        assert!(parent.narrowed(&matching).is_ok());
        let mismatched = McpSandboxOverrideConfig {
            writable_root: Some(PathBuf::from("/other")),
            ..McpSandboxOverrideConfig::default()
        };
        let err = parent.narrowed(&mismatched).expect_err("mismatched root must fail");
        assert!(format!("{err:#}").contains("must match every parent"), "unexpected error: {err:#}");
    }
}
