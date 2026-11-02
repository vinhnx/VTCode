use super::*;

#[test]
fn filter_child_tools_keeps_public_read_tools_and_removes_mutation_tools() {
    let defs = vec![
        ToolDefinition::function(
            tools::SPAWN_AGENT.to_string(),
            "Spawn".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::CODE_SEARCH.to_string(),
            "Search".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::EXEC_COMMAND.to_string(),
            "Exec".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::APPLY_PATCH.to_string(),
            "Patch".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::WRITE_STDIN.to_string(),
            "Continue".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::REQUEST_USER_INPUT.to_string(),
            "Ask".to_string(),
            serde_json::json!({"type": "object"}),
        ),
    ];
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "explorer")
        .expect("explorer");
    let filtered = filter_child_tools(&spec, defs, true, false);
    let names = filtered.iter().map(ToolDefinition::function_name).collect::<Vec<_>>();
    assert_eq!(names, vec![tools::CODE_SEARCH]);
}
#[test]
fn filter_child_tools_keeps_delegation_tools_when_nested_delegation_allowed() {
    let defs = vec![
        ToolDefinition::function(tools::AGENT.to_string(), "Agent".to_string(), serde_json::json!({"type": "object"})),
        ToolDefinition::function(
            tools::SPAWN_AGENT.to_string(),
            "Spawn".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::SEND_INPUT.to_string(),
            "Send".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::WAIT_AGENT.to_string(),
            "Wait".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::SPAWN_BACKGROUND_SUBPROCESS.to_string(),
            "Bg".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::CODE_SEARCH.to_string(),
            "Search".to_string(),
            serde_json::json!({"type": "object"}),
        ),
    ];
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");

    let filtered = filter_child_tools(&spec, defs, false, true);
    let names = filtered.iter().map(ToolDefinition::function_name).collect::<Vec<_>>();

    assert!(names.contains(&tools::AGENT), "agent tool stays exposed when nested delegation is allowed");
    assert!(names.contains(&tools::SPAWN_AGENT));
    assert!(names.contains(&tools::SEND_INPUT));
    assert!(names.contains(&tools::WAIT_AGENT));
    assert!(
        !names.contains(&tools::SPAWN_BACKGROUND_SUBPROCESS),
        "background subprocess alias stays blocked for children"
    );
    assert!(names.contains(&tools::CODE_SEARCH));
}
#[test]
fn filter_child_tools_removes_delegation_tools_by_default() {
    let defs = vec![
        ToolDefinition::function(tools::AGENT.to_string(), "Agent".to_string(), serde_json::json!({"type": "object"})),
        ToolDefinition::function(
            tools::SPAWN_AGENT.to_string(),
            "Spawn".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::CODE_SEARCH.to_string(),
            "Search".to_string(),
            serde_json::json!({"type": "object"}),
        ),
    ];
    let spec = vtcode_config::builtin_subagents()
        .into_iter()
        .find(|spec| spec.name == "worker")
        .expect("worker");

    let filtered = filter_child_tools(&spec, defs, false, false);
    let names = filtered.iter().map(ToolDefinition::function_name).collect::<Vec<_>>();
    assert_eq!(names, vec![tools::CODE_SEARCH]);
}
#[test]
fn filter_child_tools_keeps_command_session_for_shell_capable_agents() {
    let defs = vec![
        ToolDefinition::function(
            tools::UNIFIED_EXEC.to_string(),
            "Exec".to_string(),
            serde_json::json!({"type": "object"}),
        ),
        ToolDefinition::function(
            tools::CODE_SEARCH.to_string(),
            "Search".to_string(),
            serde_json::json!({"type": "object"}),
        ),
    ];
    let spec = SubagentSpec {
        name: "shell-demo".to_string(),
        description: "test".to_string(),
        prompt: String::new(),
        tools: Some(vec![tools::UNIFIED_EXEC.to_string(), tools::CODE_SEARCH.to_string()]),
        disallowed_tools: Vec::new(),
        model: None,
        color: None,
        reasoning_effort: None,
        permissions: AgentPermissionsConfig::new(PermissionDefault::Ask),
        skills: Vec::new(),
        mcp_servers: Vec::new(),
        hooks: None,
        background: false,
        mode: vtcode_config::AgentMode::Subagent,
        max_turns: None,
        nickname_candidates: Vec::new(),
        initial_prompt: None,
        memory: None,
        isolation: None,
        aliases: Vec::new(),
        source: SubagentSource::Builtin,
        file_path: None,
        warnings: Vec::new(),
        tool_policy_overrides: BTreeMap::new(),
    };

    let filtered = filter_child_tools(&spec, defs, spec.is_read_only(), false);
    assert_eq!(filtered.len(), 2);
    assert_eq!(filtered[0].function_name(), tools::UNIFIED_EXEC);
    assert_eq!(filtered[1].function_name(), tools::CODE_SEARCH);
}
