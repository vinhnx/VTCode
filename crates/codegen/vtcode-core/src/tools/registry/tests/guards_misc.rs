use super::*;

#[tokio::test]
async fn execution_history_records_harness_context() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    registry.set_harness_session("session-history");
    registry.set_harness_task(Some("task-history".to_owned()));

    registry
        .register_tool(ToolRegistration::from_tool_instance(
            CUSTOM_TOOL_NAME,
            CapabilityLevel::CodeSearch,
            CustomEchoTool,
        ))
        .await?;
    registry.allow_all_tools().await?;

    let args = json!({"input": "value"});
    let response = registry.execute_tool(CUSTOM_TOOL_NAME, args.clone()).await?;
    assert!(response["success"].as_bool().unwrap_or(false));

    let records = registry.get_recent_tool_records(1);
    let record = records.first().expect("execution record captured");
    assert_eq!(record.tool_name, CUSTOM_TOOL_NAME);
    assert_eq!(record.context.session_id, "session-history");
    assert_eq!(record.context.task_id.as_deref(), Some("task-history"));
    assert_eq!(record.args, args);
    assert!(record.success);

    Ok(())
}
#[tokio::test]
async fn reentrancy_guard_blocks_recursive_tool_loops() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    registry
        .register_tool(ToolRegistration::new(
            REENTRANT_TOOL_NAME,
            CapabilityLevel::CodeSearch,
            false,
            reentrant_tool_executor,
        ))
        .await?;
    registry.allow_all_tools().await?;

    let err = registry
        .execute_tool(REENTRANT_TOOL_NAME, json!({"input": "loop"}))
        .await
        .expect_err("reentrancy guard must reject recursive tool loops");

    let err_msg = err.to_string();
    assert!(err_msg.contains("tool reentrancy blocked"));
    assert!(err.chain().any(|cause| cause.to_string().contains(REENTRANT_TOOL_NAME)));

    Ok(())
}
#[tokio::test]
async fn reentrancy_guard_blocks_cross_tool_cycles() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    registry
        .register_tool(ToolRegistration::new(
            MUTUAL_REENTRANT_TOOL_A,
            CapabilityLevel::CodeSearch,
            false,
            mutual_reentrant_tool_a_executor,
        ))
        .await?;
    registry
        .register_tool(ToolRegistration::new(
            MUTUAL_REENTRANT_TOOL_B,
            CapabilityLevel::CodeSearch,
            false,
            mutual_reentrant_tool_b_executor,
        ))
        .await?;
    registry.allow_all_tools().await?;

    let err = registry
        .execute_tool(MUTUAL_REENTRANT_TOOL_A, json!({"input": "cycle"}))
        .await
        .expect_err("reentrancy guard must reject cross-tool cycles");

    let err_msg = err.to_string();
    assert!(err_msg.contains("tool reentrancy blocked"));
    assert!(err.chain().any(|cause| cause.to_string().contains(MUTUAL_REENTRANT_TOOL_A)));
    assert!(err.chain().any(|cause| cause.to_string().contains(MUTUAL_REENTRANT_TOOL_B)));

    Ok(())
}
#[tokio::test]
async fn full_auto_allowlist_enforced() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    registry.enable_full_auto_permission(&[tools::EXEC_COMMAND.to_string()]).await;

    assert!(registry.preflight_tool_permission(tools::EXEC_COMMAND).await?);
    assert!(!registry.preflight_tool_permission(tools::READ_FILE).await?);
    assert!(!registry.preflight_tool_permission(tools::RUN_PTY_CMD).await?);

    Ok(())
}
#[tokio::test]
async fn wildcard_initialisation_retains_registration_from_snapshot_window() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let hooks = policy_catalogue_test_hooks(&registry).await;
    let snapshot_pause = policy::PolicyCatalogueTestPause::default();
    hooks.install_after_enable_snapshot(snapshot_pause.clone());

    let enabling_registry = registry.clone();
    let enable_task = tokio::spawn(async move {
        enabling_registry
            .enable_full_auto_permission_for_session(
                &[tools::WILDCARD_ALL.to_string()],
                advanced_session_tools_config(),
            )
            .await;
    });
    wait_for_catalogue_pause(&snapshot_pause).await;

    let tool_name = "wildcard_snapshot_window_dynamic_tool";
    let registering_registry = registry.clone();
    let registration_task = tokio::spawn(async move {
        registering_registry
            .register_tool(
                ToolRegistration::new(tool_name, CapabilityLevel::Basic, false, catalogue_race_executor)
                    .with_description("tool registered after the wildcard snapshot"),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !registry.has_tool(tool_name).await {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("registration entered the wildcard snapshot window");
    assert!(!registration_task.is_finished());

    snapshot_pause.resume();
    enable_task.await.expect("wildcard enable task");
    registration_task.await.expect("registration task")?;

    assert!(registry.is_allowed_in_full_auto(tool_name).await);
    Ok(())
}
#[tokio::test]
async fn in_flight_catalogue_refresh_cannot_restore_wildcard_after_disable() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let config = advanced_session_tools_config();
    registry
        .enable_full_auto_permission_for_session(&[tools::WILDCARD_ALL.to_string()], config)
        .await;

    let hooks = policy_catalogue_test_hooks(&registry).await;
    let refresh_pause = policy::PolicyCatalogueTestPause::default();
    hooks.install_after_refresh_snapshot(refresh_pause.clone());
    let registering_registry = registry.clone();
    let registration_task = tokio::spawn(async move {
        registering_registry
            .register_tool(
                ToolRegistration::new(
                    "disable_during_refresh_dynamic_tool",
                    CapabilityLevel::Basic,
                    false,
                    catalogue_race_executor,
                )
                .with_description("tool whose registration pauses catalogue refresh"),
            )
            .await
    });
    wait_for_catalogue_pause(&refresh_pause).await;

    let disable_pause = policy::PolicyCatalogueTestPause::default();
    hooks.install_before_disable_lifecycle(disable_pause.clone());
    let disabling_registry = registry.clone();
    let mut disable_task = tokio::spawn(async move {
        disabling_registry.disable_full_auto_permission().await;
    });
    wait_for_catalogue_pause(&disable_pause).await;
    disable_pause.resume();
    assert!(!registration_task.is_finished());
    assert_catalogue_task_remains_pending(&mut disable_task, "disable task").await;

    refresh_pause.resume();
    registration_task.await.expect("registration task")?;
    disable_task.await.expect("disable task");

    assert_eq!(registry.current_full_auto_allowlist().await, None);
    Ok(())
}
#[tokio::test]
async fn in_flight_refresh_cannot_overwrite_same_config_explicit_replacement() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let config = advanced_session_tools_config();
    registry
        .enable_full_auto_permission_for_session(&[tools::WILDCARD_ALL.to_string()], config.clone())
        .await;

    let hooks = policy_catalogue_test_hooks(&registry).await;
    let refresh_pause = policy::PolicyCatalogueTestPause::default();
    hooks.install_after_refresh_snapshot(refresh_pause.clone());
    let registering_registry = registry.clone();
    let registration_task = tokio::spawn(async move {
        registering_registry
            .register_tool(
                ToolRegistration::new(
                    "replacement_during_refresh_dynamic_tool",
                    CapabilityLevel::Basic,
                    false,
                    catalogue_race_executor,
                )
                .with_description("tool whose registration pauses catalogue refresh"),
            )
            .await
    });
    wait_for_catalogue_pause(&refresh_pause).await;

    let replacement_pause = policy::PolicyCatalogueTestPause::default();
    hooks.install_before_enable_lifecycle(replacement_pause.clone());
    let replacing_registry = registry.clone();
    let mut replacement_task = tokio::spawn(async move {
        replacing_registry
            .enable_full_auto_permission_for_session(&[tools::EXEC_COMMAND.to_string()], config)
            .await;
    });
    wait_for_catalogue_pause(&replacement_pause).await;
    replacement_pause.resume();
    assert!(!registration_task.is_finished());
    assert_catalogue_task_remains_pending(&mut replacement_task, "replacement task").await;

    refresh_pause.resume();
    registration_task.await.expect("registration task")?;
    replacement_task.await.expect("replacement task");

    assert_eq!(registry.current_full_auto_allowlist().await, Some(vec![tools::EXEC_COMMAND.to_string()]));
    assert!(
        !registry
            .is_allowed_in_full_auto("replacement_during_refresh_dynamic_tool")
            .await
    );
    Ok(())
}
#[test]
fn normalizes_mcp_tool_identifiers() {
    assert_eq!(normalize_mcp_tool_identifier("sequential-thinking"), "sequentialthinking");
    assert_eq!(normalize_mcp_tool_identifier("Context7.Lookup"), "context7lookup");
    assert_eq!(normalize_mcp_tool_identifier("alpha_beta"), "alphabeta");
}
#[test]
fn timeout_policy_derives_from_config() {
    let config = TimeoutsConfig {
        default_ceiling_seconds: 0,
        pty_ceiling_seconds: 600,
        mcp_ceiling_seconds: 90,
        warning_threshold_percent: 75,
        ..Default::default()
    };

    let policy = ToolTimeoutPolicy::from_config(&config);
    assert_eq!(policy.ceiling_for(ToolTimeoutCategory::Default), None);
    assert_eq!(policy.ceiling_for(ToolTimeoutCategory::Pty), Some(Duration::from_secs(600)));
    assert_eq!(policy.ceiling_for(ToolTimeoutCategory::Mcp), Some(Duration::from_secs(90)));
    assert!((policy.warning_fraction() - 0.75).abs() < f32::EPSILON);
}
#[tokio::test]
async fn timeout_errors_are_structured_and_track_failures() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    registry
        .register_tool(ToolRegistration::from_tool_instance(
            SLOW_TIMEOUT_TOOL_NAME,
            CapabilityLevel::CodeSearch,
            SlowTimeoutTool,
        ))
        .await?;
    registry.allow_all_tools().await?;

    registry.apply_timeout_policy(&TimeoutsConfig {
        default_ceiling_seconds: 1,
        pty_ceiling_seconds: 1,
        mcp_ceiling_seconds: 1,
        ..Default::default()
    });

    let mut policy = ExecutionPolicySnapshot::default().with_max_retries(4);
    policy.retry_base_delay = Duration::from_millis(1);
    policy.retry_max_delay = Duration::from_millis(1);
    policy.retry_multiplier = 1.0;

    let request = ToolExecutionRequest::new(SLOW_TIMEOUT_TOOL_NAME, json!({})).with_policy(policy);
    let outcome = registry.execute_public_tool_request(request).await;

    assert!(!outcome.is_success());
    assert_eq!(outcome.attempts, 5);

    let error = outcome.error.expect("timeout outcome should include error");
    assert_eq!(error.tool_name, SLOW_TIMEOUT_TOOL_NAME);
    assert!(matches!(error.error_type, ToolErrorType::Timeout));
    assert_eq!(error.category, vtcode_commons::ErrorCategory::Timeout);
    assert!(error.is_recoverable);
    assert!(error.retry_after_ms.is_some());
    assert!(error.message.contains("exceeded the standard timeout ceiling"));
    assert_eq!(error.debug_context.as_ref().and_then(|ctx| ctx.surface.as_deref()), Some("tool_registry"));

    let failures = registry.execution_history.get_recent_failures(1);
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].timeout_category.as_deref(), Some("standard"));
    assert_eq!(failures[0].effective_timeout_ms, Some(1_000));

    let consecutive_failures = registry
        .resiliency
        .lock()
        .failure_trackers
        .get(&ToolTimeoutCategory::Default)
        .map(|tracker| tracker.consecutive_failures)
        .unwrap_or(0);
    assert_eq!(consecutive_failures, 5);

    Ok(())
}
#[tokio::test]
async fn code_search_executes_with_policy_capped_max_results() -> Result<()> {
    let temp_dir = TempDir::new()?;
    fs::write(temp_dir.path().join("Widget.rs"), "struct Widget;\n")?;
    let policy_path = temp_dir.path().join("tool-policy.json");
    let mut config = ToolPolicyConfig::default();
    config.policies.insert(tools::CODE_SEARCH.to_string(), ToolPolicy::Allow);
    config.constraints.insert(
        tools::CODE_SEARCH.to_string(),
        ToolConstraints {
            max_results_per_call: Some(1),
            ..ToolConstraints::default()
        },
    );
    fs::write(&policy_path, serde_json::to_vec_pretty(&config)?)?;
    let policy_manager = crate::tool_policy::ToolPolicyManager::new_with_config_path(policy_path).await?;
    let registry = ToolRegistry::new_with_custom_policy(temp_dir.path().to_path_buf(), policy_manager).await;

    let response = registry
        .execute_tool(
            tools::CODE_SEARCH,
            json!({
                "query": "Widget",
                "path": ".",
                "result_types": ["path"],
                "max_results": 50
            }),
        )
        .await?;

    assert_eq!(response["filters"]["max_results"], json!(1));
    assert_eq!(response["returned"], json!(1));
    assert_eq!(response["results"].as_array().map(Vec::len), Some(1));
    Ok(())
}
#[tokio::test]
async fn apply_patch_payload_correction_is_concrete_and_preserves_aliases() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;
    for args in [
        json!({}),
        json!({"input": 42}),
        json!({"patch": []}),
        json!(null),
        json!({"cmd":"*** Begin Patch\n*** Add File: created.txt\n+wrong field\n*** End Patch\n"}),
    ] {
        let error = registry
            .preflight_validate_call(tools::APPLY_PATCH, &args)
            .expect_err("invalid payload");
        let message = error.to_string();
        assert!(message.contains(r#"{"input":"*** Begin Patch\n"#), "{message}");
        assert!(message.contains("retry once"), "{message}");
        assert!(!message.contains("Missing required argument: patch"), "{message}");
        assert!(!temp_dir.path().join("created.txt").exists());
    }
    for (index, field) in ["input", "patch"].into_iter().enumerate() {
        let patch = format!("*** Begin Patch\n*** Add File: created{index}.txt\n+payload{index}\n*** End Patch\n");
        let outcome = registry
            .execute_public_tool_request(ToolExecutionRequest::new(tools::APPLY_PATCH, json!({field: patch})))
            .await;
        assert!(outcome.is_success(), "{outcome:?}");
        let output = outcome.output.expect("patch result");
        assert!(output["diff"].as_array().is_some_and(|diff| !diff.is_empty()));
        assert!(output["modified_files"].as_array().is_some_and(|paths| paths.len() == 1));
        assert_eq!(
            fs::read_to_string(temp_dir.path().join(format!("created{index}.txt")))?,
            format!("payload{index}\n")
        );
    }
    registry.enable_planning();
    let denied = registry
        .execute_public_tool_request(ToolExecutionRequest::new(
            tools::APPLY_PATCH,
            json!({"input":"*** Begin Patch\n*** Add File: denied.txt\n+no\n*** End Patch\n"}),
        ))
        .await;
    assert!(!denied.is_success());
    assert!(!temp_dir.path().join("denied.txt").exists());
    Ok(())
}
#[tokio::test]
async fn apply_patch_shell_invocations_are_rejected_before_process_creation() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;
    let executable = temp_dir.path().join("apply_patch");
    let marker = temp_dir.path().join("launched");
    fs::write(&executable, format!("#!/bin/sh\ntouch '{}'\n", marker.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o755))?;
    }
    for name in [
        "apply_patch".to_string(),
        "'apply_patch'".to_string(),
        "applypatch".to_string(),
        "./applypatch".to_string(),
        format!("'{}'", executable.display()),
    ] {
        let args = json!({"cmd":format!("{name} <<'PATCH'\n*** Begin Patch\n*** End Patch\nPATCH"), "workdir":temp_dir.path(), "yield_time_ms":1000});
        let error = registry
            .preflight_validate_call(tools::EXEC_COMMAND, &args)
            .expect_err("shell patch rejected");
        assert!(error.to_string().contains("apply_patch is a tool"));
        assert!(error.to_string().contains(r#"{"input":"*** Begin Patch\n"#));
        let outcome = registry
            .execute_public_tool_request(ToolExecutionRequest::new(tools::EXEC_COMMAND, args))
            .await;
        assert!(!outcome.is_success(), "shell rejection must happen before launch");
        assert!(!marker.exists(), "no process may launch");
    }
    registry.preflight_validate_call(tools::EXEC_COMMAND, &json!({"cmd":"printf '%s' apply_patch"}))?;
    registry.preflight_validate_call(tools::EXEC_COMMAND, &json!({"cmd":"rg apply_patch src"}))?;
    assert!(registry.exec_sessions.list_sessions().await.is_empty());
    Ok(())
}
#[tokio::test]
async fn apply_patch_context_diagnostics_ignore_quoted_configuration_and_bound_evidence() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;
    fs::write(temp_dir.path().join("guide.md"), "actual content\n")?;
    for quoted in [
        "vtcode.toml",
        "provider openai authentication API key credentials",
        "permission denied sandbox",
        &"vtcode.toml ".repeat(100),
    ] {
        let patch = format!("*** Begin Patch\n*** Update File: guide.md\n@@\n-{quoted}\n+fixed\n*** End Patch\n");
        let outcome = registry
            .execute_public_tool_request(ToolExecutionRequest::new(tools::APPLY_PATCH, json!({"input":patch})))
            .await;
        let error = outcome.error.expect("mismatch must fail");
        assert_eq!(error.patch_context_mismatch_path(), Some("guide.md"));
        assert_eq!(error.category, vtcode_commons::ErrorCategory::ExecutionError);
        assert!(error.is_recoverable);
        assert!(!error.retryable);
        assert!(error.recovery_suggestions[0].contains("file read limit of 1-200"));
        assert!(!error.recovery_suggestions[0].contains("configuration"));
        let wire = error.to_json_value();
        let evidence = wire["error"]["patch_failure"]["ContextMismatch"]["evidence"]
            .as_str()
            .expect("bounded mismatch evidence");
        assert!(evidence.chars().count() <= 320);
        assert_eq!(
            ToolExecutionError::from_tool_output(&wire)
                .expect("round trip")
                .patch_context_mismatch_path(),
            Some("guide.md")
        );
        assert_eq!(fs::read_to_string(temp_dir.path().join("guide.md"))?, "actual content\n");
        assert!(registry.has_patch_recovery_read(tools::READ_FILE, &json!({"path":"guide.md", "limit":10})));
    }
    Ok(())
}
