use super::*;

#[tokio::test]
async fn matrix_real_checks_collect_nonzero_and_later_success_and_timeout_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let sink = SessionStoreSink::open(root.path(), "real-checks").await.unwrap();
    let mut cfg = VTCodeConfig::default();
    cfg.automation.full_auto.enabled = true;
    cfg.commands.allow_list.extend(["false", "printf", "sleep"].map(str::to_owned));
    let controller = SubagentController::new(test_controller_config(root.path().to_path_buf(), cfg))
        .await
        .unwrap();
    controller.set_matrix_persistence(sink.matrix_persistence()).await.unwrap();
    let assignment = MatrixAssignment {
        task_id: "checks".into(),
        attempt_id: "real-one".into(),
        worker_id: "worker-one".into(),
        phase: MatrixPhase::Verify,
        generation: Some("generation".into()),
    };
    let mut checks = task("checks", WorkspaceAccess::Read);
    checks.checks = vec!["false".into(), "printf verified > proof.txt".into()];
    let result =
        Box::pin(worker::execute(&controller, assignment.clone(), checks.clone(), CancellationToken::new())).await;
    assert_eq!(result.outcome, MatrixOutcome::Failed);
    assert_eq!(result.evidence.iter().map(|record| record.exit_code).collect::<Vec<_>>(), vec![Some(1), Some(0)]);
    assert!(result.cleanup_confirmed);
    assert_eq!(std::fs::read_to_string(root.path().join("proof.txt")).unwrap(), "verified");
    checks.timeout_secs = 1;
    checks.checks = vec!["sleep 2; printf leaked > late.txt".into()];
    let result = Box::pin(worker::execute(
        &controller,
        MatrixAssignment {
            attempt_id: "real-two".into(),
            worker_id: "worker-two".into(),
            ..assignment
        },
        checks,
        CancellationToken::new(),
    ))
    .await;
    assert_eq!(result.outcome, MatrixOutcome::TimedOut);
    assert!(result.cleanup_confirmed);
    tokio::time::sleep(Duration::from_millis(1250)).await;
    assert!(
        !root.path().join("late.txt").exists(),
        "owned process group must stop before resources can be reused"
    );
    sink.close().await.unwrap();
}
#[tokio::test]
async fn matrix_verification_preserves_full_auto_denials_and_work_budgets() {
    for allow_execution in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut cfg = VTCodeConfig::default();
        cfg.agent.harness.inspection.mode = vtcode_config::core::agent::InspectionMode::Off;
        cfg.automation.full_auto.enabled = true;
        cfg.agent.harness.max_tool_calls_per_turn = 1;
        cfg.automation.full_auto.allowed_tools = if allow_execution {
            vec!["exec_command".into(), "write_stdin".into()]
        } else {
            vec!["matrix".into()]
        };
        let controller = SubagentController::new(test_controller_config(root.path().to_path_buf(), cfg))
            .await
            .unwrap();
        let assignment = MatrixAssignment {
            task_id: "budget".into(),
            attempt_id: "attempt".into(),
            worker_id: "worker".into(),
            phase: MatrixPhase::Verify,
            generation: Some("generation".into()),
        };
        let mut checks = task("budget", WorkspaceAccess::Read);
        checks.checks = vec!["printf one > first.txt".into(), "printf two > second.txt".into()];
        let result = Box::pin(worker::execute(&controller, assignment, checks, CancellationToken::new())).await;
        assert_eq!(
            result.outcome,
            if allow_execution {
                MatrixOutcome::BudgetExhausted
            } else {
                MatrixOutcome::PermissionDenied
            }
        );
        assert_eq!(result.evidence.len(), usize::from(allow_execution));
        assert_eq!(root.path().join("first.txt").exists(), allow_execution);
        assert!(!root.path().join("second.txt").exists());
        assert!(result.cleanup_confirmed);
    }
}
#[tokio::test]
async fn matrix_verification_enforces_inherited_bash_permissions_despite_full_auto_grants() {
    for deny in [Some("bash"), Some("bash(printf*)"), None] {
        let root = tempfile::tempdir().unwrap();
        let mut cfg = VTCodeConfig::default();
        cfg.agent.harness.inspection.mode = vtcode_config::core::agent::InspectionMode::Off;
        cfg.automation.full_auto.enabled = true;
        cfg.automation.full_auto.allowed_tools = vec!["exec_command".into(), "write_stdin".into()];
        cfg.commands.allow_list.push("printf".into());
        cfg.permissions.deny = deny.into_iter().map(str::to_owned).collect();
        let controller = SubagentController::new(test_controller_config(root.path().to_path_buf(), cfg))
            .await
            .unwrap();
        let assignment = MatrixAssignment {
            task_id: "permission".into(),
            attempt_id: "permission-attempt".into(),
            worker_id: "permission-worker".into(),
            phase: MatrixPhase::Verify,
            generation: Some("generation".into()),
        };
        let mut checks = task("permission", WorkspaceAccess::Read);
        checks.checks = vec!["printf verified > proof.txt".into()];
        let result = Box::pin(worker::execute(&controller, assignment, checks, CancellationToken::new())).await;
        assert_eq!(
            result.outcome,
            if deny.is_some() {
                MatrixOutcome::PermissionDenied
            } else {
                MatrixOutcome::Success
            },
            "deny={deny:?}"
        );
        assert_eq!(result.evidence.len(), usize::from(deny.is_none()));
        assert_eq!(root.path().join("proof.txt").exists(), deny.is_none());
        assert!(result.cleanup_confirmed);
        if deny.is_none() {
            assert_eq!(std::fs::read_to_string(root.path().join("proof.txt")).unwrap(), "verified");
        }
    }
}
