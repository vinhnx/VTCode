//! Sandbox manager for transforming commands into sandboxed execution environments.

use std::ffi::OsString;
use std::path::Path;

use super::child_spawn::build_sanitized_env;
use super::exec_env::{CommandSpec, ExecEnv, LinuxSandboxLauncher, SandboxType};
use super::network_proxy::ProxyEndpoints;
#[cfg(target_os = "macos")]
use super::policy::NetworkAllowlistEntry;
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
    ///
    /// `linux_launcher` describes how the Linux sandbox helper is invoked
    /// (`None` on other platforms, or when no helper could be resolved).
    pub fn transform(
        &self,
        spec: CommandSpec,
        policy: &SandboxPolicy,
        sandbox_cwd: &Path,
        linux_launcher: Option<&LinuxSandboxLauncher>,
    ) -> Result<ExecEnv, SandboxTransformError> {
        self.transform_with_proxy(spec, policy, sandbox_cwd, linux_launcher, None)
    }

    /// Transform with a running hostname proxy. Its endpoints are injected
    /// after environment sanitization and must be permitted by the sandbox
    /// profile; without a proxy, hostname allowlists fail closed.
    pub fn transform_with_proxy(
        &self,
        spec: CommandSpec,
        policy: &SandboxPolicy,
        sandbox_cwd: &Path,
        linux_launcher: Option<&LinuxSandboxLauncher>,
        proxy: Option<&ProxyEndpoints>,
    ) -> Result<ExecEnv, SandboxTransformError> {
        // Determine the sandbox type based on policy and platform
        let sandbox_type = self.determine_sandbox_type(policy)?;

        // A restrictive sandbox must not inherit secrets or dynamic-loader
        // controls, including values supplied by a caller through `spec.env`.
        // Full-access and externally managed policies intentionally preserve
        // the caller's environment because this manager is not their boundary.
        let spec = if sandbox_type == SandboxType::None {
            spec
        } else {
            let mut spec = spec;
            spec.env = build_sanitized_env(&spec.env, false, false, "", &[]);
            spec
        };

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
        let mut exec_env = match sandbox_type {
            SandboxType::MacosSeatbelt => self.transform_seatbelt(spec, policy, sandbox_cwd, proxy),
            SandboxType::LinuxLandlock => self.transform_landlock(spec, policy, sandbox_cwd, linux_launcher),
            SandboxType::WindowsRestrictedToken => self.transform_windows(spec, policy, sandbox_cwd),
            SandboxType::None => {
                return Err(SandboxTransformError::InvalidPolicy("Cannot transform with SandboxType::None".into()));
            }
        }?;

        if let Some(proxy) = proxy {
            for (key, value) in proxy.env_vars() {
                drop(exec_env.env.insert(key.to_string(), value));
            }
        }
        Ok(exec_env)
    }

    /// Determine the appropriate sandbox type for the given policy.
    fn determine_sandbox_type(&self, policy: &SandboxPolicy) -> Result<SandboxType, SandboxTransformError> {
        match policy {
            SandboxPolicy::DangerFullAccess | SandboxPolicy::ExternalSandbox { .. } => Ok(SandboxType::None),
            SandboxPolicy::ReadOnly { .. } | SandboxPolicy::WorkspaceWrite { .. } => {
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
        proxy: Option<&ProxyEndpoints>,
    ) -> Result<ExecEnv, SandboxTransformError> {
        const SEATBELT_EXECUTABLE: &str = "/usr/bin/sandbox-exec";

        // Build the seatbelt profile
        let profile = self.build_seatbelt_profile(policy, sandbox_cwd, proxy)?;

        let mut args = vec!["-p".to_string(), profile, os_string_to_arg(spec.program.clone())];
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
        _proxy: Option<&ProxyEndpoints>,
    ) -> Result<ExecEnv, SandboxTransformError> {
        Err(SandboxTransformError::UnavailableSandboxType(SandboxType::MacosSeatbelt))
    }

    /// Build a seatbelt profile string.
    ///
    /// Implements the field guide's recommendations:
    /// - "Default-deny outbound network, then allowlist."
    /// - Block sensitive paths to prevent credential leakage.
    #[cfg(target_os = "macos")]
    fn build_seatbelt_profile(
        &self,
        policy: &SandboxPolicy,
        sandbox_cwd: &Path,
        proxy: Option<&ProxyEndpoints>,
    ) -> Result<String, SandboxTransformError> {
        fn seatbelt_path(path: &Path) -> Result<&str, SandboxTransformError> {
            let path = path.to_str().ok_or_else(|| {
                SandboxTransformError::InvalidPolicy("Seatbelt paths must be valid UTF-8".to_string())
            })?;
            if path.chars().any(|ch| ch == '"' || ch == '\\' || ch.is_control()) {
                return Err(SandboxTransformError::InvalidPolicy(
                    "Seatbelt path contains unsupported characters".to_string(),
                ));
            }
            Ok(path)
        }

        fn append_network_rules(
            profile: &mut String,
            network_access: bool,
            network_allowlist: &[NetworkAllowlistEntry],
            proxy: Option<&ProxyEndpoints>,
        ) -> Result<(), SandboxTransformError> {
            if !network_allowlist.is_empty() {
                let Some(proxy) = proxy else {
                    return Err(SandboxTransformError::InvalidPolicy(
                        "hostname network allowlists require a running local proxy; refusing to widen access"
                            .to_string(),
                    ));
                };
                // Deny is the default; only the local proxy ports are reachable
                // from the sandboxed process. Domain rules are enforced by the
                // proxy after parsing CONNECT/SOCKS requests.
                // Seatbelt only accepts `localhost` or `*` as the host in a
                // `remote ip` address; a literal IPv4 host fails profile parsing.
                profile
                    .push_str(&format!("(allow network-outbound (remote ip \"localhost:{}\"))\n", proxy.http.port()));
                profile
                    .push_str(&format!("(allow network-outbound (remote ip \"localhost:{}\"))\n", proxy.socks5.port()));
                profile.push_str("(allow network* (local unix))\n");
                return Ok(());
            }

            if !network_access {
                // Keep local unix sockets available even when outbound network is restricted.
                profile.push_str("(allow network* (local unix))\n");
            }
            if network_access {
                profile.push_str("(allow network*)\n");
            }
            Ok(())
        }

        let mut profile = String::from("(version 1)\n");
        profile.push_str("(deny default)\n");
        profile.push_str("(allow process-exec)\n");
        profile.push_str("(allow process-fork)\n");
        profile.push_str("(allow sysctl-read)\n");
        profile.push_str("(allow mach-lookup)\n");
        profile.push_str("(allow ipc-posix-shm-read* (ipc-posix-name-prefix \"apple.cfprefs.\"))\n");
        profile.push_str("(allow mach-lookup (global-name \"com.apple.cfprefsd.daemon\") (global-name \"com.apple.cfprefsd.agent\") (local-name \"com.apple.cfprefsd.agent\"))\n");
        profile.push_str("(allow user-preference-read)\n");

        // Block sensitive paths BEFORE allowing general read access
        // This ensures deny rules take precedence
        let sensitive_paths = policy.sensitive_paths_for_execution(sandbox_cwd);
        for sp in &sensitive_paths {
            let expanded = sp.expand_path();
            let path_str = seatbelt_path(&expanded)?;
            if sp.block_read {
                profile.push_str(&format!("(deny file-read* (subpath \"{path_str}\"))\n"));
            }
            if sp.block_write {
                profile.push_str(&format!("(deny file-write* (subpath \"{path_str}\"))\n"));
            }
        }

        // Allow reading from everywhere (except denied sensitive paths above)
        profile.push_str("(allow file-read*)\n");

        match policy {
            SandboxPolicy::ReadOnly { network_access, network_allowlist } => {
                // Read-only: only allow writing to /dev/null
                profile.push_str("(allow file-write* (literal \"/dev/null\"))\n");
                append_network_rules(&mut profile, *network_access, network_allowlist, proxy)?;
            }
            SandboxPolicy::WorkspaceWrite { network_access, network_allowlist, .. } => {
                for root in policy.get_writable_roots_with_cwd(sandbox_cwd) {
                    let path = seatbelt_path(&root.root)?;
                    profile.push_str(&format!("(allow file-write* (subpath \"{path}\"))\n"));
                }
                append_network_rules(&mut profile, *network_access, network_allowlist, proxy)?;
            }
            _ => {}
        }

        Ok(profile)
    }

    /// Transform for Linux Landlock sandbox.
    ///
    /// Following the field guide: "Landlock + seccomp is the recommended Linux pattern."
    /// The sandbox helper binary receives both the policy (for Landlock filesystem rules)
    /// and the seccomp profile (for syscall filtering).
    fn transform_landlock(
        &self,
        spec: CommandSpec,
        policy: &SandboxPolicy,
        sandbox_cwd: &Path,
        linux_launcher: Option<&LinuxSandboxLauncher>,
    ) -> Result<ExecEnv, SandboxTransformError> {
        let launcher = linux_launcher.ok_or(SandboxTransformError::MissingSandboxExecutable)?;

        // Hostname allowlists cannot be enforced exactly by Landlock/seccomp
        // (BPF cannot inspect connect() destinations) and no managed proxy
        // exists yet — fail closed exactly like the Seatbelt profile does.
        if policy.has_network_allowlist() {
            return Err(SandboxTransformError::InvalidPolicy(
                "Linux sandbox cannot enforce hostname network allowlists exactly; refusing to run with unrestricted network"
                    .to_string(),
            ));
        }

        // Serialize the policy for the sandbox helper (includes Landlock rules)
        let policy_json = serde_json::to_string(policy)
            .map_err(|e| SandboxTransformError::CreationFailed(format!("failed to serialize sandbox policy: {e}")))?;

        // Serialize seccomp profile separately for explicit syscall filtering
        let seccomp_profile = policy.seccomp_profile();
        let seccomp_json = seccomp_profile
            .to_json()
            .map_err(|e| SandboxTransformError::CreationFailed(format!("failed to serialize seccomp profile: {e}")))?;

        // Resource limits are enforced only when a policy explicitly carries
        // them; auto-derived defaults (e.g. conservative limits for read-only)
        // are serialized as unlimited so ordinary commands keep working.
        let resource_limits = match policy {
            SandboxPolicy::WorkspaceWrite { resource_limits, .. } => resource_limits.clone(),
            _ => super::policy::ResourceLimits::unlimited(),
        };
        let limits_json = serde_json::to_string(&resource_limits)
            .map_err(|e| SandboxTransformError::CreationFailed(format!("failed to serialize resource limits: {e}")))?;

        let sandbox_cwd_str = sandbox_cwd.to_string_lossy().to_string();

        let mut args = launcher.prefix_args.clone();
        args.extend([
            "--sandbox-policy-cwd".to_string(),
            sandbox_cwd_str,
            "--sandbox-policy".to_string(),
            policy_json,
            "--seccomp-profile".to_string(),
            seccomp_json,
            "--resource-limits".to_string(),
            limits_json,
            "--".to_string(),
            os_string_to_arg(spec.program.clone()),
        ]);
        args.extend(spec.args);

        Ok(ExecEnv {
            program: launcher.program.clone(),
            args,
            cwd: spec.cwd,
            env: spec.env,
            expiration: spec.expiration,
            sandbox_active: true,
            sandbox_type: SandboxType::LinuxLandlock,
        })
    }

    /// Transform for Windows restricted token sandbox.
    ///
    /// Not yet implemented. Returns `UnavailableSandboxType` so that callers
    /// requesting a restrictive policy on Windows get an explicit error
    /// instead of silently running unsandboxed. The `is_available()` check in
    /// `transform()` normally catches this first, but this guard ensures
    /// fail-closed behavior even if the availability check is bypassed.
    fn transform_windows(
        &self,
        _spec: CommandSpec,
        _policy: &SandboxPolicy,
        _sandbox_cwd: &Path,
    ) -> Result<ExecEnv, SandboxTransformError> {
        Err(SandboxTransformError::UnavailableSandboxType(SandboxType::WindowsRestrictedToken))
    }
}

fn os_string_to_arg(value: OsString) -> String {
    value.into_string().unwrap_or_else(|value| value.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_no_sandbox_for_full_access() {
        let manager = SandboxManager::new();
        let spec = CommandSpec::new("echo").with_args(vec!["hello"]);
        let policy = SandboxPolicy::full_access();

        let env = manager.transform(spec, &policy, Path::new("/tmp"), None).unwrap();

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
        let result = manager.determine_sandbox_type(&SandboxPolicy::read_only());
        assert_eq!(result.unwrap(), SandboxType::platform_default());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_profile_includes_default_preferences_policy() {
        let manager = SandboxManager::new();
        let profile = manager
            .build_seatbelt_profile(&SandboxPolicy::read_only(), Path::new("/tmp"), None)
            .unwrap();

        assert!(profile.contains("(allow ipc-posix-shm-read* (ipc-posix-name-prefix \"apple.cfprefs.\"))"));
        assert!(profile.contains("(global-name \"com.apple.cfprefsd.daemon\")"));
        assert!(profile.contains("(global-name \"com.apple.cfprefsd.agent\")"));
        assert!(profile.contains("(local-name \"com.apple.cfprefsd.agent\")"));
        assert!(profile.contains("(allow user-preference-read)"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_rejects_hostname_allowlist_without_exact_enforcement() {
        let manager = SandboxManager::new();
        let policy = SandboxPolicy::read_only_with_network(vec![NetworkAllowlistEntry::https("api.example.com")]);

        let result = manager.build_seatbelt_profile(&policy, Path::new("/tmp"), None);

        assert!(matches!(result, Err(SandboxTransformError::InvalidPolicy(message)) if message.contains("hostname")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_profile_allows_only_local_proxy_for_hostname_allowlist() {
        let manager = SandboxManager::new();
        let policy = SandboxPolicy::read_only_with_network(vec![NetworkAllowlistEntry::https("api.example.com")]);
        let proxy = ProxyEndpoints {
            http: "127.0.0.1:3128".parse().expect("http endpoint"),
            socks5: "127.0.0.1:1080".parse().expect("socks endpoint"),
        };
        let profile = manager
            .build_seatbelt_profile(&policy, Path::new("/tmp"), Some(&proxy))
            .expect("proxy-backed allowlist profile");
        assert!(profile.contains("localhost:3128"));
        assert!(profile.contains("localhost:1080"));
        assert!(!profile.contains("(allow network*)"), "direct network must stay denied");

        let transform = manager
            .transform_with_proxy(CommandSpec::new("echo"), &policy, Path::new("/tmp"), None, Some(&proxy))
            .expect("transform");
        assert_eq!(transform.env.get("HTTPS_PROXY").map(String::as_str), Some("http://localhost:3128"));
        assert_eq!(transform.env.get("ALL_PROXY").map(String::as_str), Some("socks5://localhost:1080"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn seatbelt_rejects_profile_metacharacters_in_paths() {
        let manager = SandboxManager::new();
        let injected = Path::new("/tmp/\")(allow file-write*) ;");
        let policy = SandboxPolicy::workspace_write(vec![injected.to_path_buf()]);
        assert!(matches!(
            manager.build_seatbelt_profile(&policy, Path::new("/tmp/safe"), None),
            Err(SandboxTransformError::InvalidPolicy(_))
        ));
        assert!(matches!(
            manager.build_seatbelt_profile(&SandboxPolicy::workspace_write(vec![]), injected, None),
            Err(SandboxTransformError::InvalidPolicy(_))
        ));
        let mut sensitive_policy = SandboxPolicy::workspace_write(vec![]);
        if let SandboxPolicy::WorkspaceWrite { ref mut sensitive_paths, .. } = sensitive_policy {
            *sensitive_paths = Some(vec![super::super::policy::SensitivePath::new("/tmp/\") (allow network*) ;")]);
        }
        assert!(matches!(
            manager.build_seatbelt_profile(&sensitive_policy, Path::new("/tmp/safe"), None),
            Err(SandboxTransformError::InvalidPolicy(_))
        ));
        for path in ["/tmp/back\\slash", "/tmp/new\nline"] {
            assert!(matches!(
                manager.build_seatbelt_profile(
                    &SandboxPolicy::workspace_write(vec![path.into()]),
                    Path::new("/tmp/safe"),
                    None
                ),
                Err(SandboxTransformError::InvalidPolicy(_))
            ));
        }
        use std::os::unix::ffi::OsStringExt;
        let non_utf8 = OsString::from_vec(b"/tmp/invalid-\xff".to_vec());
        assert!(matches!(
            manager.build_seatbelt_profile(
                &SandboxPolicy::workspace_write(vec![non_utf8.into()]),
                Path::new("/tmp/safe"),
                None
            ),
            Err(SandboxTransformError::InvalidPolicy(_))
        ));
        assert!(
            manager
                .build_seatbelt_profile(&SandboxPolicy::workspace_write(vec![]), Path::new("/tmp/safe"), None)
                .is_ok()
        );
    }

    /// Windows restricted-token sandbox is not yet implemented, so
    /// `is_available()` must return `false` on **all** platforms. This
    /// prevents silent pass-through when a restrictive policy is requested.
    #[test]
    fn windows_restricted_token_is_not_available() {
        assert!(!SandboxType::WindowsRestrictedToken.is_available());
    }

    /// `transform_windows` must fail-closed with `UnavailableSandboxType`,
    /// not silently pass the command through unsandboxed.
    #[test]
    fn transform_windows_fails_closed() {
        let manager = SandboxManager::new();
        let spec = CommandSpec::new("echo").with_args(vec!["hello"]);
        let result = manager.transform_windows(spec, &SandboxPolicy::read_only(), Path::new("/tmp"));

        assert!(matches!(
            result,
            Err(SandboxTransformError::UnavailableSandboxType(SandboxType::WindowsRestrictedToken))
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn restrictive_linux_policy_requires_sandbox_helper() {
        let manager = SandboxManager::new();
        let spec = CommandSpec::new("echo").with_args(vec!["hello"]);

        let result = manager.transform(spec, &SandboxPolicy::read_only(), Path::new("/tmp"), None);

        if SandboxType::LinuxLandlock.is_available() {
            assert!(matches!(result, Err(SandboxTransformError::MissingSandboxExecutable)));
        } else {
            // Kernels without Landlock fail closed even earlier.
            assert!(matches!(result, Err(SandboxTransformError::UnavailableSandboxType(_))));
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_transform_rejects_hostname_allowlists() {
        let manager = SandboxManager::new();
        if !SandboxType::LinuxLandlock.is_available() {
            return;
        }
        let launcher = LinuxSandboxLauncher::busybox(PathBuf::from("/usr/local/bin/vtcode"));
        let policy = SandboxPolicy::read_only_with_network(vec![crate::sandboxing::NetworkAllowlistEntry::https(
            "api.example.com",
        )]);

        let result = manager.transform(CommandSpec::new("echo"), &policy, Path::new("/tmp"), Some(&launcher));

        assert!(
            matches!(&result, Err(SandboxTransformError::InvalidPolicy(message)) if message.contains("allowlist")),
            "allowlist must fail closed on Linux, got {result:?}"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn landlock_transform_prepends_busybox_subcommand() {
        let manager = SandboxManager::new();
        if !SandboxType::LinuxLandlock.is_available() {
            return;
        }
        let launcher = LinuxSandboxLauncher::busybox(PathBuf::from("/usr/local/bin/vtcode"));
        let env = manager
            .transform(
                CommandSpec::new("echo").with_args(vec!["hi"]),
                &SandboxPolicy::read_only(),
                Path::new("/tmp"),
                Some(&launcher),
            )
            .unwrap();

        assert!(env.sandbox_active);
        assert_eq!(env.program, PathBuf::from("/usr/local/bin/vtcode"));
        assert_eq!(env.args.first().map(String::as_str), Some("sandbox-exec"));
        assert!(env.args.iter().any(|arg| arg == "--sandbox-policy"));
        assert!(env.args.iter().any(|arg| arg == "--seccomp-profile"));
        assert!(env.args.iter().any(|arg| arg == "--resource-limits"));
        assert!(env.args.iter().any(|arg| arg == "--"));
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn restrictive_policy_filters_sensitive_environment_overrides() {
        use hashbrown::HashMap;

        let manager = SandboxManager::new();
        let mut env = HashMap::new();
        drop(env.insert("OPENAI_API_KEY".to_string(), "secret-value".to_string()));
        drop(env.insert("LD_PRELOAD".to_string(), "injected.so".to_string()));
        drop(env.insert("SAFE_PROJECT_NAME".to_string(), "vtcode".to_string()));
        drop(env.insert("PATH".to_string(), "/usr/bin:/bin".to_string()));
        drop(env.insert("INTERNAL_AUTH_BLOB".to_string(), "secret".to_string()));
        let spec = CommandSpec::new("echo").with_env(env);
        let sandbox_launcher = if cfg!(target_os = "linux") {
            Some(LinuxSandboxLauncher::external(Path::new("/tmp/vtcode-test-sandbox-helper").to_path_buf()))
        } else {
            None
        };

        let transformed = manager
            .transform(spec, &SandboxPolicy::read_only(), Path::new("/tmp"), sandbox_launcher.as_ref())
            .unwrap();

        assert!(transformed.sandbox_active);
        assert!(!transformed.env.contains_key("OPENAI_API_KEY"));
        assert!(!transformed.env.contains_key("LD_PRELOAD"));
        assert!(!transformed.env.contains_key("INTERNAL_AUTH_BLOB"));
        assert!(!transformed.env.contains_key("SAFE_PROJECT_NAME"));
        assert_eq!(transformed.env.get("PATH"), Some(&"/usr/bin:/bin".to_string()));
    }
    #[test]
    fn explicit_full_access_preserves_caller_environment() {
        let manager = SandboxManager::new();
        let mut env = hashbrown::HashMap::new();
        drop(env.insert("INTERNAL_AUTH_BLOB".to_string(), "explicit".to_string()));
        let result = manager
            .transform(
                CommandSpec::new("echo").with_env(env.clone()),
                &SandboxPolicy::full_access(),
                Path::new("."),
                None,
            )
            .expect("full access transform");
        assert!(!result.sandbox_active);
        assert_eq!(result.env, env);
    }
}
