use std::path::{Path, PathBuf};

use serde_json::json;

use super::{
    apply_runtime_sandbox_to_command, build_shell_execution_plan, enforce_sandbox_preflight_guards,
    exec_run_output_config, parse_command_parts, parse_requested_sandbox_permissions, prepare_exec_command,
    sandbox_policy_from_runtime_config, sandbox_policy_with_additional_permissions,
};
use crate::sandboxing::{
    AdditionalPermissions, NetworkAllowlistEntry, SandboxPermissions, SandboxPolicy, SensitivePath,
};

async fn parse_permissions(
    payload: &serde_json::Value,
    workspace_root: &Path,
    cwd: &Path,
    config: &vtcode_config::SandboxConfig,
) -> anyhow::Result<(SandboxPermissions, Option<AdditionalPermissions>)> {
    let obj = payload.as_object().expect("payload object");
    parse_requested_sandbox_permissions(obj, workspace_root, cwd, config).await
}

#[test]
fn runtime_sandbox_config_maps_workspace_policy_overrides() {
    let mut config = vtcode_config::SandboxConfig {
        enabled: true,
        default_policy: vtcode_config::SandboxPolicy::WorkspaceWrite,
        ..Default::default()
    };
    config.network.policy = vtcode_config::NetworkPolicy::AllowlistOnly;
    config.network.allowlist =
        vec![vtcode_config::NetworkAllowlistEntryConfig { domain: "api.github.com".to_string(), port: 443 }];
    config.sensitive_paths.use_defaults = false;
    config.sensitive_paths.additional = vec!["/tmp/secret".to_string()];
    config.resource_limits.preset = vtcode_config::ResourceLimitsPreset::Conservative;
    config.resource_limits.max_memory_mb = 2048;
    config.seccomp.profile = vtcode_config::SeccompProfilePreset::Strict;

    let policy = sandbox_policy_from_runtime_config(&config, PathBuf::from("/tmp/ws").as_path()).unwrap();
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
        default_policy: vtcode_config::SandboxPolicy::ReadOnly,
        ..Default::default()
    };

    let command = vec!["cargo".to_string(), "fmt".to_string()];
    let plan = build_shell_execution_plan(
        &config,
        PathBuf::from("/tmp/ws").as_path(),
        PathBuf::from("/tmp/ws").as_path(),
        &command,
        SandboxPermissions::UseDefault,
        None,
    )
    .unwrap();
    assert!(plan.approval_reason.is_some());
    assert!(matches!(plan.sandbox_policy, Some(SandboxPolicy::WorkspaceWrite { .. })));
}

#[test]
fn read_only_non_mutating_command_stays_read_only_without_prompt() {
    let config = vtcode_config::SandboxConfig {
        enabled: true,
        default_policy: vtcode_config::SandboxPolicy::ReadOnly,
        ..Default::default()
    };

    let command = vec!["ls".to_string(), "-la".to_string()];
    let plan = build_shell_execution_plan(
        &config,
        PathBuf::from("/tmp/ws").as_path(),
        PathBuf::from("/tmp/ws").as_path(),
        &command,
        SandboxPermissions::UseDefault,
        None,
    )
    .unwrap();
    assert!(plan.approval_reason.is_none());
    assert!(matches!(plan.sandbox_policy, Some(SandboxPolicy::ReadOnly { .. })));
}

#[test]
fn recursive_rm_of_mount_layout_requests_approval() {
    let config = vtcode_config::SandboxConfig {
        enabled: true,
        default_policy: vtcode_config::SandboxPolicy::DangerFullAccess,
        ..Default::default()
    };
    let command = vec!["rm".to_string(), "--recursive".to_string(), "Documents".to_string()];

    let plan = build_shell_execution_plan(
        &config,
        Path::new("/tmp/workspace"),
        Path::new("/mnt/c"),
        &command,
        SandboxPermissions::UseDefault,
        None,
    )
    .unwrap();

    assert!(plan.sandbox_policy.is_none());
    assert!(
        plan.approval_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("mounted filesystem"))
    );
}

#[test]
fn recursive_rm_after_shell_directory_change_requests_approval() {
    let config = vtcode_config::SandboxConfig {
        enabled: true,
        default_policy: vtcode_config::SandboxPolicy::DangerFullAccess,
        ..Default::default()
    };
    let command = vec![
        "bash".to_string(),
        "-lc".to_string(),
        "cd /Volumes/Backup && rm --recursive Photos".to_string(),
    ];
    let plan = build_shell_execution_plan(
        &config,
        Path::new("/tmp/workspace"),
        Path::new("/tmp/workspace"),
        &command,
        SandboxPermissions::UseDefault,
        None,
    )
    .unwrap();

    assert!(plan.sandbox_policy.is_none());
    assert!(
        plan.approval_reason
            .as_deref()
            .is_some_and(|reason| reason.contains("mounted filesystem"))
    );
}

#[test]
fn inline_interpreter_code_runs_under_restrictive_sandbox_without_prompt() {
    let config = vtcode_config::SandboxConfig {
        enabled: true,
        default_policy: vtcode_config::SandboxPolicy::ReadOnly,
        ..Default::default()
    };
    let command = vec!["python3".to_string(), "-c".to_string(), "print('ok')".to_string()];

    let plan = build_shell_execution_plan(
        &config,
        PathBuf::from("/tmp/ws").as_path(),
        PathBuf::from("/tmp/ws").as_path(),
        &command,
        SandboxPermissions::UseDefault,
        None,
    )
    .unwrap();

    assert!(plan.approval_reason.is_none());
    assert!(matches!(plan.sandbox_policy, Some(SandboxPolicy::ReadOnly { .. })));
}

#[test]
fn inline_interpreter_code_requires_approval_without_enforceable_sandbox() {
    for config in [
        vtcode_config::SandboxConfig { enabled: false, ..Default::default() },
        vtcode_config::SandboxConfig {
            enabled: true,
            default_policy: vtcode_config::SandboxPolicy::DangerFullAccess,
            ..Default::default()
        },
    ] {
        let command = vec!["node".to_string(), "-e".to_string(), "console.log('ok')".to_string()];
        let plan = build_shell_execution_plan(
            &config,
            PathBuf::from("/tmp/ws").as_path(),
            PathBuf::from("/tmp/ws").as_path(),
            &command,
            SandboxPermissions::UseDefault,
            None,
        )
        .unwrap();

        assert!(plan.sandbox_policy.is_none());
        assert!(
            plan.approval_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("Inline interpreter code"))
        );
    }
}

#[test]
fn preflight_blocks_network_commands_when_network_disabled() {
    let policy = SandboxPolicy::workspace_write(vec![PathBuf::from("/tmp/ws")]);
    let command = vec!["curl".to_string(), "https://example.com".to_string()];
    let error = enforce_sandbox_preflight_guards(&command, &policy, PathBuf::from(".").as_path())
        .expect_err("network command should be denied");
    assert!(error.to_string().contains("denies network"));
}

#[test]
fn workspace_write_allow_all_network_is_not_blocked() {
    let mut config = vtcode_config::SandboxConfig {
        enabled: true,
        default_policy: vtcode_config::SandboxPolicy::WorkspaceWrite,
        ..Default::default()
    };
    config.network.policy = vtcode_config::NetworkPolicy::AllowAll;

    let policy = sandbox_policy_from_runtime_config(&config, PathBuf::from("/tmp/ws").as_path()).unwrap();
    assert!(policy.has_full_network_access());

    let command = vec!["curl".to_string(), "https://example.com".to_string()];
    enforce_sandbox_preflight_guards(&command, &policy, PathBuf::from(".").as_path())
        .expect("allow_all network should permit network commands");
}

#[test]
fn read_only_allow_all_network_is_not_blocked() {
    let mut config = vtcode_config::SandboxConfig {
        enabled: true,
        default_policy: vtcode_config::SandboxPolicy::ReadOnly,
        ..Default::default()
    };
    config.network.policy = vtcode_config::NetworkPolicy::AllowAll;

    let policy = sandbox_policy_from_runtime_config(&config, PathBuf::from("/tmp/ws").as_path()).unwrap();
    assert!(policy.has_full_network_access());

    let command = vec!["curl".to_string(), "https://example.com".to_string()];
    enforce_sandbox_preflight_guards(&command, &policy, PathBuf::from(".").as_path())
        .expect("read-only allow_all network should permit network commands");
}

#[test]
fn read_only_allowlist_network_is_not_blocked() {
    let mut config = vtcode_config::SandboxConfig {
        enabled: true,
        default_policy: vtcode_config::SandboxPolicy::ReadOnly,
        ..Default::default()
    };
    config.network.policy = vtcode_config::NetworkPolicy::AllowlistOnly;
    config.network.allowlist =
        vec![vtcode_config::NetworkAllowlistEntryConfig { domain: "api.github.com".to_string(), port: 443 }];

    let policy = sandbox_policy_from_runtime_config(&config, PathBuf::from("/tmp/ws").as_path()).unwrap();
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
    let error = enforce_sandbox_preflight_guards(&command, &policy, PathBuf::from(".").as_path())
        .expect_err("sensitive path should be denied");
    assert!(error.to_string().contains("sensitive path"));
}

#[test]
fn preflight_blocks_writes_to_protected_workspace_metadata() {
    let policy = SandboxPolicy::workspace_write(vec![PathBuf::from("/tmp/ws")]);
    let command = vec!["touch".to_string(), "/tmp/ws/.vtcode/session.json".to_string()];
    let error = enforce_sandbox_preflight_guards(&command, &policy, PathBuf::from("/tmp/ws").as_path())
        .expect_err("protected workspace metadata should be denied");
    assert!(error.to_string().contains("blocked for writes"));
}

#[test]
fn external_mode_is_rejected_for_local_pty_execution() {
    let config = vtcode_config::SandboxConfig {
        enabled: true,
        default_policy: vtcode_config::SandboxPolicy::External,
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

#[tokio::test]
async fn additional_permissions_without_mode_normalize_to_with_additional_permissions() {
    let demo_path = std::env::temp_dir().join("demo.txt");
    let payload = json!({
        "additional_permissions": {
            "fs_write": [demo_path]
        }
    });
    let config = vtcode_config::SandboxConfig::default();

    let (sandbox_permissions, additional_permissions) =
        parse_permissions(&payload, Path::new("."), Path::new("."), &config)
            .await
            .expect("non-empty additional_permissions should imply with_additional_permissions");

    assert_eq!(sandbox_permissions, SandboxPermissions::WithAdditionalPermissions);
    assert_eq!(
        additional_permissions.expect("normalized permissions").fs_write,
        vec![
            crate::utils::path::canonicalize_allow_missing(&std::env::temp_dir().join("demo.txt"))
                .await
                .expect("canonical temp permission")
        ]
    );
}

#[tokio::test]
async fn explicit_use_default_with_additional_permissions_normalizes_to_with_additional_permissions() {
    let demo_path = std::env::temp_dir().join("demo.txt");
    let payload = json!({
        "sandbox_permissions": "use_default",
        "additional_permissions": {
            "fs_write": [demo_path]
        }
    });
    let config = vtcode_config::SandboxConfig::default();

    let (sandbox_permissions, additional_permissions) =
        parse_permissions(&payload, Path::new("."), Path::new("."), &config)
            .await
            .expect("use_default plus additional permissions should normalize");

    assert_eq!(sandbox_permissions, SandboxPermissions::WithAdditionalPermissions);
    assert_eq!(
        additional_permissions.expect("normalized permissions").fs_write,
        vec![
            crate::utils::path::canonicalize_allow_missing(&std::env::temp_dir().join("demo.txt"))
                .await
                .expect("canonical temp permission")
        ]
    );
}

#[tokio::test]
async fn empty_additional_permissions_are_ignored_for_default_sandbox_policy() {
    let payload = json!({
        "sandbox_permissions": "use_default",
        "additional_permissions": {
            "fs_read": [],
            "fs_write": []
        }
    });
    let config = vtcode_config::SandboxConfig::default();
    let (sandbox_permissions, additional_permissions) =
        parse_permissions(&payload, Path::new("."), Path::new("."), &config)
            .await
            .expect("empty permissions should be treated as absent");

    assert_eq!(sandbox_permissions, SandboxPermissions::UseDefault);
    assert!(additional_permissions.is_none());
}

#[tokio::test]
async fn with_additional_permissions_requires_non_empty_permissions() {
    let payload = json!({
        "sandbox_permissions": "with_additional_permissions",
        "additional_permissions": {
            "fs_read": [],
            "fs_write": []
        }
    });
    let config = vtcode_config::SandboxConfig::default();
    let err = parse_permissions(&payload, Path::new("."), Path::new("."), &config)
        .await
        .expect_err("empty additional_permissions should fail");
    assert!(err.to_string().contains("missing `additional_permissions`"));
}

#[test]
#[serial_test::serial(linux_sandbox_executable)]
fn with_additional_permissions_widens_read_only_for_write_roots() {
    #[cfg(target_os = "linux")]
    let (_temp_dir, _sandbox_helper_guard) = {
        let temp_dir = tempfile::tempdir().expect("temp sandbox helper dir");
        let helper = temp_dir.path().join("vtcode-linux-sandbox");
        std::fs::write(&helper, "").expect("write fake sandbox helper");
        let guard = super::set_linux_sandbox_executable_override_for_tests(helper);
        (temp_dir, guard)
    };

    let config = vtcode_config::SandboxConfig {
        enabled: true,
        default_policy: vtcode_config::SandboxPolicy::ReadOnly,
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

    let merged = sandbox_policy_with_additional_permissions(base_policy, &additional_permissions);

    assert!(merged.has_full_network_access());
    assert!(merged.is_path_writable(&extra_path.join("file.txt"), PathBuf::from("/tmp/ws").as_path()));
}

#[test]
fn with_additional_permissions_preserves_read_only_network_allowlist() {
    let base_policy = SandboxPolicy::read_only_with_network(vec![NetworkAllowlistEntry::https("api.github.com")]);
    let extra_path = PathBuf::from("/tmp/extra-write-root");
    let additional_permissions = AdditionalPermissions {
        fs_read: Vec::new(),
        fs_write: vec![extra_path.clone()],
    };

    let merged = sandbox_policy_with_additional_permissions(base_policy, &additional_permissions);

    assert!(merged.has_network_allowlist());
    assert!(merged.is_network_allowed("api.github.com", 443));
    assert!(merged.is_path_writable(&extra_path.join("file.txt"), PathBuf::from("/tmp/ws").as_path()));
}

#[test]
fn parse_command_parts_accepts_cmd_alias() {
    let payload = json!({
        "cmd": ["git", "status"],
        "args": ["--short"]
    });
    let payload = payload.as_object().expect("payload object");

    let (parts, raw_command) =
        parse_command_parts(payload, "missing command", "empty command").expect("cmd alias should normalize");

    assert_eq!(parts, vec!["git", "status", "--short"]);
    assert!(raw_command.is_none());
}

#[test]
fn parse_command_parts_rejects_non_string_args() {
    // Non-string args elements must fail loudly, not be silently dropped.
    // Silently dropping arguments could change command semantics (turn_902 class).
    let payload = json!({
        "command": ["echo"],
        "args": ["hello", 42]
    });
    let payload = payload.as_object().expect("payload object");

    let result = parse_command_parts(payload, "missing command", "empty command");
    assert!(result.is_err(), "non-string args elements must be rejected, not silently dropped");
}

#[test]
fn parse_command_parts_rejects_non_array_args() {
    let payload = json!({
        "command": ["echo"],
        "args": "not-an-array"
    });
    let payload = payload.as_object().expect("payload object");

    let result = parse_command_parts(payload, "missing command", "empty command");
    assert!(result.is_err(), "non-array args must be rejected, not silently ignored");
}

#[test]
fn parse_command_parts_accepts_raw_command_alias() {
    let payload = json!({
        "raw_command": "cargo check -p vtcode-core"
    });
    let payload = payload.as_object().expect("payload object");

    let (parts, raw_command) =
        parse_command_parts(payload, "missing command", "empty command").expect("raw_command alias should normalize");

    assert_eq!(parts, vec!["cargo", "check", "-p", "vtcode-core"]);
    assert_eq!(raw_command.as_deref(), Some("cargo check -p vtcode-core"));
}

#[test]
fn exec_run_output_config_trims_query_and_uses_command_hint() {
    let payload = json!({
        "query": "  error  ",
        "literal": true,
        "max_matches": 12
    });
    let payload = payload.as_object().expect("payload object");

    let config = exec_run_output_config(payload, "cat Cargo.toml");

    assert_eq!(config.max_tokens, 250);
    assert_eq!(config.inspect_query.as_deref(), Some("error"));
    assert!(config.inspect_literal);
    assert_eq!(config.inspect_max_matches, 12);
}

#[test]
fn exec_run_output_config_prefers_explicit_max_tokens() {
    let payload = json!({
        "max_tokens": 42,
        "query": "   "
    });
    let payload = payload.as_object().expect("payload object");

    let config = exec_run_output_config(payload, "cat Cargo.toml");

    assert_eq!(config.max_tokens, 42);
    assert!(config.inspect_query.is_none());
    assert!(!config.inspect_literal);
}

#[test]
fn prepare_exec_command_wraps_when_shell_is_missing() {
    let payload = json!({
        "raw_command": "echo hi && pwd"
    });
    let payload = payload.as_object().expect("payload object");
    let command = vec!["echo".to_string(), "hi".to_string()];

    let prepared = prepare_exec_command(payload, "/bin/zsh", true, command.clone(), Some("echo hi".into()));

    assert_eq!(prepared.requested_command, command);
    assert_eq!(
        prepared.command,
        vec![
            "/bin/zsh".to_string(),
            "-l".to_string(),
            "-c".to_string(),
            "echo hi && pwd".to_string()
        ]
    );
    assert_eq!(prepared.requested_command_display, "echo hi");
    assert!(prepared.display_command.starts_with("/bin/zsh -l -c"));
}

#[test]
fn prepare_exec_command_keeps_existing_shell_invocation() {
    let payload = json!({});
    let payload = payload.as_object().expect("payload object");
    let command = vec!["/bin/zsh".to_string(), "-c".to_string(), "echo hi".to_string()];

    let prepared = prepare_exec_command(payload, "/bin/zsh", true, command.clone(), None);

    assert_eq!(prepared.requested_command, command);
    assert_eq!(prepared.command, command);
    assert_eq!(prepared.requested_command_display, "/bin/zsh -c 'echo hi'");
    assert_eq!(prepared.display_command, "/bin/zsh -c 'echo hi'");
}

#[test]
fn require_escalated_bypasses_runtime_sandbox_enforcement() {
    let config = vtcode_config::SandboxConfig {
        enabled: true,
        default_policy: vtcode_config::SandboxPolicy::External,
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

#[tokio::test]
async fn additional_permissions_reject_parent_traversal() {
    let workspace = tempfile::tempdir().expect("workspace");
    let payload = json!({
        "additional_permissions": {
            "fs_write": ["../escape"]
        }
    });
    let config = vtcode_config::SandboxConfig::default();

    let err = parse_permissions(&payload, workspace.path(), workspace.path(), &config)
        .await
        .expect_err("parent traversal should fail");
    assert!(err.to_string().contains("must not contain '..' traversal"));
}

#[cfg(unix)]
#[tokio::test]
async fn additional_permissions_reject_symlink_escape() {
    use std::os::unix::fs::symlink;

    let workspace = tempfile::tempdir().expect("workspace");
    let outside = tempfile::tempdir().expect("outside");
    symlink(outside.path(), workspace.path().join("escape")).expect("symlink fixture");

    let payload = json!({
        "additional_permissions": {
            "fs_write": ["escape/output.txt"]
        }
    });
    let config = vtcode_config::SandboxConfig::default();

    let err = parse_permissions(&payload, workspace.path(), workspace.path(), &config)
        .await
        .expect_err("symlink escape should fail");
    assert!(err.to_string().contains("symlink resolution"), "unexpected error: {err:#}");
}

#[tokio::test]
async fn additional_permissions_reject_sensitive_paths() {
    let sensitive_root = tempfile::tempdir().expect("sensitive root");
    let payload = json!({
        "additional_permissions": {
            "fs_read": [sensitive_root.path()]
        }
    });
    let mut config = vtcode_config::SandboxConfig::default();
    config.sensitive_paths.use_defaults = false;
    config.sensitive_paths.additional = vec![sensitive_root.path().display().to_string()];

    let err = parse_permissions(&payload, Path::new("."), Path::new("."), &config)
        .await
        .expect_err("sensitive roots should fail");
    assert!(err.to_string().contains("sensitive or protected location"));
}

#[tokio::test]
async fn additional_permissions_reject_paths_outside_allowed_roots() {
    let payload = json!({
        "additional_permissions": {
            "fs_write": ["/etc/passwd"]
        }
    });
    let config = vtcode_config::SandboxConfig::default();

    let err = parse_permissions(&payload, Path::new("."), Path::new("."), &config)
        .await
        .expect_err("non-workspace, non-temp paths should fail");
    assert!(err.to_string().contains("outside the allowed workspace and temp roots"));
}

#[tokio::test]
async fn require_escalated_requires_non_empty_justification() {
    let payload = json!({
        "sandbox_permissions": "require_escalated"
    });
    let config = vtcode_config::SandboxConfig::default();
    let err = parse_permissions(&payload, Path::new("."), Path::new("."), &config)
        .await
        .expect_err("require_escalated without justification should fail");
    assert!(err.to_string().contains("missing `justification`"));

    let payload = json!({
        "sandbox_permissions": "require_escalated",
        "justification": "   "
    });
    let err = parse_permissions(&payload, Path::new("."), Path::new("."), &config)
        .await
        .expect_err("blank justification should fail");
    assert!(err.to_string().contains("missing `justification`"));
}

#[tokio::test]
async fn bypass_sandbox_requires_justification_and_rejects_additional_permissions() {
    let config = vtcode_config::SandboxConfig::default();
    let missing_justification = json!({
        "sandbox_permissions": "bypass_sandbox"
    });
    let err = parse_permissions(&missing_justification, Path::new("."), Path::new("."), &config)
        .await
        .expect_err("bypass_sandbox without justification should fail");
    assert!(err.to_string().contains("missing `justification`"));

    let conflicting = json!({
        "sandbox_permissions": "bypass_sandbox",
        "justification": "Do you want to bypass the sandbox for this command?",
        "additional_permissions": {
            "fs_write": ["/tmp/demo.txt"]
        }
    });
    let err = parse_permissions(&conflicting, Path::new("."), Path::new("."), &config)
        .await
        .expect_err("bypass_sandbox plus additional permissions should fail");
    assert!(
        err.to_string()
            .contains("requires `sandbox_permissions` set to `with_additional_permissions`")
    );
}

#[tokio::test]
async fn require_escalated_rejects_additional_permissions_even_with_justification() {
    let payload = json!({
        "sandbox_permissions": "require_escalated",
        "justification": "Do you want to rerun this command without sandbox restrictions?",
        "additional_permissions": {
            "fs_write": ["/tmp/demo.txt"]
        }
    });
    let config = vtcode_config::SandboxConfig::default();

    let err = parse_permissions(&payload, Path::new("."), Path::new("."), &config)
        .await
        .expect_err("escalated requests must not combine additional permissions");
    assert!(
        err.to_string()
            .contains("requires `sandbox_permissions` set to `with_additional_permissions`")
    );
}

#[tokio::test]
async fn require_escalated_accepts_justified_request() {
    let payload = json!({
        "sandbox_permissions": "require_escalated",
        "justification": "Do you want to rerun this command without sandbox restrictions?"
    });
    let (sandbox_permissions, additional_permissions) =
        parse_permissions(&payload, Path::new("."), Path::new("."), &vtcode_config::SandboxConfig::default())
            .await
            .expect("justified require_escalated should parse");
    assert_eq!(sandbox_permissions, SandboxPermissions::RequireEscalated);
    assert!(additional_permissions.is_none());
}
