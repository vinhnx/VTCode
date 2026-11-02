use super::*;
use vtcode_core::config::constants::tools;
use vtcode_core::core::agent::session::AgentSessionState;
use vtcode_core::core::agent::steering::{MAX_OVERFLOW_FOLLOW_UP_INTENTS, MAX_QUEUED_FOLLOW_UP_INTENTS};
use vtcode_core::llm::provider::MessageRole;

const PENDING_REASON: &str = "Verification is still pending: changed code requires a terminal verifier result.";

struct RecoveryFixture {
    _workspace: tempfile::TempDir,
    registry: ToolRegistry,
    runtime: AgentRuntime,
    stats: SessionStats,
}

impl RecoveryFixture {
    async fn new() -> Self {
        let workspace = tempfile::tempdir().unwrap();
        let registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
        let state = AgentSessionState::new("verification-recovery".into(), 4, 4, 16_000);
        Self {
            _workspace: workspace,
            registry,
            runtime: AgentRuntime::new(state, None, None),
            stats: SessionStats::default(),
        }
    }

    fn recover(&mut self, reason: &str, cfg: Option<&VTCodeConfig>) -> VerificationBlockResult {
        handle_verification_block(reason, cfg, &self.registry, &mut self.runtime, &mut self.stats)
    }
}

#[tokio::test]
async fn verification_recovery_queues_the_required_check_after_existing_user_input() {
    let mut fixture = RecoveryFixture::new().await;
    fixture.registry.allow_all_tools().await.unwrap();
    let tracker = fixture
        .registry
        .execute_tool(
            tools::TASK_TRACKER,
            serde_json::json!({
                "action":"create", "items":[{"description":"Verify docs", "verify":["./verify-docs.sh"]}]
            }),
        )
        .await
        .unwrap();
    assert!(tracker.get("error").is_none(), "{tracker}");
    fixture.registry.restore_completion_verification_debt();
    fixture.runtime.state.add_user_message("Original request".into());
    fixture.runtime.try_queue_follow_up_input("Existing user input".into()).unwrap();
    let mut cfg = VTCodeConfig::default();
    cfg.agent.harness.verification.default_verifier_override = Some("rustc --version".into());
    cfg.agent.harness.verification.cross_turn_turns = 9;

    let VerificationBlockResult::Queued { status_line } = fixture.recover(PENDING_REASON, Some(&cfg)) else {
        panic!("the first recovery turn should be queued");
    };
    assert!(status_line.contains("./verify-docs.sh"), "{status_line}");
    assert!(!status_line.contains("rustc --version"), "{status_line}");
    assert_eq!(fixture.stats.verification_auto_recovery_turns(), 1);
    assert_eq!(fixture.stats.turn_stall_reason(), Some(PENDING_REASON));
    assert_eq!(fixture.runtime.state.messages.len(), 2);
    let directive = fixture.runtime.state.messages.last().unwrap();
    assert_eq!(directive.role, MessageRole::System);
    assert!(directive.content.as_text().contains("./verify-docs.sh"));
    assert_eq!(fixture.runtime.run_until_idle().as_deref(), Some("Existing user input"));
    let recovery_input = fixture
        .runtime
        .run_until_idle()
        .expect("the accepted recovery must remain queued");
    assert!(recovery_input.contains("./verify-docs.sh"), "{recovery_input}");
    assert!(fixture.stats.register_follow_up_prompt("continue").is_stalled_recovery());

    let history_len = fixture.runtime.state.messages.len();
    assert!(matches!(fixture.recover(PENDING_REASON, Some(&cfg)), VerificationBlockResult::Handoff { .. }));
    assert_eq!(fixture.runtime.state.messages.len(), history_len);
    assert!(!fixture.runtime.has_pending_follow_up_inputs());
}

#[tokio::test]
async fn verification_recovery_full_queue_cannot_publish_an_orphan_directive() {
    let mut fixture = RecoveryFixture::new().await;
    let mut cfg = VTCodeConfig::default();
    cfg.agent.harness.verification.cross_turn_turns = 2;
    cfg.agent.harness.verification.default_verifier_override = Some("rustc --version".into());
    fixture.runtime.state.add_user_message("Keep this request".into());
    let original_history = fixture.runtime.state.messages.clone();
    for index in 0..MAX_QUEUED_FOLLOW_UP_INTENTS + MAX_OVERFLOW_FOLLOW_UP_INTENTS {
        fixture
            .runtime
            .try_queue_follow_up_input(format!("user-input-{index}"))
            .unwrap();
    }
    assert!(fixture.runtime.try_queue_follow_up_input("must-not-fit".into()).is_err());
    fixture
        .stats
        .record_verification_auto_failure("rustc --version".into(), "previous failure");
    assert!(fixture.stats.record_verification_auto_recovery_turn_with_limit(2));

    assert!(matches!(fixture.recover(PENDING_REASON, Some(&cfg)), VerificationBlockResult::Handoff { .. }));
    assert_eq!(fixture.runtime.state.messages, original_history);
    assert_eq!(fixture.stats.verification_auto_recovery_turns(), 1);
    assert_eq!(fixture.stats.verification_consecutive_failures(), 1);
    assert_eq!(fixture.stats.last_verification_failure().unwrap().excerpt_tail, "previous failure");
    assert!(!fixture.stats.turn_stalled());
    assert_eq!(fixture.runtime.run_until_idle().as_deref(), Some("user-input-0"));
}

#[tokio::test]
async fn verification_recovery_preserves_disabled_exhausted_and_escalated_boundaries() {
    #[derive(Debug, PartialEq)]
    enum RecoveryBlocker {
        Disabled,
        AutoExecuteDisabled,
        Budget,
        Preflight,
        Failures,
    }
    for blocker in [
        RecoveryBlocker::Disabled,
        RecoveryBlocker::AutoExecuteDisabled,
        RecoveryBlocker::Budget,
        RecoveryBlocker::Preflight,
        RecoveryBlocker::Failures,
    ] {
        let mut fixture = RecoveryFixture::new().await;
        let mut cfg = VTCodeConfig::default();
        cfg.agent.harness.verification.cross_turn_turns = 9;
        cfg.agent.harness.verification.max_consecutive_failures = 2;
        cfg.agent.harness.verification.default_verifier_override = Some("rustc --version".into());
        match blocker {
            RecoveryBlocker::Disabled => cfg.agent.harness.verification.cross_turn_turns = 0,
            RecoveryBlocker::AutoExecuteDisabled => cfg.agent.harness.verification.auto_execute = false,
            RecoveryBlocker::Budget => assert!(fixture.stats.record_verification_auto_recovery_turn_with_limit(1)),
            RecoveryBlocker::Preflight => {
                fixture.stats.preflight_recovery.synthesis();
                fixture.stats.preflight_recovery.begin_corrective_turn();
                fixture.stats.preflight_recovery.finish_corrective_turn();
            }
            RecoveryBlocker::Failures => {
                for _ in 0..2 {
                    fixture
                        .stats
                        .record_verification_auto_failure("cargo check --locked".into(), "compiler error evidence");
                }
            }
        }
        let VerificationBlockResult::Handoff { reason } = fixture.recover(PENDING_REASON, Some(&cfg)) else {
            panic!("{blocker:?} must retain a handoff");
        };
        assert!(fixture.runtime.state.messages.is_empty(), "{blocker:?}");
        assert!(!fixture.runtime.has_pending_follow_up_inputs(), "{blocker:?}");
        assert_eq!(fixture.stats.verification_auto_recovery_turns(), u8::from(blocker == RecoveryBlocker::Budget));
        if blocker == RecoveryBlocker::Failures {
            assert!(reason.contains("cargo check --locked"), "{reason}");
            assert!(reason.contains("compiler error evidence"), "{reason}");
            assert_eq!(fixture.stats.verification_consecutive_failures(), 2);
        }
    }
}

#[tokio::test]
async fn verification_recovery_leaves_other_blocks_and_existing_inputs_untouched() {
    let mut fixture = RecoveryFixture::new().await;
    fixture.runtime.state.add_user_message("Keep this history".into());
    fixture
        .runtime
        .try_queue_follow_up_input("Keep this queued input".into())
        .unwrap();
    let original_history = fixture.runtime.state.messages.clone();
    for reason in [
        "Tool denied by planning workflow",
        "Permission denied",
        "Turn budget exhausted",
    ] {
        assert_eq!(fixture.recover(reason, None), VerificationBlockResult::NotVerification);
    }
    assert_eq!(fixture.runtime.state.messages, original_history);
    assert_eq!(fixture.stats.verification_auto_recovery_turns(), 0);
    assert_eq!(fixture.runtime.run_until_idle().as_deref(), Some("Keep this queued input"));
    assert!(!fixture.runtime.has_pending_follow_up_inputs());
}

#[tokio::test]
async fn exhausted_recovery_keeps_non_escalated_exit_and_bounded_failure_evidence() {
    let mut fixture = RecoveryFixture::new().await;
    let mut cfg = VTCodeConfig::default();
    cfg.agent.harness.verification.cross_turn_turns = 9;
    cfg.agent.harness.verification.max_consecutive_failures = 3;
    let command = "grep -n '---\\s\\+\\|' README.md";
    cfg.agent.harness.verification.default_verifier_override = Some(command.into());
    fixture.stats.record_verification_auto_failure(
        command.into(),
        &serde_json::json!({"exit_code":2,"output":format!("{}invalid grep option", "x".repeat(5000))}).to_string(),
    );
    assert!(fixture.stats.record_verification_auto_recovery_turn_with_limit(1));
    std::sync::Arc::make_mut(&mut fixture.runtime.state.messages)
        .push(Message::assistant("The requested work is complete.".into()));
    let VerificationBlockResult::Handoff { reason } = fixture.recover(PENDING_REASON, Some(&cfg)) else {
        panic!("the exhausted episode must hand off once");
    };
    assert!(reason.contains(command), "{reason}");
    assert!(reason.contains("Terminal exit: 2"), "{reason}");
    assert!(reason.contains("invalid grep option"), "{reason}");
    assert!(reason.len() < 4000);
    assert!(!fixture.runtime.has_pending_follow_up_inputs());
    assert_eq!(fixture.stats.verification_auto_recovery_turns(), 1);
}

#[tokio::test]
async fn pending_verification_budget_stop_preserves_evidence_without_queueing_recovery() {
    let mut fixture = RecoveryFixture::new().await;
    fixture.registry.restore_completion_verification_debt();
    fixture
        .stats
        .record_verification_auto_failure("rg -q count README.md".into(), r#"{"exit_code":1,"output":"no match"}"#);
    let base = "Tool wall-clock budget exhausted for this turn (600s). Changes remain unverified; no additional verifier was scheduled.";
    let VerificationBlockResult::Handoff { reason } = fixture.recover(base, None) else {
        panic!("pending checks at budget exhaustion must hand off without another recovery");
    };
    assert!(reason.starts_with(base));
    assert!(reason.contains("Previous check `rg -q count README.md`: terminal exit 1"));
    assert!(reason.contains("no match"));
    assert!(!reason.contains("so autonomous recovery stopped"));
    assert_eq!(fixture.stats.verification_auto_recovery_turns(), 0);
    assert_eq!(fixture.stats.verification_consecutive_failures(), 1);
    assert!(!fixture.runtime.has_pending_follow_up_inputs());
    assert!(fixture.runtime.state.messages.is_empty());
    assert!(fixture.registry.completion_verification_pending());
}
