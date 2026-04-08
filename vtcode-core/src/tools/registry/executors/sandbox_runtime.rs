use crate::sandboxing::{
    AdditionalPermissions, CommandSpec as SandboxCommandSpec, NetworkAllowlistEntry,
    ResourceLimits, SandboxManager, SandboxPermissions, SandboxPolicy, SandboxTransformError,
    SeccompProfile, SensitivePath, WritableRoot, default_sensitive_paths,
};
use crate::tools::tool_intent;
use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use vtcode_config::{
    ResourceLimitsPreset, SandboxMode as RuntimeSandboxMode, SeccompProfilePreset,
};

#[derive(Debug, Clone)]
pub(super) struct ShellExecutionPlan {
    pub(super) approval_reason: Option<String>,
    pub(super) sandbox_policy: Option<SandboxPolicy>,
}

pub(super) const MISSING_ADDITIONAL_PERMISSIONS_MESSAGE: &str = "missing `additional_permissions`; provide `fs_read` and/or `fs_write` when using `with_additional_permissions`";
pub(super) const MISSING_ESCALATION_JUSTIFICATION_MESSAGE: &str = "missing `justification`; provide a short approval question when using `sandbox_permissions=require_escalated`";

fn push_unique_reason(reasons: &mut Vec<String>, reason: impl Into<String>) {
    let reason = reason.into();
    if !reasons.iter().any(|existing| existing == &reason) {
        reasons.push(reason);
    }
}

fn join_shell_approval_reasons(reasons: Vec<String>) -> Option<String> {
    if reasons.is_empty() {
        None
    } else {
        Some(reasons.join(" "))
    }
}

fn shell_permission_approval_reason(permissions: SandboxPermissions) -> Option<&'static str> {
    match permissions {
        SandboxPermissions::UseDefault => None,
        SandboxPermissions::RequireEscalated | SandboxPermissions::BypassSandbox => {
            Some("Command requested execution without sandbox restrictions.")
        }
        SandboxPermissions::WithAdditionalPermissions => {
            Some("Command requested additional sandboxed filesystem access.")
        }
    }
}

pub(super) fn shell_run_payload<'a>(
    tool_name: &str,
    tool_args: Option<&'a Value>,
) -> Option<&'a serde_json::Map<String, Value>> {
    let args_value = tool_args?;
    let args = args_value.as_object()?;
    tool_intent::is_command_run_tool_call(tool_name, args_value).then_some(args)
}

pub(super) fn shell_working_dir_value(payload: &serde_json::Map<String, Value>) -> Option<&str> {
    crate::tools::command_args::working_dir_text_from_payload(payload)
}

pub(super) fn build_shell_execution_plan(
    sandbox_config: &vtcode_config::SandboxConfig,
    workspace_root: &Path,
    requested_command: &[String],
    sandbox_permissions: SandboxPermissions,
    additional_permissions: Option<&AdditionalPermissions>,
) -> Result<ShellExecutionPlan> {
    let mut approval_reasons = Vec::new();
    if crate::command_safety::command_might_be_dangerous(requested_command) {
        push_unique_reason(
            &mut approval_reasons,
            "Command appears dangerous and requires approval.",
        );
    }
    if let Some(reason) = shell_permission_approval_reason(sandbox_permissions) {
        push_unique_reason(&mut approval_reasons, reason);
    }

    if sandbox_permissions.requires_escalated_permissions() || !sandbox_config.enabled {
        return Ok(ShellExecutionPlan {
            approval_reason: join_shell_approval_reasons(approval_reasons),
            sandbox_policy: None,
        });
    }

    let mut policy = sandbox_policy_from_runtime_config(sandbox_config, workspace_root)?;
    if matches!(policy, SandboxPolicy::ReadOnly { .. })
        && command_likely_writes_workspace(requested_command)
    {
        push_unique_reason(
            &mut approval_reasons,
            "Command appears to modify workspace files and needs workspace-write sandbox access.",
        );
        policy = workspace_write_policy_from_runtime_config(sandbox_config, workspace_root);
    }

    if sandbox_permissions.uses_additional_permissions() {
        let Some(additional_permissions) = additional_permissions else {
            return Err(anyhow!(MISSING_ADDITIONAL_PERMISSIONS_MESSAGE));
        };
        policy = sandbox_policy_with_additional_permissions(policy, additional_permissions);
    }

    let sandbox_policy = if matches!(policy, SandboxPolicy::DangerFullAccess) {
        None
    } else {
        Some(policy)
    };

    Ok(ShellExecutionPlan {
        approval_reason: join_shell_approval_reasons(approval_reasons),
        sandbox_policy,
    })
}

pub(super) fn sandbox_policy_from_runtime_config(
    sandbox_config: &vtcode_config::SandboxConfig,
    workspace_root: &Path,
) -> Result<SandboxPolicy> {
    match sandbox_config.default_mode {
        RuntimeSandboxMode::ReadOnly => Ok(read_only_policy_from_runtime_config(sandbox_config)),
        RuntimeSandboxMode::DangerFullAccess => Ok(SandboxPolicy::full_access()),
        RuntimeSandboxMode::External => Ok(SandboxPolicy::ExternalSandbox {
            description: format!(
                "external sandbox requested ({:?})",
                sandbox_config.external.sandbox_type
            ),
        }),
        RuntimeSandboxMode::WorkspaceWrite => Ok(workspace_write_policy_from_runtime_config(
            sandbox_config,
            workspace_root,
        )),
    }
}

fn read_only_policy_from_runtime_config(
    sandbox_config: &vtcode_config::SandboxConfig,
) -> SandboxPolicy {
    let (network_allow_all, network_allowlist) = runtime_network_policy(sandbox_config);

    if network_allow_all {
        SandboxPolicy::read_only_with_full_network()
    } else if network_allowlist.is_empty() {
        SandboxPolicy::read_only()
    } else {
        SandboxPolicy::read_only_with_network(network_allowlist)
    }
}

fn workspace_write_policy_from_runtime_config(
    sandbox_config: &vtcode_config::SandboxConfig,
    workspace_root: &Path,
) -> SandboxPolicy {
    let (network_allow_all, network_allowlist) = runtime_network_policy(sandbox_config);
    let network_enabled = network_allow_all || !network_allowlist.is_empty();
    let sensitive_paths = sensitive_paths_from_runtime_config(&sandbox_config.sensitive_paths);
    let resource_limits = resource_limits_from_runtime_config(&sandbox_config.resource_limits);
    let seccomp_profile = seccomp_profile_from_runtime_config(sandbox_config, network_enabled);

    let mut policy = SandboxPolicy::workspace_write_full(
        vec![workspace_root.to_path_buf()],
        network_allowlist,
        Some(sensitive_paths),
        resource_limits,
        seccomp_profile,
    );
    if network_allow_all
        && let SandboxPolicy::WorkspaceWrite {
            network_access,
            network_allowlist,
            ..
        } = &mut policy
    {
        *network_access = true;
        network_allowlist.clear();
    }
    policy
}

fn runtime_network_policy(
    sandbox_config: &vtcode_config::SandboxConfig,
) -> (bool, Vec<NetworkAllowlistEntry>) {
    let network_blocked = sandbox_config.network.block_all;
    let network_allow_all = !network_blocked && sandbox_config.network.allow_all;
    let network_allowlist = if network_blocked || sandbox_config.network.allow_all {
        Vec::new()
    } else {
        sandbox_config
            .network
            .allowlist
            .iter()
            .filter_map(|entry| {
                let domain = entry.domain.trim();
                (!domain.is_empty())
                    .then(|| NetworkAllowlistEntry::with_port(domain.to_string(), entry.port))
            })
            .collect()
    };

    (network_allow_all, network_allowlist)
}

fn resource_limits_from_runtime_config(
    limits_config: &vtcode_config::ResourceLimitsConfig,
) -> ResourceLimits {
    let mut limits = match limits_config.preset {
        ResourceLimitsPreset::Unlimited => ResourceLimits::unlimited(),
        ResourceLimitsPreset::Conservative => ResourceLimits::conservative(),
        ResourceLimitsPreset::Moderate => ResourceLimits::moderate(),
        ResourceLimitsPreset::Generous => ResourceLimits::generous(),
        ResourceLimitsPreset::Custom => ResourceLimits::default(),
    };

    if limits_config.max_memory_mb > 0 {
        limits.max_memory_mb = limits_config.max_memory_mb;
    }
    if limits_config.max_pids > 0 {
        limits.max_pids = limits_config.max_pids;
    }
    if limits_config.max_disk_mb > 0 {
        limits.max_disk_mb = limits_config.max_disk_mb;
    }
    if limits_config.cpu_time_secs > 0 {
        limits.cpu_time_secs = limits_config.cpu_time_secs;
    }
    if limits_config.timeout_secs > 0 {
        limits.timeout_secs = limits_config.timeout_secs;
    }

    limits
}

fn seccomp_profile_from_runtime_config(
    sandbox_config: &vtcode_config::SandboxConfig,
    network_enabled: bool,
) -> SeccompProfile {
    let seccomp_cfg = &sandbox_config.seccomp;
    let mut seccomp_profile =
        if !seccomp_cfg.enabled || seccomp_cfg.profile == SeccompProfilePreset::Disabled {
            SeccompProfile::permissive()
        } else {
            match seccomp_cfg.profile {
                SeccompProfilePreset::Strict => SeccompProfile::strict(),
                SeccompProfilePreset::Permissive => SeccompProfile::permissive(),
                SeccompProfilePreset::Disabled => SeccompProfile::permissive(),
            }
        };

    if network_enabled {
        seccomp_profile = seccomp_profile.with_network();
    }
    if seccomp_cfg.log_only {
        seccomp_profile = seccomp_profile.with_logging();
    }
    for syscall in &seccomp_cfg.additional_blocked {
        let syscall = syscall.trim();
        if !syscall.is_empty() {
            seccomp_profile = seccomp_profile.block_syscall(syscall.to_string());
        }
    }

    seccomp_profile
}

fn sensitive_paths_from_runtime_config(
    sensitive_paths_config: &vtcode_config::SensitivePathsConfig,
) -> Vec<SensitivePath> {
    let mut sensitive_paths = if sensitive_paths_config.use_defaults {
        default_sensitive_paths()
    } else {
        Vec::new()
    };

    for path in &sensitive_paths_config.additional {
        let path = path.trim();
        if !path.is_empty() {
            sensitive_paths.push(SensitivePath::new(path.to_string()));
        }
    }

    if !sensitive_paths_config.exceptions.is_empty() {
        let exception_paths = sensitive_paths_config
            .exceptions
            .iter()
            .filter_map(|path| {
                let path = path.trim();
                (!path.is_empty()).then(|| expand_tilde_path(path))
            })
            .collect::<Vec<_>>();
        sensitive_paths.retain(|entry| {
            let expanded = entry.expand_path();
            !exception_paths
                .iter()
                .any(|allowed| expanded.starts_with(allowed))
        });
    }

    sensitive_paths
}

pub(super) fn parse_requested_sandbox_permissions(
    payload: &serde_json::Map<String, Value>,
    cwd: &Path,
) -> Result<(SandboxPermissions, Option<AdditionalPermissions>)> {
    let sandbox_permissions = payload
        .get("sandbox_permissions")
        .cloned()
        .map(serde_json::from_value::<SandboxPermissions>)
        .transpose()
        .with_context(|| {
            "Invalid sandbox_permissions. Use one of: use_default, with_additional_permissions, require_escalated."
        })?
        .unwrap_or_default();

    let additional_permissions = payload
        .get("additional_permissions")
        .cloned()
        .map(serde_json::from_value::<AdditionalPermissions>)
        .transpose()
        .with_context(|| {
            "Invalid additional_permissions. Expected object with fs_read/fs_write string arrays."
        })?
        .filter(|permissions| !permissions.is_empty());

    if sandbox_permissions.requires_escalated_permissions() {
        let justification = payload
            .get("justification")
            .and_then(Value::as_str)
            .map(str::trim);
        if justification.is_none_or(str::is_empty) {
            return Err(anyhow!(MISSING_ESCALATION_JUSTIFICATION_MESSAGE));
        }
    }

    let additional_permissions = if sandbox_permissions.uses_additional_permissions() {
        let Some(additional_permissions) = additional_permissions else {
            return Err(anyhow!(MISSING_ADDITIONAL_PERMISSIONS_MESSAGE));
        };
        let normalized = normalize_additional_permissions(additional_permissions, cwd)?;
        if normalized.is_empty() {
            return Err(anyhow!(
                "`additional_permissions` must include at least one path in `fs_read` or `fs_write`"
            ));
        }
        Some(normalized)
    } else {
        if additional_permissions.is_some() {
            return Err(anyhow!(
                "`additional_permissions` requires `sandbox_permissions` set to `with_additional_permissions`"
            ));
        }
        None
    };

    Ok((sandbox_permissions, additional_permissions))
}

fn normalize_permission_paths(
    paths: Vec<PathBuf>,
    command_cwd: &Path,
    permission_kind: &str,
) -> Result<Vec<PathBuf>> {
    let mut out = Vec::with_capacity(paths.len());
    let mut seen = BTreeSet::new();

    for path in paths {
        if path.as_os_str().is_empty() {
            return Err(anyhow!("{permission_kind} contains an empty path"));
        }

        let resolved = if path.is_absolute() {
            path
        } else {
            command_cwd.join(path)
        };
        let normalized = crate::utils::path::normalize_path(&resolved);
        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
    }

    Ok(out)
}

fn normalize_additional_permissions(
    additional_permissions: AdditionalPermissions,
    command_cwd: &Path,
) -> Result<AdditionalPermissions> {
    let fs_read =
        normalize_permission_paths(additional_permissions.fs_read, command_cwd, "fs_read")?;
    let fs_write =
        normalize_permission_paths(additional_permissions.fs_write, command_cwd, "fs_write")?;

    Ok(AdditionalPermissions { fs_read, fs_write })
}

fn dedupe_writable_roots(writable_roots: Vec<WritableRoot>) -> Vec<WritableRoot> {
    let mut deduped = Vec::with_capacity(writable_roots.len());
    let mut seen = BTreeSet::new();

    for root in writable_roots {
        let normalized = crate::utils::path::normalize_path(&root.root);
        if seen.insert(normalized.clone()) {
            deduped.push(WritableRoot::new(normalized));
        }
    }

    deduped
}

pub(super) fn sandbox_policy_with_additional_permissions(
    sandbox_policy: SandboxPolicy,
    additional_permissions: &AdditionalPermissions,
) -> SandboxPolicy {
    if additional_permissions.is_empty() {
        return sandbox_policy;
    }

    match sandbox_policy {
        SandboxPolicy::DangerFullAccess | SandboxPolicy::ExternalSandbox { .. } => sandbox_policy,
        SandboxPolicy::WorkspaceWrite {
            writable_roots,
            network_access,
            network_allowlist,
            sensitive_paths,
            resource_limits,
            seccomp_profile,
            exclude_tmpdir_env_var,
            exclude_slash_tmp,
        } => {
            let mut merged_writes = writable_roots;
            merged_writes.extend(
                additional_permissions
                    .fs_write
                    .iter()
                    .cloned()
                    .map(WritableRoot::new),
            );
            SandboxPolicy::WorkspaceWrite {
                writable_roots: dedupe_writable_roots(merged_writes),
                network_access,
                network_allowlist,
                sensitive_paths,
                resource_limits,
                seccomp_profile,
                exclude_tmpdir_env_var,
                exclude_slash_tmp,
            }
        }
        SandboxPolicy::ReadOnly {
            network_access,
            network_allowlist,
        } => {
            if additional_permissions.fs_write.is_empty() {
                SandboxPolicy::ReadOnly {
                    network_access,
                    network_allowlist,
                }
            } else {
                let network_enabled = network_access || !network_allowlist.is_empty();
                SandboxPolicy::WorkspaceWrite {
                    writable_roots: dedupe_writable_roots(
                        additional_permissions
                            .fs_write
                            .iter()
                            .cloned()
                            .map(WritableRoot::new)
                            .collect(),
                    ),
                    network_access,
                    network_allowlist,
                    sensitive_paths: None,
                    resource_limits: ResourceLimits::conservative(),
                    seccomp_profile: if network_enabled {
                        SeccompProfile::strict().with_network()
                    } else {
                        SeccompProfile::strict()
                    },
                    exclude_tmpdir_env_var: false,
                    exclude_slash_tmp: false,
                }
            }
        }
    }
}

pub(super) fn apply_runtime_sandbox_to_command(
    command: Vec<String>,
    requested_command: &[String],
    sandbox_config: &vtcode_config::SandboxConfig,
    workspace_root: &Path,
    working_dir: &Path,
    sandbox_permissions: SandboxPermissions,
    additional_permissions: Option<&AdditionalPermissions>,
) -> Result<Vec<String>> {
    let plan = build_shell_execution_plan(
        sandbox_config,
        workspace_root,
        requested_command,
        sandbox_permissions,
        additional_permissions,
    )?;
    let Some(policy) = plan.sandbox_policy else {
        return Ok(command);
    };
    if matches!(policy, SandboxPolicy::ExternalSandbox { .. }) {
        return Err(anyhow!(
            "Sandbox mode 'external' is not supported by local command-session execution. \
             Use `read_only`/`workspace_write` or disable sandbox for this run."
        ));
    }

    enforce_sandbox_preflight_guards(requested_command, &policy, working_dir)?;
    transform_command_with_sandbox_policy(command, &policy, working_dir)
}

fn transform_command_with_sandbox_policy(
    command: Vec<String>,
    policy: &SandboxPolicy,
    sandbox_cwd: &Path,
) -> Result<Vec<String>> {
    if command.is_empty() {
        return Err(anyhow!("Sandbox transform received an empty command"));
    }

    let spec = SandboxCommandSpec::new(command[0].clone())
        .with_args(command[1..].to_vec())
        .with_cwd(sandbox_cwd.to_path_buf());
    let manager = SandboxManager::new();
    let linux_sandbox_executable = resolve_linux_sandbox_executable();
    let exec_env = manager
        .transform(
            spec,
            policy,
            sandbox_cwd,
            linux_sandbox_executable.as_deref(),
        )
        .map_err(|err| map_sandbox_transform_error(err, policy))?;

    let executable = exec_env.program.to_string_lossy().to_string();
    if exec_env.sandbox_active && !Path::new(&executable).exists() {
        return Err(anyhow!(
            "Sandbox is enabled but executable '{}' was not found on this system.",
            executable
        ));
    }

    let mut transformed = Vec::with_capacity(1 + exec_env.args.len());
    transformed.push(executable);
    transformed.extend(exec_env.args);
    Ok(transformed)
}

fn map_sandbox_transform_error(
    error: SandboxTransformError,
    policy: &SandboxPolicy,
) -> anyhow::Error {
    match error {
        SandboxTransformError::MissingSandboxExecutable => anyhow!(
            "Sandbox is enabled for '{}' but no Linux sandbox helper is configured. \
             Set `VTCODE_LINUX_SANDBOX_EXECUTABLE` to a helper that accepts \
             `--sandbox-policy`, `--seccomp-profile`, and `--resource-limits`.",
            policy.description()
        ),
        SandboxTransformError::UnavailableSandboxType(sandbox_type) => anyhow!(
            "Sandbox policy '{}' requires {:?}, which is unavailable on this platform.",
            policy.description(),
            sandbox_type
        ),
        SandboxTransformError::CreationFailed(msg) | SandboxTransformError::InvalidPolicy(msg) => {
            anyhow!(
                "Failed to initialize sandbox for command execution: {}",
                msg
            )
        }
    }
}

pub(super) fn enforce_sandbox_preflight_guards(
    requested_command: &[String],
    policy: &SandboxPolicy,
    working_dir: &Path,
) -> Result<()> {
    if requested_command.is_empty() {
        return Ok(());
    }

    let network_disabled = !policy.has_full_network_access() && !policy.has_network_allowlist();
    if network_disabled && command_likely_needs_network(requested_command) {
        return Err(anyhow!(
            "Command '{}' appears to require network access, but sandbox policy '{}' denies network.",
            shell_words::join(requested_command.iter().map(String::as_str)),
            policy.description()
        ));
    }

    let mut blocked_paths = BTreeSet::new();
    for argument in requested_command.iter().skip(1) {
        if let Some(candidate) = resolve_argument_path(argument, working_dir)
            && !policy.is_path_readable(&candidate)
        {
            blocked_paths.insert(candidate.display().to_string());
        }
    }
    if !blocked_paths.is_empty() {
        let listed = blocked_paths
            .into_iter()
            .take(3)
            .collect::<Vec<_>>()
            .join(", ");
        return Err(anyhow!(
            "Command references sensitive path(s) blocked by sandbox policy '{}': {}",
            policy.description(),
            listed
        ));
    }

    if command_likely_writes_workspace(requested_command) {
        let mut blocked_write_paths = BTreeSet::new();
        for argument in requested_command.iter().skip(1) {
            if let Some(candidate) = resolve_argument_path(argument, working_dir)
                && !policy.is_path_writable(&candidate, working_dir)
            {
                blocked_write_paths.insert(candidate.display().to_string());
            }
        }
        if !blocked_write_paths.is_empty() {
            let listed = blocked_write_paths
                .into_iter()
                .take(3)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(anyhow!(
                "Command references path(s) blocked for writes by sandbox policy '{}': {}",
                policy.description(),
                listed
            ));
        }
    }

    Ok(())
}

fn command_likely_needs_network(command: &[String]) -> bool {
    let Some(program) = command.first() else {
        return false;
    };
    let name = Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(program.as_str())
        .to_ascii_lowercase();
    if matches!(
        name.as_str(),
        "curl"
            | "wget"
            | "ping"
            | "ssh"
            | "scp"
            | "sftp"
            | "ftp"
            | "telnet"
            | "nc"
            | "ncat"
            | "nmap"
            | "dig"
            | "nslookup"
            | "host"
    ) {
        return true;
    }
    if name == "git" {
        return command.iter().skip(1).any(|arg| {
            matches!(
                arg.as_str(),
                "clone" | "fetch" | "pull" | "push" | "ls-remote" | "remote" | "submodule"
            )
        });
    }
    false
}

fn command_likely_writes_workspace(command: &[String]) -> bool {
    let Some(program) = command.first() else {
        return false;
    };
    let name = Path::new(program)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(program.as_str())
        .to_ascii_lowercase();

    if matches!(
        name.as_str(),
        "touch"
            | "mkdir"
            | "rm"
            | "rmdir"
            | "mv"
            | "cp"
            | "chmod"
            | "chown"
            | "ln"
            | "install"
            | "truncate"
            | "rustfmt"
            | "gofmt"
    ) {
        return true;
    }

    if name == "sed" {
        return command
            .iter()
            .skip(1)
            .any(|arg| arg == "-i" || arg.starts_with("-i"));
    }

    if name == "perl" {
        return command
            .iter()
            .skip(1)
            .any(|arg| arg == "-i" || arg.starts_with("-i"));
    }

    if name == "cargo" {
        return command.iter().skip(1).any(|arg| {
            matches!(
                arg.as_str(),
                "fmt" | "fix" | "build" | "check" | "clippy" | "test" | "nextest" | "clean"
            )
        });
    }

    if matches!(name.as_str(), "npm" | "pnpm" | "yarn" | "bun") {
        return command
            .iter()
            .skip(1)
            .any(|arg| matches!(arg.as_str(), "install" | "ci" | "add" | "update"));
    }

    if name == "go" {
        return command
            .iter()
            .skip(1)
            .any(|arg| matches!(arg.as_str(), "fmt" | "test" | "build" | "mod"));
    }

    if name == "git" {
        return command.iter().skip(1).any(|arg| {
            matches!(
                arg.as_str(),
                "add"
                    | "apply"
                    | "checkout"
                    | "switch"
                    | "merge"
                    | "rebase"
                    | "cherry-pick"
                    | "commit"
                    | "stash"
                    | "reset"
                    | "restore"
                    | "rm"
                    | "mv"
            )
        });
    }

    false
}

fn resolve_argument_path(argument: &str, working_dir: &Path) -> Option<PathBuf> {
    let trimmed = argument.trim().trim_matches(|ch| ch == '"' || ch == '\'');
    if trimmed.is_empty() || trimmed.starts_with('-') || trimmed.contains("://") {
        return None;
    }

    let candidate = if trimmed.starts_with("~/") || trimmed == "~" {
        Some(expand_tilde_path(trimmed))
    } else if trimmed.starts_with('/') {
        Some(PathBuf::from(trimmed))
    } else if trimmed.starts_with("./") || trimmed.starts_with("../") {
        Some(working_dir.join(trimmed))
    } else if let Some((_, value)) = trimmed.split_once('=') {
        if value.starts_with("~/")
            || value == "~"
            || value.starts_with('/')
            || value.starts_with("./")
            || value.starts_with("../")
        {
            Some(resolve_argument_path(value, working_dir)?)
        } else {
            None
        }
    } else {
        None
    }?;

    Some(candidate)
}

fn expand_tilde_path(path: &str) -> PathBuf {
    if path == "~" {
        return dirs::home_dir().unwrap_or_else(|| PathBuf::from(path));
    }
    if let Some(rest) = path.strip_prefix("~/")
        && let Some(home) = dirs::home_dir()
    {
        return home.join(rest);
    }
    PathBuf::from(path)
}

#[cfg(target_os = "linux")]
fn resolve_linux_sandbox_executable() -> Option<PathBuf> {
    std::env::var_os("VTCODE_LINUX_SANDBOX_EXECUTABLE").map(PathBuf::from)
}

#[cfg(not(target_os = "linux"))]
fn resolve_linux_sandbox_executable() -> Option<PathBuf> {
    None
}
