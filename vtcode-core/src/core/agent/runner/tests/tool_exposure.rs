#![allow(missing_docs)]

use super::*;

#[tokio::test]
async fn full_auto_allowlist_hides_tools_from_exposure() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-allowlist".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        Some(VTCodeConfig::default()),
        None,
    ))
    .await
    .expect("runner");

    runner
        .enable_full_auto(&[tools::UNIFIED_FILE.to_string()])
        .await;

    assert!(runner.is_tool_exposed(tools::UNIFIED_FILE).await);
    assert!(!runner.is_tool_exposed(tools::UNIFIED_EXEC).await);

    let snapshot = runner
        .build_universal_tool_snapshot()
        .await
        .expect("full-auto snapshot");
    assert!(
        snapshot
            .active_tool_names
            .contains(&tools::UNIFIED_FILE.to_string())
    );
    assert!(
        !snapshot
            .active_tool_names
            .contains(&tools::UNIFIED_EXEC.to_string())
    );
}

#[tokio::test]
async fn runner_uses_public_tool_resolution_for_validation() {
    let temp = TempDir::new().expect("tempdir");
    let runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-tool-validation".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        Some(VTCodeConfig::default()),
        None,
    ))
    .await
    .expect("runner");

    assert!(runner.is_valid_tool(tools::READ_FILE).await);
    assert!(runner.is_valid_tool("Exec code").await);
    assert!(!runner.is_valid_tool("exec_code").await);
}

#[tokio::test]
async fn build_universal_tools_matches_registry_agent_runner_snapshot() {
    let temp = TempDir::new().expect("tempdir");
    let runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-tool-snapshot".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        Some(VTCodeConfig::default()),
        None,
    ))
    .await
    .expect("runner");

    let registry_tools = runner
        .tool_registry
        .model_tools(SessionToolsConfig {
            surface: SessionSurface::AgentRunner,
            capability_level: CapabilityLevel::CodeSearch,
            documentation_mode: runner.config().agent.tool_documentation_mode,
            planning_active: runner.tool_registry.is_planning_active(),
            request_user_input_enabled: false,
            model_capabilities: ToolModelCapabilities::for_model_name(&runner.model),
            deferred_tool_policy: crate::tools::handlers::deferred_tool_policy_for_runtime(
                crate::llm::factory::infer_provider(
                    Some(&runner.config().agent.provider),
                    &runner.model,
                ),
                runner
                    .provider_client
                    .supports_responses_compaction(&runner.model),
                Some(runner.config()),
            ),
            anthropic_native_memory_enabled:
                crate::tools::handlers::anthropic_native_memory_enabled_for_runtime(
                    crate::llm::factory::infer_provider(
                        Some(&runner.config().agent.provider),
                        &runner.model,
                    ),
                    &runner.model,
                    Some(runner.config()),
                ),
        })
        .await;
    let mut expected = Vec::new();
    for tool in registry_tools {
        if runner.is_tool_exposed(tool.function_name()).await {
            expected.push(tool.function_name().to_string());
        }
    }
    let actual = runner
        .build_universal_tools()
        .await
        .expect("universal tools")
        .into_iter()
        .map(|tool| tool.function_name().to_string())
        .collect::<Vec<_>>();

    expected.sort();
    let mut actual = actual;
    actual.sort();
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn build_universal_tools_uses_override_when_present() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-tool-override".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        Some(VTCodeConfig::default()),
        None,
    ))
    .await
    .expect("runner");

    runner.set_tool_definitions_override(vec![crate::llm::provider::ToolDefinition::function(
        "only_tool".to_string(),
        "Only tool".to_string(),
        json!({ "type": "object" }),
    )]);

    let tools = runner.build_universal_tools().await.expect("tool override");

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].function_name(), "only_tool");
}

#[tokio::test]
async fn active_primary_agent_policy_filters_provider_exposure_and_execution() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-primary-agent-policy".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        Some(VTCodeConfig::default()),
        None,
    ))
    .await
    .expect("runner");

    let mut spec = vtcode_config::builtin_primary_auto_agent();
    spec.tools = Some(vec![tools::READ_FILE.to_string()]);
    spec.permissions = AgentPermissionsConfig::new(PermissionDefault::Deny);
    runner.set_active_primary_agent(ActivePrimaryAgent::from_spec(&spec));

    assert!(!runner.is_tool_exposed(tools::READ_FILE).await);
    assert!(!runner.is_tool_exposed(tools::UNIFIED_EXEC).await);

    let snapshot = runner
        .build_universal_tool_snapshot()
        .await
        .expect("permission-filtered snapshot");
    assert_provider_hides_tool(&snapshot, tools::READ_FILE);
    assert_provider_hides_tool(&snapshot, tools::UNIFIED_EXEC);

    let mut session_state =
        AgentSessionState::new("thread-primary-agent-policy".to_string(), 1, 1, 8_000);
    let denied = runner
        .admit_tool_call(
            tools::READ_FILE,
            json!({ "path": "Cargo.toml" }),
            &mut session_state,
        )
        .expect_err("primary-agent deny should block execution");

    assert!(
        denied
            .to_string()
            .contains("denied by active primary agent 'auto'")
    );
}

#[tokio::test]
async fn explicit_tool_policy_deny_filters_runtime_state_and_allowed_tools() {
    let temp = TempDir::new().expect("tempdir");
    let runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-explicit-policy-deny".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        Some(VTCodeConfig::default()),
        None,
    ))
    .await
    .expect("runner");

    runner
        .tool_registry
        .set_tool_policy(tools::UNIFIED_EXEC, ToolPolicy::Deny)
        .await
        .expect("set policy");

    let snapshot = runner
        .build_universal_tool_snapshot()
        .await
        .expect("snapshot");
    assert_provider_hides_tool(&snapshot, tools::UNIFIED_EXEC);
    assert_provider_exposes_tool(&snapshot, tools::READ_FILE);

    let choice = ToolChoice::allowed_tools_auto(snapshot.active_tool_names.as_ref().clone());
    let ToolChoice::AllowedTools(choice) = choice else {
        panic!("expected allowed-tools choice");
    };
    assert!(
        !choice.tools.contains(&tools::UNIFIED_EXEC.to_string()),
        "provider-native advisory allowed_tools must use the policy-filtered subset"
    );
}

#[tokio::test]
async fn category_read_deny_filters_advertised_active_tools() {
    let temp = TempDir::new().expect("tempdir");
    let mut config = VTCodeConfig::default();
    config.permissions.deny = vec!["read".to_string()];
    let runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-read-category-deny".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        Some(config),
        None,
    ))
    .await
    .expect("runner");

    let snapshot = runner
        .build_universal_tool_snapshot()
        .await
        .expect("snapshot");
    assert_provider_hides_tool(&snapshot, tools::READ_FILE);
    assert_provider_exposes_tool(&snapshot, tools::UNIFIED_EXEC);
}

#[tokio::test]
async fn category_bash_deny_filters_advertised_active_tools_and_allowed_tools() {
    let temp = TempDir::new().expect("tempdir");
    let mut config = VTCodeConfig::default();
    config.permissions.deny = vec!["bash".to_string()];
    let runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-bash-category-deny".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        Some(config),
        None,
    ))
    .await
    .expect("runner");

    let snapshot = runner
        .build_universal_tool_snapshot()
        .await
        .expect("snapshot");
    assert_provider_hides_tool(&snapshot, tools::UNIFIED_EXEC);
    assert_provider_exposes_tool(&snapshot, tools::READ_FILE);

    let choice = ToolChoice::allowed_tools_auto(snapshot.active_tool_names.as_ref().clone());
    let ToolChoice::AllowedTools(choice) = choice else {
        panic!("expected allowed-tools choice");
    };
    assert!(!choice.tools.contains(&tools::UNIFIED_EXEC.to_string()));
}

#[tokio::test]
async fn category_edit_deny_filters_advertised_file_tool_conservatively() {
    let temp = TempDir::new().expect("tempdir");
    let mut config = VTCodeConfig::default();
    config.permissions.deny = vec!["edit".to_string()];
    let runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-edit-category-deny".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        Some(config),
        None,
    ))
    .await
    .expect("runner");

    let snapshot = runner
        .build_universal_tool_snapshot()
        .await
        .expect("snapshot");
    assert_provider_hides_tool(&snapshot, tools::UNIFIED_FILE);
    assert_provider_exposes_tool(&snapshot, tools::UNIFIED_EXEC);
}

#[tokio::test]
async fn category_write_deny_filters_advertised_file_tool_conservatively() {
    let temp = TempDir::new().expect("tempdir");
    let mut config = VTCodeConfig::default();
    config.permissions.deny = vec!["write".to_string()];
    let runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-write-category-deny".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        Some(config),
        None,
    ))
    .await
    .expect("runner");

    let snapshot = runner
        .build_universal_tool_snapshot()
        .await
        .expect("snapshot");
    assert_provider_hides_tool(&snapshot, tools::UNIFIED_FILE);
    assert_provider_exposes_tool(&snapshot, tools::UNIFIED_EXEC);
}

#[tokio::test]
async fn webfetch_domain_deny_filters_representative_unified_search_tool() {
    let temp = TempDir::new().expect("tempdir");
    let mut config = VTCodeConfig::default();
    config.permissions.deny = vec!["webfetch(domain:example.com)".to_string()];
    let runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-webfetch-domain-deny".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        Some(config),
        None,
    ))
    .await
    .expect("runner");

    let snapshot = runner
        .build_universal_tool_snapshot()
        .await
        .expect("snapshot");
    assert_provider_hides_tool(&snapshot, tools::UNIFIED_SEARCH);
    assert_provider_exposes_tool(&snapshot, tools::UNIFIED_EXEC);
}

#[tokio::test]
async fn planning_mode_filters_provider_facing_mutating_tools() {
    let temp = TempDir::new().expect("tempdir");
    let runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-planning-mode-tools".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        Some(VTCodeConfig::default()),
        None,
    ))
    .await
    .expect("runner");

    runner.tool_registry.enable_planning();

    let snapshot = runner
        .build_universal_tool_snapshot()
        .await
        .expect("snapshot");
    assert_provider_hides_tool(&snapshot, tools::APPLY_PATCH);
    assert_provider_exposes_tool(&snapshot, tools::READ_FILE);
}

#[tokio::test]
async fn active_primary_agent_policy_filters_provider_snapshot_to_allowed_tools() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-primary-agent-stable-catalogue".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        Some(VTCodeConfig::default()),
        None,
    ))
    .await
    .expect("runner");

    let baseline = runner
        .build_universal_tool_snapshot()
        .await
        .expect("baseline snapshot");
    assert_provider_exposes_tool(&baseline, tools::READ_FILE);
    assert_provider_exposes_tool(&baseline, tools::UNIFIED_EXEC);

    let mut spec = vtcode_config::builtin_primary_auto_agent();
    spec.tools = Some(vec![tools::READ_FILE.to_string()]);
    runner.set_active_primary_agent(ActivePrimaryAgent::from_spec(&spec));

    let restricted = runner
        .build_universal_tool_snapshot()
        .await
        .expect("restricted snapshot");
    let baseline_names = provider_tool_names(&baseline);
    let restricted_names = provider_tool_names(&restricted);
    assert_ne!(restricted_names, baseline_names);
    assert_provider_exposes_tool(&restricted, tools::READ_FILE);
    assert_provider_hides_tool(&restricted, tools::UNIFIED_EXEC);
}

#[tokio::test]
async fn normalize_tool_args_applies_transform_after_defaults() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-tool-transform".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        Some(VTCodeConfig::default()),
        None,
    ))
    .await
    .expect("runner");
    runner.set_tool_arg_transform(Arc::new(|name, value| {
        let mut obj = value.as_object().cloned().expect("object args");
        obj.insert("tool_name".to_string(), json!(name));
        serde_json::Value::Object(obj)
    }));

    let mut state = AgentSessionState::new("session".to_string(), 5, 5, 10_000);
    state.last_dir_path = Some(temp.path().display().to_string());
    let normalized =
        runner.normalize_tool_args(tools::UNIFIED_SEARCH, json!({"action": "list"}), &mut state);

    assert_eq!(normalized["tool_name"], tools::UNIFIED_SEARCH);
    assert_eq!(normalized["path"], json!(temp.path().display().to_string()));
}

#[tokio::test]
async fn review_tool_allowlist_excludes_mutating_and_plan_only_tools() {
    let temp = TempDir::new().expect("tempdir");
    let runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-review".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        Some(VTCodeConfig::default()),
        None,
    ))
    .await
    .expect("runner");

    let allowlist = runner
        .review_tool_allowlist(&[
            tools::UNIFIED_FILE.to_string(),
            tools::UNIFIED_EXEC.to_string(),
            "task_tracker".to_string(),
            "start_planning".to_string(),
        ])
        .await;

    assert_eq!(allowlist, vec![tools::UNIFIED_FILE.to_string()]);
}

#[tokio::test]
async fn review_tool_allowlist_expands_wildcard_read_only() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-review-wildcard".to_string(),
        RunnerSettings {
            reasoning_effort: None,
            verbosity: None,
        },
        None,
        ThreadBootstrap::new(None),
        Some(VTCodeConfig::default()),
        None,
    ))
    .await
    .expect("runner");

    runner
        .enable_full_auto(&[tools::UNIFIED_FILE.to_string()])
        .await;

    assert!(runner.is_tool_exposed(tools::UNIFIED_FILE).await);
    assert!(!runner.is_tool_exposed(tools::UNIFIED_EXEC).await);
}
