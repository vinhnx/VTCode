use super::*;

#[tokio::test]
async fn test_skill_tool_adapter_exposes_underlying_skill_name() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };

    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Instructions".to_string()).expect("failed to create skill");

    let adapter = SkillToolAdapter::new(skill);
    assert_eq!(adapter.skill().name(), "test-skill");
}
#[tokio::test]
async fn test_skill_tool_adapter_execute() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };

    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");

    let adapter = SkillToolAdapter::new(skill);
    let args = serde_json::json!({"test": "value"});
    let result = adapter.execute(args).await;

    assert!(result.is_ok());
    let res = result.unwrap();
    assert_eq!(res["skill_name"], "test-skill");
    assert_eq!(res["status"], "executing");
}
#[tokio::test]
async fn test_fork_skill_adapter_uses_fork_executor() {
    let manifest = SkillManifest {
        name: "fork-skill".to_string(),
        description: "Forked skill".to_string(),
        context: Some("fork".to_string()),
        vtcode_native: Some(true),
        ..Default::default()
    };

    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");

    let adapter = SkillToolAdapter::with_fork_executor(skill, Arc::new(FakeForkExecutor));
    let args = serde_json::json!({"task": "value"});
    let result = adapter.execute(args.clone()).await.expect("fork execution");

    assert_eq!(result["execution_context"], "fork");
    assert_eq!(result["delegate_session_id"], "child-session");
    assert_eq!(result["echo"], args);
}
#[tokio::test]
async fn blank_skill_input_uses_default_prompt_for_sub_llm() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;

    let result = execute_skill_with_sub_llm(
        &skill,
        String::new(),
        &EchoFirstUserProvider,
        &mut registry,
        Vec::new(),
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("blank input should be normalized");

    assert_eq!(result, EMPTY_SKILL_INPUT_PROMPT);
}
#[tokio::test]
async fn non_empty_skill_input_is_preserved_for_sub_llm() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;

    let result = execute_skill_with_sub_llm(
        &skill,
        "security".to_string(),
        &EchoFirstUserProvider,
        &mut registry,
        Vec::new(),
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("non-empty input should be preserved");

    assert_eq!(result, "security");
}
#[tokio::test]
async fn skill_executor_uses_normalized_stream_when_non_streaming_is_unsupported() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let provider = StreamingOnlySkillProvider { stream_calls: Mutex::new(0) };

    let result = execute_skill_with_sub_llm(
        &skill,
        "review".to_string(),
        &provider,
        &mut registry,
        Vec::new(),
        "gpt-5.2-codex".to_string(),
    )
    .await
    .expect("streaming-only skill execution should succeed");

    assert_eq!(result, "streamed skill result");
    assert_eq!(*provider.stream_calls.lock().expect("stream calls mutex"), 1);
}
#[tokio::test]
async fn skill_executor_errors_when_final_response_has_no_visible_content() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;

    let error = execute_skill_with_sub_llm(
        &skill,
        "review".to_string(),
        &EmptyFinalSkillProvider,
        &mut registry,
        Vec::new(),
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect_err("empty final response should be visible as an error");

    assert!(error.to_string().contains("completed without a visible final response"));
}
#[tokio::test]
async fn skill_executor_allows_tool_only_response_before_final_content() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let tool_name = "tool_only_skill_test_tool";
    let tool_calls = Arc::new(Mutex::new(0usize));
    registry
        .register_tool(
            ToolRegistration::from_tool_instance(
                tool_name,
                CapabilityLevel::CodeSearch,
                CountingSkillTool { calls: Arc::clone(&tool_calls) },
            )
            .with_network_access(ToolNetworkAccess::Local),
        )
        .await
        .expect("register tool");
    registry.allow_all_tools().await.expect("allow tools");
    let provider = ToolOnlyThenFinalizeProvider { tool_name, calls: Mutex::new(0) };

    let result = execute_skill_with_sub_llm(
        &skill,
        "review".to_string(),
        &provider,
        &mut registry,
        vec![ToolDefinition::function(
            tool_name.to_string(),
            "Tool-only test tool".to_string(),
            json!({"type": "object"}),
        )],
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("tool-only response should continue to final content");

    assert_eq!(result, "finalized after tool-only response");
    assert_eq!(*tool_calls.lock().expect("tool calls mutex"), 1);
}
#[tokio::test]
async fn skill_executor_continues_after_stop_response_with_tool_calls() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let tool_name = "stop_finish_reason_skill_test_tool";
    let tool_calls = Arc::new(Mutex::new(0usize));
    registry
        .register_tool(
            ToolRegistration::from_tool_instance(
                tool_name,
                CapabilityLevel::CodeSearch,
                CountingSkillTool { calls: Arc::clone(&tool_calls) },
            )
            .with_network_access(ToolNetworkAccess::Local),
        )
        .await
        .expect("register tool");
    registry.allow_all_tools().await.expect("allow tools");
    let provider = StopWithToolCallsThenFinalizeProvider { tool_name, calls: Mutex::new(0) };

    let result = execute_skill_with_sub_llm(
        &skill,
        "review".to_string(),
        &provider,
        &mut registry,
        vec![ToolDefinition::function(
            tool_name.to_string(),
            "Stop finish reason test tool".to_string(),
            json!({"type": "object"}),
        )],
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("stop response with tool calls should continue to final content");

    assert_eq!(result, "finalized after stop tool call");
    assert_eq!(*provider.calls.lock().expect("provider calls mutex"), 2);
    assert_eq!(*tool_calls.lock().expect("tool calls mutex"), 1);
}
#[tokio::test]
async fn skill_executor_forces_final_synthesis_after_unknown_tool() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    registry.allow_all_tools().await.expect("allow tools");
    let provider = UnknownToolThenFinalizeProvider { calls: Mutex::new(0) };

    let result = execute_skill_with_sub_llm(
        &skill,
        "review".to_string(),
        &provider,
        &mut registry,
        vec![ToolDefinition::function(
            "read_file".to_string(),
            "Read".to_string(),
            json!({"type": "object"}),
        )],
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("unknown tool should trigger final synthesis");

    assert_eq!(result, "finalized after unknown tool");
}
#[tokio::test]
async fn skill_executor_blocks_registered_tool_outside_filtered_scope() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let tool_name = "skill_out_of_scope_test_tool";
    let tool_calls = Arc::new(Mutex::new(0usize));
    registry
        .register_tool(
            ToolRegistration::from_tool_instance(
                tool_name,
                CapabilityLevel::CodeSearch,
                CountingSkillTool { calls: Arc::clone(&tool_calls) },
            )
            .with_network_access(ToolNetworkAccess::Local),
        )
        .await
        .expect("register tool");
    registry.allow_all_tools().await.expect("allow tools");
    let provider = OutOfScopeToolThenFinalizeProvider { tool_name, calls: Mutex::new(0) };

    let result = execute_skill_with_sub_llm(
        &skill,
        "review".to_string(),
        &provider,
        &mut registry,
        vec![ToolDefinition::function(
            "read_file".to_string(),
            "Read".to_string(),
            json!({"type": "object"}),
        )],
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("out-of-scope tool should trigger final synthesis");

    assert_eq!(result, "finalized after out-of-scope tool");
    assert_eq!(*tool_calls.lock().expect("tool calls mutex"), 0);
}
#[tokio::test]
async fn skill_executor_skips_repeated_tool_call_and_finalizes() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let tool_name = "skill_loop_test_tool";
    let tool_calls = Arc::new(Mutex::new(0usize));
    registry
        .register_tool(
            ToolRegistration::from_tool_instance(
                tool_name,
                CapabilityLevel::CodeSearch,
                CountingSkillTool { calls: Arc::clone(&tool_calls) },
            )
            .with_network_access(ToolNetworkAccess::Local),
        )
        .await
        .expect("register tool");
    registry.allow_all_tools().await.expect("allow tools");
    let provider = RepeatToolThenFinalizeProvider { tool_name, calls: Mutex::new(0) };

    let result = execute_skill_with_sub_llm(
        &skill,
        "review".to_string(),
        &provider,
        &mut registry,
        vec![ToolDefinition::function(
            tool_name.to_string(),
            "Loop test tool".to_string(),
            json!({"type": "object"}),
        )],
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("looping tool calls should force a final synthesis");

    assert_eq!(result, "finalized after loop detection");
    assert_eq!(*tool_calls.lock().expect("tool calls mutex"), 1);
}
#[tokio::test]
async fn skill_executor_forces_final_synthesis_after_max_iterations() {
    let manifest = SkillManifest {
        name: "test-skill".to_string(),
        description: "Test skill".to_string(),
        vtcode_native: Some(true),
        ..Default::default()
    };
    let skill =
        Skill::new(manifest, PathBuf::from("/tmp"), "# Test Instructions".to_string()).expect("failed to create skill");
    let workspace = tempdir().expect("temp workspace");
    let mut registry = ToolRegistry::new(workspace.path().to_path_buf()).await;
    let tool_calls = Arc::new(Mutex::new(0usize));
    let mut available_tools = Vec::with_capacity(MAX_SKILL_LLM_ITERATIONS);
    let mut tool_names = Vec::with_capacity(MAX_SKILL_LLM_ITERATIONS);

    for index in 0..MAX_SKILL_LLM_ITERATIONS {
        let tool_name = format!("skill_iteration_test_tool_{index}");
        registry
            .register_tool(
                ToolRegistration::from_tool_instance(
                    tool_name.as_str(),
                    CapabilityLevel::CodeSearch,
                    CountingSkillTool { calls: Arc::clone(&tool_calls) },
                )
                .with_network_access(ToolNetworkAccess::Local),
            )
            .await
            .unwrap_or_else(|error| panic!("register tool {tool_name}: {error}"));
        available_tools.push(ToolDefinition::function(
            tool_name.clone(),
            format!("Iteration tool {index}"),
            json!({"type": "object"}),
        ));
        tool_names.push(tool_name);
    }

    registry.allow_all_tools().await.expect("allow tools");
    let provider = MaxIterationsThenFinalizeProvider { tool_names, calls: Mutex::new(0) };

    let result = execute_skill_with_sub_llm(
        &skill,
        "analyze".to_string(),
        &provider,
        &mut registry,
        available_tools,
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("max-iteration recovery should force a final synthesis");

    assert_eq!(result, "finalized after max iterations");
    assert_eq!(*tool_calls.lock().expect("tool calls mutex"), MAX_SKILL_LLM_ITERATIONS);
}
