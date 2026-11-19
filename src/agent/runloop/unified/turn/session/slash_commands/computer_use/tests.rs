use super::*;

fn setup() -> (tempfile::TempDir, ConfigManager, VTCodeConfig) {
    let dir = tempfile::tempdir().expect("workspace");
    let mut cfg = VTCodeConfig::default();
    cfg.computer_use.browser_build = Some(std::path::PathBuf::from("/tmp/fixture-build"));
    let path = dir.path().join("vtcode.toml");
    ConfigManager::save_config_to_path(&path, &cfg).expect("write config");
    let manager = ConfigManager::load_from_file(&path).expect("load config");
    (dir, manager, cfg)
}

fn persisted_enabled(manager: &ConfigManager) -> bool {
    ConfigManager::load_from_file(manager.config_path().expect("config path"))
        .expect("reload config")
        .config()
        .computer_use
        .enabled
}

#[test]
fn computer_use_transitions_persist_and_report_changes() {
    let (_dir, mut manager, mut cfg) = setup();
    for (action, expected, changed) in [
        (ComputerUseAction::Status, false, false),
        (ComputerUseAction::On, true, true),
        (ComputerUseAction::On, true, true),
        (ComputerUseAction::Status, true, false),
        (ComputerUseAction::Toggle, false, true),
        (ComputerUseAction::Toggle, true, true),
        (ComputerUseAction::Off, false, true),
        (ComputerUseAction::Off, false, true),
    ] {
        let outcome = apply_computer_use_action(&mut cfg, &mut manager, action).unwrap();
        assert_eq!(outcome.changed, changed, "{expected}");
        assert_eq!(cfg.computer_use.enabled, expected);
        assert_eq!(persisted_enabled(&manager), expected);
    }
}

#[test]
fn computer_use_enable_requires_a_browser_target() {
    let (_dir, mut manager, mut cfg) = setup();
    cfg.computer_use.browser_build = None;
    let error = apply_computer_use_action(&mut cfg, &mut manager, ComputerUseAction::On).unwrap_err();
    assert!(error.to_string().contains("browser target"), "unexpected error: {error:#}");
    assert!(!cfg.computer_use.enabled);
    assert!(!persisted_enabled(&manager));

    let error = apply_computer_use_action(&mut cfg, &mut manager, ComputerUseAction::Toggle).unwrap_err();
    assert!(error.to_string().contains("browser target"), "unexpected error: {error:#}");

    let outcome = apply_computer_use_action(&mut cfg, &mut manager, ComputerUseAction::Off).unwrap();
    assert!(outcome.changed);
    assert!(!cfg.computer_use.enabled);
}

#[test]
fn computer_use_target_requires_an_absolute_existing_directory() {
    let (_dir, mut manager, mut cfg) = setup();
    let target = tempfile::tempdir().expect("target");
    let outcome = apply_computer_use_action(
        &mut cfg,
        &mut manager,
        ComputerUseAction::SetTarget { path: target.path().display().to_string() },
    )
    .unwrap();
    assert!(outcome.changed);
    assert_eq!(cfg.computer_use.browser_build.as_deref(), Some(target.path()));
    let saved = ConfigManager::load_from_file(manager.config_path().expect("config path")).expect("reload");
    assert_eq!(saved.config().computer_use.browser_build.as_deref(), Some(target.path()));

    for bad in ["relative/path", "/definitely/not/a/computer-use/target"] {
        let error =
            apply_computer_use_action(&mut cfg, &mut manager, ComputerUseAction::SetTarget { path: bad.to_string() })
                .unwrap_err();
        assert!(!error.to_string().is_empty(), "{bad}");
    }
    assert_eq!(cfg.computer_use.browser_build.as_deref(), Some(target.path()));
}

#[test]
fn computer_use_reports_name_the_changed_state() {
    let (_dir, _manager, mut cfg) = setup();
    cfg.computer_use.enabled = true;
    let lines = state_report_lines(&cfg.computer_use, true);
    assert_eq!(lines[0], "Computer use is now on.");
    assert!(lines[1].contains("/tmp/fixture-build"), "{lines:?}");
    assert!(lines[2].contains("still asks for approval"), "{lines:?}");

    cfg.computer_use.enabled = false;
    let lines = state_report_lines(&cfg.computer_use, false);
    assert_eq!(lines, vec!["Computer use is off.".to_string()]);

    cfg.computer_use.browser_build = None;
    let lines = target_report_lines(&cfg.computer_use);
    assert_eq!(lines[0], "Browser target: no browser target configured.");
}

#[test]
fn computer_use_run_builds_a_single_task_request() {
    let request = computer_task_request("test-model", "take a screenshot of the login page");
    assert_eq!(request.model, "test-model");
    assert_eq!(request.messages.len(), 1);
    assert_eq!(request.messages[0].role, MessageRole::User);
    assert!(matches!(
        &request.messages[0].content,
        MessageContent::Text(text) if text == "take a screenshot of the login page"
    ));
}

#[test]
fn computer_use_overlay_approves_only_on_explicit_run() {
    use vtcode_ui::tui::app::TransientHotkeyAction;
    assert_eq!(
        map_computer_use_submission(TransientSubmission::Selection(InlineListSelection::ConfigAction(
            "run".to_string()
        ))),
        Some(true)
    );
    assert_eq!(
        map_computer_use_submission(TransientSubmission::Selection(InlineListSelection::ConfigAction(
            "deny".to_string()
        ))),
        Some(false)
    );
    assert_eq!(map_computer_use_submission(TransientSubmission::DiffApply), None);
    assert_eq!(map_computer_use_submission(TransientSubmission::Hotkey(TransientHotkeyAction::InterruptJob)), None);
}

#[test]
fn computer_use_turn_reports_name_every_outcome_without_payloads() {
    use crate::agent::runloop::unified::turn::tool_outcomes::handlers::computer_use::execution::{
        ComputerFailureStage, RedactedTurnSummary, TeardownOutcome,
    };
    let summary = RedactedTurnSummary {
        target_kind: "browser",
        target: "fixture-app".to_string(),
        recipient: "test-model (test)".to_string(),
        action_kinds: vec!["screenshot".to_string()],
        action_count: 1,
        executed_units: 1,
        review: None,
        teardown_trigger: "finished",
        teardown: TeardownOutcome::Removed,
    };
    let ok_block = serde_json::json!({"type": "tool_result", "tool_use_id": "c1", "is_error": false});
    let err_block =
        serde_json::json!({"type": "tool_result", "tool_use_id": "c2", "is_error": true, "content": "secret"});
    let shot = serde_json::json!({"type": "computer_call_output", "call_id": "call_1", "output": {"image_url": "data:image/png;base64,SECRET"}});
    let outcome = |kind| ComputerTurnOutcome { kind, model: "test-model".to_string(), usage: None };
    for (kind, needle) in [
        (ComputerTurnOutcomeKind::Disabled { reason: "disabled" }, "did not run: disabled."),
        (ComputerTurnOutcomeKind::NoNativeCalls { content: None, ordinaries: vec![] }, "no computer calls"),
        (ComputerTurnOutcomeKind::ApprovalDenied, "denied."),
        (
            ComputerTurnOutcomeKind::BudgetExceeded { proposed: 60, max: 50 },
            "60 proposed actions exceed the budget of 50",
        ),
        (ComputerTurnOutcomeKind::ReviewHalted, "SUSPECT"),
        (ComputerTurnOutcomeKind::Cancelled, "cancelled."),
        (
            ComputerTurnOutcomeKind::Failed {
                stage: ComputerFailureStage::Execute,
                message: "boom".to_string(),
            },
            "failed at the Execute stage: boom",
        ),
    ] {
        let lines = turn_report_lines(&outcome(kind));
        assert!(lines.iter().any(|line| line.contains(needle)), "{lines:?} missing {needle}");
    }
    let lines = turn_report_lines(&outcome(ComputerTurnOutcomeKind::NoNativeCalls {
        content: Some("just chatting".to_string()),
        ordinaries: vec![],
    }));
    assert!(lines.iter().any(|line| line == "Model said: just chatting"), "{lines:?}");
    let lines = turn_report_lines(&outcome(ComputerTurnOutcomeKind::Completed {
        results: NativeTurnResults::Anthropic(vec![(1, ok_block), (3, err_block)]),
        summary: summary.clone(),
    }));
    assert!(lines[0].contains("completed (test-model): 1 units"), "{lines:?}");
    assert!(lines.iter().any(|line| line == "call c1 (#1): ok"), "{lines:?}");
    assert!(lines.iter().any(|line| line == "call c2 (#3): error"), "{lines:?}");
    assert!(!lines.iter().any(|line| line.contains("secret")), "{lines:?}");
    let lines = turn_report_lines(&outcome(ComputerTurnOutcomeKind::Completed {
        results: NativeTurnResults::OpenAi(vec![shot]),
        summary,
    }));
    assert!(lines.iter().any(|line| line == "call call_1: ok"), "{lines:?}");
    assert!(!lines.iter().any(|line| line.contains("SECRET")), "{lines:?}");
}

#[test]
fn computer_use_header_sync_shows_and_hides_the_badge() {
    use tokio::sync::mpsc;
    use vtcode_ui::tui::app::{InlineCommand, InlineHandle, InlineHeaderContext};

    let (tx, mut rx) = mpsc::unbounded_channel();
    let handle = InlineHandle::new_for_tests(tx);
    let mut header = InlineHeaderContext::default();

    sync_computer_use_header(&mut header, &handle, true);
    assert_eq!(header.computer_use.as_deref(), Some("Computer use: on"));
    match rx.blocking_recv().expect("header command") {
        InlineCommand::SetHeaderContext { context } => {
            assert_eq!(context.computer_use.as_deref(), Some("Computer use: on"));
        }
        _ => panic!("expected SetHeaderContext"),
    }

    sync_computer_use_header(&mut header, &handle, false);
    assert_eq!(header.computer_use, None);
    match rx.blocking_recv().expect("header command") {
        InlineCommand::SetHeaderContext { context } => assert_eq!(context.computer_use, None),
        _ => panic!("expected SetHeaderContext"),
    }
}
