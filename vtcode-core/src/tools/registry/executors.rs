use crate::exec::code_executor::Language;
use crate::exec::skill_manager::{Skill, SkillMetadata};
use crate::mcp::{DetailLevel, ToolDiscovery};
use crate::sandboxing::{
    AdditionalPermissions, CommandSpec as SandboxCommandSpec, NetworkAllowlistEntry,
    ResourceLimits, SandboxManager, SandboxPermissions, SandboxPolicy, SandboxTransformError,
    SeccompProfile, SensitivePath, WritableRoot, default_sensitive_paths,
};
use crate::tools::continuation::PtyContinuationArgs;
use crate::tools::edited_file_monitor::{MutationLease, conflict_override_snapshot};
use crate::tools::file_tracker::FileTracker;
use crate::tools::registry::unified_actions::{
    UnifiedExecAction, UnifiedFileAction, UnifiedSearchAction,
};
use crate::tools::shell::resolve_fallback_shell;
use crate::tools::tool_intent;
use crate::tools::traits::Tool;
use crate::tools::types::VTCodeExecSession;
use crate::zsh_exec_bridge::ZshExecBridgeSession;
use regex::Regex;

use anyhow::{Context, Result, anyhow};
use chrono;
use futures::future::BoxFuture;
use hashbrown::HashMap;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};
use tokio::fs;
use vtcode_config::{
    ResourceLimitsPreset, SandboxMode as RuntimeSandboxMode, SeccompProfilePreset,
};

use super::ToolRegistry;

#[derive(Debug, Clone)]
struct ShellExecutionPlan {
    approval_reason: Option<String>,
    sandbox_policy: Option<SandboxPolicy>,
}

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

fn shell_run_payload<'a>(
    tool_name: &str,
    tool_args: Option<&'a Value>,
) -> Option<&'a serde_json::Map<String, Value>> {
    let args_value = tool_args?;
    let args = args_value.as_object()?;
    tool_intent::is_command_run_tool_call(tool_name, args_value).then_some(args)
}

fn shell_working_dir_value(payload: &serde_json::Map<String, Value>) -> Option<&str> {
    crate::tools::command_args::working_dir_text_from_payload(payload)
}

fn build_shell_execution_plan(
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

fn sandbox_policy_from_runtime_config(
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

const MISSING_ADDITIONAL_PERMISSIONS_MESSAGE: &str = "missing `additional_permissions`; provide `fs_read` and/or `fs_write` when using `with_additional_permissions`";
const MISSING_ESCALATION_JUSTIFICATION_MESSAGE: &str = "missing `justification`; provide a short approval question when using `sandbox_permissions=require_escalated`";

fn parse_requested_sandbox_permissions(
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

fn sandbox_policy_with_additional_permissions(
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

fn apply_runtime_sandbox_to_command(
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

fn enforce_sandbox_preflight_guards(
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

fn summarized_arg_keys(args: &Value) -> String {
    match args.as_object() {
        Some(map) => {
            if map.is_empty() {
                return "<none>".to_string();
            }
            let mut keys: Vec<&str> = map.keys().map(|k| k.as_str()).collect();
            keys.sort_unstable();
            let mut preview = keys.into_iter().take(10).collect::<Vec<_>>().join(", ");
            if map.len() > 10 {
                preview.push_str(", ...");
            }
            preview
        }
        None => match args {
            Value::Null => "<null>".to_string(),
            Value::Array(_) => "<array>".to_string(),
            Value::String(_) => "<string>".to_string(),
            Value::Bool(_) => "<bool>".to_string(),
            Value::Number(_) => "<number>".to_string(),
            Value::Object(_) => "<object>".to_string(),
        },
    }
}

fn serialized_payload_size_bytes(args: &Value) -> usize {
    serde_json::to_vec(args)
        .map(|bytes| bytes.len())
        .unwrap_or_else(|_| args.to_string().len())
}

fn missing_unified_exec_action_error(args: &Value) -> anyhow::Error {
    anyhow!(
        "Missing unified_exec action. Use `action` or fields: \
         `command|cmd|raw_command` (run), `session_id`+`input|chars|text` (write), \
         `session_id` (poll), `action:\"continue\"` with `session_id` and optional `input|chars|text`, \
         `spool_path|query|head_lines|tail_lines|max_matches|literal` (inspect), \
         or `action:\"list\"|\"close\"`. Keys: {}",
        summarized_arg_keys(args)
    )
}

fn missing_unified_file_action_error(args: &Value) -> anyhow::Error {
    anyhow!(
        "Missing action in unified_file. Provide `action` or file-operation fields such as \
         `path`, `content`, `old_str`, `patch`, or `destination`. Received keys: {}",
        summarized_arg_keys(args)
    )
}

fn missing_unified_search_action_error(args: &Value) -> anyhow::Error {
    anyhow!(
        "Missing unified_search action. Use `action` or fields: \
         `pattern|query` (grep), `action:\"structural\"` with `pattern` (structural search), `path` (list), `keyword` (tools), \
         `scope` (errors), `url` (web), `sub_action|name` (skill). Keys: {}",
        summarized_arg_keys(args)
    )
}

fn is_valid_pty_session_id(session_id: &str) -> bool {
    !session_id.trim().is_empty()
        && session_id.len() <= 128
        && session_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn validate_exec_session_id<'a>(raw_session_id: &'a str, context: &str) -> Result<&'a str> {
    let session_id = raw_session_id.trim();
    if is_valid_pty_session_id(session_id) {
        Ok(session_id)
    } else {
        Err(anyhow!(
            "Invalid session_id for {}: '{}'. Expected an ASCII token (letters, digits, '-', '_').",
            context,
            raw_session_id
        ))
    }
}

fn build_session_command_display_parts(command: &str, args: &[String]) -> String {
    if let Some(flag_index) = args
        .iter()
        .position(|arg| matches!(arg.as_str(), "-c" | "/C" | "-Command"))
        && let Some(command) = args.get(flag_index + 1)
        && !command.trim().is_empty()
    {
        return command.clone();
    }

    let mut parts = Vec::with_capacity(1 + args.len());
    if !command.trim().is_empty() {
        parts.push(command);
    }
    for arg in args {
        if !arg.trim().is_empty() {
            parts.push(arg.as_str());
        }
    }

    if parts.is_empty() {
        "unknown".to_string()
    } else {
        shell_words::join(parts)
    }
}

fn build_exec_session_command_display(session: &VTCodeExecSession) -> String {
    build_session_command_display_parts(&session.command, &session.args)
}

fn is_pty_exec_session(session: &VTCodeExecSession) -> bool {
    session.backend == "pty"
}

fn attach_exec_response_context(
    response: &mut Value,
    session: &VTCodeExecSession,
    command: &str,
    is_exited: bool,
) {
    response["session_id"] = json!(session.id);
    response["command"] = json!(command);
    if let Some(value) = session.working_dir.as_deref() {
        response["working_directory"] = json!(value);
    }
    response["backend"] = json!(session.backend);
    if let Some(rows) = session.rows {
        response["rows"] = json!(rows);
    }
    if let Some(cols) = session.cols {
        response["cols"] = json!(cols);
    }
    response["is_exited"] = json!(is_exited);
}

fn extract_run_session_id_from_tool_output_path(path: &str) -> Option<String> {
    let file_name = Path::new(path).file_name()?.to_str()?;
    let session_id = file_name.strip_suffix(".txt")?;
    if session_id.starts_with("run-") && is_valid_pty_session_id(session_id) {
        Some(session_id.to_string())
    } else {
        None
    }
}

fn extract_run_session_id_from_read_file_error(error_message: &str) -> Option<String> {
    let marker = "session_id=\"";
    let start = error_message.find(marker)? + marker.len();
    let rest = &error_message[start..];
    let end = rest.find('"')?;
    let session_id = &rest[..end];
    if session_id.starts_with("run-") && is_valid_pty_session_id(session_id) {
        Some(session_id.to_string())
    } else {
        None
    }
}

fn build_read_pty_fallback_args(args: &Value, error_message: &str) -> Option<Value> {
    let session_id = args
        .get("path")
        .or_else(|| args.get("file_path"))
        .or_else(|| args.get("filepath"))
        .or_else(|| args.get("target_path"))
        .and_then(Value::as_str)
        .and_then(extract_run_session_id_from_tool_output_path)
        .or_else(|| extract_run_session_id_from_read_file_error(error_message))?;

    let mut payload = serde_json::Map::new();
    payload.insert("session_id".to_string(), json!(session_id));

    if let Some(yield_time_ms) = args.get("yield_time_ms").cloned() {
        payload.insert("yield_time_ms".to_string(), yield_time_ms);
    }

    Some(Value::Object(payload))
}

const DEFAULT_INSPECT_HEAD_LINES: usize = 30;
const DEFAULT_INSPECT_TAIL_LINES: usize = 30;
const DEFAULT_INSPECT_MAX_MATCHES: usize = 200;
const MIN_EXEC_YIELD_MS: u64 = 250;
const MAX_EXEC_YIELD_MS: u64 = 30_000;
const EXEC_OUTPUT_TRUNCATED_SENTINEL: &str = "\n[Output truncated]";

struct ExecOutputPreview {
    raw_output: String,
    output: String,
    truncated: bool,
}

fn attach_pty_continuation(response: &mut Value, session_id: &str) {
    response["next_continue_args"] = PtyContinuationArgs::new(session_id).to_value();
}

fn clamp_exec_yield_ms(value: Option<u64>, default: u64) -> u64 {
    value
        .unwrap_or(default)
        .clamp(MIN_EXEC_YIELD_MS, MAX_EXEC_YIELD_MS)
}

fn clamp_peek_yield_ms(value: Option<u64>) -> u64 {
    value.unwrap_or(0).min(MAX_EXEC_YIELD_MS)
}

fn max_output_tokens_from_payload(payload: &serde_json::Map<String, Value>) -> Option<usize> {
    payload
        .get("max_output_tokens")
        .or_else(|| payload.get("max_tokens"))
        .and_then(Value::as_u64)
        .map(|value| value as usize)
}

fn floor_exec_char_boundary(text: &str, index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }

    let mut boundary = index;
    while boundary > 0 && !text.is_char_boundary(boundary) {
        boundary -= 1;
    }
    boundary
}

fn build_exec_output_preview(raw_output: String, max_tokens: usize) -> ExecOutputPreview {
    let max_output_len = max_tokens.saturating_mul(4);
    if max_tokens == 0 || raw_output.len() <= max_output_len {
        return ExecOutputPreview {
            output: raw_output.clone(),
            raw_output,
            truncated: false,
        };
    }

    let preview_end = floor_exec_char_boundary(&raw_output, max_output_len);
    let mut output = raw_output[..preview_end].to_string();
    output.push_str(EXEC_OUTPUT_TRUNCATED_SENTINEL);

    ExecOutputPreview {
        raw_output,
        output,
        truncated: true,
    }
}

fn first_command_token(command: &str) -> Option<String> {
    shell_words::split(command)
        .ok()
        .and_then(|parts| parts.into_iter().next())
        .filter(|part| !part.trim().is_empty())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CargoTestCommandKind {
    Test,
    Nextest,
}

fn cargo_test_command_kind(command: &str) -> Option<CargoTestCommandKind> {
    let parts = shell_words::split(command).ok()?;
    match parts.as_slice() {
        [cargo, test, ..] if cargo == "cargo" && test == "test" => Some(CargoTestCommandKind::Test),
        [cargo, nextest, run, ..] if cargo == "cargo" && nextest == "nextest" && run == "run" => {
            Some(CargoTestCommandKind::Nextest)
        }
        _ => None,
    }
}

fn cargo_package_from_command(command: &str) -> Option<String> {
    let parts = shell_words::split(command).ok()?;
    let mut iter = parts.iter();
    while let Some(part) = iter.next() {
        match part.as_str() {
            "-p" | "--package" => {
                let value = iter.next()?.trim();
                if !value.is_empty() {
                    return Some(value.to_string());
                }
            }
            _ => {}
        }
    }
    None
}

fn infer_cargo_test_binary_kind(
    source_file: Option<&str>,
    test_fqname: Option<&str>,
) -> &'static str {
    let normalized_path = source_file.map(|path| path.replace('\\', "/"));
    if let Some(path) = normalized_path.as_deref() {
        if path.starts_with("tests/") || path.contains("/tests/") {
            return "integration";
        }
        if path.starts_with("src/") || path.contains("/src/") {
            return "unit";
        }
    }

    if test_fqname.is_some_and(|name| name.contains("::")) {
        "unit"
    } else {
        "unknown"
    }
}

fn cargo_test_rerun_hint(
    command_kind: CargoTestCommandKind,
    package: &str,
    binary_kind: &str,
    test_fqname: &str,
) -> String {
    match command_kind {
        CargoTestCommandKind::Nextest => format!("cargo nextest run -p {package} {test_fqname}"),
        CargoTestCommandKind::Test if binary_kind == "unit" => {
            format!("cargo test -p {package} --lib {test_fqname} -- --nocapture")
        }
        CargoTestCommandKind::Test => {
            format!("cargo test -p {package} {test_fqname} -- --nocapture")
        }
    }
}

fn cargo_selector_error_diagnostics(
    command_kind: CargoTestCommandKind,
    command: &str,
    output: &str,
) -> Option<Value> {
    let regex =
        Regex::new(r"(?m)^error: no test target named `([^`]+)` in `([^`]+)` package$").ok()?;
    let captures = regex.captures(output)?;
    let requested_target = captures.get(1)?.as_str().trim();
    let package = captures.get(2)?.as_str().trim();
    if requested_target.is_empty() || package.is_empty() {
        return None;
    }

    let validation_hint =
        format!("cargo test -p {package} --lib -- --list | rg '{requested_target}'");
    let rerun_hint = match command_kind {
        CargoTestCommandKind::Nextest => {
            format!("cargo nextest run -p {package} {requested_target}")
        }
        CargoTestCommandKind::Test => {
            format!("cargo test -p {package} --lib {requested_target} -- --nocapture")
        }
    };

    Some(json!({
        "kind": "cargo_test_selector_error",
        "package": package,
        "binary_kind": "test_target_selector",
        "requested_test_target": requested_target,
        "selector_error": true,
        "validation_hint": validation_hint,
        "rerun_hint": rerun_hint,
        "critical_note": format!(
            "Cargo rejected `{requested_target}` as a test target. This usually means a unit test name was passed to `--test`."
        ),
        "next_action": format!(
            "Validate whether `{requested_target}` is a unit test with: {validation_hint}"
        ),
        "command": command,
    }))
}

fn cargo_failed_test_and_package(output: &str) -> (Option<String>, Option<String>) {
    let fail_line =
        Regex::new(r"(?m)^\s*FAIL \[[^\]]+\](?: \(\s*\d+/\d+\))? ([^\s]+) ([^\s]+)\s*$").ok();
    for line in output.lines().rev() {
        let trimmed = line.trim();
        if !trimmed.starts_with("FAIL [") {
            continue;
        }
        if let Some(regex) = fail_line.as_ref()
            && let Some(captures) = regex.captures(trimmed)
        {
            let package = captures.get(1).map(|value| value.as_str().trim());
            let test_fqname = captures.get(2).map(|value| value.as_str().trim());
            if let (Some(package), Some(test_fqname)) = (package, test_fqname)
                && !package.is_empty()
                && !test_fqname.is_empty()
            {
                return (Some(package.to_string()), Some(test_fqname.to_string()));
            }
        }
    }

    let thread_regex = Regex::new(r"thread '([^']+)'").ok();
    let test_fqname = thread_regex.and_then(|regex| {
        regex.captures_iter(output).find_map(|captures| {
            let candidate = captures.get(1)?.as_str().trim();
            (!candidate.is_empty()).then(|| candidate.to_string())
        })
    });
    (None, test_fqname)
}

fn cargo_panic_location_and_message(output: &str) -> (Option<String>, Option<u64>, Option<String>) {
    let panic_location = Regex::new(r"^(.+):(\d+):\d+:$").ok();
    let lines: Vec<&str> = output.lines().collect();
    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        let Some((_, location)) = trimmed.split_once(" panicked at ") else {
            continue;
        };
        let Some(regex) = panic_location.as_ref() else {
            break;
        };
        let Some(captures) = regex.captures(location) else {
            continue;
        };

        let source_file = captures
            .get(1)
            .map(|value| value.as_str().trim().to_string());
        let source_line = captures
            .get(2)
            .and_then(|value| value.as_str().parse::<u64>().ok());
        let panic_message = lines.iter().skip(index + 1).find_map(|candidate| {
            let trimmed = candidate.trim();
            if trimmed.is_empty() {
                return None;
            }
            if trimmed.starts_with("note:") || trimmed.starts_with("stack backtrace:") {
                return None;
            }
            Some(trimmed.to_string())
        });
        return (source_file, source_line, panic_message);
    }

    (None, None, None)
}

fn cargo_test_failure_diagnostics(
    command: &str,
    output: &str,
    exit_code: Option<i32>,
) -> Option<Value> {
    if exit_code == Some(0) {
        return None;
    }

    let command_kind = cargo_test_command_kind(command)?;
    if let Some(diagnostics) = cargo_selector_error_diagnostics(command_kind, command, output) {
        return Some(diagnostics);
    }

    let (package_from_output, test_fqname) = cargo_failed_test_and_package(output);
    let (source_file, source_line, panic_message) = cargo_panic_location_and_message(output);
    let package = package_from_output.or_else(|| cargo_package_from_command(command))?;
    let test_fqname = test_fqname?;
    let binary_kind =
        infer_cargo_test_binary_kind(source_file.as_deref(), Some(test_fqname.as_str()));
    let rerun_hint = cargo_test_rerun_hint(command_kind, &package, binary_kind, &test_fqname);

    Some(json!({
        "kind": "cargo_test_failure",
        "package": package,
        "binary_kind": binary_kind,
        "test_fqname": test_fqname,
        "panic": panic_message,
        "source_file": source_file,
        "source_line": source_line,
        "rerun_hint": rerun_hint,
        "critical_note": "Cargo reported a concrete failing test with a panic location.",
        "next_action": format!("Rerun the failing test directly with: {rerun_hint}"),
        "command": command,
    }))
}

fn attach_failure_diagnostics_metadata(response: &mut Value, diagnostics: &Value) {
    if let Some(obj) = response.as_object_mut() {
        for key in [
            "package",
            "binary_kind",
            "test_fqname",
            "panic",
            "source_file",
            "source_line",
            "selector_error",
            "validation_hint",
            "rerun_hint",
            "critical_note",
            "next_action",
        ] {
            if let Some(value) = diagnostics.get(key) {
                obj.insert(key.to_string(), value.clone());
            }
        }
        obj.insert("failure_diagnostics".to_string(), diagnostics.clone());
    }
}

fn attach_exec_recovery_guidance(response: &mut Value, command: &str, exit_code: Option<i32>) {
    if exit_code != Some(127) {
        return;
    }

    let command_name = first_command_token(command).unwrap_or_else(|| "command".to_string());
    response["critical_note"] = json!(format!("Command `{command_name}` was not found in PATH."));
    response["next_action"] =
        json!("Check the command name or install the missing binary, then rerun the command.");
}

fn build_exec_response(
    session: &VTCodeExecSession,
    command: &str,
    capture: &PtyEphemeralCapture,
    output_preview: ExecOutputPreview,
    matched_count: Option<usize>,
    query_truncated: bool,
    running_process_id: Option<&str>,
) -> Value {
    let ExecOutputPreview {
        raw_output,
        output,
        truncated,
    } = output_preview;
    let cargo_test_diagnostics =
        cargo_test_failure_diagnostics(command, &raw_output, capture.exit_code);
    let mut response = json!({
        "success": true,
        "output": output,
        "raw_output": raw_output,
        "wall_time": capture.duration.as_secs_f64(),
    });
    if let Some(count) = matched_count {
        response["matched_count"] = json!(count);
        response["query_truncated"] = json!(query_truncated);
    }

    attach_exec_response_context(&mut response, session, command, capture.exit_code.is_some());

    if let Some(code) = capture.exit_code {
        response["exit_code"] = json!(code);
    } else if let Some(process_id) = running_process_id {
        response["process_id"] = json!(process_id);
    }

    if truncated {
        response["truncated"] = json!(true);
    }
    if capture.exit_code.is_none() {
        attach_pty_continuation(&mut response, &session.id);
    }

    attach_exec_recovery_guidance(&mut response, command, capture.exit_code);
    if let Some(diagnostics) = cargo_test_diagnostics {
        attach_failure_diagnostics_metadata(&mut response, &diagnostics);
    }
    response
}

fn clamp_inspect_lines(value: Option<u64>, default: usize) -> usize {
    value.map(|v| v as usize).unwrap_or(default).min(5_000)
}

fn clamp_max_matches(value: Option<u64>) -> usize {
    value
        .map(|v| v as usize)
        .unwrap_or(DEFAULT_INSPECT_MAX_MATCHES)
        .clamp(1, 10_000)
}

fn build_head_tail_preview(content: &str, head_lines: usize, tail_lines: usize) -> (String, bool) {
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return (String::new(), false);
    }

    let head = head_lines.max(1);
    let tail = tail_lines.max(1);
    if lines.len() <= head + tail {
        return (lines.join("\n"), false);
    }

    let omitted = lines.len().saturating_sub(head + tail);
    let mut preview = Vec::with_capacity(head + tail + 1);
    preview.extend(lines.iter().take(head).copied().map(String::from));
    preview.push(format!("[... omitted {} lines ...]", omitted));
    preview.extend(
        lines
            .iter()
            .rev()
            .take(tail)
            .rev()
            .copied()
            .map(String::from),
    );
    (preview.join("\n"), true)
}

fn filter_lines(
    content: &str,
    query: &str,
    literal: bool,
    max_matches: usize,
) -> Result<(String, usize, bool)> {
    let matcher = if literal {
        None
    } else {
        Some(Regex::new(query).with_context(|| format!("Invalid regex query: {}", query))?)
    };

    let mut matches = Vec::new();
    let mut total_matches = 0usize;

    for (idx, line) in content.lines().enumerate() {
        let is_match = if literal {
            line.contains(query)
        } else {
            matcher
                .as_ref()
                .map(|regex| regex.is_match(line))
                .unwrap_or(false)
        };
        if !is_match {
            continue;
        }

        total_matches = total_matches.saturating_add(1);
        if matches.len() < max_matches {
            matches.push(format!("{}: {}", idx + 1, line));
        }
    }

    let truncated = total_matches > max_matches;
    Ok((matches.join("\n"), total_matches, truncated))
}

fn resolve_workspace_scoped_path(workspace_root: &Path, raw_path: &str) -> Result<PathBuf> {
    let path = Path::new(raw_path.trim());
    if path.as_os_str().is_empty() {
        return Err(anyhow!("spool_path cannot be empty"));
    }

    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace_root.join(path)
    };
    let normalized = crate::utils::path::normalize_path(&absolute);
    let normalized_workspace = crate::utils::path::normalize_path(workspace_root);
    if !normalized.starts_with(&normalized_workspace) {
        return Err(anyhow!(
            "spool_path must stay within workspace: {}",
            raw_path
        ));
    }

    Ok(normalized)
}

enum PlannedPatchWrite {
    Text { path: PathBuf, content: String },
    Removal { path: PathBuf },
}

impl ToolRegistry {
    pub async fn shell_run_approval_reason(
        &self,
        tool_name: &str,
        tool_args: Option<&Value>,
    ) -> Result<Option<String>> {
        let resolved_tool_name = self
            .resolve_public_tool_name_sync(tool_name)
            .unwrap_or_else(|_| tool_name.to_string());
        let Some(payload) = shell_run_payload(&resolved_tool_name, tool_args) else {
            return Ok(None);
        };

        let (requested_command, _) = parse_command_parts(
            payload,
            "shell run request requires a command",
            "shell run request command cannot be empty",
        )?;
        let working_dir_path = self
            .pty_manager()
            .resolve_working_dir(shell_working_dir_value(payload))
            .await?;
        let (sandbox_permissions, additional_permissions) =
            parse_requested_sandbox_permissions(payload, &working_dir_path)?;
        let sandbox_config = self.sandbox_config();
        let plan = build_shell_execution_plan(
            &sandbox_config,
            self.workspace_root(),
            &requested_command,
            sandbox_permissions,
            additional_permissions.as_ref(),
        )?;

        Ok(plan.approval_reason)
    }

    pub(super) fn unified_exec_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_unified_exec(args).await })
    }

    pub(super) fn unified_file_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_unified_file(args).await })
    }

    pub(super) fn unified_search_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_unified_search(args).await })
    }

    pub(super) async fn execute_unified_exec(&self, args: Value) -> Result<Value> {
        self.execute_unified_exec_internal(args, false).await
    }

    pub(super) async fn execute_unified_exec_internal(
        &self,
        args: Value,
        settle_noninteractive_exec: bool,
    ) -> Result<Value> {
        let args = crate::tools::command_args::normalize_shell_args(&args)
            .map_err(|error| anyhow!(error))?;

        let action_str = tool_intent::unified_exec_action(&args)
            .ok_or_else(|| missing_unified_exec_action_error(&args))?;
        let action: UnifiedExecAction = serde_json::from_value(json!(action_str))
            .with_context(|| format!("Invalid action: {}", action_str))?;

        match action {
            UnifiedExecAction::Run => {
                self.execute_command_session_run_internal(args, settle_noninteractive_exec)
                    .await
            }
            UnifiedExecAction::Write => self.execute_command_session_write(args).await,
            UnifiedExecAction::Poll => {
                self.execute_command_session_poll_internal(args, settle_noninteractive_exec)
                    .await
            }
            UnifiedExecAction::Continue => {
                self.execute_command_session_continue_internal(args, settle_noninteractive_exec)
                    .await
            }
            UnifiedExecAction::Inspect => self.execute_command_session_inspect(args).await,
            UnifiedExecAction::List => self.execute_command_session_list().await,
            UnifiedExecAction::Close => self.execute_command_session_close(args).await,
            UnifiedExecAction::Code => self.execute_code(args).await,
        }
    }

    async fn execute_command_session_run_internal(
        &self,
        args: Value,
        settle_noninteractive_exec: bool,
    ) -> Result<Value> {
        let tty = args.get("tty").and_then(Value::as_bool).unwrap_or(false);
        if tty {
            self.execute_command_session_run_pty(args).await
        } else {
            self.execute_run_pipe_cmd(args, settle_noninteractive_exec)
                .await
        }
    }

    pub(super) async fn execute_unified_file(&self, args: Value) -> Result<Value> {
        let action_str = tool_intent::unified_file_action(&args)
            .ok_or_else(|| missing_unified_file_action_error(&args))?;

        let action: UnifiedFileAction = serde_json::from_value(json!(action_str))
            .with_context(|| format!("Invalid action: {}", action_str))?;
        self.log_unified_file_payload_diagnostics(action_str, &args);

        match action {
            UnifiedFileAction::Read => {
                let tool = self.inventory.file_ops_tool().clone();
                match tool.read_file(args.clone()).await {
                    Ok(response) => Ok(response),
                    Err(read_err) => {
                        let read_err_text = read_err.to_string();
                        if let Some(fallback_args) =
                            build_read_pty_fallback_args(&args, &read_err_text)
                        {
                            let session_id = fallback_args
                                .get("session_id")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                                .to_string();
                            tracing::info!(
                                session_id = %session_id,
                                "Auto-recovering unified_file read via unified_exec poll"
                            );
                            match self.execute_command_session_poll(fallback_args).await {
                                Ok(mut recovered) => {
                                    if let Some(obj) = recovered.as_object_mut() {
                                        obj.insert("auto_recovered".to_string(), json!(true));
                                        obj.insert(
                                            "recovery_tool".to_string(),
                                            json!("unified_exec"),
                                        );
                                        obj.insert("recovery_action".to_string(), json!("poll"));
                                        obj.insert(
                                            "recovery_reason".to_string(),
                                            json!("missing_pty_spool_file"),
                                        );
                                    }
                                    return Ok(recovered);
                                }
                                Err(recovery_err) => {
                                    tracing::warn!(
                                        session_id = %session_id,
                                        error = %recovery_err,
                                        "Failed auto-recovery via unified_exec poll"
                                    );
                                }
                            }
                        }
                        Err(read_err)
                    }
                }
            }
            UnifiedFileAction::Write => {
                let tool = self.inventory.file_ops_tool().clone();
                tool.write_file(args).await
            }
            UnifiedFileAction::Edit => self.edit_file(args).await,
            UnifiedFileAction::Patch => self.execute_apply_patch(args).await,
            UnifiedFileAction::Delete => {
                let tool = self.inventory.file_ops_tool().clone();
                tool.delete_file(args).await
            }
            UnifiedFileAction::Move => {
                let tool = self.inventory.file_ops_tool().clone();
                tool.move_file(args).await
            }
            UnifiedFileAction::Copy => {
                let tool = self.inventory.file_ops_tool().clone();
                tool.copy_file(args).await
            }
        }
    }

    pub(super) async fn execute_unified_search(&self, args: Value) -> Result<Value> {
        let mut args = tool_intent::normalize_unified_search_args(&args);

        let action_str = tool_intent::unified_search_action(&args)
            .ok_or_else(|| missing_unified_search_action_error(&args))?;

        let action: UnifiedSearchAction = serde_json::from_value(json!(action_str))
            .with_context(|| format!("Invalid action: {}", action_str))?;

        // Default to workspace root when path is omitted for list/grep actions to reduce friction
        if matches!(
            action,
            UnifiedSearchAction::Grep | UnifiedSearchAction::List
        ) {
            let has_path = args
                .get("path")
                .and_then(|v| v.as_str())
                .map(|p| !p.trim().is_empty())
                .unwrap_or(false);
            if !has_path {
                args["path"] = json!(".");
            }
        }

        match action {
            UnifiedSearchAction::Grep => {
                let manager = self.inventory.grep_file_manager();
                manager
                    .perform_search(serde_json::from_value(args)?)
                    .await
                    .map(|r| json!(r))
            }
            UnifiedSearchAction::List => {
                let tool = self.inventory.file_ops_tool().clone();
                tool.execute(args).await
            }
            UnifiedSearchAction::Structural => {
                crate::tools::structural_search::execute_structural_search(
                    self.workspace_root(),
                    args,
                )
                .await
            }
            UnifiedSearchAction::Intelligence => Ok(
                serde_json::json!({"error": "Action 'intelligence' is deprecated. Use action='grep' or action='list'."}),
            ),
            UnifiedSearchAction::Tools => self.execute_search_tools(args).await,
            UnifiedSearchAction::Errors => self.execute_get_errors(args).await,
            UnifiedSearchAction::Agent => self.execute_agent_info().await,
            UnifiedSearchAction::Web => self.execute_web_fetch(args).await,
            UnifiedSearchAction::Skill => self.execute_skill(args).await,
        }
    }

    pub(super) async fn execute_code(&self, args: Value) -> Result<Value> {
        let code = args
            .get("command")
            .or_else(|| args.get("code"))
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing code/command in execute_code"))?;

        let language_str = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("python3");

        let language = match language_str {
            "python3" | "python" => Language::Python3,
            "javascript" | "js" => Language::JavaScript,
            _ => Language::Python3,
        };

        let track_files = args
            .get("track_files")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mcp_client = self
            .mcp_client()
            .ok_or_else(|| anyhow!("MCP client not available"))?;

        let workspace_root = self.workspace_root_owned();
        let executor = crate::exec::code_executor::CodeExecutor::new(
            language,
            mcp_client.clone(),
            workspace_root.clone(),
        );
        let execution_start = SystemTime::now();

        let result = executor.execute(code).await?;

        let mut response = json!(result);

        if track_files {
            let tracker = FileTracker::new(workspace_root);
            if let Ok(changes) = tracker.detect_new_files(execution_start).await {
                response["generated_files"] = json!({
                    "count": changes.len(),
                    "files": changes,
                    "summary": tracker.generate_file_summary(&changes),
                });
            }
        }

        Ok(response)
    }

    pub(super) async fn execute_web_fetch(&self, args: Value) -> Result<Value> {
        let url = args
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow!("Missing url in web_fetch"))?;

        let raw = args.get("raw").and_then(|v| v.as_bool()).unwrap_or(false);

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent("VT Code/1.0")
            .build()?;

        let response = client.get(url).send().await?;
        let status = response.status();

        if !status.is_success() {
            return Err(anyhow!("Web fetch failed with status: {}", status));
        }

        if raw {
            let body = response.text().await?;
            Ok(json!({ "success": true, "content": body, "url": url }))
        } else {
            let body = response.text().await?;
            // Fallback to raw content if html2md is not available
            Ok(json!({ "success": true, "content": body, "url": url }))
        }
    }

    pub(super) async fn execute_skill(&self, args: Value) -> Result<Value> {
        let sub_action = args
            .get("sub_action")
            .and_then(|v| v.as_str())
            .or_else(|| {
                if args.get("name").is_some() {
                    Some("load")
                } else {
                    None
                }
            })
            .ok_or_else(|| anyhow!("Missing sub_action in skill"))?;

        let skill_manager = self.inventory.skill_manager();

        match sub_action {
            "save" => {
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing name in skill save"))?;
                let code = args
                    .get("code")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing code in skill save"))?;
                let description = args
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let language = args
                    .get("language")
                    .and_then(|v| v.as_str())
                    .unwrap_or("python3");

                let metadata = SkillMetadata {
                    name: name.to_string(),
                    description: description.to_string(),
                    language: language.to_string(),
                    inputs: vec![],
                    output: "".to_string(),
                    examples: vec![],
                    tags: vec![],
                    created_at: chrono::Utc::now().to_rfc3339(),
                    modified_at: chrono::Utc::now().to_rfc3339(),
                    tool_dependencies: vec![],
                };

                let skill = Skill {
                    metadata,
                    code: code.to_string(),
                };

                skill_manager.save_skill(skill).await?;
                Ok(json!({ "success": true, "name": name }))
            }
            "load" => {
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing name in skill load"))?;
                let skill = skill_manager.load_skill(name).await?;
                Ok(json!({
                    "success": true,
                    "name": skill.metadata.name,
                    "code": skill.code,
                    "language": skill.metadata.language
                }))
            }
            "list" => {
                let skills = skill_manager.list_skills().await?;
                Ok(json!({ "success": true, "skills": skills }))
            }
            _ => Err(anyhow!("Unknown skill sub_action: {}", sub_action)),
        }
    }

    pub(super) async fn execute_apply_patch(&self, args: Value) -> Result<Value> {
        let (patch_args, patch_input_bytes, patch_base64) = self.prepare_apply_patch_args(args)?;
        let context = self.harness_context_snapshot();
        tracing::debug!(
            tool = "unified_file",
            action = "patch",
            payload_bytes = serialized_payload_size_bytes(&patch_args),
            patch_input_bytes,
            patch_base64,
            patch_decoded_bytes = patch_args
                .get("input")
                .and_then(|v| v.as_str())
                .map(|s| s.len())
                .unwrap_or(0),
            session_id = %context.session_id,
            task_id = %context.task_id.as_deref().unwrap_or(""),
            "Prepared patch payload for apply_patch"
        );

        self.execute_apply_patch_internal(patch_args).await
    }

    fn prepare_apply_patch_args(&self, args: Value) -> Result<(Value, usize, bool)> {
        let patch_input = crate::tools::apply_patch::decode_apply_patch_input(&args)?
            .ok_or_else(|| anyhow!("Missing patch input"))?;
        let patch_input_bytes = patch_input.source_bytes;
        let patch_base64 = patch_input.was_base64;

        let mut patch_args = args;
        patch_args["input"] = json!(patch_input.text);
        Ok((patch_args, patch_input_bytes, patch_base64))
    }

    fn log_unified_file_payload_diagnostics(&self, action: &str, args: &Value) {
        let context = self.harness_context_snapshot();
        let (patch_source_bytes, patch_base64) =
            crate::tools::apply_patch::patch_source_from_args(args)
                .map(|source| (source.len(), source.starts_with("base64:")))
                .unwrap_or((0, false));

        tracing::debug!(
            tool = "unified_file",
            action,
            payload_bytes = serialized_payload_size_bytes(args),
            patch_source_bytes,
            patch_base64,
            session_id = %context.session_id,
            task_id = %context.task_id.as_deref().unwrap_or(""),
            "Captured unified_file payload diagnostics"
        );
    }

    // ============================================================
    // SPECIALIZED EXECUTORS (Hidden from LLM, used by unified tools)
    // ============================================================

    pub(super) fn read_file_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.file_ops_tool().clone();
        Box::pin(async move { tool.read_file(args).await })
    }

    pub(super) fn write_file_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        let tool = self.inventory.file_ops_tool().clone();
        Box::pin(async move { tool.write_file(args).await })
    }

    pub(super) fn edit_file_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.edit_file(args).await })
    }

    pub(super) fn run_pty_cmd_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let mut args = crate::tools::command_args::normalize_shell_args(&args)
                .map_err(|error| anyhow!(error))?;
            if let Some(payload) = args.as_object_mut() {
                payload
                    .entry("action".to_string())
                    .or_insert_with(|| json!("run"));
                payload
                    .entry("tty".to_string())
                    .or_insert_with(|| json!(true));
            }
            self.execute_unified_exec(args)
                .await
                .map(super::normalize_tool_output)
        })
    }

    pub(super) fn send_pty_input_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let mut args = args;
            if let Some(payload) = args.as_object_mut() {
                payload
                    .entry("action".to_string())
                    .or_insert_with(|| json!("write"));
            }
            self.execute_unified_exec(args)
                .await
                .map(super::normalize_tool_output)
        })
    }

    pub(super) fn read_pty_session_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let mut args = args;
            if let Some(payload) = args.as_object_mut() {
                payload
                    .entry("action".to_string())
                    .or_insert_with(|| json!("poll"));
            }
            self.execute_unified_exec(args)
                .await
                .map(super::normalize_tool_output)
        })
    }

    pub(super) fn create_pty_session_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let mut args = crate::tools::command_args::normalize_shell_args(&args)
                .map_err(|error| anyhow!(error))?;
            if let Some(payload) = args.as_object_mut() {
                payload
                    .entry("action".to_string())
                    .or_insert_with(|| json!("run"));
                payload
                    .entry("tty".to_string())
                    .or_insert_with(|| json!(true));
            }
            self.execute_unified_exec(args)
                .await
                .map(super::normalize_tool_output)
        })
    }

    pub(super) fn list_pty_sessions_executor(&self, _args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            self.execute_unified_exec(json!({"action": "list"}))
                .await
                .map(super::normalize_tool_output)
        })
    }

    pub(super) fn close_pty_session_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let mut args = args;
            if let Some(payload) = args.as_object_mut() {
                payload
                    .entry("action".to_string())
                    .or_insert_with(|| json!("close"));
            }
            self.execute_unified_exec(args)
                .await
                .map(super::normalize_tool_output)
        })
    }

    pub(super) fn get_errors_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_get_errors(args).await })
    }

    pub(super) fn apply_patch_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move { self.execute_apply_patch(args).await })
    }

    // ============================================================
    // INTERNAL IMPLEMENTATIONS
    // ============================================================

    async fn execute_command_session_run_pty(&self, args: Value) -> Result<Value> {
        let payload = args
            .as_object()
            .ok_or_else(|| anyhow!("command execution requires a JSON object"))?;

        let (mut command, auto_raw_command) = parse_command_parts(
            payload,
            "command execution requires a 'command' value",
            "PTY command cannot be empty",
        )?;
        let requested_command = command.clone();
        let is_git_diff = is_git_diff_command(&command);

        let shell_program = resolve_shell_preference_with_zsh_fork(
            payload.get("shell").and_then(|value| value.as_str()),
            self.pty_config(),
        )?;
        let login_shell = payload
            .get("login")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let confirm = payload
            .get("confirm")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        let normalized_shell = normalized_shell_name(&shell_program);
        let existing_shell = command
            .first()
            .map(|existing| normalized_shell_name(existing));

        if existing_shell != Some(normalized_shell.clone()) {
            // Prefer explicit raw_command, fallback to auto-detected from string command
            let raw_command = payload
                .get("raw_command")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())
                .or(auto_raw_command);

            let command_string =
                build_shell_command_string(raw_command.as_deref(), &command, &shell_program);

            let mut shell_invocation = Vec::with_capacity(4);
            shell_invocation.push(shell_program.clone());

            if login_shell && !should_use_windows_command_tokenizer(Some(&shell_program)) {
                shell_invocation.push("-l".to_string());
            }

            let command_flag = if should_use_windows_command_tokenizer(Some(&shell_program)) {
                match normalized_shell.as_str() {
                    "cmd" | "cmd.exe" => "/C".to_string(),
                    "powershell" | "powershell.exe" | "pwsh" => "-Command".to_string(),
                    _ => "-c".to_string(),
                }
            } else {
                "-c".to_string()
            };

            shell_invocation.push(command_flag);
            shell_invocation.push(command_string);
            command = shell_invocation;
        }

        let rows =
            parse_pty_dimension("rows", payload.get("rows"), self.pty_config().default_rows)?;
        let cols =
            parse_pty_dimension("cols", payload.get("cols"), self.pty_config().default_cols)?;

        let working_dir_path = self
            .pty_manager()
            .resolve_working_dir(shell_working_dir_value(payload))
            .await?;
        let (sandbox_permissions, additional_permissions) =
            parse_requested_sandbox_permissions(payload, &working_dir_path)?;

        let display_command = if should_use_windows_command_tokenizer(Some(&shell_program)) {
            join_windows_command(&command)
        } else {
            shell_words::join(command.iter().map(|part| part.as_str()))
        };
        let requested_command_display =
            if should_use_windows_command_tokenizer(Some(&shell_program)) {
                join_windows_command(&requested_command)
            } else {
                shell_words::join(requested_command.iter().map(|part| part.as_str()))
            };

        // Use explicit max_tokens if provided, otherwise check if command suggests a limit
        let max_tokens = max_output_tokens_from_payload(payload)
            .or_else(|| suggest_max_tokens_for_command(&display_command))
            .unwrap_or(crate::config::constants::defaults::DEFAULT_PTY_OUTPUT_MAX_TOKENS);
        let inspect_query = payload
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let inspect_literal = payload
            .get("literal")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let inspect_max_matches =
            clamp_max_matches(payload.get("max_matches").and_then(Value::as_u64));

        enforce_pty_command_policy(&display_command, confirm)?;
        let sandbox_config = self.sandbox_config();
        command = apply_runtime_sandbox_to_command(
            command,
            &requested_command,
            &sandbox_config,
            self.workspace_root(),
            &working_dir_path,
            sandbox_permissions,
            additional_permissions.as_ref(),
        )?;

        let yield_duration = Duration::from_millis(clamp_exec_yield_ms(
            payload.get("yield_time_ms").and_then(Value::as_u64),
            10_000,
        ));

        let mut session_env = HashMap::new();
        let mut zsh_exec_bridge = None;
        if self.pty_config().shell_zsh_fork {
            let wrapper_executable = std::env::current_exe()
                .context("resolve current executable for zsh exec bridge")?;
            let bridge = ZshExecBridgeSession::spawn(confirm)
                .context("initialize zsh exec bridge session")?;
            session_env = bridge.env_vars(&wrapper_executable);
            zsh_exec_bridge = Some(bridge);
        }

        let session_id = generate_session_id("run");
        let session_metadata = self
            .exec_sessions
            .create_pty_session(
                session_id.clone(),
                command,
                working_dir_path,
                portable_pty::PtySize {
                    rows,
                    cols,
                    pixel_width: 0,
                    pixel_height: 0,
                },
                session_env,
                zsh_exec_bridge,
            )
            .await
            .context("Maximum PTY sessions reached; cannot start new session")?;
        self.increment_active_pty_sessions();

        let capture = self
            .wait_for_exec_yield(
                &session_id,
                yield_duration,
                Some(crate::config::constants::tools::UNIFIED_EXEC),
                true,
            )
            .await;
        let raw_output = filter_pty_output(&strip_ansi(&capture.output));
        let mut matched_count = None;
        let mut query_truncated = false;
        let filtered_output = if let Some(query) = inspect_query {
            let (filtered, count, truncated_matches) =
                filter_lines(&raw_output, query, inspect_literal, inspect_max_matches)?;
            matched_count = Some(count);
            query_truncated = truncated_matches;
            filtered
        } else {
            raw_output.clone()
        };
        let preview = build_exec_output_preview(filtered_output, max_tokens);
        let mut response = build_exec_response(
            &session_metadata,
            &requested_command_display,
            &capture,
            ExecOutputPreview {
                raw_output,
                output: preview.output,
                truncated: preview.truncated,
            },
            matched_count,
            query_truncated,
            Some(&session_id),
        );

        if capture.exit_code.is_some() {
            self.prune_completed_exec_session(&session_id).await?;
        }
        if is_git_diff {
            response["no_spool"] = json!(true);
            response["content_type"] = json!("git_diff");
        }

        Ok(response)
    }

    async fn execute_run_pipe_cmd(
        &self,
        args: Value,
        settle_noninteractive_exec: bool,
    ) -> Result<Value> {
        let payload = args
            .as_object()
            .ok_or_else(|| anyhow!("unified_exec run requires a JSON object"))?;

        let (mut command, auto_raw_command) = parse_command_parts(
            payload,
            "unified_exec run requires a 'command' value",
            "Command cannot be empty",
        )?;
        let requested_command = command.clone();
        let is_git_diff = is_git_diff_command(&command);

        let shell_program = resolve_shell_preference(
            payload.get("shell").and_then(|value| value.as_str()),
            self.pty_config(),
        );
        let login_shell = payload
            .get("login")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);
        let confirm = payload
            .get("confirm")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        let normalized_shell = normalized_shell_name(&shell_program);
        let existing_shell = command
            .first()
            .map(|existing| normalized_shell_name(existing));

        if existing_shell != Some(normalized_shell.clone()) {
            let raw_command = payload
                .get("raw_command")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())
                .or(auto_raw_command);

            let command_string =
                build_shell_command_string(raw_command.as_deref(), &command, &shell_program);

            let mut shell_invocation = Vec::with_capacity(4);
            shell_invocation.push(shell_program.clone());

            if login_shell && !should_use_windows_command_tokenizer(Some(&shell_program)) {
                shell_invocation.push("-l".to_string());
            }

            let command_flag = if should_use_windows_command_tokenizer(Some(&shell_program)) {
                match normalized_shell.as_str() {
                    "cmd" | "cmd.exe" => "/C".to_string(),
                    "powershell" | "powershell.exe" | "pwsh" => "-Command".to_string(),
                    _ => "-c".to_string(),
                }
            } else {
                "-c".to_string()
            };

            shell_invocation.push(command_flag);
            shell_invocation.push(command_string);
            command = shell_invocation;
        }

        let working_dir_path = self
            .pty_manager()
            .resolve_working_dir(shell_working_dir_value(payload))
            .await?;
        let (sandbox_permissions, additional_permissions) =
            parse_requested_sandbox_permissions(payload, &working_dir_path)?;

        let display_command = if should_use_windows_command_tokenizer(Some(&shell_program)) {
            join_windows_command(&command)
        } else {
            shell_words::join(command.iter().map(|part| part.as_str()))
        };
        let requested_command_display =
            if should_use_windows_command_tokenizer(Some(&shell_program)) {
                join_windows_command(&requested_command)
            } else {
                shell_words::join(requested_command.iter().map(|part| part.as_str()))
            };

        let max_tokens = max_output_tokens_from_payload(payload)
            .or_else(|| suggest_max_tokens_for_command(&display_command))
            .unwrap_or(crate::config::constants::defaults::DEFAULT_PTY_OUTPUT_MAX_TOKENS);
        let inspect_query = payload
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let inspect_literal = payload
            .get("literal")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let inspect_max_matches =
            clamp_max_matches(payload.get("max_matches").and_then(Value::as_u64));

        enforce_pty_command_policy(&display_command, confirm)?;
        let sandbox_config = self.sandbox_config();
        command = apply_runtime_sandbox_to_command(
            command,
            &requested_command,
            &sandbox_config,
            self.workspace_root(),
            &working_dir_path,
            sandbox_permissions,
            additional_permissions.as_ref(),
        )?;

        let yield_duration = Duration::from_millis(clamp_exec_yield_ms(
            payload.get("yield_time_ms").and_then(Value::as_u64),
            10_000,
        ));

        let session_id = generate_session_id("run");
        let session_env = self.build_pipe_session_env(&shell_program, HashMap::new());
        let session_metadata = self
            .exec_sessions
            .create_pipe_session(session_id.clone(), command, working_dir_path, session_env)
            .await?;

        let capture = self
            .capture_exec_session_output(
                &session_id,
                yield_duration,
                Some(crate::config::constants::tools::UNIFIED_EXEC),
                settle_noninteractive_exec,
            )
            .await?;
        let raw_output = filter_pty_output(&strip_ansi(&capture.output));
        let mut matched_count = None;
        let mut query_truncated = false;
        let filtered_output = if let Some(query) = inspect_query {
            let (filtered, count, truncated_matches) =
                filter_lines(&raw_output, query, inspect_literal, inspect_max_matches)?;
            matched_count = Some(count);
            query_truncated = truncated_matches;
            filtered
        } else {
            raw_output.clone()
        };
        let preview = build_exec_output_preview(filtered_output, max_tokens);
        let mut response = build_exec_response(
            &session_metadata,
            &requested_command_display,
            &capture,
            ExecOutputPreview {
                raw_output,
                output: preview.output,
                truncated: preview.truncated,
            },
            matched_count,
            query_truncated,
            Some(&session_id),
        );

        if capture.exit_code.is_some() {
            self.prune_completed_exec_session(&session_id).await?;
        }
        if is_git_diff {
            response["no_spool"] = json!(true);
            response["content_type"] = json!("git_diff");
        }

        Ok(response)
    }

    async fn execute_command_session_write(&self, args: Value) -> Result<Value> {
        let payload = args
            .as_object()
            .ok_or_else(|| anyhow!("command session write requires a JSON object"))?;

        let raw_sid = crate::tools::command_args::session_id_text(&args)
            .ok_or_else(|| anyhow!("session_id is required for command session write"))?;
        let sid = validate_exec_session_id(raw_sid, "command session write")?;

        let input = crate::tools::command_args::interactive_input_text(&args)
            .ok_or_else(|| anyhow!("input is required for command session write"))?;

        let yield_time_ms =
            clamp_exec_yield_ms(payload.get("yield_time_ms").and_then(Value::as_u64), 250);

        let max_tokens = max_output_tokens_from_payload(payload)
            .unwrap_or(crate::config::constants::defaults::DEFAULT_PTY_OUTPUT_MAX_TOKENS);
        let session_metadata = self.exec_session_metadata(sid).await?;
        let session_command = build_exec_session_command_display(&session_metadata);

        self.send_input_to_exec_session(sid, input.as_bytes(), false)
            .await?;

        let capture = self
            .wait_for_exec_yield(
                sid,
                Duration::from_millis(yield_time_ms),
                Some(crate::config::constants::tools::UNIFIED_EXEC),
                true,
            )
            .await;
        let raw_output = filter_pty_output(&strip_ansi(&capture.output));
        let preview = build_exec_output_preview(raw_output.clone(), max_tokens);
        let response = build_exec_response(
            &session_metadata,
            &session_command,
            &capture,
            ExecOutputPreview {
                raw_output,
                output: preview.output,
                truncated: preview.truncated,
            },
            None,
            false,
            None,
        );

        if capture.exit_code.is_some() {
            self.prune_completed_exec_session(sid).await?;
        }

        Ok(response)
    }

    async fn execute_command_session_poll(&self, args: Value) -> Result<Value> {
        self.execute_command_session_poll_internal(args, false)
            .await
    }

    async fn execute_command_session_poll_internal(
        &self,
        args: Value,
        settle_noninteractive_exec: bool,
    ) -> Result<Value> {
        let payload = args
            .as_object()
            .ok_or_else(|| anyhow!("command session read requires a JSON object"))?;

        let raw_sid = crate::tools::command_args::session_id_text(&args)
            .ok_or_else(|| anyhow!("session_id is required for command session read"))?;
        let sid = validate_exec_session_id(raw_sid, "command session read")?;
        let session_metadata = self.exec_session_metadata(sid).await?;
        let session_command = build_exec_session_command_display(&session_metadata);

        let yield_time_ms =
            clamp_exec_yield_ms(payload.get("yield_time_ms").and_then(Value::as_u64), 1000);

        let capture = self
            .capture_exec_session_output(
                sid,
                Duration::from_millis(yield_time_ms),
                Some(crate::config::constants::tools::UNIFIED_EXEC),
                settle_noninteractive_exec && session_metadata.backend == "pipe",
            )
            .await?;

        let raw_output = filter_pty_output(&strip_ansi(&capture.output));
        let response = build_exec_response(
            &session_metadata,
            &session_command,
            &capture,
            ExecOutputPreview {
                raw_output: raw_output.clone(),
                output: raw_output,
                truncated: false,
            },
            None,
            false,
            None,
        );

        if capture.exit_code.is_some() {
            self.prune_completed_exec_session(sid).await?;
        }

        Ok(response)
    }

    async fn execute_command_session_continue_internal(
        &self,
        args: Value,
        settle_noninteractive_exec: bool,
    ) -> Result<Value> {
        if args
            .get("input")
            .or_else(|| args.get("chars"))
            .or_else(|| args.get("text"))
            .is_some()
        {
            self.execute_command_session_write(args).await
        } else {
            self.execute_command_session_poll_internal(args, settle_noninteractive_exec)
                .await
        }
    }

    async fn execute_command_session_inspect(&self, args: Value) -> Result<Value> {
        let payload = args
            .as_object()
            .ok_or_else(|| anyhow!("inspect requires a JSON object"))?;

        let query = payload
            .get("query")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let literal = payload
            .get("literal")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let max_matches = clamp_max_matches(payload.get("max_matches").and_then(Value::as_u64));
        let head_lines = clamp_inspect_lines(
            payload.get("head_lines").and_then(Value::as_u64),
            DEFAULT_INSPECT_HEAD_LINES,
        );
        let tail_lines = clamp_inspect_lines(
            payload.get("tail_lines").and_then(Value::as_u64),
            DEFAULT_INSPECT_TAIL_LINES,
        );

        let source_session_id = payload
            .get("session_id")
            .and_then(Value::as_str)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let source_spool_path = payload
            .get("spool_path")
            .and_then(Value::as_str)
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());

        let content = if let Some(spool_path) = source_spool_path.as_deref() {
            let resolved =
                resolve_workspace_scoped_path(self.inventory.workspace_root(), spool_path)?;
            fs::read_to_string(&resolved).await.with_context(|| {
                format!("Failed to read inspect spool path: {}", resolved.display())
            })?
        } else if let Some(session_id) = source_session_id.as_deref() {
            let session_id = validate_exec_session_id(session_id, "inspect")?;

            let yield_time_ms =
                clamp_peek_yield_ms(payload.get("yield_time_ms").and_then(Value::as_u64));
            let capture = self
                .wait_for_exec_yield(
                    session_id,
                    Duration::from_millis(yield_time_ms),
                    None,
                    false,
                )
                .await;
            filter_pty_output(&strip_ansi(&capture.output))
        } else {
            return Err(anyhow!(
                "inspect requires either `session_id` or `spool_path`"
            ));
        };

        let (output, matched_count, truncated) = if let Some(query) = query {
            let (filtered, count, is_truncated) =
                filter_lines(&content, query, literal, max_matches)?;
            (filtered, count, is_truncated)
        } else {
            let (preview, is_truncated) = build_head_tail_preview(&content, head_lines, tail_lines);
            (preview, 0, is_truncated)
        };

        let mut response = json!({
            "success": true,
            "output": output,
            "matched_count": matched_count,
            "truncated": truncated,
            "content_type": "exec_inspect"
        });
        if let Some(session_id) = source_session_id {
            response["session_id"] = json!(session_id);
        }
        if let Some(spool_path) = source_spool_path {
            response["spool_path"] = json!(spool_path);
        }

        Ok(response)
    }

    async fn execute_command_session_list(&self) -> Result<Value> {
        let sessions = self.list_exec_sessions().await;
        Ok(json!({ "success": true, "sessions": sessions }))
    }

    async fn execute_command_session_close(&self, args: Value) -> Result<Value> {
        let _payload = args
            .as_object()
            .ok_or_else(|| anyhow!("command session close requires a JSON object"))?;

        let sid = crate::tools::command_args::session_id_text(&args)
            .ok_or_else(|| anyhow!("session_id is required for command session close"))?;
        let sid = validate_exec_session_id(sid, "command session close")?;

        let session_metadata = self.close_exec_session(sid).await?;
        self.handle_closed_exec_session(&session_metadata);

        Ok(json!({
            "success": true,
            "session_id": sid,
            "backend": session_metadata.backend
        }))
    }

    async fn execute_get_errors(&self, args: Value) -> Result<Value> {
        // Simplified version of get_errors logic
        let scope = args
            .get("scope")
            .and_then(|v| v.as_str())
            .unwrap_or("archive");
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;

        let mut error_report = json!({
            "timestamp": chrono::Utc::now().to_rfc3339(),
            "scope": scope,
            "total_errors": 0,
            "recent_errors": Vec::<Value>::new(),
        });

        if scope == "archive" || scope == "all" {
            let sessions = crate::utils::session_archive::list_recent_sessions(limit).await?;
            let mut issues = Vec::new();
            let mut total_errors = 0usize;

            for listing in sessions {
                for message in listing.snapshot.messages {
                    if message.role == crate::llm::provider::MessageRole::Assistant {
                        let text = message.content.as_text();
                        let lower = text.to_lowercase();
                        let error_patterns = crate::tools::constants::ERROR_DETECTION_PATTERNS;

                        if error_patterns.iter().any(|&pat| lower.contains(pat)) {
                            total_errors += 1;
                            issues.push(json!({
                                "type": "session_error",
                                "message": text.trim(),
                                "timestamp": listing.snapshot.ended_at.to_rfc3339(),
                            }));
                        }
                    }
                }
            }

            error_report["recent_errors"] = json!(issues);
            error_report["total_errors"] = json!(total_errors);
        }

        Ok(error_report)
    }

    fn build_pipe_session_env(
        &self,
        shell_program: &str,
        extra_env: HashMap<String, String>,
    ) -> HashMap<String, String> {
        let mut env: HashMap<String, String> = std::env::vars().collect();
        env.insert(
            "WORKSPACE_DIR".to_string(),
            self.workspace_root().display().to_string(),
        );
        env.insert("PAGER".to_string(), "cat".to_string());
        env.insert("GIT_PAGER".to_string(), "cat".to_string());
        env.insert("NO_COLOR".to_string(), "1".to_string());
        env.insert("CARGO_TERM_COLOR".to_string(), "never".to_string());
        if !shell_program.trim().is_empty() {
            env.insert("SHELL".to_string(), shell_program.to_string());
        }
        env.extend(extra_env);
        env
    }

    async fn exec_session_metadata(&self, session_id: &str) -> Result<VTCodeExecSession> {
        self.exec_sessions.snapshot_session(session_id).await
    }

    async fn list_exec_sessions(&self) -> Vec<VTCodeExecSession> {
        self.exec_sessions.list_sessions().await
    }

    async fn read_exec_session_output(
        &self,
        session_id: &str,
        drain: bool,
    ) -> Result<Option<String>> {
        self.exec_sessions
            .read_session_output(session_id, drain)
            .await
    }

    async fn send_input_to_exec_session(
        &self,
        session_id: &str,
        data: &[u8],
        append_newline: bool,
    ) -> Result<usize> {
        self.exec_sessions
            .send_input_to_session(session_id, data, append_newline)
            .await
    }

    pub(super) async fn exec_session_completed(&self, session_id: &str) -> Result<Option<i32>> {
        self.exec_sessions.is_session_completed(session_id).await
    }

    async fn close_exec_session(&self, session_id: &str) -> Result<VTCodeExecSession> {
        self.exec_sessions.close_session(session_id).await
    }

    fn handle_closed_exec_session(&self, session_metadata: &VTCodeExecSession) {
        if is_pty_exec_session(session_metadata) {
            self.decrement_active_pty_sessions();
        }
    }

    async fn prune_completed_exec_session(&self, session_id: &str) -> Result<()> {
        if let Some(session_metadata) = self.exec_sessions.prune_exited_session(session_id).await? {
            self.handle_closed_exec_session(&session_metadata);
        }
        Ok(())
    }

    async fn execute_agent_info(&self) -> Result<Value> {
        let available_tools = self.available_tools().await;
        Ok(json!({
            "tools_registered": available_tools,
            "workspace_root": self.workspace_root_str(),
            "available_tools_count": available_tools.len(),
            "agent_type": self.agent_type,
        }))
    }

    async fn execute_search_tools(&self, args: Value) -> Result<Value> {
        let keyword = args
            .get("keyword")
            .or_else(|| args.get("query"))
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let detail_level_str = args
            .get("detail_level")
            .and_then(|v| v.as_str())
            .unwrap_or("name-and-description");
        let detail_level = match detail_level_str {
            "name-only" => DetailLevel::NameOnly,
            "full" => DetailLevel::Full,
            _ => DetailLevel::NameAndDescription,
        };

        // 1. Search local tools and aliases
        let mut results = Vec::new();
        let available_tools = self.available_tools().await;

        for tool_name in available_tools {
            // Skip MCP tools as they will be handled by ToolDiscovery
            if tool_name.starts_with("mcp_") {
                continue;
            }

            // Get description from inventory if available
            let description = if let Some(reg) = self.inventory.get_registration(&tool_name) {
                reg.metadata().description().unwrap_or("").to_string()
            } else {
                "".to_string()
            };

            if keyword.is_empty()
                || tool_name.to_lowercase().contains(&keyword.to_lowercase())
                || description.to_lowercase().contains(&keyword.to_lowercase())
            {
                results.push(json!({
                    "name": tool_name,
                    "provider": "builtin",
                    "description": description,
                }));
            }
        }

        // 2. Search MCP tools using ToolDiscovery
        if let Some(mcp_client) = self.mcp_client() {
            let discovery = ToolDiscovery::new(mcp_client);
            if let Ok(mcp_results) = discovery.search_tools(keyword, detail_level).await {
                for r in mcp_results {
                    results.push(r.to_json(detail_level));
                }
            }
        }

        // 3. Search skills
        let skill_manager = self.inventory.skill_manager();
        if let Ok(skills) = skill_manager.list_skills().await {
            for skill in skills {
                if keyword.is_empty()
                    || skill.name.to_lowercase().contains(&keyword.to_lowercase())
                    || skill
                        .description
                        .to_lowercase()
                        .contains(&keyword.to_lowercase())
                {
                    results.push(json!({
                        "name": skill.name,
                        "provider": "skill",
                        "description": skill.description,
                    }));
                }
            }
        }

        Ok(json!({ "tools": results }))
    }

    async fn execute_apply_patch_internal(&self, args: Value) -> Result<Value> {
        let patch_input = crate::tools::apply_patch::decode_apply_patch_input(&args)?
            .ok_or_else(|| anyhow!("Missing patch input (use 'input' or 'patch' parameter)"))?;
        let override_snapshot = conflict_override_snapshot(&args);

        let patch = crate::tools::editing::Patch::parse(&patch_input.text)?;
        let _mutation_leases = self.acquire_patch_mutations(&patch).await?;
        let planned_writes = self.planned_patch_writes(&patch).await?;
        for operation in patch.operations() {
            if let Some(conflict) = self
                .detect_patch_operation_conflict(operation, override_snapshot.clone())
                .await?
            {
                return Ok(conflict.to_tool_output(self.workspace_root()));
            }
        }
        let results = patch.apply(&self.workspace_root_owned()).await?;
        for write in planned_writes {
            let (path, result) = match write {
                PlannedPatchWrite::Text { path, content } => {
                    let result = self
                        .edited_file_monitor()
                        .record_agent_write_text(&path, &content);
                    (path, result)
                }
                PlannedPatchWrite::Removal { path } => {
                    let result = self.edited_file_monitor().record_agent_removal(&path);
                    (path, result)
                }
            };

            if let Err(err) = result {
                tracing::warn!(
                    path = %path.display(),
                    error = %err,
                    "Failed to refresh edited-file snapshot after apply_patch"
                );
            }
        }

        Ok(json!({
            "success": true,
            "applied": results,
        }))
    }

    async fn patch_mutation_paths(
        &self,
        patch: &crate::tools::editing::Patch,
    ) -> Result<Vec<PathBuf>> {
        let mut paths = Vec::new();
        for operation in patch.operations() {
            match operation {
                crate::tools::editing::PatchOperation::AddFile { path, .. }
                | crate::tools::editing::PatchOperation::DeleteFile { path } => {
                    paths.push(self.file_ops_tool().normalize_user_path(path).await?);
                }
                crate::tools::editing::PatchOperation::UpdateFile { path, new_path, .. } => {
                    paths.push(self.file_ops_tool().normalize_user_path(path).await?);
                    if let Some(destination) = new_path
                        .as_ref()
                        .filter(|candidate| candidate.as_str() != path.as_str())
                    {
                        paths.push(
                            self.file_ops_tool()
                                .normalize_user_path(destination)
                                .await?,
                        );
                    }
                }
            }
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    async fn planned_patch_writes(
        &self,
        patch: &crate::tools::editing::Patch,
    ) -> Result<Vec<PlannedPatchWrite>> {
        let mut writes = Vec::new();
        for operation in patch.operations() {
            writes.extend(self.planned_patch_writes_for_operation(operation).await?);
        }
        Ok(writes)
    }

    async fn acquire_patch_mutations(
        &self,
        patch: &crate::tools::editing::Patch,
    ) -> Result<Vec<MutationLease>> {
        let mut leases = Vec::new();
        for path in self.patch_mutation_paths(patch).await? {
            leases.push(self.edited_file_monitor().acquire_mutation(&path).await);
        }
        Ok(leases)
    }

    async fn detect_patch_operation_conflict(
        &self,
        operation: &crate::tools::editing::PatchOperation,
        override_snapshot: Option<crate::tools::edited_file_monitor::FileSnapshot>,
    ) -> Result<Option<crate::tools::edited_file_monitor::FileConflict>> {
        let monitor = self.edited_file_monitor();
        match operation {
            crate::tools::editing::PatchOperation::AddFile { path, content } => {
                let canonical_path = self.file_ops_tool().normalize_user_path(path).await?;
                monitor
                    .detect_conflict(&canonical_path, Some(content.clone()), override_snapshot)
                    .await
            }
            crate::tools::editing::PatchOperation::DeleteFile { path } => {
                let canonical_path = self.file_ops_tool().normalize_user_path(path).await?;
                monitor
                    .detect_conflict(&canonical_path, Some(String::new()), override_snapshot)
                    .await
            }
            crate::tools::editing::PatchOperation::UpdateFile { path, chunks, .. } => {
                let canonical_path = self.file_ops_tool().normalize_user_path(path).await?;
                let intended_content =
                    if let Some(content) = monitor.tracked_read_text(&canonical_path).await {
                        match crate::tools::editing::patch::render_patch_update_content(
                            &canonical_path,
                            &content,
                            chunks,
                            path,
                        )
                        .await
                        {
                            Ok(rendered) => Some(rendered),
                            Err(err) => {
                                tracing::debug!(
                                    path = %canonical_path.display(),
                                    error = %err,
                                    "Failed to render patch conflict preview content"
                                );
                                None
                            }
                        }
                    } else {
                        None
                    };

                monitor
                    .detect_conflict(&canonical_path, intended_content, override_snapshot)
                    .await
            }
        }
    }

    async fn planned_patch_writes_for_operation(
        &self,
        operation: &crate::tools::editing::PatchOperation,
    ) -> Result<Vec<PlannedPatchWrite>> {
        match operation {
            crate::tools::editing::PatchOperation::AddFile { path, content } => {
                Ok(vec![PlannedPatchWrite::Text {
                    path: self.file_ops_tool().normalize_user_path(path).await?,
                    content: content.clone(),
                }])
            }
            crate::tools::editing::PatchOperation::DeleteFile { path } => {
                Ok(vec![PlannedPatchWrite::Removal {
                    path: self.file_ops_tool().normalize_user_path(path).await?,
                }])
            }
            crate::tools::editing::PatchOperation::UpdateFile {
                path,
                new_path,
                chunks,
            } => {
                let canonical_path = self.file_ops_tool().normalize_user_path(path).await?;
                let source_content = if let Some(content) = self
                    .edited_file_monitor()
                    .tracked_read_text(&canonical_path)
                    .await
                {
                    content
                } else {
                    fs::read_to_string(&canonical_path).await.with_context(|| {
                        format!(
                            "Failed to read patch source content for {}",
                            canonical_path.display()
                        )
                    })?
                };

                let rendered = crate::tools::editing::patch::render_patch_update_content(
                    &canonical_path,
                    &source_content,
                    chunks,
                    path,
                )
                .await
                .map_err(|err| {
                    anyhow!(
                        "Failed to plan patch output for {}: {err}",
                        canonical_path.display()
                    )
                })?;

                let mut writes = Vec::new();
                if let Some(destination) = new_path
                    .as_ref()
                    .filter(|candidate| candidate.as_str() != path.as_str())
                {
                    writes.push(PlannedPatchWrite::Removal {
                        path: canonical_path,
                    });
                    writes.push(PlannedPatchWrite::Text {
                        path: self
                            .file_ops_tool()
                            .normalize_user_path(destination)
                            .await?,
                        content: rendered,
                    });
                } else {
                    writes.push(PlannedPatchWrite::Text {
                        path: canonical_path,
                        content: rendered,
                    });
                }

                Ok(writes)
            }
        }
    }

    async fn wait_for_exec_yield(
        &self,
        session_id: &str,
        yield_duration: Duration,
        tool_name: Option<&str>,
        drain_output: bool,
    ) -> PtyEphemeralCapture {
        let mut output = String::new();
        let mut peeked_bytes = 0usize;
        let start = Instant::now();
        let poll_interval = Duration::from_millis(50);

        // Get the progress callback for streaming output to the TUI
        let progress_callback = self.progress_callback();

        // Throttle TUI updates to prevent excessive redraws
        let mut last_ui_update = Instant::now();
        let ui_update_interval = Duration::from_millis(100);
        let mut pending_lines = String::new();

        loop {
            if let Ok(Some(code)) = self.exec_session_completed(session_id).await {
                if let Ok(Some(final_output)) = self
                    .next_exec_session_output(session_id, drain_output, &mut peeked_bytes)
                    .await
                {
                    output.push_str(&final_output);

                    // Stream final output to TUI
                    if let Some(tool_name) = tool_name
                        && let Some(ref callback) = progress_callback
                    {
                        pending_lines.push_str(&final_output);
                        if !pending_lines.is_empty() {
                            callback(tool_name, &pending_lines);
                        }
                    }
                }
                return PtyEphemeralCapture {
                    output,
                    exit_code: Some(code),
                    duration: start.elapsed(),
                };
            }

            if let Ok(Some(new_output)) = self
                .next_exec_session_output(session_id, drain_output, &mut peeked_bytes)
                .await
            {
                output.push_str(&new_output);
                if tool_name.is_some() {
                    pending_lines.push_str(&new_output);
                }

                // Stream output to TUI with throttling
                if let Some(tool_name) = tool_name
                    && let Some(ref callback) = progress_callback
                {
                    let now = Instant::now();
                    // Flush pending lines if interval elapsed or if we have a complete line
                    if (now.duration_since(last_ui_update) >= ui_update_interval
                        || pending_lines.contains('\n'))
                        && !pending_lines.is_empty()
                    {
                        callback(tool_name, &pending_lines);
                        pending_lines.clear();
                        last_ui_update = now;
                    }
                }
            }

            if start.elapsed() >= yield_duration {
                // Flush any remaining pending lines
                if let Some(tool_name) = tool_name
                    && let Some(ref callback) = progress_callback
                    && !pending_lines.is_empty()
                {
                    callback(tool_name, &pending_lines);
                }
                return PtyEphemeralCapture {
                    output,
                    exit_code: None,
                    duration: start.elapsed(),
                };
            }

            tokio::time::sleep(poll_interval).await;
        }
    }

    async fn capture_exec_session_output(
        &self,
        session_id: &str,
        yield_duration: Duration,
        tool_name: Option<&str>,
        settle_until_terminal: bool,
    ) -> Result<PtyEphemeralCapture> {
        if !settle_until_terminal {
            return Ok(self
                .wait_for_exec_yield(session_id, yield_duration, tool_name, true)
                .await);
        }

        let start = Instant::now();
        let mut output = String::new();

        loop {
            let capture = self
                .wait_for_exec_yield(session_id, yield_duration, tool_name, true)
                .await;
            output.push_str(&capture.output);

            if let Some(exit_code) = capture.exit_code {
                return Ok(PtyEphemeralCapture {
                    output,
                    exit_code: Some(exit_code),
                    duration: start.elapsed(),
                });
            }

            self.exec_session_metadata(session_id)
                .await
                .with_context(|| {
                    format!(
                        "exec session '{}' disappeared during settlement",
                        session_id
                    )
                })?;
        }
    }

    async fn next_exec_session_output(
        &self,
        session_id: &str,
        drain_output: bool,
        peeked_bytes: &mut usize,
    ) -> Result<Option<String>> {
        let Some(output) = self
            .read_exec_session_output(session_id, drain_output)
            .await?
        else {
            return Ok(None);
        };
        if drain_output {
            return Ok(Some(output));
        }
        if output.len() <= *peeked_bytes {
            return Ok(None);
        }

        let next = output
            .get(*peeked_bytes..)
            .ok_or_else(|| {
                anyhow!(
                    "exec session '{}' output boundary became invalid",
                    session_id
                )
            })?
            .to_string();
        *peeked_bytes = output.len();
        if next.is_empty() {
            Ok(None)
        } else {
            Ok(Some(next))
        }
    }
}

// Helper functions and structs for PTY execution

struct PtyEphemeralCapture {
    output: String,
    exit_code: Option<i32>,
    duration: Duration,
}

fn parse_command_parts(
    payload: &serde_json::Map<String, Value>,
    missing_error: &str,
    empty_error: &str,
) -> Result<(Vec<String>, Option<String>)> {
    let normalized_payload = (!payload.contains_key("command")
        && (payload.contains_key("cmd")
            || payload.contains_key("raw_command")
            || payload.contains_key("command.0")
            || payload.contains_key("command.1")))
    .then(|| {
        crate::tools::command_args::normalize_shell_args(&Value::Object(payload.clone()))
            .map_err(|error| anyhow!(error))
    })
    .transpose()?;
    let payload = normalized_payload
        .as_ref()
        .and_then(Value::as_object)
        .unwrap_or(payload);

    let (mut parts, raw_command) = match payload.get("command") {
        Some(Value::String(command)) => {
            // Preserve the original command string to avoid splitting shell operators
            let parts = shell_words::split(command).context("Failed to parse command string")?;
            (parts, Some(command.to_string()))
        }
        Some(Value::Array(values)) => {
            let parts = values
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .map(|part| part.to_string())
                        .ok_or_else(|| anyhow!("command array must contain only strings"))
                })
                .collect::<Result<Vec<_>>>()?;
            (parts, None)
        }
        _ => match crate::tools::command_args::parse_indexed_command_parts(payload)
            .map_err(|error| anyhow!(error))?
        {
            Some(indexed_parts) => (indexed_parts, None),
            None => return Err(anyhow!("{}", missing_error)),
        },
    };

    if let Some(args_value) = payload.get("args")
        && let Some(args_array) = args_value.as_array()
    {
        for value in args_array {
            if let Some(part) = value.as_str() {
                parts.push(part.to_string());
            }
        }
    }

    if parts.is_empty() {
        return Err(anyhow!("{}", empty_error));
    }

    Ok((parts, raw_command))
}

fn is_git_diff_command(parts: &[String]) -> bool {
    let Some(first) = parts.first() else {
        return false;
    };
    let basename = Path::new(first)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(first.as_str())
        .to_ascii_lowercase();
    if basename != "git" && basename != "git.exe" {
        return false;
    }
    parts.iter().skip(1).any(|part| part == "diff")
}

fn resolve_shell_preference(pref: Option<&str>, config: &crate::config::PtyConfig) -> String {
    pref.map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            config
                .preferred_shell
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(resolve_fallback_shell)
}

fn resolve_shell_preference_with_zsh_fork(
    pref: Option<&str>,
    config: &crate::config::PtyConfig,
) -> Result<String> {
    if let Some(zsh_path) = config.zsh_fork_shell_path()? {
        return Ok(zsh_path.to_string());
    }

    Ok(resolve_shell_preference(pref, config))
}

fn normalized_shell_name(shell: &str) -> String {
    PathBuf::from(shell)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(shell)
        .to_lowercase()
}

fn build_shell_command_string(raw: Option<&str>, parts: &[String], _shell: &str) -> String {
    let fallback = || shell_words::join(parts.iter().map(|s| s.as_str()));

    let Some(raw) = raw else {
        return fallback();
    };

    // Preserve original shell syntax from `command` (operators, quoting), while still
    // appending any separate `args` entries that were merged into `parts`.
    let Ok(raw_parts) = shell_words::split(raw) else {
        return fallback();
    };

    if parts.len() <= raw_parts.len() || !parts.starts_with(&raw_parts) {
        return raw.to_string();
    }

    let suffix = shell_words::join(parts[raw_parts.len()..].iter().map(|s| s.as_str()));
    if suffix.is_empty() {
        raw.to_string()
    } else {
        format!("{} {}", raw, suffix)
    }
}

#[cfg(test)]
mod shell_preference_tests {
    use super::{resolve_shell_preference, resolve_shell_preference_with_zsh_fork};
    use crate::config::PtyConfig;
    use crate::tools::shell::resolve_fallback_shell;

    #[test]
    fn explicit_shell_overrides_config_preference() {
        let config = PtyConfig {
            preferred_shell: Some("/bin/bash".to_string()),
            ..Default::default()
        };

        let resolved = resolve_shell_preference(Some(" /bin/zsh "), &config);
        assert_eq!(resolved, "/bin/zsh");
    }

    #[test]
    fn config_preferred_shell_used_when_explicit_missing() {
        let config = PtyConfig {
            preferred_shell: Some("zsh".to_string()),
            ..Default::default()
        };

        let resolved = resolve_shell_preference(None, &config);
        assert_eq!(resolved, "zsh");
    }

    #[test]
    fn blank_explicit_shell_falls_back_to_config_preference() {
        let config = PtyConfig {
            preferred_shell: Some("bash".to_string()),
            ..Default::default()
        };

        let resolved = resolve_shell_preference(Some("   "), &config);
        assert_eq!(resolved, "bash");
    }

    #[test]
    fn blank_config_shell_falls_back_to_default_resolver() {
        let config = PtyConfig {
            preferred_shell: Some("   ".to_string()),
            ..Default::default()
        };

        let resolved = resolve_shell_preference(None, &config);
        assert_eq!(resolved, resolve_fallback_shell());
    }

    #[test]
    fn missing_preferences_fall_back_to_default_resolver() {
        let config = PtyConfig::default();
        let resolved = resolve_shell_preference(None, &config);
        assert_eq!(resolved, resolve_fallback_shell());
    }

    #[test]
    fn zsh_fork_disabled_uses_standard_shell_resolution() -> anyhow::Result<()> {
        let config = PtyConfig {
            preferred_shell: Some("/bin/bash".to_string()),
            ..Default::default()
        };
        let resolved = resolve_shell_preference_with_zsh_fork(None, &config)?;
        assert_eq!(resolved, "/bin/bash");
        Ok(())
    }

    #[test]
    fn zsh_fork_missing_path_returns_error() {
        let config = PtyConfig {
            shell_zsh_fork: true,
            zsh_path: None,
            ..PtyConfig::default()
        };
        assert!(resolve_shell_preference_with_zsh_fork(Some("/bin/bash"), &config).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn zsh_fork_ignores_explicit_shell_and_uses_configured_path() -> anyhow::Result<()> {
        let zsh = tempfile::NamedTempFile::new()?;
        let expected = zsh.path().to_string_lossy().to_string();
        let config = PtyConfig {
            shell_zsh_fork: true,
            zsh_path: Some(expected.clone()),
            ..PtyConfig::default()
        };
        let resolved = resolve_shell_preference_with_zsh_fork(Some("/bin/bash"), &config)?;
        assert_eq!(resolved, expected);
        Ok(())
    }
}

/// Check if a command is a file display command that should have limited output.
/// Returns suggested max_tokens if the command is a file display command without explicit limits.
pub fn suggest_max_tokens_for_command(cmd: &str) -> Option<usize> {
    let trimmed = cmd.trim().to_lowercase();

    // Skip if command already has output limiting
    if trimmed.contains("head") || trimmed.contains("tail") || trimmed.contains("| ") {
        return None;
    }

    // File display commands that benefit from token limits
    let file_display_cmds = ["cat ", "bat ", "type "]; // type for Windows

    for prefix in &file_display_cmds {
        if trimmed.starts_with(prefix) {
            // Suggest 250 tokens (~1000 chars) for file preview
            return Some(250);
        }
    }

    None
}

fn should_use_windows_command_tokenizer(shell: Option<&str>) -> bool {
    if cfg!(windows) {
        if let Some(s) = shell {
            let lower = s.to_lowercase();
            return lower.contains("cmd") || lower.contains("powershell") || lower.contains("pwsh");
        }
        return true;
    }
    false
}

fn join_windows_command(parts: &[String]) -> String {
    parts.join(" ")
}

fn parse_pty_dimension(name: &str, value: Option<&Value>, default: u16) -> Result<u16> {
    match value {
        Some(v) => {
            let n = v
                .as_u64()
                .ok_or_else(|| anyhow!("{} must be a number", name))?;
            Ok(n as u16)
        }
        None => Ok(default),
    }
}

fn generate_session_id(prefix: &str) -> String {
    format!("{}-{}", prefix, &uuid::Uuid::new_v4().to_string()[..8])
}

fn strip_ansi(text: &str) -> String {
    crate::utils::ansi_parser::strip_ansi(text)
}

fn filter_pty_output(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

// Conservative PTY command policy inspired by bash allow/deny defaults.
const PTY_DENY_PREFIXES: &[&str] = &[
    "bash -i",
    "sh -i",
    "zsh -i",
    "fish -i",
    "python -i",
    "python3 -i",
    "ipython",
    "nano",
    "vim",
    "vi",
    "emacs",
    "top",
    "htop",
    "less",
    "more",
    "screen",
    "tmux",
];

const PTY_DENY_STANDALONE: &[&str] = &["python", "python3", "bash", "sh", "zsh", "fish"];

#[allow(dead_code)]
const PTY_ALLOW_PREFIXES: &[&str] = &[
    "pwd",
    "whoami",
    "ls",
    "git status",
    "git diff",
    "git log",
    "stat",
    "which",
    "echo",
    "cat",
];

fn enforce_pty_command_policy(display_command: &str, confirm: bool) -> Result<()> {
    let lower = display_command.to_ascii_lowercase();
    let trimmed = lower.trim();
    let is_standalone = trimmed.split_whitespace().count() == 1;

    let deny_match = PTY_DENY_PREFIXES
        .iter()
        .any(|prefix| trimmed.starts_with(prefix));
    let standalone_denied = is_standalone && PTY_DENY_STANDALONE.contains(&trimmed);

    if deny_match || standalone_denied {
        if confirm {
            return Ok(());
        }
        return Err(anyhow!(
            "Command '{}' is blocked by PTY safety policy. Set confirm=true to force execution.",
            display_command
        ));
    }

    // Allowlisted commands are simply allowed; we rely on general policy for others.
    Ok(())
}

#[cfg(test)]
mod token_efficiency_tests {
    use super::*;

    #[test]
    fn test_suggests_limit_for_cat() {
        assert_eq!(suggest_max_tokens_for_command("cat file.txt"), Some(250));
        assert_eq!(
            suggest_max_tokens_for_command("cat /path/to/file.rs"),
            Some(250)
        );
        assert_eq!(suggest_max_tokens_for_command("CAT file.txt"), Some(250)); // case insensitive
    }

    #[test]
    fn test_suggests_limit_for_bat() {
        assert_eq!(suggest_max_tokens_for_command("bat file.rs"), Some(250));
    }

    #[test]
    fn test_no_limit_when_already_limited() {
        assert_eq!(suggest_max_tokens_for_command("cat file.txt | head"), None);
        assert_eq!(suggest_max_tokens_for_command("head -n 50 file.txt"), None);
        assert_eq!(suggest_max_tokens_for_command("tail -n 20 file.txt"), None);
    }

    #[test]
    fn test_no_limit_for_other_commands() {
        assert_eq!(suggest_max_tokens_for_command("ls -la"), None);
        assert_eq!(suggest_max_tokens_for_command("grep pattern file"), None);
        assert_eq!(suggest_max_tokens_for_command("echo hello"), None);
    }
}

#[cfg(test)]
mod pty_output_filter_tests {
    use super::filter_pty_output;

    #[test]
    fn normalizes_crlf_sequences() {
        let raw = "a\r\nb\rc\n";
        assert_eq!(filter_pty_output(raw), "a\nb\nc\n");
    }
}

#[cfg(test)]
mod pty_context_tests {
    use super::{
        ExecOutputPreview, PtyEphemeralCapture, attach_exec_response_context,
        attach_pty_continuation, build_exec_response, build_exec_session_command_display,
    };
    use crate::tools::types::VTCodeExecSession;
    use serde_json::json;

    #[test]
    fn build_exec_session_command_display_unwraps_shell_c_argument() {
        let session = VTCodeExecSession {
            id: "run-123".to_string(),
            backend: "pty".to_string(),
            command: "zsh".to_string(),
            args: vec![
                "-l".to_string(),
                "-c".to_string(),
                "cargo check".to_string(),
            ],
            working_dir: Some(".".to_string()),
            rows: Some(24),
            cols: Some(80),
        };

        assert_eq!(build_exec_session_command_display(&session), "cargo check");
    }

    #[test]
    fn attach_exec_response_context_sets_expected_keys() {
        let mut response = json!({ "output": "ok" });
        let session = VTCodeExecSession {
            id: "run-123".to_string(),
            backend: "pty".to_string(),
            command: "zsh".to_string(),
            args: vec![
                "-l".to_string(),
                "-c".to_string(),
                "cargo check".to_string(),
            ],
            working_dir: Some(".".to_string()),
            rows: Some(30),
            cols: Some(120),
        };

        attach_exec_response_context(&mut response, &session, "cargo check", false);

        assert_eq!(response["session_id"], "run-123");
        assert_eq!(response["command"], "cargo check");
        assert_eq!(response["working_directory"], ".");
        assert_eq!(response["backend"], "pty");
        assert_eq!(response["rows"], 30);
        assert_eq!(response["cols"], 120);
        assert_eq!(response["is_exited"], false);
    }

    #[test]
    fn attach_pty_continuation_compacts_next_continue_args() {
        let mut response = json!({ "output": "ok" });
        attach_pty_continuation(&mut response, "run-123");

        assert!(response.get("follow_up_prompt").is_none());
        assert!(response.get("next_poll_args").is_none());
        assert_eq!(
            response["next_continue_args"],
            json!({ "session_id": "run-123" })
        );
        assert!(response.get("preferred_next_action").is_none());
    }

    #[test]
    fn attach_pty_continuation_keeps_payload_compact() {
        let mut response = json!({ "output": "ok" });
        attach_pty_continuation(&mut response, "run-123");

        assert!(response.get("follow_up_prompt").is_none());
        assert!(response.get("next_poll_args").is_none());
        assert_eq!(
            response["next_continue_args"],
            json!({ "session_id": "run-123" })
        );
    }

    #[test]
    fn build_exec_response_skips_continuation_after_exit() {
        let session = VTCodeExecSession {
            id: "run-123".to_string(),
            backend: "pipe".to_string(),
            command: "cargo".to_string(),
            args: vec!["check".to_string()],
            working_dir: Some(".".to_string()),
            rows: None,
            cols: None,
        };
        let capture = PtyEphemeralCapture {
            output: "first\nsecond\n".to_string(),
            exit_code: Some(0),
            duration: std::time::Duration::from_millis(25),
        };

        let response = build_exec_response(
            &session,
            "cargo check",
            &capture,
            ExecOutputPreview {
                raw_output: "first\nsecond\n".to_string(),
                output: "first\n[Output truncated]".to_string(),
                truncated: true,
            },
            None,
            false,
            None,
        );

        assert_eq!(response["exit_code"], 0);
        assert!(response.get("next_continue_args").is_none());
    }
}

#[cfg(test)]
mod git_diff_tests {
    use super::is_git_diff_command;

    #[test]
    fn detects_git_diff() {
        let cmd = vec!["git".to_string(), "diff".to_string()];
        assert!(is_git_diff_command(&cmd));
    }

    #[test]
    fn detects_git_diff_with_flags() {
        let cmd = vec![
            "git".to_string(),
            "-c".to_string(),
            "color.ui=always".to_string(),
            "diff".to_string(),
            "--stat".to_string(),
        ];
        assert!(is_git_diff_command(&cmd));
    }

    #[test]
    fn detects_git_diff_with_path() {
        let cmd = vec!["/usr/bin/git".to_string(), "diff".to_string()];
        assert!(is_git_diff_command(&cmd));
    }

    #[test]
    fn ignores_other_git_commands() {
        let cmd = vec!["git".to_string(), "status".to_string()];
        assert!(!is_git_diff_command(&cmd));
    }
}

#[cfg(test)]
mod unified_action_error_tests {
    use super::{
        CargoTestCommandKind, ExecOutputPreview, PtyEphemeralCapture,
        attach_exec_recovery_guidance, attach_failure_diagnostics_metadata,
        build_exec_output_preview, build_exec_response, build_head_tail_preview,
        cargo_selector_error_diagnostics, cargo_test_failure_diagnostics, cargo_test_rerun_hint,
        clamp_inspect_lines, clamp_max_matches, extract_run_session_id_from_read_file_error,
        extract_run_session_id_from_tool_output_path, filter_lines,
        missing_unified_exec_action_error, missing_unified_search_action_error,
        summarized_arg_keys,
    };
    use crate::tools::types::VTCodeExecSession;
    use serde_json::json;
    use std::time::Duration;

    #[test]
    fn summarized_arg_keys_reports_shape_for_non_object_payloads() {
        assert_eq!(summarized_arg_keys(&json!(null)), "<null>");
        assert_eq!(summarized_arg_keys(&json!(["a", "b"])), "<array>");
        assert_eq!(summarized_arg_keys(&json!("x")), "<string>");
    }

    #[test]
    fn unified_exec_missing_action_error_includes_received_keys() {
        let err = missing_unified_exec_action_error(&json!({
            "foo": "bar",
            "session_id": "123"
        }));
        let text = err.to_string();
        assert!(text.contains("Missing unified_exec action"));
        assert!(text.contains("foo"));
        assert!(text.contains("session_id"));
    }

    #[test]
    fn unified_search_missing_action_error_includes_received_keys() {
        let err = missing_unified_search_action_error(&json!({
            "unexpected": true
        }));
        let text = err.to_string();
        assert!(text.contains("Missing unified_search action"));
        assert!(text.contains("unexpected"));
    }

    #[test]
    fn extracts_run_session_id_from_tool_output_path() {
        assert_eq!(
            extract_run_session_id_from_tool_output_path(
                ".vtcode/context/tool_outputs/run-abc123.txt"
            ),
            Some("run-abc123".to_string())
        );
        assert_eq!(
            extract_run_session_id_from_tool_output_path(
                ".vtcode/context/tool_outputs/not-a-session.txt"
            ),
            None
        );
    }

    #[test]
    fn extracts_run_session_id_from_read_file_error() {
        let error = "Use unified_exec with session_id=\"run-zz9\" instead of read_file.";
        assert_eq!(
            extract_run_session_id_from_read_file_error(error),
            Some("run-zz9".to_string())
        );
        assert_eq!(
            extract_run_session_id_from_read_file_error("no session"),
            None
        );
    }

    #[test]
    fn inspect_helpers_clamp_limits() {
        assert_eq!(clamp_inspect_lines(Some(0), 30), 0);
        assert_eq!(clamp_inspect_lines(Some(9_999), 30), 5_000);
        assert_eq!(clamp_max_matches(None), 200);
        assert_eq!(clamp_max_matches(Some(0)), 1);
        assert_eq!(clamp_max_matches(Some(50_000)), 10_000);
    }

    #[test]
    fn inspect_helpers_build_head_tail_preview() {
        let content = "l1\nl2\nl3\nl4\nl5\nl6";
        let (preview, truncated) = build_head_tail_preview(content, 2, 2);
        assert!(truncated);
        assert!(preview.contains("l1"));
        assert!(preview.contains("l2"));
        assert!(preview.contains("l5"));
        assert!(preview.contains("l6"));
    }

    #[test]
    fn inspect_helpers_filter_lines_literal() {
        let (output, matched, truncated) =
            filter_lines("alpha\nbeta\nalpha2", "alpha", true, 1).expect("filter");
        assert_eq!(matched, 2);
        assert!(truncated);
        assert!(output.contains("1: alpha"));
    }

    #[test]
    fn exec_output_preview_truncates_on_utf8_boundaries() {
        let preview = build_exec_output_preview("a🙂b".to_string(), 1);

        assert!(preview.truncated);
        assert_eq!(preview.raw_output, "a🙂b");
        assert_eq!(preview.output, "a\n[Output truncated]");
        assert!(std::str::from_utf8(preview.output.as_bytes()).is_ok());
    }

    #[test]
    fn exec_recovery_guidance_sets_command_not_found_metadata() {
        let session = VTCodeExecSession {
            id: "run-123".to_string(),
            backend: "pipe".to_string(),
            command: "zsh".to_string(),
            args: vec!["-c".to_string(), "pip install pymupdf".to_string()],
            working_dir: Some(".".to_string()),
            rows: None,
            cols: None,
        };
        let capture = PtyEphemeralCapture {
            output: String::new(),
            exit_code: Some(127),
            duration: Duration::from_millis(42),
        };

        let response = build_exec_response(
            &session,
            "pip install pymupdf",
            &capture,
            ExecOutputPreview {
                raw_output: "bash: pip: command not found".to_string(),
                output: "bash: pip: command not found".to_string(),
                truncated: false,
            },
            None,
            false,
            None,
        );

        assert_eq!(response["output"], "bash: pip: command not found");
        assert_eq!(response["exit_code"], 127);
        assert_eq!(response["session_id"], "run-123");
        assert_eq!(response["command"], "pip install pymupdf");
        assert_eq!(
            response["critical_note"],
            "Command `pip` was not found in PATH."
        );
        assert_eq!(
            response["next_action"],
            "Check the command name or install the missing binary, then rerun the command."
        );
    }

    #[test]
    fn exec_recovery_guidance_ignores_non_command_not_found_exit_codes() {
        let mut response = json!({});
        attach_exec_recovery_guidance(&mut response, "cargo test", Some(1));
        assert!(response.get("critical_note").is_none());
        assert!(response.get("next_action").is_none());
    }

    #[test]
    fn cargo_selector_error_diagnostics_classifies_missing_test_target() {
        let output = "error: no test target named `exec_only_policy_skips_when_full_auto_is_disabled` in `vtcode-core` package\n";

        let diagnostics = cargo_selector_error_diagnostics(
            CargoTestCommandKind::Nextest,
            "cargo nextest run --test exec_only_policy_skips_when_full_auto_is_disabled -p vtcode-core --no-capture",
            output,
        )
        .expect("selector diagnostics");

        assert_eq!(diagnostics["kind"], "cargo_test_selector_error");
        assert_eq!(diagnostics["package"], "vtcode-core");
        assert_eq!(
            diagnostics["requested_test_target"],
            "exec_only_policy_skips_when_full_auto_is_disabled"
        );
        assert_eq!(diagnostics["selector_error"], true);
        assert_eq!(
            diagnostics["validation_hint"],
            "cargo test -p vtcode-core --lib -- --list | rg 'exec_only_policy_skips_when_full_auto_is_disabled'"
        );
        assert_eq!(
            diagnostics["rerun_hint"],
            "cargo nextest run -p vtcode-core exec_only_policy_skips_when_full_auto_is_disabled"
        );
    }

    #[test]
    fn cargo_test_failure_diagnostics_extracts_unit_test_failure_details() {
        let output = r#"────────────
    Nextest run ID 18fffe01-0ef9-4113-9a81-2344a7cc3c16 with nextest profile: default
        FAIL [   0.216s] ( 363/2669) vtcode-core core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled
    stderr ───
    thread 'core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled' (382951) panicked at vtcode-core/src/core/agent/runner/tests.rs:692:10:
    task result: Invalid request: QueuedProvider has no queued responses
"#;

        let diagnostics =
            cargo_test_failure_diagnostics("cargo nextest run -p vtcode-core", output, Some(100))
                .expect("failure diagnostics");

        assert_eq!(diagnostics["kind"], "cargo_test_failure");
        assert_eq!(diagnostics["package"], "vtcode-core");
        assert_eq!(diagnostics["binary_kind"], "unit");
        assert_eq!(
            diagnostics["test_fqname"],
            "core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled"
        );
        assert_eq!(
            diagnostics["panic"],
            "task result: Invalid request: QueuedProvider has no queued responses"
        );
        assert_eq!(
            diagnostics["source_file"],
            "vtcode-core/src/core/agent/runner/tests.rs"
        );
        assert_eq!(diagnostics["source_line"], 692);
        assert_eq!(
            diagnostics["rerun_hint"],
            cargo_test_rerun_hint(
                CargoTestCommandKind::Nextest,
                "vtcode-core",
                "unit",
                "core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled",
            )
        );
    }

    #[test]
    fn build_exec_response_attaches_cargo_failure_diagnostics() {
        let session = VTCodeExecSession {
            id: "run-123".to_string(),
            backend: "pipe".to_string(),
            command: "cargo".to_string(),
            args: vec![
                "nextest".to_string(),
                "run".to_string(),
                "-p".to_string(),
                "vtcode-core".to_string(),
            ],
            working_dir: Some(".".to_string()),
            rows: None,
            cols: None,
        };
        let raw_output = r#"
        FAIL [   0.216s] ( 363/2669) vtcode-core core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled
    thread 'core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled' (382951) panicked at vtcode-core/src/core/agent/runner/tests.rs:692:10:
    task result: Invalid request: QueuedProvider has no queued responses
"#;
        let capture = PtyEphemeralCapture {
            output: raw_output.to_string(),
            exit_code: Some(100),
            duration: Duration::from_millis(42),
        };

        let response = build_exec_response(
            &session,
            "cargo nextest run -p vtcode-core",
            &capture,
            ExecOutputPreview {
                raw_output: raw_output.to_string(),
                output: raw_output.to_string(),
                truncated: false,
            },
            None,
            false,
            None,
        );

        assert_eq!(
            response["failure_diagnostics"]["test_fqname"],
            "core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled"
        );
        assert_eq!(response["package"], "vtcode-core");
        assert_eq!(response["binary_kind"], "unit");
        assert_eq!(
            response["source_file"],
            "vtcode-core/src/core/agent/runner/tests.rs"
        );
        assert_eq!(response["source_line"], 692);
        assert_eq!(
            response["rerun_hint"],
            "cargo nextest run -p vtcode-core core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled"
        );
        assert_eq!(
            response["next_action"],
            "Rerun the failing test directly with: cargo nextest run -p vtcode-core core::agent::runner::tests::exec_only_policy_skips_when_full_auto_is_disabled"
        );
    }

    #[test]
    fn attach_failure_diagnostics_metadata_promotes_selector_hints() {
        let mut response = json!({
            "success": true,
            "command": "cargo nextest run --test bad -p vtcode-core"
        });
        let diagnostics = json!({
            "kind": "cargo_test_selector_error",
            "package": "vtcode-core",
            "binary_kind": "test_target_selector",
            "requested_test_target": "bad",
            "selector_error": true,
            "validation_hint": "cargo test -p vtcode-core --lib -- --list | rg 'bad'",
            "rerun_hint": "cargo nextest run -p vtcode-core bad",
            "critical_note": "selector mismatch",
            "next_action": "validate first"
        });

        attach_failure_diagnostics_metadata(&mut response, &diagnostics);

        assert_eq!(response["package"], "vtcode-core");
        assert_eq!(response["binary_kind"], "test_target_selector");
        assert_eq!(response["selector_error"], true);
        assert_eq!(
            response["validation_hint"],
            "cargo test -p vtcode-core --lib -- --list | rg 'bad'"
        );
        assert_eq!(
            response["rerun_hint"],
            "cargo nextest run -p vtcode-core bad"
        );
        assert_eq!(response["critical_note"], "selector mismatch");
        assert_eq!(response["next_action"], "validate first");
        assert_eq!(
            response["failure_diagnostics"]["kind"],
            "cargo_test_selector_error"
        );
    }
}

#[cfg(test)]
mod sandbox_runtime_tests {
    use super::{
        apply_runtime_sandbox_to_command, build_shell_execution_plan,
        enforce_sandbox_preflight_guards, parse_command_parts, parse_requested_sandbox_permissions,
        sandbox_policy_from_runtime_config, sandbox_policy_with_additional_permissions,
    };
    use crate::sandboxing::{
        AdditionalPermissions, NetworkAllowlistEntry, SandboxPermissions, SandboxPolicy,
        SensitivePath,
    };
    use serde_json::json;
    use std::path::PathBuf;

    #[test]
    fn runtime_sandbox_config_maps_workspace_policy_overrides() {
        let mut config = vtcode_config::SandboxConfig {
            enabled: true,
            default_mode: vtcode_config::SandboxMode::WorkspaceWrite,
            ..Default::default()
        };
        config.network.allow_all = false;
        config.network.allowlist = vec![vtcode_config::NetworkAllowlistEntryConfig {
            domain: "api.github.com".to_string(),
            port: 443,
        }];
        config.sensitive_paths.use_defaults = false;
        config.sensitive_paths.additional = vec!["/tmp/secret".to_string()];
        config.resource_limits.preset = vtcode_config::ResourceLimitsPreset::Conservative;
        config.resource_limits.max_memory_mb = 2048;
        config.seccomp.profile = vtcode_config::SeccompProfilePreset::Strict;

        let policy =
            sandbox_policy_from_runtime_config(&config, PathBuf::from("/tmp/ws").as_path())
                .unwrap();
        assert!(policy.has_network_allowlist());
        assert!(policy.is_network_allowed("api.github.com", 443));
        assert!(!policy.is_network_allowed("example.com", 443));
        assert!(!policy.is_path_readable(&PathBuf::from("/tmp/secret/token")));
        assert_eq!(policy.resource_limits().max_memory_mb, 2048);
    }

    #[test]
    fn read_only_mutating_command_requires_approval_and_workspace_write() {
        let config = vtcode_config::SandboxConfig {
            enabled: true,
            default_mode: vtcode_config::SandboxMode::ReadOnly,
            ..Default::default()
        };

        let command = vec!["cargo".to_string(), "fmt".to_string()];
        let plan = build_shell_execution_plan(
            &config,
            PathBuf::from("/tmp/ws").as_path(),
            &command,
            SandboxPermissions::UseDefault,
            None,
        )
        .unwrap();
        assert!(plan.approval_reason.is_some());
        assert!(matches!(
            plan.sandbox_policy,
            Some(SandboxPolicy::WorkspaceWrite { .. })
        ));
    }

    #[test]
    fn read_only_non_mutating_command_stays_read_only_without_prompt() {
        let config = vtcode_config::SandboxConfig {
            enabled: true,
            default_mode: vtcode_config::SandboxMode::ReadOnly,
            ..Default::default()
        };

        let command = vec!["ls".to_string(), "-la".to_string()];
        let plan = build_shell_execution_plan(
            &config,
            PathBuf::from("/tmp/ws").as_path(),
            &command,
            SandboxPermissions::UseDefault,
            None,
        )
        .unwrap();
        assert!(plan.approval_reason.is_none());
        assert!(matches!(
            plan.sandbox_policy,
            Some(SandboxPolicy::ReadOnly { .. })
        ));
    }

    #[test]
    fn preflight_blocks_network_commands_when_network_disabled() {
        let policy = SandboxPolicy::workspace_write(vec![PathBuf::from("/tmp/ws")]);
        let command = vec!["curl".to_string(), "https://example.com".to_string()];
        let error =
            enforce_sandbox_preflight_guards(&command, &policy, PathBuf::from(".").as_path())
                .expect_err("network command should be denied");
        assert!(error.to_string().contains("denies network"));
    }

    #[test]
    fn workspace_write_allow_all_network_is_not_blocked() {
        let mut config = vtcode_config::SandboxConfig {
            enabled: true,
            default_mode: vtcode_config::SandboxMode::WorkspaceWrite,
            ..Default::default()
        };
        config.network.allow_all = true;
        config.network.block_all = false;

        let policy =
            sandbox_policy_from_runtime_config(&config, PathBuf::from("/tmp/ws").as_path())
                .unwrap();
        assert!(policy.has_full_network_access());

        let command = vec!["curl".to_string(), "https://example.com".to_string()];
        enforce_sandbox_preflight_guards(&command, &policy, PathBuf::from(".").as_path())
            .expect("allow_all network should permit network commands");
    }

    #[test]
    fn read_only_allow_all_network_is_not_blocked() {
        let mut config = vtcode_config::SandboxConfig {
            enabled: true,
            default_mode: vtcode_config::SandboxMode::ReadOnly,
            ..Default::default()
        };
        config.network.allow_all = true;
        config.network.block_all = false;

        let policy =
            sandbox_policy_from_runtime_config(&config, PathBuf::from("/tmp/ws").as_path())
                .unwrap();
        assert!(policy.has_full_network_access());

        let command = vec!["curl".to_string(), "https://example.com".to_string()];
        enforce_sandbox_preflight_guards(&command, &policy, PathBuf::from(".").as_path())
            .expect("read-only allow_all network should permit network commands");
    }

    #[test]
    fn read_only_allowlist_network_is_not_blocked() {
        let mut config = vtcode_config::SandboxConfig {
            enabled: true,
            default_mode: vtcode_config::SandboxMode::ReadOnly,
            ..Default::default()
        };
        config.network.allow_all = false;
        config.network.allowlist = vec![vtcode_config::NetworkAllowlistEntryConfig {
            domain: "api.github.com".to_string(),
            port: 443,
        }];

        let policy =
            sandbox_policy_from_runtime_config(&config, PathBuf::from("/tmp/ws").as_path())
                .unwrap();
        assert!(policy.has_network_allowlist());
        assert!(policy.is_network_allowed("api.github.com", 443));

        let command = vec!["curl".to_string(), "https://api.github.com".to_string()];
        enforce_sandbox_preflight_guards(&command, &policy, PathBuf::from(".").as_path())
            .expect("read-only allowlist network should permit network commands");
    }

    #[test]
    fn preflight_blocks_sensitive_path_arguments() {
        let policy = SandboxPolicy::workspace_write_with_sensitive_paths(
            vec![PathBuf::from("/tmp/ws")],
            vec![SensitivePath::new("/tmp/blocked")],
        );
        let command = vec!["cat".to_string(), "/tmp/blocked/secret.txt".to_string()];
        let error =
            enforce_sandbox_preflight_guards(&command, &policy, PathBuf::from(".").as_path())
                .expect_err("sensitive path should be denied");
        assert!(error.to_string().contains("sensitive path"));
    }

    #[test]
    fn preflight_blocks_writes_to_protected_workspace_metadata() {
        let policy = SandboxPolicy::workspace_write(vec![PathBuf::from("/tmp/ws")]);
        let command = vec![
            "touch".to_string(),
            "/tmp/ws/.vtcode/session.json".to_string(),
        ];
        let error =
            enforce_sandbox_preflight_guards(&command, &policy, PathBuf::from("/tmp/ws").as_path())
                .expect_err("protected workspace metadata should be denied");
        assert!(error.to_string().contains("blocked for writes"));
    }

    #[test]
    fn external_mode_is_rejected_for_local_pty_execution() {
        let config = vtcode_config::SandboxConfig {
            enabled: true,
            default_mode: vtcode_config::SandboxMode::External,
            ..Default::default()
        };

        let command = vec!["ls".to_string()];
        let requested = command.clone();
        let error = apply_runtime_sandbox_to_command(
            command,
            &requested,
            &config,
            PathBuf::from(".").as_path(),
            PathBuf::from(".").as_path(),
            SandboxPermissions::UseDefault,
            None,
        )
        .expect_err("external sandbox should not be allowed in local PTY flow");
        assert!(error.to_string().contains("not supported"));
    }

    #[test]
    fn additional_permissions_validation_requires_with_additional_permissions() {
        let payload = json!({
            "additional_permissions": {
                "fs_write": ["/tmp/demo.txt"]
            }
        });
        let obj = payload.as_object().expect("payload object");
        let err = parse_requested_sandbox_permissions(obj, PathBuf::from(".").as_path())
            .expect_err("additional_permissions without with_additional_permissions should fail");
        assert!(
            err.to_string()
                .contains("requires `sandbox_permissions` set to `with_additional_permissions`")
        );
    }

    #[test]
    fn empty_additional_permissions_are_ignored_for_default_sandbox_mode() {
        let payload = json!({
            "sandbox_permissions": "use_default",
            "additional_permissions": {
                "fs_read": [],
                "fs_write": []
            }
        });
        let obj = payload.as_object().expect("payload object");
        let (sandbox_permissions, additional_permissions) =
            parse_requested_sandbox_permissions(obj, PathBuf::from(".").as_path())
                .expect("empty permissions should be treated as absent");

        assert_eq!(sandbox_permissions, SandboxPermissions::UseDefault);
        assert!(additional_permissions.is_none());
    }

    #[test]
    fn with_additional_permissions_requires_non_empty_permissions() {
        let payload = json!({
            "sandbox_permissions": "with_additional_permissions",
            "additional_permissions": {
                "fs_read": [],
                "fs_write": []
            }
        });
        let obj = payload.as_object().expect("payload object");
        let err = parse_requested_sandbox_permissions(obj, PathBuf::from(".").as_path())
            .expect_err("empty additional_permissions should fail");
        assert!(err.to_string().contains("must include at least one path"));
    }

    #[test]
    fn with_additional_permissions_widens_read_only_for_write_roots() {
        let config = vtcode_config::SandboxConfig {
            enabled: true,
            default_mode: vtcode_config::SandboxMode::ReadOnly,
            ..Default::default()
        };

        let command = vec!["bash".to_string(), "-lc".to_string(), "echo hi".to_string()];
        let requested = command.clone();
        let extra_path = PathBuf::from("/tmp/extra-write-root");
        let additional_permissions = AdditionalPermissions {
            fs_read: Vec::new(),
            fs_write: vec![extra_path.clone()],
        };
        let transformed = apply_runtime_sandbox_to_command(
            command,
            &requested,
            &config,
            PathBuf::from("/tmp/ws").as_path(),
            PathBuf::from("/tmp/ws").as_path(),
            SandboxPermissions::WithAdditionalPermissions,
            Some(&additional_permissions),
        )
        .expect("sandbox transform should succeed");
        let needle = extra_path.to_string_lossy().to_string();

        assert!(
            transformed.iter().any(|arg| arg.contains(&needle)),
            "transformed sandbox command should include additional writable root"
        );
    }

    #[test]
    fn with_additional_permissions_preserves_read_only_network_access() {
        let base_policy = SandboxPolicy::read_only_with_full_network();
        let extra_path = PathBuf::from("/tmp/extra-write-root");
        let additional_permissions = AdditionalPermissions {
            fs_read: Vec::new(),
            fs_write: vec![extra_path.clone()],
        };

        let merged =
            sandbox_policy_with_additional_permissions(base_policy, &additional_permissions);

        assert!(merged.has_full_network_access());
        assert!(merged.is_path_writable(
            &extra_path.join("file.txt"),
            PathBuf::from("/tmp/ws").as_path()
        ));
    }

    #[test]
    fn with_additional_permissions_preserves_read_only_network_allowlist() {
        let base_policy =
            SandboxPolicy::read_only_with_network(vec![NetworkAllowlistEntry::https(
                "api.github.com",
            )]);
        let extra_path = PathBuf::from("/tmp/extra-write-root");
        let additional_permissions = AdditionalPermissions {
            fs_read: Vec::new(),
            fs_write: vec![extra_path.clone()],
        };

        let merged =
            sandbox_policy_with_additional_permissions(base_policy, &additional_permissions);

        assert!(merged.has_network_allowlist());
        assert!(merged.is_network_allowed("api.github.com", 443));
        assert!(merged.is_path_writable(
            &extra_path.join("file.txt"),
            PathBuf::from("/tmp/ws").as_path()
        ));
    }

    #[test]
    fn parse_command_parts_accepts_cmd_alias() {
        let payload = json!({
            "cmd": ["git", "status"],
            "args": ["--short"]
        });
        let payload = payload.as_object().expect("payload object");

        let (parts, raw_command) = parse_command_parts(payload, "missing command", "empty command")
            .expect("cmd alias should normalize");

        assert_eq!(parts, vec!["git", "status", "--short"]);
        assert!(raw_command.is_none());
    }

    #[test]
    fn parse_command_parts_accepts_raw_command_alias() {
        let payload = json!({
            "raw_command": "cargo check -p vtcode-core"
        });
        let payload = payload.as_object().expect("payload object");

        let (parts, raw_command) = parse_command_parts(payload, "missing command", "empty command")
            .expect("raw_command alias should normalize");

        assert_eq!(parts, vec!["cargo", "check", "-p", "vtcode-core"]);
        assert_eq!(raw_command.as_deref(), Some("cargo check -p vtcode-core"));
    }

    #[test]
    fn require_escalated_bypasses_runtime_sandbox_enforcement() {
        let config = vtcode_config::SandboxConfig {
            enabled: true,
            default_mode: vtcode_config::SandboxMode::External,
            ..Default::default()
        };

        let command = vec!["echo".to_string(), "hello".to_string()];
        let requested = command.clone();
        let out = apply_runtime_sandbox_to_command(
            command.clone(),
            &requested,
            &config,
            PathBuf::from(".").as_path(),
            PathBuf::from(".").as_path(),
            SandboxPermissions::RequireEscalated,
            None,
        )
        .expect("require_escalated should bypass sandbox transform");
        assert_eq!(out, command);
    }

    #[test]
    fn require_escalated_requires_non_empty_justification() {
        let payload = json!({
            "sandbox_permissions": "require_escalated"
        });
        let obj = payload.as_object().expect("payload object");
        let err = parse_requested_sandbox_permissions(obj, PathBuf::from(".").as_path())
            .expect_err("require_escalated without justification should fail");
        assert!(err.to_string().contains("missing `justification`"));

        let payload = json!({
            "sandbox_permissions": "require_escalated",
            "justification": "   "
        });
        let obj = payload.as_object().expect("payload object");
        let err = parse_requested_sandbox_permissions(obj, PathBuf::from(".").as_path())
            .expect_err("blank justification should fail");
        assert!(err.to_string().contains("missing `justification`"));
    }

    #[test]
    fn require_escalated_accepts_justified_request() {
        let payload = json!({
            "sandbox_permissions": "require_escalated",
            "justification": "Do you want to rerun this command without sandbox restrictions?"
        });
        let obj = payload.as_object().expect("payload object");
        let (sandbox_permissions, additional_permissions) =
            parse_requested_sandbox_permissions(obj, PathBuf::from(".").as_path())
                .expect("justified require_escalated should parse");
        assert_eq!(sandbox_permissions, SandboxPermissions::RequireEscalated);
        assert!(additional_permissions.is_none());
    }
}
