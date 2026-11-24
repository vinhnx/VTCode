use super::*;

#[test]
fn require_escalated_bypasses_runtime_sandbox_enforcement() {
    let config = vtcode_config::SandboxConfig {
        enabled: true,
        default_policy: vtcode_config::SandboxPolicy::External,
        ..Default::default()
    };

    let command = vec!["echo".to_string(), "hello".to_string()];
    let requested = command.clone();
    let plan = build_shell_execution_plan(
        &config,
        PathBuf::from(".").as_path(),
        PathBuf::from(".").as_path(),
        &requested,
        None,
        SandboxPermissions::RequireEscalated,
        None,
    )
    .expect("sandbox plan");
    let out = apply_runtime_sandbox_to_command(
        command.clone(),
        &requested,
        None,
        PathBuf::from(".").as_path(),
        plan.sandbox_policy,
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

#[cfg(unix)]
#[tokio::test]
async fn additional_permissions_reject_dangling_symlink() {
    let workspace = tempfile::tempdir().expect("workspace");
    let outside = tempfile::tempdir().expect("outside");
    let link = workspace.path().join("link");
    std::os::unix::fs::symlink(outside.path().join("missing"), &link).expect("symlink fixture");
    let payload = json!({ "additional_permissions": { "fs_write": [link] } });
    let error = parse_permissions(&payload, workspace.path(), workspace.path(), &Default::default())
        .await
        .expect_err("a dangling symlink must not be granted as a missing path");
    assert!(format!("{error:#}").contains("Failed to resolve canonical path"));
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

#[cfg(unix)]
#[tokio::test]
async fn additional_permissions_fail_closed_on_unresolvable_sensitive_root() {
    let workspace = tempfile::tempdir().expect("workspace");
    let protected_target = workspace.path().join("future-secret");
    let protected_link = workspace.path().join("protected");
    std::os::unix::fs::symlink(&protected_target, &protected_link).expect("symlink fixture");
    let payload = json!({ "additional_permissions": { "fs_write": [protected_target] } });
    let mut config = vtcode_config::SandboxConfig::default();
    config.sensitive_paths.use_defaults = false;
    config.sensitive_paths.additional = vec![protected_link.display().to_string()];

    let error = parse_permissions(&payload, workspace.path(), workspace.path(), &config)
        .await
        .expect_err("an unresolved protected root must prevent a grant");
    assert!(format!("{error:#}").contains("failed to resolve additional-permission root"));

    config.sensitive_paths.additional = vec![workspace.path().join("other-missing").display().to_string()];
    assert!(
        parse_permissions(&payload, workspace.path(), workspace.path(), &config)
            .await
            .is_ok()
    );
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
