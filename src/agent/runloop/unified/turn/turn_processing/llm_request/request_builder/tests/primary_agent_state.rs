use super::*;

#[tokio::test]
async fn active_primary_agent_runtime_state_is_system_prompt_context() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing
        .add_tool_definition(ToolDefinition::function(
            "code_search".to_string(),
            "Search project files".to_string(),
            json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" },
                    "path": { "type": "string" },
                    "file_types": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "result_types": {
                        "type": "array",
                        "items": {
                            "type": "string",
                            "enum": ["definition", "usage", "text", "path"]
                        }
                    },
                    "max_results": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 100
                    }
                },
                "required": ["query"],
                "additionalProperties": false
            }),
        ))
        .await;
    let spec = test_primary_agent_spec("planner", "Plan carefully before editing.");
    backing.select_primary_agent_from_specs(&[spec], "planner");

    let built = {
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.push(uni::Message::user("hello".to_string()));
        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
            .await
            .expect("request should build")
    };

    assert_eq!(built.request.messages.len(), 1);
    assert_eq!(built.request.messages[0], uni::Message::user("hello".to_string()));
    let runtime_context = built.request.system_prompt.as_ref().expect("system prompt").as_ref();
    assert!(runtime_context.contains("## Active Primary Agent Runtime State"));
    assert!(runtime_context.contains("- Active agent: planner"));
    assert!(runtime_context.contains("- Effective request tools: code_search"));
    assert!(runtime_context.contains("- Session state: planning_workflow=false, full_auto=false"));
    assert!(!runtime_context.contains("auto_permission="));
    assert!(!runtime_context.contains("permission default"));
    assert!(runtime_context.contains("Plan carefully before editing."));
    assert_eq!(built.continuation_messages.as_slice(), [uni::Message::user("hello".to_string())]);
}
#[tokio::test]
async fn active_primary_agent_memory_appendix_uses_canonical_name_for_alias_selection() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let workspace = backing.workspace_path().to_path_buf();
    std::fs::create_dir_all(workspace.join(".vtcode/agent-memory/reviewer")).expect("canonical memory dir");
    std::fs::write(
        workspace.join(".vtcode/agent-memory/reviewer/MEMORY.md"),
        "# Reviewer Memory\n\n- Canonical reviewer memory.\n",
    )
    .expect("canonical memory");
    std::fs::create_dir_all(workspace.join(".vtcode/agent-memory/critic")).expect("alias memory dir");
    std::fs::write(
        workspace.join(".vtcode/agent-memory/critic/MEMORY.md"),
        "# Critic Memory\n\n- Alias memory must not load.\n",
    )
    .expect("alias memory");

    let mut spec = test_primary_agent_spec("reviewer", "Review carefully.");
    spec.memory = Some(SubagentMemoryScope::Project);
    spec.aliases = vec!["critic".to_string()];
    backing.select_primary_agent_from_specs(&[spec], "critic");

    let built = {
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.push(uni::Message::user("hello".to_string()));
        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
            .await
            .expect("request should build")
    };

    let runtime_context = system_prompt_text(&built.request);
    assert!(runtime_context.contains("### Memory Appendix"));
    assert!(runtime_context.contains("Primary-agent memory file:"));
    assert!(runtime_context.contains(".vtcode/agent-memory/reviewer/MEMORY.md"));
    assert!(runtime_context.contains("Canonical reviewer memory."));
    assert!(!runtime_context.contains("Alias memory must not load."));
    assert!(!runtime_context.contains("Create or update `MEMORY.md`"));
    assert!(!runtime_context.contains("Read and maintain `MEMORY.md`"));
}
#[tokio::test]
async fn active_primary_agent_missing_memory_is_noop_and_does_not_expand_tools() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.add_tool_definition(named_tool("code_search")).await;
    backing.add_tool_definition(named_tool("apply_patch")).await;
    let workspace = backing.workspace_path().to_path_buf();
    let memory_dir = workspace.join(".vtcode/agent-memory/planner");

    let mut spec = test_primary_agent_spec("planner", "Plan carefully.");
    spec.memory = Some(SubagentMemoryScope::Project);
    spec.tools = Some(vec!["code_search".to_string()]);
    spec.disallowed_tools = Vec::new();
    backing.select_primary_agent_from_specs(&[spec], "planner");

    let built = {
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.push(uni::Message::user("hello".to_string()));
        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
            .await
            .expect("request should build")
    };

    let runtime_context = system_prompt_text(&built.request);
    assert!(!runtime_context.contains("### Memory Appendix"));
    assert!(!runtime_context.contains("Create or update `MEMORY.md`"));
    assert!(!memory_dir.exists());
    assert_eq!(request_tool_names(&built.request), vec!["code_search"]);
}
#[tokio::test]
async fn active_primary_agent_memory_appendix_is_replaced_on_switch() {
    let mut backing = TestTurnProcessingBacking::new().await;
    let workspace = backing.workspace_path().to_path_buf();
    for (agent, memory) in [
        ("planner", "Planner-only durable note."),
        ("reviewer", "Reviewer-only durable note."),
    ] {
        let memory_dir = workspace.join(".vtcode/agent-memory").join(agent);
        std::fs::create_dir_all(&memory_dir).expect("memory dir");
        std::fs::write(memory_dir.join("MEMORY.md"), format!("- {memory}\n")).expect("memory");
    }

    let mut planner = test_primary_agent_spec("planner", "Planner instructions.");
    planner.memory = Some(SubagentMemoryScope::Project);
    let mut reviewer = test_primary_agent_spec("reviewer", "Reviewer instructions.");
    reviewer.memory = Some(SubagentMemoryScope::Project);

    backing.select_primary_agent_from_specs(std::slice::from_ref(&planner), "planner");
    let first_built = {
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.push(uni::Message::user("hello".to_string()));
        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
            .await
            .expect("first request should build")
    };

    backing.select_primary_agent_from_specs(std::slice::from_ref(&reviewer), "reviewer");
    let second_built = {
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.clear();
        ctx.working_history.push(uni::Message::user("hello".to_string()));
        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        build_turn_request(&mut ctx, 2, "noop-model", &snapshot, Some(320), None, false)
            .await
            .expect("second request should build")
    };

    let first_runtime = system_prompt_text(&first_built.request);
    let second_runtime = system_prompt_text(&second_built.request);
    assert!(first_runtime.contains("Planner-only durable note."));
    assert!(!first_runtime.contains("Reviewer-only durable note."));
    assert!(second_runtime.contains("Reviewer-only durable note."));
    assert!(!second_runtime.contains("Planner-only durable note."));
}
#[tokio::test]
async fn active_primary_agent_tool_allow_list_intersects_baseline_tools() {
    let mut backing = TestTurnProcessingBacking::new().await;
    backing.add_tool_definition(named_tool("code_search")).await;
    backing.add_tool_definition(named_tool("apply_patch")).await;
    backing.add_tool_definition(named_tool("exec_command")).await;
    let mut spec = test_primary_agent_spec("planner", "Use limited tools.");
    spec.tools = Some(vec!["code_search".to_string(), "missing_tool".to_string()]);
    spec.disallowed_tools = Vec::new();
    backing.select_primary_agent_from_specs(&[spec], "planner");

    let built = {
        let mut ctx = backing.turn_processing_context();
        ctx.working_history.push(uni::Message::user("hello".to_string()));
        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
            .await
            .expect("request should build")
    };

    assert_eq!(request_tool_names(&built.request), vec!["code_search"]);
    assert!(built.has_tools);
}
#[tokio::test]
async fn coordinator_selection_restricts_request_tools_and_restores_build_guidance() {
    let mut backing = TestTurnProcessingBacking::new().await;
    for name in [
        "matrix",
        "agent",
        "request_user_input",
        "code_search",
        "apply_patch",
        "exec_command",
    ] {
        backing.add_tool_definition(named_tool(name)).await;
    }
    let specs = [
        vtcode_config::builtin_primary_coordinator_agent(),
        vtcode_config::builtin_primary_build_agent(),
    ];
    backing.select_primary_agent_from_specs(&specs, "coordinator");
    let coordinated = {
        let mut ctx = backing.turn_processing_context();
        ctx.context_manager
            .set_base_system_prompt(vtcode_core::prompts::system::minimal_system_prompt().to_string());
        ctx.working_history
            .push(uni::Message::user("Inspect the asymmetric matrix".to_string()));
        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        build_turn_request(&mut ctx, 1, "noop-model", &snapshot, Some(320), None, false)
            .await
            .expect("coordinator request")
    };
    let tool_names = request_tool_names(&coordinated.request);
    assert!(tool_names.contains(&"matrix".to_string()));
    for forbidden in ["code_search", "apply_patch", "exec_command"] {
        assert!(!tool_names.contains(&forbidden.to_string()), "{forbidden}");
    }
    let coordinator_prompt = system_prompt_text(&coordinated.request);
    assert!(coordinator_prompt.contains("## Coordinator Role"));
    assert!(!coordinator_prompt.contains("Prefer skills and tool search"));

    backing.select_primary_agent_from_specs(&specs, "build");
    let built = {
        let mut ctx = backing.turn_processing_context();
        let snapshot = capture_turn_request_snapshot(&mut ctx, "noop-model", false);
        build_turn_request(&mut ctx, 2, "noop-model", &snapshot, Some(320), None, false)
            .await
            .expect("Build request")
    };
    assert!(request_tool_names(&built.request).contains(&"exec_command".to_string()));
    let build_prompt = system_prompt_text(&built.request);
    assert!(!build_prompt.contains("## Coordinator Role"));
    assert!(build_prompt.contains("Prefer skills and tool search"));
}
