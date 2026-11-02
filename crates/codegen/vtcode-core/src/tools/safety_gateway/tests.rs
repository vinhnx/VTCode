use super::*;
use vtcode_config::core::DotfileProtectionConfig;

fn make_ctx() -> SafetyContext {
    SafetyContext::new("test-session")
}

#[tokio::test]
async fn test_allow_read_only_tools() {
    let gateway = SafetyGateway::new();
    let ctx = make_ctx();

    let decision = gateway
        .check_safety(&ctx, "read_file", &serde_json::json!({"path": "/tmp/test"}))
        .await;

    assert!(decision.is_allowed());
}

#[tokio::test]
async fn test_destructive_tool_needs_approval() {
    let gateway = SafetyGateway::new();
    let ctx = make_ctx();

    let decision = gateway
        .check_safety(&ctx, "delete_file", &serde_json::json!({"path": "/tmp/test"}))
        .await;

    assert!(decision.needs_approval());
}

#[tokio::test]
async fn test_planning_workflow_does_not_double_block_mutating() {
    let gateway = SafetyGateway::new();
    gateway.set_planning_workflow(true);
    let ctx = make_ctx();

    let decision = gateway
        .check_safety(&ctx, "apply_patch", &serde_json::json!({"patch": "..."}))
        .await;

    assert!(decision.needs_approval());
}

/// The command text is extracted once per check and threaded to the
/// policy, risk, and justification consumers. This pins the observable
/// behavior (identical policy verdict, risk sentence, and justification
/// command line as re-derivation would produce) after the extraction
/// stopped being repeated at each site.
#[tokio::test]
async fn command_extraction_feeds_policy_risk_and_justification_identically() {
    let policy = CommandPolicyEvaluator::from_config(&CommandsConfig {
        allow_list: vec!["cargo".into()],
        ..Default::default()
    });
    let gateway = SafetyGateway::new().with_command_policy(policy);

    // Allowed by policy: command is still classified for risk (mutating
    // verifier) and surfaces verbatim in any justification.
    let allowed = gateway
        .check_safety(&make_ctx(), tools::EXEC_COMMAND, &serde_json::json!({"command": "cargo test"}))
        .await;
    assert!(allowed.is_allowed());

    // Denied by policy: the denial reports the same single command.
    let denied = gateway
        .check_safety(&make_ctx(), tools::EXEC_COMMAND, &serde_json::json!({"command": "rm -rf /"}))
        .await;
    let SafetyDecision::Deny(reason) = denied else {
        panic!("policy denial expected");
    };
    assert!(reason.contains("rm -rf /"), "denial must carry the extracted command: {reason}");
    assert!(reason.starts_with("Command 'rm -rf /' blocked by policy"), "unexpected denial shape: {reason}");

    // Justification path: the risk classifier and the justification see
    // the same extracted command (marked unsafe by the gateway config).
    gateway.config.write().approval_risk_threshold.clone_from(&RiskLevel::Low);
    let justified = gateway
        .check_safety(&make_ctx(), tools::EXEC_COMMAND, &serde_json::json!({"command": "cargo publish"}))
        .await;
    if let SafetyDecision::NeedsApproval(justification) = justified {
        assert!(
            justification.contains("Command: cargo publish"),
            "justification must carry the extracted command: {justification}"
        );
    }
    gateway.config.write().approval_risk_threshold = RiskLevel::High;
}

#[test]
fn build_risk_context_uses_caller_supplied_command() {
    let gateway = SafetyGateway::new();
    let args = serde_json::json!({"command": "rg --files"});
    let intent = classify_tool_intent(tools::EXEC_COMMAND, &args);

    let with_command = gateway.build_risk_context(tools::EXEC_COMMAND, &args, &intent, Some("rg --files"));
    assert_eq!(with_command.command_args, vec!["rg".to_string(), "--files".to_string()]);

    let without_command = gateway.build_risk_context(tools::EXEC_COMMAND, &args, &intent, None);
    assert!(without_command.command_args.is_empty());
}

#[tokio::test]
async fn test_preapproved_tools_allowed() {
    let gateway = SafetyGateway::new();
    gateway.preapprove("delete_file");
    let ctx = make_ctx();

    let decision = gateway
        .check_safety(&ctx, "delete_file", &serde_json::json!({"path": "/tmp/test"}))
        .await;

    assert!(decision.is_allowed());
}

#[tokio::test]
async fn test_trust_level_bypass() {
    let gateway = SafetyGateway::new();
    let mut ctx = make_ctx();
    ctx.trust_level = SafetyTrustLevel::Full;

    let decision = gateway
        .check_safety(&ctx, "delete_file", &serde_json::json!({"path": "/tmp/test"}))
        .await;

    assert!(decision.is_allowed());
}

#[tokio::test]
async fn test_rate_limiting() {
    let config = SafetyGatewayConfig { rate_limit_per_second: 2, ..Default::default() };
    let gateway = SafetyGateway::with_config(config);
    let ctx = make_ctx();

    // First two calls should succeed
    gateway.record_execution();
    gateway.record_execution();

    // Third call should be denied
    let decision = gateway.check_safety(&ctx, "read_file", &serde_json::json!({})).await;

    assert!(decision.is_denied());
    assert!(decision.reason().unwrap().contains("Rate limit"));
}

#[test]
fn safety_gateway_defaults_use_shared_tool_call_budget() {
    let config = SafetyGatewayConfig::default();

    assert_eq!(config.max_per_turn, DEFAULT_MAX_TOOL_CALLS_PER_TURN);
    assert_eq!(config.max_per_session, DEFAULT_SAFETY_MAX_TOOL_CALLS_PER_SESSION);
}

#[tokio::test]
async fn test_atomic_check_and_record_rate_limited() {
    let config = SafetyGatewayConfig { rate_limit_per_second: 2, ..Default::default() };
    let gateway = SafetyGateway::with_config(config);
    let ctx = make_ctx();

    let first = gateway.check_and_record(&ctx, "read_file", &serde_json::json!({})).await;
    assert!(first.decision.is_allowed());

    let second = gateway.check_and_record(&ctx, "read_file", &serde_json::json!({})).await;
    assert!(second.decision.is_allowed());

    let third = gateway.check_and_record(&ctx, "read_file", &serde_json::json!({})).await;
    assert!(third.decision.is_denied());
    assert!(third.retry_after.is_some());
    assert!(matches!(third.violation, Some(SafetyError::RateLimitExceeded { .. })));
}

#[tokio::test]
async fn test_command_policy_enforcement() {
    let mut commands_config = CommandsConfig::default();
    commands_config.deny_list.push("rm".to_string());

    let gateway = SafetyGateway::new().with_commands_config(&commands_config);
    let ctx = make_ctx();

    let decision = gateway
        .check_safety(&ctx, "shell", &serde_json::json!({"command": "rm -rf /"}))
        .await;

    assert!(decision.is_denied());
}

#[tokio::test]
async fn test_command_session_command_policy_enforcement_with_indexed_args() {
    let mut commands_config = CommandsConfig::default();
    commands_config.deny_list.push("rm".to_string());

    let gateway = SafetyGateway::new().with_commands_config(&commands_config);
    let ctx = make_ctx();

    let decision = gateway
        .check_safety(
            &ctx,
            tools::UNIFIED_EXEC,
            &serde_json::json!({
                "command.0": "rm",
                "command.1": "-rf",
                "command.2": "/"
            }),
        )
        .await;

    assert!(decision.is_denied());
}

#[tokio::test]
async fn test_command_session_continue_command_policy_enforcement_with_input() {
    let mut commands_config = CommandsConfig::default();
    commands_config.deny_list.push("rm".to_string());

    let gateway = SafetyGateway::new().with_commands_config(&commands_config);
    let ctx = make_ctx();

    let decision = gateway
        .check_safety(
            &ctx,
            tools::UNIFIED_EXEC,
            &serde_json::json!({
                "action": "continue",
                "session_id": "run-123",
                "input": "rm -rf /\n"
            }),
        )
        .await;

    assert!(decision.is_denied());
}

#[tokio::test]
async fn test_send_pty_input_command_policy_enforcement_with_input() {
    let mut commands_config = CommandsConfig::default();
    commands_config.deny_list.push("rm".to_string());

    let gateway = SafetyGateway::new().with_commands_config(&commands_config);
    let ctx = make_ctx();

    let decision = gateway
        .check_safety(
            &ctx,
            tools::SEND_PTY_INPUT,
            &serde_json::json!({
                "session_id": "run-123",
                "input": "rm -rf /\n"
            }),
        )
        .await;

    assert!(decision.is_denied());
}

#[tokio::test]
async fn test_apply_patch_dotfile_input_dotfile_protection_requires_approval() {
    let guardian = Arc::new(
        DotfileGuardian::new(DotfileProtectionConfig {
            audit_logging_enabled: false,
            create_backups: false,
            ..Default::default()
        })
        .await
        .expect("guardian should initialize"),
    );
    let gateway = SafetyGateway::new().with_dotfile_guardian(guardian);
    let ctx = make_ctx();

    let decision = gateway
        .check_safety(
            &ctx,
            tools::APPLY_PATCH,
            &serde_json::json!({
                "input": "*** Begin Patch\n*** Update File: .gitignore\n@@\n-old\n+new\n*** End Patch\n"
            }),
        )
        .await;

    assert!(decision.needs_approval());
    assert!(decision.reason().is_some_and(|reason| reason.contains(".gitignore")));
}

#[test]
fn test_patch_file_access_targets_preserve_patch_order() {
    let targets = patch_file_access_targets(&serde_json::json!({
        "input": "*** Begin Patch\n*** Update File: src/main.rs\n@@\n-old\n+new\n*** Update File: .gitignore\n@@\n-old\n+new\n*** End Patch\n"
    }));

    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].path, PathBuf::from("src/main.rs"));
    assert_eq!(targets[0].access_type, AccessType::Modify);
    assert_eq!(targets[1].path, PathBuf::from(".gitignore"));
    assert_eq!(targets[1].access_type, AccessType::Modify);
}

#[test]
fn unified_file_targets_mirror_legacy_file_helpers() {
    let write = file_access_targets(tools::UNIFIED_FILE, &serde_json::json!({"action": "write", "path": "a.txt"}));
    assert_eq!(write.len(), 1);
    assert_eq!(write[0].path, PathBuf::from("a.txt"));
    assert_eq!(write[0].access_type, AccessType::Write);

    let moved = file_access_targets(
        tools::UNIFIED_FILE,
        &serde_json::json!({"action": "move", "path": "a.txt", "destination": "b.txt"}),
    );
    assert_eq!(moved.len(), 2);
    assert_eq!(moved[0].access_type, AccessType::Modify);
    assert_eq!(moved[1].access_type, AccessType::Write);

    let unknown = file_access_targets(tools::UNIFIED_FILE, &serde_json::json!({"action": "read", "path": "a.txt"}));
    assert!(unknown.is_empty());
}

#[test]
fn proposed_changes_preview_bounds_old_and_new_str() {
    // Short replacements render exactly as before (no truncation
    // marker); long ones stay bounded with a marker; a multi-byte
    // char on the 500-byte boundary must not panic.
    let short = proposed_changes_preview(&serde_json::json!({"old_str": "a", "new_str": "b"}));
    assert_eq!(short, "Replace:\n  'a'\nWith:\n  'b'");

    let long_old = "a".repeat(600);
    let bounded = proposed_changes_preview(&serde_json::json!({"old_str": long_old, "new_str": ""}));
    assert!(bounded.contains("..."));
    assert!(bounded.len() < 600, "preview must stay bounded: {}", bounded.len());

    let boundary_old = format!("{}{}", "a".repeat(499), "日".repeat(10));
    let boundary = proposed_changes_preview(&serde_json::json!({"old_str": boundary_old, "new_str": "b"}));
    assert!(boundary.contains("..."));
    assert!(boundary.ends_with("With:\n  'b'"));
}

#[test]
fn proposed_changes_preview_never_splits_utf8_boundary() {
    // 500-byte cut lands mid-`日` (3 bytes): the old `&text[..500]`
    // panicked; the canonical prefix rounds down to the boundary.
    let content = format!("{}{}", "a".repeat(499), "日".repeat(10));
    let preview = proposed_changes_preview(&serde_json::json!({"content": content}));
    assert!(preview.contains("Content ("));
    assert!(preview.ends_with("..."));
    assert!(preview.len() < content.len() + 64);
}

#[tokio::test]
async fn test_apply_patch_patch_field_dotfile_protection_requires_approval() {
    let guardian = Arc::new(
        DotfileGuardian::new(DotfileProtectionConfig {
            audit_logging_enabled: false,
            create_backups: false,
            ..Default::default()
        })
        .await
        .expect("guardian should initialize"),
    );
    let gateway = SafetyGateway::new().with_dotfile_guardian(guardian);
    let ctx = make_ctx();

    let decision = gateway
        .check_safety(
            &ctx,
            tools::APPLY_PATCH,
            &serde_json::json!({
                "patch": "*** Begin Patch\n*** Update File: .gitignore\n@@\n-old\n+new\n*** End Patch\n"
            }),
        )
        .await;

    assert!(decision.needs_approval());
    assert!(decision.reason().is_some_and(|reason| reason.contains(".gitignore")));
}

#[tokio::test]
async fn test_stats_tracking() {
    let gateway = SafetyGateway::new();
    gateway.preapprove("test_tool");
    gateway.record_execution();
    gateway.record_execution();

    let stats = gateway.get_stats();
    assert_eq!(stats.turn_count, 2);
    assert_eq!(stats.session_count, 2);
    assert_eq!(stats.preapproved_count, 1);
}

#[tokio::test]
async fn test_start_turn_resets_counters() {
    let gateway = SafetyGateway::new();

    gateway.record_execution();
    gateway.record_execution();

    let stats_before = gateway.get_stats();
    assert_eq!(stats_before.turn_count, 2);

    gateway.start_turn();

    let stats_after = gateway.get_stats();
    assert_eq!(stats_after.turn_count, 0);
    assert_eq!(stats_after.session_count, 2); // Session count preserved
}

#[tokio::test]
async fn test_increase_session_limit_updates_limit() {
    let gateway = SafetyGateway::new();
    gateway.set_limits(10, 1);
    let ctx = make_ctx();

    let first = gateway.check_and_record(&ctx, "read_file", &serde_json::json!({})).await;
    assert!(first.decision.is_allowed());

    let second = gateway.check_and_record(&ctx, "read_file", &serde_json::json!({})).await;
    assert!(second.decision.is_denied());

    gateway.increase_session_limit(1);

    let third = gateway.check_and_record(&ctx, "read_file", &serde_json::json!({})).await;
    assert!(third.decision.is_allowed());
}

#[tokio::test]
async fn control_plane_wait_does_not_consume_per_turn_budget() {
    let config = SafetyGatewayConfig { max_per_turn: 1, ..Default::default() };
    let gateway = SafetyGateway::with_config(config);
    let ctx = make_ctx();

    // The one ordinary slot is consumed by a real work call.
    let work = gateway
        .check_and_record(&ctx, "read_file", &serde_json::json!({"path": "x.txt"}))
        .await;
    assert!(work.decision.is_allowed());

    // A blocking wait is control-plane: it must not be denied by the
    // per-turn counter even though the budget is exhausted.
    let wait = gateway
        .check_and_record(&ctx, tools::UNIFIED_EXEC, &serde_json::json!({"action": "wait", "session_id": "run-1"}))
        .await;
    assert!(wait.decision.is_allowed(), "wait must bypass the per-turn work budget");

    // Ordinary calls remain denied while the turn budget stays exhausted.
    let denied = gateway
        .check_and_record(&ctx, "read_file", &serde_json::json!({"path": "y.txt"}))
        .await;
    assert!(denied.decision.is_denied());
}

#[tokio::test]
async fn control_plane_calls_are_bounded_by_their_own_cap() {
    let config = SafetyGatewayConfig {
        max_per_turn: 1_000,
        enforce_rate_limits: false,
        ..Default::default()
    };
    let gateway = SafetyGateway::with_config(config);
    let ctx = make_ctx();

    for _ in 0..MAX_CONTROL_PLANE_TOOL_CALLS_PER_TURN {
        let call = gateway
            .check_and_record(&ctx, tools::UNIFIED_EXEC, &serde_json::json!({"action": "wait", "session_id": "run-1"}))
            .await;
        assert!(call.decision.is_allowed());
    }

    // The exempt bucket has its own ceiling; the next wait is denied.
    let overflow = gateway
        .check_and_record(&ctx, tools::UNIFIED_EXEC, &serde_json::json!({"action": "wait", "session_id": "run-1"}))
        .await;
    assert!(overflow.decision.is_denied());
    assert!(overflow.decision.reason().is_some_and(|reason| reason.contains("limit")));
}

#[tokio::test]
async fn control_plane_calls_still_consume_session_and_rate_budget() {
    // A tiny session fuse must still trip on control-plane calls.
    let config = SafetyGatewayConfig {
        max_per_turn: 1_000,
        max_per_session: 2,
        enforce_rate_limits: false,
        ..Default::default()
    };
    let gateway = SafetyGateway::with_config(config);
    let ctx = make_ctx();

    for _ in 0..2 {
        let call = gateway
            .check_and_record(&ctx, tools::UNIFIED_EXEC, &serde_json::json!({"action": "wait", "session_id": "run-1"}))
            .await;
        assert!(call.decision.is_allowed());
    }
    let third = gateway
        .check_and_record(&ctx, tools::UNIFIED_EXEC, &serde_json::json!({"action": "wait", "session_id": "run-1"}))
        .await;
    assert!(third.decision.is_denied(), "session fuse must bound control-plane calls");
}

#[tokio::test]
async fn start_turn_resets_control_plane_counter() {
    let config = SafetyGatewayConfig {
        max_per_turn: 1_000,
        enforce_rate_limits: false,
        ..Default::default()
    };
    let gateway = SafetyGateway::with_config(config);
    let ctx = make_ctx();
    let wait_args = serde_json::json!({"action": "wait", "session_id": "run-1"});

    for _ in 0..MAX_CONTROL_PLANE_TOOL_CALLS_PER_TURN {
        assert!(
            gateway
                .check_and_record(&ctx, tools::UNIFIED_EXEC, &wait_args)
                .await
                .decision
                .is_allowed()
        );
    }
    assert!(
        gateway
            .check_and_record(&ctx, tools::UNIFIED_EXEC, &wait_args)
            .await
            .decision
            .is_denied()
    );

    gateway.start_turn();

    assert!(
        gateway
            .check_and_record(&ctx, tools::UNIFIED_EXEC, &wait_args)
            .await
            .decision
            .is_allowed(),
        "a fresh turn must refill the control-plane bucket"
    );
}
