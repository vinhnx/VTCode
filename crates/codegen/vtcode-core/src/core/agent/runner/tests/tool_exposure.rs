#![allow(
    missing_docs,
    reason = "Intentional compatibility, platform, or test-only suppression."
)]

use super::*;

#[tokio::test]
async fn headless_planning_prompt_preserves_canonical_contract_at_both_densities() {
    use crate::config::types::{ShellPromptProfile, SystemPromptMode};
    use crate::prompts::system::*;

    for mode in [
        SystemPromptMode::Default,
        SystemPromptMode::Minimal,
        SystemPromptMode::Lightweight,
        SystemPromptMode::Specialized,
    ] {
        for environment in [false, true] {
            for budget in [100_000, 1] {
                let temp = TempDir::new().expect("workspace");
                let mut config = VTCodeConfig::default();
                config.agent.system_prompt_mode = mode;
                config.agent.include_temporal_context = false;
                config.agent.include_working_directory = environment;
                config.agent.instruction_max_bytes = 0;
                config.agent.shell_prompt_profile = ShellPromptProfile::UnixLike;
                config.agent.max_system_prompt_tokens = budget;
                config.agent.trim_system_prompt = false;
                config.tools.profile = ToolProfile::AdvancedVtCode;
                let mut runner = Box::pin(AgentRunner::new_with_bootstrap(
                    AgentType::Single,
                    ModelId::default(),
                    "test-key".to_string(),
                    temp.path().to_path_buf(),
                    "thread-planning-contract".to_string(),
                    RunnerSettings { reasoning_effort: None, verbosity: None },
                    None,
                    ThreadBootstrap::new(None),
                    Some(config),
                    None,
                ))
                .await
                .expect("runner");
                runner.provider_client = Box::new(RecordingQueuedProvider::new(Vec::new()));
                runner.tool_registry.enable_planning();
                let bundle = runner
                    .build_validated_runtime_prompt_bundle(false)
                    .await
                    .expect("planning prompt bundle");
                let prompt = bundle.request_envelope.system_prompt();
                for line in [
                    PLANNING_WORKFLOW_PLAN_PERSISTENCE_POLICY_LINE,
                    PLANNING_WORKFLOW_PLAN_QUALITY_LINE,
                    PLANNING_WORKFLOW_RESEARCH_SCOPE_LINE,
                    PLANNING_WORKFLOW_PLAN_POLICY_LINE,
                ] {
                    assert_eq!(prompt.matches(line).count(), 1, "{mode:?}, environment={environment}, budget={budget}");
                }
                assert_eq!(prompt.matches("## Active Tools").count(), 1);
                assert_eq!(prompt.matches("## Environment").count(), usize::from(environment));
                assert!(prompt.contains(PLANNING_WORKFLOW_NO_REQUEST_USER_INPUT_POLICY_LINE));
                for duplicate in [
                    "Monitor the available planning tool-loop budget",
                    "Every implementation step in the final plan must",
                    "emit only one `<proposed_plan>` block",
                    "Stop research when the plan is specified or the budget is near",
                ] {
                    assert!(!prompt.contains(duplicate));
                }
                assert_eq!(prompt.contains("- Planning is read-only."), budget == 1);
                let rebuilt = runner
                    .build_validated_runtime_prompt_bundle(false)
                    .await
                    .expect("rebuilt bundle");
                assert_eq!(prompt, rebuilt.request_envelope.system_prompt());
            }
        }
    }
}

#[tokio::test]
async fn full_auto_allowlist_retains_explicit_internal_tool_without_advertising_it() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-allowlist".to_string(),
        RunnerSettings { reasoning_effort: None, verbosity: None },
        None,
        ThreadBootstrap::new(None),
        Some(VTCodeConfig::default()),
        None,
    ))
    .await
    .expect("runner");

    runner.enable_full_auto(&[tools::UNIFIED_FILE.to_string()]).await;

    assert!(runner.is_tool_exposed(tools::UNIFIED_FILE).await);
    assert!(!runner.is_tool_exposed(tools::UNIFIED_EXEC).await);

    let snapshot = runner.build_universal_tool_snapshot().await.expect("full-auto snapshot");
    assert!(!snapshot.active_tool_names.contains(&tools::UNIFIED_FILE.to_string()));
    assert!(!snapshot.active_tool_names.contains(&tools::UNIFIED_EXEC.to_string()));
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
        RunnerSettings { reasoning_effort: None, verbosity: None },
        None,
        ThreadBootstrap::new(None),
        Some(VTCodeConfig::default()),
        None,
    ))
    .await
    .expect("runner");

    assert!(runner.is_valid_tool(tools::EXEC_COMMAND).await);
    assert!(runner.is_valid_tool(tools::WRITE_STDIN).await);
    assert!(runner.is_valid_tool(tools::APPLY_PATCH).await);
    assert!(!runner.is_valid_tool(tools::READ_FILE).await);
    assert!(!runner.is_valid_tool(tools::WRITE_FILE).await);
    assert!(!runner.is_valid_tool(tools::UNIFIED_EXEC).await);
    assert!(!runner.is_valid_tool(tools::UNIFIED_FILE).await);
    assert!(!runner.is_valid_tool(tools::UNIFIED_SEARCH).await);
    assert!(!runner.is_valid_tool("exec_code").await);

    let mut session_state = AgentSessionState::new("thread-tool-validation".to_string(), 1, 1, 8_000);
    let exec_call = runner
        .admit_tool_call(tools::EXEC_COMMAND, json!({ "cmd": "echo hi" }), &mut session_state)
        .expect("exec_command should be admitted by public name");
    assert_eq!(exec_call.canonical_name, tools::EXEC_COMMAND);
    assert_eq!(exec_call.effective_args["cmd"], "echo hi");

    let stdin_call = runner
        .admit_tool_call(tools::WRITE_STDIN, json!({ "session_id": "exec-1", "chars": "q" }), &mut session_state)
        .expect("write_stdin should be admitted by public name");
    assert_eq!(stdin_call.canonical_name, tools::WRITE_STDIN);
    assert_eq!(stdin_call.effective_args["chars"], "q");
}

#[tokio::test]
async fn build_universal_tools_matches_registry_agent_runner_snapshot() {
    let temp = TempDir::new().expect("tempdir");
    let mut runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-tool-snapshot".to_string(),
        RunnerSettings { reasoning_effort: None, verbosity: None },
        None,
        ThreadBootstrap::new(None),
        Some(VTCodeConfig::default()),
        None,
    ))
    .await
    .expect("runner");
    runner.provider_client = Box::new(RecordingQueuedProvider::new(Vec::new()));

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
                crate::llm::factory::infer_provider(Some(&runner.config().agent.provider), &runner.model),
                runner.provider_client.supports_responses_compaction(&runner.model),
                Some(runner.config()),
            ),
            anthropic_native_memory_enabled: crate::tools::handlers::anthropic_native_memory_enabled_for_runtime(
                crate::llm::factory::infer_provider(Some(&runner.config().agent.provider), &runner.model),
                &runner.model,
                Some(runner.config()),
            ),
            tool_profile: runner.config().tools.profile,
        })
        .await;
    let registry_tool_names = registry_tools
        .iter()
        .map(|tool| tool.function_name().to_string())
        .collect::<Vec<_>>();
    assert!(registry_tool_names.contains(&tools::EXEC_COMMAND.to_string()));
    assert!(registry_tool_names.contains(&tools::WRITE_STDIN.to_string()));
    assert!(!registry_tool_names.contains(&tools::UNIFIED_EXEC.to_string()));
    assert!(!registry_tool_names.contains(&tools::CODE_SEARCH.to_string()));
    let mut expected = Vec::new();
    for tool in registry_tools {
        if runner.is_tool_exposed(tool.function_name()).await {
            expected.push(tool.function_name().to_string());
        }
    }
    let snapshot = runner.build_universal_tool_snapshot().await.expect("universal tool snapshot");
    assert_provider_catalogues_inactive_tool(&snapshot, tools::CODE_SEARCH);
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
async fn advanced_tool_profile_reaches_agent_runner_catalogue_and_validation() {
    let temp = TempDir::new().expect("tempdir");
    let mut config = VTCodeConfig::default();
    config.tools.profile = ToolProfile::AdvancedVtCode;
    let runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-advanced-tool-profile".to_string(),
        RunnerSettings { reasoning_effort: None, verbosity: None },
        None,
        ThreadBootstrap::new(None),
        Some(config),
        None,
    ))
    .await
    .expect("runner");

    let snapshot = runner.build_universal_tool_snapshot().await.expect("advanced snapshot");

    assert_provider_exposes_tool(&snapshot, tools::CODE_SEARCH);
    assert!(runner.is_valid_tool(tools::CODE_SEARCH).await);
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
        RunnerSettings { reasoning_effort: None, verbosity: None },
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
        RunnerSettings { reasoning_effort: None, verbosity: None },
        None,
        ThreadBootstrap::new(None),
        Some(VTCodeConfig::default()),
        None,
    ))
    .await
    .expect("runner");

    let mut spec = vtcode_config::builtin_primary_auto_agent();
    spec.tools = Some(vec![tools::EXEC_COMMAND.to_string()]);
    spec.permissions = AgentPermissionsConfig::new(PermissionDefault::Deny);
    runner.set_active_primary_agent(ActivePrimaryAgent::from_spec(&spec));

    assert!(!runner.is_tool_exposed(tools::EXEC_COMMAND).await);
    assert!(!runner.is_tool_exposed(tools::WRITE_STDIN).await);

    let snapshot = runner
        .build_universal_tool_snapshot()
        .await
        .expect("permission-filtered snapshot");
    assert_provider_hides_tool(&snapshot, tools::EXEC_COMMAND);
    assert_provider_hides_tool(&snapshot, tools::WRITE_STDIN);

    let mut session_state = AgentSessionState::new("thread-primary-agent-policy".to_string(), 1, 1, 8_000);
    let denied = runner
        .admit_tool_call(tools::EXEC_COMMAND, json!({ "cmd": "echo denied" }), &mut session_state)
        .expect_err("primary-agent deny should block execution");

    assert!(denied.to_string().contains("denied by active primary agent 'auto'"));
}

#[tokio::test]
async fn advertisement_and_admission_preserve_global_and_agent_permission_precedence() {
    struct PermissionCase {
        global_deny: bool,
        global_ask: bool,
        agent_default: Option<PermissionDefault>,
        agent_deny: bool,
        exposed: bool,
    }

    for case in [
        PermissionCase {
            global_deny: true,
            global_ask: false,
            agent_default: Some(PermissionDefault::Auto),
            agent_deny: false,
            exposed: false,
        },
        PermissionCase {
            global_deny: false,
            global_ask: false,
            agent_default: Some(PermissionDefault::Auto),
            agent_deny: true,
            exposed: false,
        },
        PermissionCase {
            global_deny: false,
            global_ask: true,
            agent_default: Some(PermissionDefault::Auto),
            agent_deny: false,
            exposed: true,
        },
        PermissionCase {
            global_deny: false,
            global_ask: true,
            agent_default: Some(PermissionDefault::Deny),
            agent_deny: false,
            exposed: false,
        },
        PermissionCase {
            global_deny: true,
            global_ask: false,
            agent_default: None,
            agent_deny: false,
            exposed: false,
        },
        PermissionCase {
            global_deny: false,
            global_ask: true,
            agent_default: None,
            agent_deny: false,
            exposed: true,
        },
    ] {
        let temp = TempDir::new().expect("workspace");
        let mut config = VTCodeConfig::default();
        config.permissions.allow = vec!["Bash".to_string()];
        if case.global_deny {
            config.permissions.deny = vec!["Bash".to_string()];
        }
        if case.global_ask {
            config.permissions.ask = vec!["Bash".to_string()];
        }
        let mut runner = Box::pin(AgentRunner::new_with_bootstrap(
            AgentType::Single,
            ModelId::default(),
            "test-key".to_string(),
            temp.path().to_path_buf(),
            "thread-permission-precedence".to_string(),
            RunnerSettings { reasoning_effort: None, verbosity: None },
            None,
            ThreadBootstrap::new(None),
            Some(config),
            None,
        ))
        .await
        .expect("runner");
        runner.active_primary_agent = case.agent_default.map(|default| {
            let mut spec = vtcode_config::builtin_primary_auto_agent();
            spec.permissions = AgentPermissionsConfig::new(default);
            if case.agent_deny {
                spec.permissions.deny = vec!["Bash".to_string()];
                spec.permissions.allow = vec!["Bash".to_string()];
            }
            ActivePrimaryAgent::from_spec(&spec)
        });

        assert_eq!(runner.is_tool_exposed(tools::EXEC_COMMAND).await, case.exposed);
        let mut state = AgentSessionState::new("thread-permission-precedence".to_string(), 1, 1, 8_000);
        assert_eq!(
            runner
                .admit_tool_call(tools::EXEC_COMMAND, json!({"cmd": "true"}), &mut state)
                .is_ok(),
            case.exposed
        );
    }
}

#[tokio::test]
async fn advertisement_keeps_multi_action_tools_conservative_with_and_without_an_agent() {
    for has_agent in [false, true] {
        for deny_write in [false, true] {
            let temp = TempDir::new().expect("workspace");
            let mut config = VTCodeConfig::default();
            if deny_write {
                config.permissions.deny = vec!["Write".to_string()];
            }
            let mut runner = Box::pin(AgentRunner::new_with_bootstrap(
                AgentType::Single,
                ModelId::default(),
                "test-key".to_string(),
                temp.path().to_path_buf(),
                "thread-multi-action-permissions".to_string(),
                RunnerSettings { reasoning_effort: None, verbosity: None },
                None,
                ThreadBootstrap::new(None),
                Some(config),
                None,
            ))
            .await
            .expect("runner");
            runner.active_primary_agent =
                has_agent.then(|| ActivePrimaryAgent::from_spec(&vtcode_config::builtin_primary_auto_agent()));

            assert_eq!(runner.is_tool_permitted_for_advertisement(tools::UNIFIED_FILE).await, !deny_write);
            assert!(runner.is_tool_exposed(tools::APPLY_PATCH).await);
        }
    }
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
        RunnerSettings { reasoning_effort: None, verbosity: None },
        None,
        ThreadBootstrap::new(None),
        Some(VTCodeConfig::default()),
        None,
    ))
    .await
    .expect("runner");

    runner
        .tool_registry
        .set_tool_policy(tools::EXEC_COMMAND, ToolPolicy::Deny)
        .await
        .expect("set policy");

    let snapshot = runner.build_universal_tool_snapshot().await.expect("snapshot");
    assert_provider_hides_tool(&snapshot, tools::EXEC_COMMAND);
    assert_provider_exposes_tool(&snapshot, tools::WRITE_STDIN);

    let choice = ToolChoice::allowed_tools_auto(snapshot.active_tool_names.as_ref().clone());
    let ToolChoice::AllowedTools(choice) = choice else {
        panic!("expected allowed-tools choice");
    };
    assert!(
        !choice.tools.contains(&tools::EXEC_COMMAND.to_string()),
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
        RunnerSettings { reasoning_effort: None, verbosity: None },
        None,
        ThreadBootstrap::new(None),
        Some(config),
        None,
    ))
    .await
    .expect("runner");

    let snapshot = runner.build_universal_tool_snapshot().await.expect("snapshot");
    assert_provider_hides_tool(&snapshot, tools::READ_FILE);
    assert_provider_exposes_tool(&snapshot, tools::EXEC_COMMAND);
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
        RunnerSettings { reasoning_effort: None, verbosity: None },
        None,
        ThreadBootstrap::new(None),
        Some(config),
        None,
    ))
    .await
    .expect("runner");

    let snapshot = runner.build_universal_tool_snapshot().await.expect("snapshot");
    assert_provider_hides_tool(&snapshot, tools::EXEC_COMMAND);
    assert_provider_exposes_tool(&snapshot, tools::APPLY_PATCH);

    let choice = ToolChoice::allowed_tools_auto(snapshot.active_tool_names.as_ref().clone());
    let ToolChoice::AllowedTools(choice) = choice else {
        panic!("expected allowed-tools choice");
    };
    assert!(!choice.tools.contains(&tools::EXEC_COMMAND.to_string()));
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
        RunnerSettings { reasoning_effort: None, verbosity: None },
        None,
        ThreadBootstrap::new(None),
        Some(config),
        None,
    ))
    .await
    .expect("runner");

    let snapshot = runner.build_universal_tool_snapshot().await.expect("snapshot");
    assert_provider_hides_tool(&snapshot, tools::APPLY_PATCH);
    assert_provider_exposes_tool(&snapshot, tools::EXEC_COMMAND);
}

#[tokio::test]
async fn category_write_deny_keeps_edit_only_apply_patch_exposed() {
    let temp = TempDir::new().expect("tempdir");
    let mut config = VTCodeConfig::default();
    config.permissions.deny = vec!["write".to_string()];
    let runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-write-category-deny".to_string(),
        RunnerSettings { reasoning_effort: None, verbosity: None },
        None,
        ThreadBootstrap::new(None),
        Some(config),
        None,
    ))
    .await
    .expect("runner");

    let snapshot = runner.build_universal_tool_snapshot().await.expect("snapshot");
    assert_provider_exposes_tool(&snapshot, tools::APPLY_PATCH);
    assert_provider_exposes_tool(&snapshot, tools::EXEC_COMMAND);
}

#[tokio::test]
async fn webfetch_domain_deny_does_not_filter_code_search() {
    let temp = TempDir::new().expect("tempdir");
    let mut config = VTCodeConfig::default();
    config.permissions.deny = vec!["webfetch(domain:example.com)".to_string()];
    config.tools.profile = ToolProfile::AdvancedVtCode;
    let runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-webfetch-domain-deny".to_string(),
        RunnerSettings { reasoning_effort: None, verbosity: None },
        None,
        ThreadBootstrap::new(None),
        Some(config),
        None,
    ))
    .await
    .expect("runner");

    let snapshot = runner.build_universal_tool_snapshot().await.expect("snapshot");
    assert_provider_exposes_tool(&snapshot, tools::CODE_SEARCH);
    assert_provider_exposes_tool(&snapshot, tools::EXEC_COMMAND);
}

#[tokio::test]
async fn planning_mode_filters_provider_facing_mutating_tools() {
    let temp = TempDir::new().expect("tempdir");
    let mut config = VTCodeConfig::default();
    config.tools.profile = ToolProfile::AdvancedVtCode;
    let mut runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-planning-mode-tools".to_string(),
        RunnerSettings { reasoning_effort: None, verbosity: None },
        None,
        ThreadBootstrap::new(None),
        Some(config),
        None,
    ))
    .await
    .expect("runner");
    runner.provider_client = Box::new(RecordingQueuedProvider::new(Vec::new()));

    let before_planning = runner.build_universal_tool_snapshot().await.expect("snapshot before planning");
    assert_provider_exposes_tool(&before_planning, tools::EXEC_COMMAND);
    assert_provider_exposes_tool(&before_planning, tools::WRITE_STDIN);
    assert_provider_exposes_tool(&before_planning, tools::APPLY_PATCH);
    assert_provider_exposes_tool(&before_planning, tools::SEARCH_TOOLS);
    assert_provider_exposes_tool(&before_planning, tools::CODE_SEARCH);

    runner.tool_registry.enable_planning();

    let snapshot = runner.build_universal_tool_snapshot().await.expect("snapshot");
    assert_provider_catalogues_inactive_tool(&snapshot, tools::WRITE_STDIN);
    assert_provider_catalogues_inactive_tool(&snapshot, tools::APPLY_PATCH);
    assert_provider_exposes_tool(&snapshot, tools::SEARCH_TOOLS);
    assert_provider_hides_tool(&snapshot, tools::READ_FILE);
    assert_provider_exposes_tool(&snapshot, tools::EXEC_COMMAND);
    assert_provider_exposes_tool(&snapshot, tools::CODE_SEARCH);
    assert!(!runner.is_tool_exposed(tools::WRITE_STDIN).await);
    assert!(!runner.is_tool_exposed(tools::APPLY_PATCH).await);
    assert_task_tracker_snapshot_schema(&before_planning, 0, "^[1-9][0-9]*$");
    assert_task_tracker_snapshot_schema(&snapshot, 1, "^[1-9][0-9]*(\\.[1-9][0-9]*)*$");
}

#[tokio::test]
async fn runner_task_tracker_snapshot_schema_tracks_planning_state() {
    let temp = TempDir::new().expect("tempdir");
    let mut config = VTCodeConfig::default();
    config.tools.profile = ToolProfile::AdvancedVtCode;
    let mut runner = Box::pin(AgentRunner::new_with_bootstrap(
        AgentType::Single,
        ModelId::default(),
        "test-key".to_string(),
        temp.path().to_path_buf(),
        "thread-task-tracker-schema".to_string(),
        RunnerSettings { reasoning_effort: None, verbosity: None },
        None,
        ThreadBootstrap::new(None),
        Some(config),
        None,
    ))
    .await
    .expect("runner");
    runner.provider_client = Box::new(RecordingQueuedProvider::new(Vec::new()));

    let standard_snapshot = runner.build_universal_tool_snapshot().await.expect("standard snapshot");
    assert_task_tracker_snapshot_schema(&standard_snapshot, 0, "^[1-9][0-9]*$");

    runner.tool_registry.enable_planning();

    let planning_snapshot = runner.build_universal_tool_snapshot().await.expect("planning snapshot");
    assert_task_tracker_snapshot_schema(&planning_snapshot, 1, "^[1-9][0-9]*(\\.[1-9][0-9]*)*$");
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
        RunnerSettings { reasoning_effort: None, verbosity: None },
        None,
        ThreadBootstrap::new(None),
        Some(VTCodeConfig::default()),
        None,
    ))
    .await
    .expect("runner");

    let baseline = runner.build_universal_tool_snapshot().await.expect("baseline snapshot");
    assert_provider_exposes_tool(&baseline, tools::EXEC_COMMAND);
    assert_provider_exposes_tool(&baseline, tools::WRITE_STDIN);
    assert_provider_exposes_tool(&baseline, tools::APPLY_PATCH);
    assert_provider_exposes_tool(&baseline, tools::SEARCH_TOOLS);

    let mut spec = vtcode_config::builtin_primary_auto_agent();
    spec.tools = Some(vec![tools::EXEC_COMMAND.to_string()]);
    runner.set_active_primary_agent(ActivePrimaryAgent::from_spec(&spec));

    let restricted = runner.build_universal_tool_snapshot().await.expect("restricted snapshot");
    let baseline_names = provider_tool_names(&baseline);
    let restricted_names = provider_tool_names(&restricted);
    assert_ne!(restricted_names, baseline_names);
    assert_provider_exposes_tool(&restricted, tools::EXEC_COMMAND);
    assert_provider_hides_tool(&restricted, tools::WRITE_STDIN);
    assert_provider_hides_tool(&restricted, tools::APPLY_PATCH);
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
        RunnerSettings { reasoning_effort: None, verbosity: None },
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
    let normalized = runner.normalize_tool_args(tools::LIST_FILES, json!({}), &mut state);

    assert_eq!(normalized["tool_name"], tools::LIST_FILES);
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
        RunnerSettings { reasoning_effort: None, verbosity: None },
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
            tools::UNIFIED_SEARCH.to_string(),
            "task_tracker".to_string(),
            "start_planning".to_string(),
        ])
        .await;

    assert!(allowlist.is_empty());
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
        RunnerSettings { reasoning_effort: None, verbosity: None },
        None,
        ThreadBootstrap::new(None),
        Some(VTCodeConfig::default()),
        None,
    ))
    .await
    .expect("runner");

    let allowlist = runner.review_tool_allowlist(&[tools::WILDCARD_ALL.to_string()]).await;
    assert!(allowlist.contains(&tools::CODE_SEARCH.to_string()));

    runner.enable_full_auto(&allowlist).await;

    assert!(!runner.is_tool_exposed(tools::UNIFIED_FILE).await);
    assert!(!runner.is_tool_exposed(tools::UNIFIED_EXEC).await);
    let snapshot = runner.build_universal_tool_snapshot().await.expect("review snapshot");
    assert_provider_exposes_tool(&snapshot, tools::CODE_SEARCH);
    assert_provider_hides_tool(&snapshot, tools::APPLY_PATCH);
}

fn assert_task_tracker_snapshot_schema(
    snapshot: &SessionToolCatalogSnapshot,
    expected_minimum: u64,
    expected_pattern: &str,
) {
    let parameters = snapshot
        .snapshot
        .as_ref()
        .and_then(|tools| tools.iter().find(|tool| tool.function_name() == tools::TASK_TRACKER))
        .and_then(|tool| tool.function.as_ref())
        .map(|function| &function.parameters)
        .expect("task_tracker parameters should be present in snapshot");

    assert_eq!(parameters["properties"]["index"]["minimum"], json!(expected_minimum));
    assert_eq!(parameters["properties"]["index_path"]["pattern"], json!(expected_pattern));
}
