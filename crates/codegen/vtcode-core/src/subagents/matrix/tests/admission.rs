use super::*;

#[tokio::test]
async fn matrix_public_role_admission_and_worker_identity_are_enforced() {
    let root = tempfile::tempdir().unwrap();
    let registry = ToolRegistry::new(root.path().to_path_buf()).await;
    registry.set_subagent_controller(Arc::new(
        SubagentController::new(test_controller_config(root.path().to_path_buf(), VTCodeConfig::default()))
            .await
            .unwrap(),
    ));
    registry.set_matrix_coordinator(true);
    let catalog_config = crate::tools::handlers::SessionToolsConfig::full_public(
        crate::tools::handlers::SessionSurface::AgentRunner,
        crate::config::types::CapabilityLevel::CodeSearch,
        crate::config::ToolDocumentationMode::Full,
        Default::default(),
    );
    let catalog = registry.model_tools(catalog_config.clone()).await;
    let names = catalog.iter().map(|tool| tool.function_name()).collect::<Vec<_>>();
    for required in ["matrix", "agent", "task_tracker", "record_decision"] {
        assert!(names.contains(&required), "missing coordinator control: {required}");
    }
    assert!(names.iter().all(|name| registry.enforce_matrix_role(name).is_ok()));
    assert!(catalog.iter().all(|tool| tool.defer_loading != Some(true)));
    for name in ["exec_command", "apply_patch", "code_search"] {
        assert!(
            registry
                .execute_tool(name, json!({}))
                .await
                .unwrap_err()
                .to_string()
                .contains("coordinator delegates")
        );
    }
    assert!(rejected(
        registry
            .execute_tool("matrix", json!({"action":"report","outcome":"executed"}))
            .await
    ));
    let assignment = MatrixAssignment {
        task_id: "owned".into(),
        attempt_id: "attempt".into(),
        worker_id: "worker".into(),
        phase: MatrixPhase::Execute,
        generation: None,
    };
    registry.set_matrix_worker(MatrixWorkerContext::new(assignment));
    let worker_catalog = registry.model_tools(catalog_config).await;
    assert!(worker_catalog.iter().any(|tool| tool.function_name() == "matrix"));
    assert!(worker_catalog.iter().any(|tool| tool.function_name() == "exec_command"));
    assert!(worker_catalog.iter().all(|tool| tool.function_name() != "agent"));
    assert!(
        registry
            .execute_tool("agent", json!({"action":"spawn"}))
            .await
            .unwrap_err()
            .to_string()
            .contains("nested delegation")
    );
    assert!(rejected(
        registry
            .execute_tool("matrix", json!({"action":"report","task_id":"other","outcome":"executed"}))
            .await
    ));
    let accepted = registry
        .execute_tool("matrix", json!({"action":"report","outcome":"executed"}))
        .await
        .unwrap();
    assert_eq!(accepted["accepted"], true);

    assert!(rejected(
        registry
            .execute_tool("matrix", json!({"action":"report","outcome":"executed"}))
            .await
    ));
    assert!(
        registry.enforce_matrix_role("exec_command").is_ok(),
        "coordinator catalog must not restrict authorized worker execution"
    );
}
#[tokio::test]
async fn matrix_read_lease_rejects_mutating_worker_overrides_before_model_setup() {
    for (access, mutating, expected) in [
        (WorkspaceAccess::Read, true, MatrixOutcome::PermissionDenied),
        (WorkspaceAccess::Write, true, MatrixOutcome::Failed),
        (WorkspaceAccess::Read, false, MatrixOutcome::Failed),
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut specs = vtcode_config::builtin_subagents();
        let name = if access == WorkspaceAccess::Read {
            "explorer"
        } else {
            "default"
        };
        let spec = specs.iter_mut().find(|spec| spec.name == name).unwrap();
        if mutating {
            spec.tools = Some(vec!["apply_patch".into()]);
            spec.permissions = vtcode_config::core::permissions::AgentPermissionsConfig::new(
                vtcode_config::core::permissions::PermissionDefault::Allow,
            );
        }
        assert_eq!(spec.is_read_only(), !mutating);
        // A rejected model stops admitted cases before provider I/O. The read
        // lease violation must instead fail at worker-policy validation.
        spec.model = Some("invalid-matrix-regression-model".into());
        let controller = SubagentController::new_with_discovered(
            test_controller_config(root.path().to_path_buf(), VTCodeConfig::default()),
            vtcode_config::DiscoveredSubagents { effective: specs, shadowed: vec![] },
        )
        .await
        .unwrap();
        let result = Box::pin(worker::execute(
            &controller,
            MatrixAssignment {
                task_id: "read-policy".into(),
                attempt_id: "owned-attempt".into(),
                worker_id: "owned-worker".into(),
                phase: MatrixPhase::Execute,
                generation: None,
            },
            task("read-policy", access),
            CancellationToken::new(),
        ))
        .await;
        assert_eq!(result.outcome, expected, "{access:?}, mutating={mutating}");
        assert!(result.cleanup_confirmed);
        assert!(result.evidence.is_empty());
        assert!(!root.path().join("proof.txt").exists());
    }
}
