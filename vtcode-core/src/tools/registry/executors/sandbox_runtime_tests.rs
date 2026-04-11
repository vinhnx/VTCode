use super::{
    apply_runtime_sandbox_to_command, build_shell_execution_plan, enforce_sandbox_preflight_guards,
    exec_run_output_config, parse_command_parts, parse_requested_sandbox_permissions,
    prepare_exec_command, sandbox_policy_from_runtime_config,
    sandbox_policy_with_additional_permissions,
};
use crate::sandboxing::{
    AdditionalPermissions, NetworkAllowlistEntry, SandboxPermissions, SandboxPolicy, SensitivePath,
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
        sandbox_policy_from_runtime_config(&config, PathBuf::from("/tmp/ws").as_path()).unwrap();
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
    let error = enforce_sandbox_preflight_guards(&command, &policy, PathBuf::from(".").as_path())
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
        sandbox_policy_from_runtime_config(&config, PathBuf::from("/tmp/ws").as_path()).unwrap();
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
        sandbox_policy_from_runtime_config(&config, PathBuf::from("/tmp/ws").as_path()).unwrap();
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
        sandbox_policy_from_runtime_config(&config, PathBuf::from("/tmp/ws").as_path()).unwrap();
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
    assert!(err.to_string().contains("missing `additional_permissions`"));
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

    let merged = sandbox_policy_with_additional_permissions(base_policy, &additional_permissions);

    assert!(merged.has_full_network_access());
    assert!(merged.is_path_writable(
        &extra_path.join("file.txt"),
        PathBuf::from("/tmp/ws").as_path()
    ));
}

#[test]
fn with_additional_permissions_preserves_read_only_network_allowlist() {
    let base_policy =
        SandboxPolicy::read_only_with_network(vec![NetworkAllowlistEntry::https("api.github.com")]);
    let extra_path = PathBuf::from("/tmp/extra-write-root");
    let additional_permissions = AdditionalPermissions {
        fs_read: Vec::new(),
        fs_write: vec![extra_path.clone()],
    };

    let merged = sandbox_policy_with_additional_permissions(base_policy, &additional_permissions);

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

    let prepared = prepare_exec_command(
        payload,
        "/bin/zsh",
        true,
        command.clone(),
        Some("echo hi".into()),
    );

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
    let command = vec![
        "/bin/zsh".to_string(),
        "-c".to_string(),
        "echo hi".to_string(),
    ];

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
