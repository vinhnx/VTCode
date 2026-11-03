use super::*;

#[tokio::test]
async fn prevalidated_execution_enforces_planning_workflow_guards() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;
    registry.enable_planning();
    registry.planning_workflow_state().enable();

    let blocked_path = temp_dir.path().join("blocked.txt");
    let args = json!({
        "path": blocked_path.to_string_lossy().to_string(),
        "content": "should-not-write"
    });

    let err = registry
        .execute_tool_ref_prevalidated(tools::WRITE_FILE, &args)
        .await
        .expect_err("planning workflow should block prevalidated mutating tool call");
    assert!(err.to_string().contains("planning workflow"));
    assert!(!blocked_path.exists());

    Ok(())
}
#[tokio::test]
async fn prevalidated_execution_denies_direct_stdin_and_patch_in_planning_workflow() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;
    registry.enable_planning();

    let stdin_error = registry
        .execute_tool_ref_prevalidated(
            tools::WRITE_STDIN,
            &json!({"session_id": "planning-session", "chars": "echo blocked\n"}),
        )
        .await
        .expect_err("direct write_stdin must remain blocked during planning");
    assert!(stdin_error.to_string().contains("planning workflow"));

    let patch_error = registry
        .execute_tool_ref_prevalidated(
            tools::APPLY_PATCH,
            &json!({
                "patch": "*** Begin Patch\n*** Add File: blocked-by-planning.txt\n+blocked\n*** End Patch"
            }),
        )
        .await
        .expect_err("direct apply_patch must remain blocked during planning");
    assert!(patch_error.to_string().contains("planning workflow"));
    assert!(!temp_dir.path().join("blocked-by-planning.txt").exists());

    Ok(())
}
#[tokio::test]
async fn prevalidated_execution_allows_task_tracker_in_planning_workflow() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    registry.allow_all_tools().await?;
    registry.enable_planning();
    registry.planning_workflow_state().enable();

    let plans_dir = temp_dir.path().join(".vtcode").join("plans");
    fs::create_dir_all(&plans_dir)?;
    let plan_file = plans_dir.join("adaptive-test.md");
    fs::write(&plan_file, "# Adaptive test\n")?;
    registry.planning_workflow_state().set_plan_file(Some(plan_file));

    let args = json!({"action": "create", "items": ["Track step"]});

    let response = registry
        .execute_tool_ref_prevalidated(tools::TASK_TRACKER, &args)
        .await
        .expect("task_tracker should be allowed in planning workflow");
    assert_eq!(response["status"], "created");

    Ok(())
}
#[tokio::test]
async fn preflight_task_tracker_schema_tracks_planning_state() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    registry.preflight_validate_call(
        tools::TASK_TRACKER,
        &json!({
            "action": "update",
            "index": 0,
            "status": "completed"
        }),
    )?;

    registry.enable_planning();
    registry.planning_workflow_state().enable();

    let plans_dir = temp_dir.path().join(".vtcode").join("plans");
    fs::create_dir_all(&plans_dir)?;
    let plan_file = plans_dir.join("preflight-schema.md");
    fs::write(&plan_file, "# Preflight schema\n")?;
    registry.planning_workflow_state().set_plan_file(Some(plan_file));

    registry.preflight_validate_call(
        tools::TASK_TRACKER,
        &json!({
            "action": "update",
            "index_path": "1.1",
            "status": "completed"
        }),
    )?;

    let error = registry
        .preflight_validate_call(
            tools::TASK_TRACKER,
            &json!({
                "action": "update",
                "index": 0,
                "status": "completed"
            }),
        )
        .expect_err("planning schema should reject index 0");
    let error_text = error.to_string();
    assert!(error_text.contains("Invalid arguments for tool 'task_tracker'"));
    assert!(error_text.contains("index"));

    Ok(())
}
#[tokio::test]
async fn preflight_task_tracker_create_rejects_update_fields_actionably() -> Result<()> {
    // Regression for session-vtcode-20261005T085022Z: a `create` bundled with
    // `index`/`index_path`/`status` previously failed with a bare
    // `field '(root)' failed validation: {...}` dump. It must name the
    // forbidden fields and point at the action-aware correction.
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let error = registry
        .preflight_validate_call(
            tools::TASK_TRACKER,
            &json!({
                "action": "create",
                "title": "README improvement plan",
                "items": ["Review README structure"],
                "index_path": "1",
                "index": 1,
                "status": "completed",
                "description": ""
            }),
        )
        .expect_err("create must reject index/index_path");
    let error_text = error.to_string();
    assert!(error_text.contains("Invalid arguments for tool 'task_tracker'"), "{error_text}");
    assert!(error_text.contains("must not include"), "{error_text}");
    assert!(error_text.contains("\"index\""), "{error_text}");
    assert!(error_text.contains("\"index_path\""), "{error_text}");
    assert!(!error_text.contains("failed validation: {\"action\""), "{error_text}");
    assert!(error_text.contains("For create, index/index_path are forbidden"), "{error_text}");

    Ok(())
}
#[tokio::test]
async fn preflight_task_tracker_value_error_omits_shape_correction() -> Result<()> {
    // A single-field value error (bad enum) must not carry the shape
    // correction: telling the model to remove index fields would point at
    // the wrong axis. The enum message alone must identify the field.
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let error = registry
        .preflight_validate_call(
            tools::TASK_TRACKER,
            &json!({
                "action": "update",
                "index": 1,
                "status": "done"
            }),
        )
        .expect_err("bad status enum must fail");
    let error_text = error.to_string();
    assert!(error_text.contains("is not one of the allowed enum"), "{error_text}");
    assert!(error_text.contains("/status"), "{error_text}");
    assert!(!error_text.contains("For create, index/index_path are forbidden"), "{error_text}");

    Ok(())
}
