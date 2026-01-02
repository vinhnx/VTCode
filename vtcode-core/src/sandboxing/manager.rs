//! Sandbox manager for transforming commands into sandboxed execution environments.

use std::path::Path;

use super::exec_env::{CommandSpec, ExecEnv, SandboxType};
use super::policy::SandboxPolicy;

/// Error type for sandbox transformation failures.
#[derive(Debug, thiserror::Error)]
pub enum SandboxTransformError {
    #[error("missing sandbox executable path")]
    MissingSandboxExecutable,

    #[error("sandbox type {0:?} is not available on this platform")]
    UnavailableSandboxType(SandboxType),

    #[error("failed to create sandbox environment: {0}")]
    CreationFailed(String),

    #[error("invalid sandbox policy: {0}")]
    InvalidPolicy(String),
}

/// Manager for sandbox transformation.
///
/// Transforms a `CommandSpec` into an `ExecEnv` by applying the appropriate
/// sandbox wrapper based on the platform and policy.
#[derive(Debug, Default)]
pub struct SandboxManager;

impl SandboxManager {
    /// Create a new sandbox manager.
    pub fn new() -> Self {
        Self
    }

    /// Transform a command specification into a sandboxed execution environment.
    pub fn transform(
        &self,
        spec: CommandSpec,
        policy: &SandboxPolicy,
        sandbox_cwd: &Path,
        sandbox_executable: Option<&Path>,
    ) -> Result<ExecEnv, SandboxTransformError> {
        // Determine the sandbox type based on policy and platform
        let sandbox_type = self.determine_sandbox_type(policy)?;

        // If no sandbox needed or full access, return direct execution
        if sandbox_type == SandboxType::None {
            return Ok(ExecEnv {
                program: spec.program.into(),
                args: spec.args,
                cwd: spec.cwd,
                env: spec.env,
                expiration: spec.expiration,
                sandbox_active: false,
                sandbox_type: SandboxType::None,
            });
        }

        // Check sandbox availability
        if !sandbox_type.is_available() {
            return Err(SandboxTransformError::UnavailableSandboxType(sandbox_type));
        }

        // Transform based on sandbox type
        match sandbox_type {
            SandboxType::MacosSeatbelt => self.transform_seatbelt(spec, policy, sandbox_cwd),
            SandboxType::LinuxLandlock => {
                self.transform_landlock(spec, policy, sandbox_cwd, sandbox_executable)
            }
            SandboxType::WindowsRestrictedToken => {
                self.transform_windows(spec, policy, sandbox_cwd)
            }
            SandboxType::None => unreachable!(),
        }
    }

    /// Determine the appropriate sandbox type for the given policy.
    fn determine_sandbox_type(
        &self,
        policy: &SandboxPolicy,
    ) -> Result<SandboxType, SandboxTransformError> {
        match policy {
            SandboxPolicy::DangerFullAccess | SandboxPolicy::ExternalSandbox { .. } => {
                Ok(SandboxType::None)
            }
            SandboxPolicy::ReadOnly | SandboxPolicy::WorkspaceWrite { .. } => {
                Ok(SandboxType::platform_default())
            }
        }
    }

    /// Transform for macOS Seatbelt sandbox.
    #[cfg(target_os = "macos")]
    fn transform_seatbelt(
        &self,
        spec: CommandSpec,
        policy: &SandboxPolicy,
        sandbox_cwd: &Path,
    ) -> Result<ExecEnv, SandboxTransformError> {
        const SEATBELT_EXECUTABLE: &str = "/usr/bin/sandbox-exec";

        // Build the seatbelt profile
        let profile = self.build_seatbelt_profile(policy, sandbox_cwd);

        let mut args = vec!["-p".to_string(), profile, spec.program.clone()];
        args.extend(spec.args);

        Ok(ExecEnv {
            program: SEATBELT_EXECUTABLE.into(),
            args,
            cwd: spec.cwd,
            env: spec.env,
            expiration: spec.expiration,
            sandbox_active: true,
            sandbox_type: SandboxType::MacosSeatbelt,
        })
    }

    #[cfg(not(target_os = "macos"))]
    fn transform_seatbelt(
        &self,
        _spec: CommandSpec,
        _policy: &SandboxPolicy,
        _sandbox_cwd: &Path,
    ) -> Result<ExecEnv, SandboxTransformError> {
        Err(SandboxTransformError::UnavailableSandboxType(
            SandboxType::MacosSeatbelt,
        ))
    }

    /// Build a seatbelt profile string.
    #[cfg(target_os = "macos")]
    fn build_seatbelt_profile(&self, policy: &SandboxPolicy, sandbox_cwd: &Path) -> String {
        let mut profile = String::from("(version 1)\n");
        profile.push_str("(deny default)\n");
        profile.push_str("(allow process-exec)\n");
        profile.push_str("(allow process-fork)\n");
        profile.push_str("(allow sysctl-read)\n");
        profile.push_str("(allow mach-lookup)\n");

        // Allow reading from everywhere
        profile.push_str("(allow file-read*)\n");

        match policy {
            SandboxPolicy::ReadOnly => {
                // Read-only: only allow writing to /dev/null
                profile.push_str("(allow file-write* (literal \"/dev/null\"))\n");
            }
            SandboxPolicy::WorkspaceWrite {
                writable_roots,
                network_access,
                ..
            } => {
                // Allow writing to workspace roots
                for root in writable_roots {
                    let path = root.root.display();
                    profile.push_str(&format!("(allow file-write* (subpath \"{}\"))\n", path));
                }
                // Always allow writing to cwd
                profile.push_str(&format!(
                    "(allow file-write* (subpath \"{}\"))\n",
                    sandbox_cwd.display()
                ));

                // Network access
                if *network_access {
                    profile.push_str("(allow network*)\n");
                } else {
                    profile.push_str("(allow network* (local unix))\n");
                }
            }
            _ => {}
        }

        profile
    }

    /// Transform for Linux Landlock sandbox.
    fn transform_landlock(
        &self,
        spec: CommandSpec,
        policy: &SandboxPolicy,
        sandbox_cwd: &Path,
        sandbox_executable: Option<&Path>,
    ) -> Result<ExecEnv, SandboxTransformError> {
        let sandbox_exe =
            sandbox_executable.ok_or(SandboxTransformError::MissingSandboxExecutable)?;

        // Serialize the policy for the sandbox helper
        let policy_json = serde_json::to_string(policy).map_err(|e| {
            SandboxTransformError::CreationFailed(format!(
                "failed to serialize sandbox policy: {}",
                e
            ))
        })?;

        let sandbox_cwd_str = sandbox_cwd.to_string_lossy().to_string();

        let mut args = vec![
            "--sandbox-policy-cwd".to_string(),
            sandbox_cwd_str,
            "--sandbox-policy".to_string(),
            policy_json,
            "--".to_string(),
            spec.program.clone(),
        ];
        args.extend(spec.args);

        Ok(ExecEnv {
            program: sandbox_exe.to_path_buf(),
            args,
            cwd: spec.cwd,
            env: spec.env,
            expiration: spec.expiration,
            sandbox_active: true,
            sandbox_type: SandboxType::LinuxLandlock,
        })
    }

    /// Transform for Windows restricted token sandbox.
    fn transform_windows(
        &self,
        spec: CommandSpec,
        _policy: &SandboxPolicy,
        _sandbox_cwd: &Path,
    ) -> Result<ExecEnv, SandboxTransformError> {
        // Windows sandbox uses restricted tokens - for now, pass through
        // A full implementation would use Windows job objects and restricted tokens
        Ok(ExecEnv {
            program: spec.program.into(),
            args: spec.args,
            cwd: spec.cwd,
            env: spec.env,
            expiration: spec.expiration,
            sandbox_active: false, // TODO: Implement Windows sandboxing
            sandbox_type: SandboxType::WindowsRestrictedToken,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_sandbox_for_full_access() {
        let manager = SandboxManager::new();
        let spec = CommandSpec::new("echo").with_args(vec!["hello"]);
        let policy = SandboxPolicy::full_access();

        let env = manager
            .transform(spec, &policy, Path::new("/tmp"), None)
            .unwrap();

        assert!(!env.sandbox_active);
        assert_eq!(env.sandbox_type, SandboxType::None);
    }

    #[test]
    fn test_sandbox_type_determination() {
        let manager = SandboxManager::new();

        // Full access = no sandbox
        let result = manager.determine_sandbox_type(&SandboxPolicy::DangerFullAccess);
        assert_eq!(result.unwrap(), SandboxType::None);

        // Read-only = platform default
        let result = manager.determine_sandbox_type(&SandboxPolicy::ReadOnly);
        assert_eq!(result.unwrap(), SandboxType::platform_default());
    }
}
