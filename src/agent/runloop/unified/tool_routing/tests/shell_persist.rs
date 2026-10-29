use super::*;

#[tokio::test]
async fn full_auto_allowlist_denies_outside_tools_before_auto_review() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.enable_full_auto_permission(&[tools::READ_FILE.to_string()]).await;
    let mut session = create_headless_session();
    let handle = session.clone_inline_handle();
    let mut renderer = AnsiRenderer::with_inline_ui(handle.clone(), Default::default());
    let ctrl_c_state = Arc::new(CtrlCState::new());
    let ctrl_c_notify = Arc::new(Notify::new());
    let permissions = PermissionsConfig::default();
    let agent_permissions = AgentPermissionsConfig::new(PermissionDefault::Auto);
    let mut provider = StaticProvider {
        responses: std::sync::Mutex::new(vec!["ALLOW".to_string()]),
    };
    let config = runtime_config();
    let history = vec![uni::Message::user("run the command".to_string())];
    let mut session_stats = SessionStats::default();

    let flow = ensure_tool_permission(
        PermissionsContextBuilder::new(&registry, &mut renderer, &handle, &mut session, &ctrl_c_state, &ctrl_c_notify)
            .with_permissions_config(&permissions)
            .with_active_agent_permissions(&agent_permissions)
            .with_auto_permission_runtime(AutoPermissionRuntimeContext {
                config: &config,
                vt_cfg: None,
                provider_client: &mut provider,
                working_history: &history,
            })
            .with_session_stats(&mut session_stats)
            .with_approval_policy(AskForApproval::OnRequest)
            .with_skip_confirmations(false)
            .build(),
        tools::UNIFIED_EXEC,
        Some(&json!({"action": "run", "command": "echo hi"})),
    )
    .await
    .expect("permission flow");

    assert_eq!(flow, ToolPermissionFlow::Denied);
    assert_eq!(
        provider.responses.lock().expect("responses lock").len(),
        1,
        "full-auto allow-list denials should not consume classifier responses"
    );
}
#[tokio::test]
async fn permanent_shell_approval_persists_segmented_commands() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let args = json!({
        "action": "run",
        "command": ["/bin/zsh", "-lc", "git status && cargo check"],
    });

    persist_segment_approval_cache_keys(&registry, "exec_command", "exec_command", Some(&args)).await;

    assert!(
        registry
            .has_persisted_approval("git status|sandbox_permissions=\"use_default\"|additional_permissions=null")
            .await
    );
    assert!(
        registry
            .has_persisted_approval("cargo check|sandbox_permissions=\"use_default\"|additional_permissions=null")
            .await
    );
}
#[tokio::test]
async fn permanent_shell_approval_reuses_for_loop_body_commands() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let approved_args = json!({
        "action": "run",
        "command": "cd crates/codegen/vtcode-core/src/tools/registry && for f in *.rs; do echo \"=== $f ===\"; grep -nE '^(pub )?(struct|enum|fn)' \"$f\" | head -50; done",
    });

    persist_segment_approval_cache_keys(&registry, "exec_command", "exec_command", Some(&approved_args)).await;

    let later_args = json!({
        "action": "run",
        "command": "cd crates/codegen/vtcode-core/src/tools/registry && for f in approval_recorder.rs assembly.rs; do echo \"=== $f ===\"; grep -nE '^(pub )?(struct|enum|fn)' \"$f\" | head -50; done",
    });

    assert!(
        persisted_segment_approval_hit_key(&registry, "exec_command", Some(&later_args))
            .await
            .is_some()
    );
}
#[tokio::test]
async fn learned_shell_family_approval_reuses_for_new_arguments() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    isolate_workspace_config(temp_dir.path());
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    // "Always approve" on a safe `awk` read now persists the learned family key
    // (not just the exact line range that changes on every call).
    let first = json!({
        "action": "run",
        "command": r#"awk 'NR>=208 && NR<=212 {n=index(rest,"| "); print n}' README.md"#,
    });
    let family_key = match persistent_approval_target("exec_command", Some(&first), "Run Command") {
        super::super::shell_approval::PersistentApprovalTarget::LearnedPattern { key, .. } => key,
        other => panic!("expected learned family target, got {other:?}"),
    };
    assert!(family_key.starts_with("shell-pattern:awk README.md|sandbox_permissions="));
    registry
        .persist_approval_cache_key(&family_key)
        .await
        .expect("persist learned family key");

    // A new safe line range under the same family reuses the approval.
    let later = json!({"action": "run", "command": "awk 'NR>=1 && NR<=5' README.md"});
    let later_learning = approval_learning_target("exec_command", Some(&later), "Run Command");
    let later_exact = exact_shell_approval_target("exec_command", Some(&later), "Run Command");
    assert!(
        persisted_approval_hit_key(&registry, &later_learning, later_exact.as_ref())
            .await
            .is_some(),
        "a new safe awk range must reuse the learned family approval"
    );

    // A mutating `awk` shares the program name but is not safe, so it must not
    // inherit the family approval.
    let destructive = json!({"action": "run", "command": "awk '{print > \"out.txt\"}' README.md"});
    let destructive_learning = approval_learning_target("exec_command", Some(&destructive), "Run Command");
    let destructive_exact = exact_shell_approval_target("exec_command", Some(&destructive), "Run Command");
    assert!(
        persisted_approval_hit_key(&registry, &destructive_learning, destructive_exact.as_ref())
            .await
            .is_none(),
        "a mutating awk must not inherit the learned family approval"
    );

    // A different workspace family is not covered by the README.md approval.
    let other_family = json!({"action": "run", "command": "awk 'NR>=1 && NR<=5' src/lib.rs"});
    let other_learning = approval_learning_target("exec_command", Some(&other_family), "Run Command");
    let other_exact = exact_shell_approval_target("exec_command", Some(&other_family), "Run Command");
    assert!(
        persisted_approval_hit_key(&registry, &other_learning, other_exact.as_ref())
            .await
            .is_none(),
        "a different family must not inherit the approval"
    );
}
#[tokio::test]
async fn permanent_decision_persists_learned_family_key() {
    let temp_dir = tempfile::TempDir::new().expect("temp dir");
    isolate_workspace_config(temp_dir.path());
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let args = json!({
        "action": "run",
        "command": r#"awk 'NR>=208 && NR<=212 {n=index(rest,"| "); print n}' README.md"#,
    });
    let learning = approval_learning_target("exec_command", Some(&args), "Run Command");
    let exact = exact_shell_approval_target("exec_command", Some(&args), "Run Command");
    let persistent = persistent_approval_target("exec_command", Some(&args), "Run Command");
    let family_key = match &persistent {
        super::super::shell_approval::PersistentApprovalTarget::LearnedPattern { key, .. } => key.clone(),
        other => panic!("expected learned family target, got {other:?}"),
    };

    let flow = finalize_permission_decision(
        &registry,
        "exec_command",
        "exec_command",
        Some(&args),
        "exec_command:awk",
        None,
        None,
        &learning,
        exact.as_ref(),
        &persistent,
        HitlDecision::ApprovedPermanent,
        None,
    )
    .await
    .expect("finalize permanent decision");

    assert_eq!(flow, ToolPermissionFlow::Approved { updated_args: None });
    assert!(
        registry.has_persisted_approval(&family_key).await,
        "permanent approval must persist the learned family key: {family_key}"
    );
}
#[test]
fn session_approval_keys_include_learned_family() {
    let args = json!({
        "action": "run",
        "command": r#"awk 'NR>=208 && NR<=212 {n=index(rest,"| "); print n}' README.md"#,
    });
    let learning = approval_learning_target("exec_command", Some(&args), "Run Command");
    let exact = exact_shell_approval_target("exec_command", Some(&args), "Run Command");
    let pattern_key = learning.pattern.as_ref().expect("family pattern").key.clone();
    let keys: Vec<&str> =
        super::super::session_approval_cache_keys("exec_command", "exec_command:cmd", &learning, exact.as_ref())
            .collect();
    assert!(
        keys.contains(&pattern_key.as_str()),
        "session approval keys must include the learned family: {keys:?}"
    );
}
