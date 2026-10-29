use super::*;

#[tokio::test]
async fn registers_builtin_tools() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let available = registry.available_tools().await;

    assert!(available.contains(&tools::EXEC_COMMAND.to_string()));
    assert!(available.contains(&tools::WRITE_STDIN.to_string()));
    assert!(available.contains(&tools::APPLY_PATCH.to_string()));
    assert!(available.contains(&tools::SEARCH_TOOLS.to_string()));
    assert!(!available.contains(&tools::CODE_SEARCH.to_string()));
    assert!(!available.contains(&tools::UNIFIED_SEARCH.to_string()));
    assert!(!available.contains(&tools::UNIFIED_FILE.to_string()));
    assert!(!available.contains(&tools::UNIFIED_EXEC.to_string()));
    assert!(!available.contains(&tools::READ_FILE.to_string()));
    assert!(!available.contains(&tools::WRITE_FILE.to_string()));
    assert!(!available.contains(&tools::DELETE_FILE.to_string()));
    assert!(!available.contains(&tools::MOVE_FILE.to_string()));
    assert!(!available.contains(&tools::COPY_FILE.to_string()));
    assert!(!available.contains(&tools::RUN_PTY_CMD.to_string()));
    Ok(())
}
#[tokio::test]
async fn request_user_input_aliases_are_not_registered() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    assert!(registry.get_tool(tools::REQUEST_USER_INPUT).is_some());
    assert!(registry.get_tool(tools::ASK_QUESTIONS).is_none());
    assert!(registry.get_tool(tools::ASK_USER_QUESTION).is_none());

    Ok(())
}
#[tokio::test]
async fn public_tool_projections_stay_in_sync() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let config = SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ConfigToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    );

    let names = registry
        .public_tool_names(SessionSurface::Interactive, CapabilityLevel::CodeSearch)
        .await;
    let schema_names = registry
        .schema_entries(config.clone())
        .await
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    let declaration_names = registry
        .function_declarations(config.clone())
        .await
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    let model_tool_names = registry
        .model_tools(config)
        .await
        .into_iter()
        .map(|tool| tool.function_name().to_string())
        .collect::<Vec<_>>();

    assert_eq!(schema_names, names);
    assert_eq!(declaration_names, names);
    assert_eq!(model_tool_names, names);
    assert_eq!(
        names,
        vec![
            tools::APPLY_PATCH.to_string(),
            tools::EXEC_COMMAND.to_string(),
            tools::WRITE_STDIN.to_string(),
            tools::SEARCH_TOOLS.to_string(),
            tools::RECORD_DECISION.to_string(),
        ]
    );
    for removed_tool in [
        tools::UNIFIED_EXEC,
        tools::UNIFIED_FILE,
        tools::UNIFIED_SEARCH,
        tools::LIST_FILES,
        tools::READ_FILE,
        tools::WRITE_FILE,
        tools::DELETE_FILE,
        tools::MOVE_FILE,
        tools::COPY_FILE,
    ] {
        assert!(
            registry.get_tool_schema(removed_tool).await.is_none(),
            "{removed_tool} schema should not be discoverable"
        );
        assert!(!registry.has_tool(removed_tool).await, "{removed_tool} should not be reported as available");
    }

    let code_search_schema = registry
        .get_tool_schema(tools::CODE_SEARCH)
        .await
        .expect("code_search schema should be discoverable on request");
    let code_search_parameters = &code_search_schema["parameters"];
    assert_eq!(code_search_parameters["required"], json!(["query"]));
    assert_eq!(code_search_parameters["additionalProperties"], false);
    let mut property_names = code_search_parameters["properties"]
        .as_object()
        .expect("code_search properties")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    property_names.sort_unstable();
    assert_eq!(property_names, ["file_types", "max_results", "path", "query", "result_types"]);

    Ok(())
}
#[tokio::test]
async fn advanced_profile_exposes_code_search_without_removed_public_names() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;
    let config = SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ConfigToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    )
    .with_tool_profile(ToolProfile::AdvancedVtCode);

    let names = registry
        .schema_entries(config.clone())
        .await
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    assert!(names.contains(&tools::CODE_SEARCH.to_string()), "advanced profile should expose code_search");
    for removed_tool in [
        tools::UNIFIED_EXEC,
        tools::UNIFIED_FILE,
        tools::UNIFIED_SEARCH,
        tools::READ_FILE,
        tools::WRITE_FILE,
        tools::DELETE_FILE,
        tools::MOVE_FILE,
        tools::COPY_FILE,
    ] {
        assert!(
            !names.contains(&removed_tool.to_string()),
            "{removed_tool} must not be exposed in the advanced profile"
        );
    }

    let model_tool_names = registry
        .model_tools(config)
        .await
        .into_iter()
        .map(|tool| tool.function_name().to_string())
        .collect::<Vec<_>>();
    assert!(model_tool_names.contains(&tools::CODE_SEARCH.to_string()));
    assert!(!model_tool_names.contains(&tools::UNIFIED_SEARCH.to_string()));

    Ok(())
}
#[tokio::test]
async fn registered_planning_task_tracker_exposes_planning_index_contract() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let planning_config = SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ConfigToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    )
    .with_tool_profile(ToolProfile::AdvancedVtCode)
    .with_planning_active(true);
    let planning_entry = registry
        .schema_entries(planning_config)
        .await
        .into_iter()
        .find(|entry| entry.name == tools::TASK_TRACKER)
        .expect("planning task tracker schema");

    assert_eq!(planning_entry.parameters["properties"]["index"]["minimum"], 1);
    assert_eq!(planning_entry.parameters["properties"]["index_path"]["pattern"], "^[1-9][0-9]*(\\.[1-9][0-9]*)*$");

    let standard_state = PlanningWorkflowState::new(temp_dir.path().to_path_buf());
    let standard_tool = TaskTrackerTool::new(temp_dir.path().to_path_buf(), standard_state);
    let standard_parameters = standard_tool.parameter_schema().expect("standard task tracker schema");
    assert_eq!(standard_parameters["properties"]["index"]["minimum"], 0);
    assert_eq!(standard_parameters["properties"]["index_path"]["pattern"], "^[1-9][0-9]*$");
    Ok(())
}
#[tokio::test]
async fn get_tool_schema_uses_runtime_planning_state_for_task_tracker() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let standard_schema = registry
        .get_tool_schema(tools::TASK_TRACKER)
        .await
        .expect("standard task_tracker schema");
    assert_eq!(
        standard_schema["description"].as_str(),
        Some(crate::tools::handlers::task_tracker::task_tracker_description_for_workflow(false))
    );
    assert_eq!(standard_schema["parameters"]["properties"]["index"]["minimum"], 0);
    assert_eq!(standard_schema["parameters"]["properties"]["index_path"]["pattern"], "^[1-9][0-9]*$");

    registry.enable_planning();
    registry.planning_workflow_state().enable();

    let planning_schema = registry
        .get_tool_schema(tools::TASK_TRACKER)
        .await
        .expect("planning task_tracker schema");
    assert_eq!(
        planning_schema["description"].as_str(),
        Some(crate::tools::handlers::task_tracker::task_tracker_description_for_workflow(true))
    );
    assert_eq!(planning_schema["parameters"]["properties"]["index"]["minimum"], 1);
    assert_eq!(
        planning_schema["parameters"]["properties"]["index_path"]["pattern"],
        "^[1-9][0-9]*(\\.[1-9][0-9]*)*$"
    );

    Ok(())
}
#[tokio::test]
async fn public_routing_keeps_aliases_private_and_rebuilds_on_dynamic_updates() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    let test_file = temp_dir.path().join("alias-read.txt");
    fs::write(&test_file, "via alias\n")?;

    let err = registry
        .execute_public_tool_ref(tools::READ_FILE, &json!({"path": test_file.to_string_lossy().to_string()}))
        .await
        .expect_err("read_file should not resolve through public routing");
    assert!(err.to_string().contains("Unknown tool"));

    registry
        .register_tool(
            ToolRegistration::from_tool_instance(CUSTOM_TOOL_NAME, CapabilityLevel::CodeSearch, CustomEchoTool)
                .with_description("Custom echo tool for testing")
                .with_parameter_schema(json!({
                    "type": "object",
                    "properties": {
                        "input": {"type": "string"}
                    }
                }))
                .with_aliases(["custom_tool_alias"]),
        )
        .await?;

    registry.allow_all_tools().await?;

    let public_names = registry
        .public_tool_names(SessionSurface::Interactive, CapabilityLevel::CodeSearch)
        .await;
    assert!(!public_names.contains(&CUSTOM_TOOL_NAME.to_string()));
    assert!(!public_names.contains(&"custom_tool_alias".to_string()));

    let schema_names = registry
        .schema_entries(SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ConfigToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        ))
        .await
        .into_iter()
        .map(|entry| entry.name)
        .collect::<Vec<_>>();
    assert!(!schema_names.contains(&CUSTOM_TOOL_NAME.to_string()));
    assert!(!schema_names.contains(&"custom_tool_alias".to_string()));

    let dynamic_result = registry
        .execute_public_tool_ref("custom_tool_alias", &json!({"input": "value"}))
        .await?;
    assert_eq!(dynamic_result["success"].as_bool(), Some(true));

    registry.unregister_tool(CUSTOM_TOOL_NAME).await?;

    let public_names = registry
        .public_tool_names(SessionSurface::Interactive, CapabilityLevel::CodeSearch)
        .await;
    assert!(!public_names.contains(&CUSTOM_TOOL_NAME.to_string()));

    let err = registry
        .execute_public_tool_ref("custom_tool_alias", &json!({"input": "value"}))
        .await
        .expect_err("alias should be removed with the registration");
    assert!(err.to_string().contains("Unknown tool"));

    Ok(())
}
#[tokio::test]
async fn allows_registering_custom_tools() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    registry
        .register_tool(
            ToolRegistration::from_tool_instance(CUSTOM_TOOL_NAME, CapabilityLevel::CodeSearch, CustomEchoTool)
                .with_parameter_schema(json!({
                    "type": "object",
                    "properties": {
                        "input": {"type": "string"}
                    }
                })),
        )
        .await?;

    registry.allow_all_tools().await?;

    let available = registry.available_tools().await;
    assert!(!available.contains(&CUSTOM_TOOL_NAME.to_string()));

    let response = registry.execute_tool(CUSTOM_TOOL_NAME, json!({"input": "value"})).await?;
    assert!(response["success"].as_bool().unwrap_or(false));
    Ok(())
}
#[tokio::test]
async fn dynamic_tool_registration_keeps_policy_catalog_in_sync() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let policy_path = temp_dir.path().join("tool-policy.json");
    let policy_manager = crate::tool_policy::ToolPolicyManager::new_with_config_path(&policy_path).await?;
    let registry = ToolRegistry::new_with_custom_policy(temp_dir.path().to_path_buf(), policy_manager).await;

    registry
        .register_tool(ToolRegistration::from_tool_instance(
            CUSTOM_TOOL_NAME,
            CapabilityLevel::CodeSearch,
            CustomEchoTool,
        ))
        .await?;

    let config: ToolPolicyConfig = serde_json::from_str(&fs::read_to_string(&policy_path)?)?;
    assert!(config.available_tools.contains(&CUSTOM_TOOL_NAME.to_string()));

    registry.unregister_tool(CUSTOM_TOOL_NAME).await?;

    let config: ToolPolicyConfig = serde_json::from_str(&fs::read_to_string(&policy_path)?)?;
    assert!(!config.available_tools.contains(&CUSTOM_TOOL_NAME.to_string()));

    Ok(())
}
#[tokio::test]
async fn duplicate_registration_replaces_schema_and_runtime_dispatch() -> Result<()> {
    let temp_dir = TempDir::new()?;
    let registry = ToolRegistry::new(temp_dir.path().to_path_buf()).await;

    registry
        .register_tool(
            ToolRegistration::new(
                REPLACE_DISPATCH_TOOL_NAME,
                CapabilityLevel::CodeSearch,
                false,
                replacement_first_executor,
            )
            .with_description("first version")
            .with_parameter_schema(json!({
                "type": "object",
                "properties": {
                    "old": {"type": "string"}
                }
            })),
        )
        .await?;
    registry
        .register_tool(
            ToolRegistration::new(
                REPLACE_DISPATCH_TOOL_NAME,
                CapabilityLevel::CodeSearch,
                false,
                replacement_second_executor,
            )
            .with_description("second version")
            .with_parameter_schema(json!({
                "type": "object",
                "properties": {
                    "new": {"type": "string"}
                }
            })),
        )
        .await?;
    registry.allow_all_tools().await?;

    let schema = registry
        .get_tool_schema(REPLACE_DISPATCH_TOOL_NAME)
        .await
        .expect("replacement schema should be present");
    assert_eq!(schema.pointer("/parameters/properties/new/type"), Some(&json!("string")));
    assert!(schema.pointer("/parameters/properties/old").is_none());

    let response = registry.execute_tool(REPLACE_DISPATCH_TOOL_NAME, json!({})).await?;
    assert_eq!(response.get("version"), Some(&json!(2)));

    Ok(())
}
