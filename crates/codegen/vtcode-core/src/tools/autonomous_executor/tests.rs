use super::*;
use serde_json::json;

#[test]
fn test_readonly_tools_auto_execute() {
    let executor = AutonomousExecutor::new();

    assert_eq!(
        executor.get_policy(tools::CODE_SEARCH, &json!({"query": "Widget", "path": "src"})),
        AutonomousPolicy::AutoExecute
    );
    assert_eq!(
        executor.get_policy(tools::UNIFIED_FILE, &json!({"action": "read", "path": "README.md"})),
        AutonomousPolicy::AutoExecute
    );
    assert_eq!(
        executor.get_policy(tools::UNIFIED_EXEC, &json!({"action": "poll", "session_id": "run-1"})),
        AutonomousPolicy::AutoExecute
    );
    assert_eq!(
        executor.get_policy(tools::UNIFIED_EXEC, &json!({"action": "continue", "session_id": "run-1"})),
        AutonomousPolicy::AutoExecute
    );
}

#[test]
fn test_destructive_commands_require_confirmation() {
    let executor = AutonomousExecutor::new();

    let destructive_cmds = vec![
        "rm -rf /tmp/test",
        "git reset --hard HEAD~1",
        "git push --force origin main",
        "git clean -fdx",
        "chmod -R 777 /",
    ];

    for cmd in destructive_cmds {
        let args = json!({"command": cmd});
        let policy = executor.get_policy("shell", &args);
        assert_eq!(policy, AutonomousPolicy::RequireConfirmation, "unexpected policy for command: {cmd}");
    }
}

#[test]
fn test_verification_tools_need_preview() {
    let executor = AutonomousExecutor::new();

    for tool in VERIFICATION_REQUIRED_TOOLS {
        let policy = executor.get_policy(tool, &json!({}));
        assert_eq!(policy, AutonomousPolicy::VerifyThenExecute);
    }
}

#[test]
fn test_unified_tools_use_action_specific_policies() {
    let executor = AutonomousExecutor::new();

    assert_eq!(
        executor.get_policy(tools::UNIFIED_FILE, &json!({"action": "write", "path": "foo.txt", "content": "hello"})),
        AutonomousPolicy::VerifyThenExecute
    );
    assert_eq!(
        executor
            .get_policy(tools::UNIFIED_FILE, &json!({"action": "patch", "input": "*** Begin Patch\n*** End Patch"})),
        AutonomousPolicy::RequireConfirmation
    );
    assert_eq!(
        executor.get_policy(tools::UNIFIED_EXEC, &json!({"cmd": "cargo build"})),
        AutonomousPolicy::VerifyThenExecute
    );
    assert_eq!(executor.get_policy(tools::UNIFIED_EXEC, &json!({"cmd": "echo hi"})), AutonomousPolicy::AutoExecute);
    assert_eq!(
        executor.get_policy(
            tools::UNIFIED_EXEC,
            &json!({"action": "write", "session_id": "run-1", "input": "rm -rf /tmp/test"})
        ),
        AutonomousPolicy::RequireConfirmation
    );
}

#[test]
fn test_exec_aliases_use_command_session_preview_policy() {
    let executor = AutonomousExecutor::new();

    assert_eq!(
        executor.get_policy(tools::EXEC_COMMAND, &json!({"cmd": "cargo build"})),
        AutonomousPolicy::VerifyThenExecute
    );
    assert_eq!(
        executor.get_policy(tools::RUN_PTY_CMD, &json!({"command": "cargo build"})),
        AutonomousPolicy::VerifyThenExecute
    );
}

#[test]
fn test_loop_detection_integration() {
    let executor = AutonomousExecutor::new();
    let args = json!({"query": "Widget", "path": "src/"});

    // First two calls should not block
    assert!(executor.should_block(tools::CODE_SEARCH, &args).is_none());
    executor.record_tool_call(tools::CODE_SEARCH, &args);

    assert!(executor.should_block(tools::CODE_SEARCH, &args).is_none());
    executor.record_tool_call(tools::CODE_SEARCH, &args);

    // Third call should trigger warning
    executor.record_tool_call(tools::CODE_SEARCH, &args);
    let block_msg = executor.should_block(tools::CODE_SEARCH, &args);
    assert!(block_msg.is_some());
    let message = block_msg.unwrap();
    assert!(
        message.contains("alternative") || message.contains("blocked"),
        "unexpected loop warning message: {message}"
    );
}

#[test]
fn test_turn_reset_clears_loop_detection_state() {
    let executor = AutonomousExecutor::new();
    let args = json!({"query": "Widget", "path": "src/"});

    executor.record_tool_call(tools::CODE_SEARCH, &args);
    executor.record_tool_call(tools::CODE_SEARCH, &args);
    executor.record_tool_call(tools::CODE_SEARCH, &args);
    assert!(executor.should_block(tools::CODE_SEARCH, &args).is_some());

    executor.reset_turn_loop_detection();
    assert!(executor.should_block(tools::CODE_SEARCH, &args).is_none());
}

#[test]
fn test_execution_stats_tracking() {
    let executor = AutonomousExecutor::new();

    // Record some executions
    executor.record_execution(tools::CODE_SEARCH, true);
    executor.record_execution(tools::CODE_SEARCH, true);
    executor.record_execution(tools::CODE_SEARCH, false);

    // Check stats
    let (total, success, failed) = executor.get_tool_stats(tools::CODE_SEARCH).unwrap();
    assert_eq!(total, 3);
    assert_eq!(success, 2);
    assert_eq!(failed, 1);

    // Check success rate
    let rate = executor.get_success_rate(tools::CODE_SEARCH);
    assert!((rate - 0.666).abs() < 0.01);
}

#[test]
fn test_workspace_boundary_validation() {
    let mut executor = AutonomousExecutor::new();
    // A dedicated workspace, not bare temp_dir(): on Linux temp_dir() is /tmp,
    // which would legitimately contain the prefix-sibling assertions below.
    let workspace = tempfile::tempdir().expect("workspace dir");
    executor.set_workspace_dir(workspace.path().to_path_buf());

    // Absolute path outside workspace should fail
    let args = json!({"path": "/etc/passwd"});
    let result = executor.validate_args(tools::WRITE_FILE, &args);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("workspace boundary") || err.contains("Path traversal") || err.contains("system directory"),
        "{err}"
    );

    // /tmp/vtcode should be allowed
    let args = json!({"path": "/tmp/vtcode/test.txt"});
    let result = executor.validate_args(tools::WRITE_FILE, &args);
    result.unwrap();

    // A sibling with the same string prefix must not inherit the exception.
    for sibling in ["/tmp/vtcode-evil/secret.txt", "/tmp/vtcode_backdoor/key.pem"] {
        let args = json!({"path": sibling});
        let result = executor.validate_args(tools::WRITE_FILE, &args);
        assert!(result.is_err(), "prefix-sibling {sibling} must be rejected");
    }
}

#[test]
fn test_command_session_validation_uses_command_aliases() {
    let executor = AutonomousExecutor::new();

    let err = executor
        .validate_args(tools::UNIFIED_EXEC, &json!({"cmd": "rm -rf /tmp/test"}))
        .expect_err("destructive command should fail");

    assert!(err.to_string().contains("requires explicit confirmation"));
}

#[test]
fn test_file_operation_validation_checks_destinations() {
    let mut executor = AutonomousExecutor::new();
    executor.set_workspace_dir(PathBuf::from("/workspace"));

    let err = executor
        .validate_args(
            tools::UNIFIED_FILE,
            &json!({
                "action": "move",
                "path": "src/main.rs",
                "destination": "/etc/passwd"
            }),
        )
        .expect_err("destination outside workspace should fail");

    let err = err.to_string();
    assert!(
        err.contains("workspace boundary") || err.contains("Path traversal") || err.contains("system directory"),
        "{err}"
    );
}

#[test]
fn test_enhanced_destructive_patterns() {
    let executor = AutonomousExecutor::new();

    let destructive_cmds = vec![
        "rm -r somedir",
        "git branch -D feature",
        "npm uninstall -g package",
        "cargo uninstall tool",
    ];

    for cmd in destructive_cmds {
        assert!(executor.is_destructive_command(cmd));
    }
}

#[test]
fn test_enhanced_preview_generation() {
    let executor = AutonomousExecutor::new();

    // Test write_file preview
    let args = json!({
        "path": "test.rs",
        "content": "line1\nline2\nline3"
    });
    let preview = executor.generate_preview(tools::WRITE_FILE, &args);
    assert!(preview.contains("3 lines"));
    assert!(preview.contains("test.rs"));

    // Test edit_file preview
    let args = json!({
        "path": "main.rs",
        "old_str": "old code",
        "new_str": "new code"
    });
    let preview = executor.generate_preview(tools::EDIT_FILE, &args);
    assert!(preview.contains("main.rs"));
    assert!(preview.contains("old code"));
    assert!(preview.contains("new code"));

    // Test destructive command preview
    let args = json!({"command": "rm -rf /tmp/test"});
    let preview = executor.generate_preview("shell", &args);
    assert!(preview.contains("WARNING"));
    assert!(preview.contains("destructive"));

    let preview = executor.generate_preview(tools::EXEC_COMMAND, &json!({"cmd": "git status"}));
    assert!(preview.contains("git status"));

    let preview = executor.generate_preview(
        tools::APPLY_PATCH,
        &json!({
            "input": "*** Begin Patch\n*** Add File: note.txt\n+hello\n*** End Patch"
        }),
    );
    assert!(preview.contains("apply patch"));
}

#[test]
fn test_parent_traversal_detection() {
    let mut executor = AutonomousExecutor::new();
    let workspace = PathBuf::from("/workspace");
    executor.set_workspace_dir(workspace);

    // Parent traversal is rejected before canonical workspace checks.
    let args = json!({"path": "src/../lib/file.rs"});
    let result = executor.validate_args(tools::WRITE_FILE, &args);
    let err = result.expect_err("parent traversal should fail").to_string();
    assert!(err.contains("Path traversal"), "{err}");
}
