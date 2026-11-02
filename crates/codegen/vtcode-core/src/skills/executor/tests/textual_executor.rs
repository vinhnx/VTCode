use super::*;

#[tokio::test]
async fn skill_executor_runs_textual_tool_markup_without_native_calls() {
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
    // Register under the canonical name the textual fallback produces so the
    // wiring (parse -> scope check -> registry dispatch) is exercised.
    let tool_name = tool_constants::EXEC_COMMAND;
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
    let provider = TextualToolThenFinalizeProvider { calls: Mutex::new(0) };

    let result = execute_skill_with_sub_llm(
        &skill,
        "review".to_string(),
        &provider,
        &mut registry,
        vec![ToolDefinition::function(
            tool_name.to_string(),
            "Textual fallback test tool".to_string(),
            json!({"type": "object"}),
        )],
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("textual tool markup should execute and finalize");

    assert_eq!(result, "finalized after textual tool call");
    assert_eq!(*provider.calls.lock().expect("provider calls mutex"), 2);
    assert_eq!(*tool_calls.lock().expect("tool calls mutex"), 1);
}
#[tokio::test]
async fn skill_executor_forces_final_synthesis_after_textual_unknown_tool() {
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
    let provider = TextualUnknownToolThenFinalizeProvider { calls: Mutex::new(0) };

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
    .expect("textual unknown tool should trigger final synthesis");

    assert_eq!(result, "finalized after textual unknown tool");
}
/// Textual `tool_call` markup must not bypass skill tool scope: a call for a tool
/// absent from the skill's tool definitions is denied before registry dispatch
/// (even when the registry has the tool registered and allowed) and forces
/// tool-free synthesis.
#[tokio::test]
async fn skill_executor_denies_textual_tool_markup_outside_skill_scope() {
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
    // Registry has exec_command available; skill scope must still deny it.
    let tool_name = tool_constants::EXEC_COMMAND;
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
    let provider = TextualOutOfScopeToolThenFinalizeProvider { calls: Mutex::new(0) };

    let result = execute_skill_with_sub_llm(
        &skill,
        "review".to_string(),
        &provider,
        &mut registry,
        // Skill only sees read_file; textual bash/exec_command must be denied.
        vec![ToolDefinition::function(
            "read_file".to_string(),
            "Read".to_string(),
            json!({"type": "object"}),
        )],
        "gpt-5.1-codex".to_string(),
    )
    .await
    .expect("out-of-scope textual tool should force final synthesis");

    assert_eq!(result, "finalized after out-of-scope textual tool");
    assert_eq!(
        *tool_calls.lock().expect("tool calls mutex"),
        0,
        "out-of-scope textual tool must never reach the registry"
    );
    assert_eq!(*provider.calls.lock().expect("provider calls mutex"), 2);
}
