use super::*;

#[test]
fn default_profile_exposes_only_codex_baseline_tools() {
    let registrations = vec![
        registration(tools::EXEC_COMMAND)
            .with_description("Run command")
            .with_parameter_schema(empty_object_schema()),
        registration(tools::WRITE_STDIN)
            .with_description("Write stdin")
            .with_parameter_schema(empty_object_schema()),
        registration(tools::APPLY_PATCH)
            .with_llm_visibility(false)
            .with_description("Apply patch")
            .with_parameter_schema(apply_patch_parameters())
            .with_behavior(ToolBehavior::apply_patch(ToolMutationModel::Mutating, false, true)),
        registration(tools::CODE_SEARCH)
            .with_description("Search code")
            .with_parameter_schema(empty_object_schema()),
        registration(tools::SEARCH_TOOLS)
            .with_description("Discover deferred tools")
            .with_parameter_schema(empty_object_schema()),
        registration(tools::READ_FILE)
            .with_llm_visibility(false)
            .with_description("Read file")
            .with_parameter_schema(empty_object_schema()),
        registration(tools::WRITE_FILE)
            .with_llm_visibility(false)
            .with_description("Write file")
            .with_parameter_schema(empty_object_schema()),
        registration(tools::DELETE_FILE)
            .with_description("Delete file")
            .with_parameter_schema(empty_object_schema()),
        registration(tools::MOVE_FILE)
            .with_description("Move file")
            .with_parameter_schema(empty_object_schema()),
        registration(tools::COPY_FILE)
            .with_description("Copy file")
            .with_parameter_schema(empty_object_schema()),
        registration("ls")
            .with_description("List directory")
            .with_parameter_schema(empty_object_schema()),
        registration("rg")
            .with_description("Search text")
            .with_parameter_schema(empty_object_schema()),
        registration("find")
            .with_description("Find files")
            .with_parameter_schema(empty_object_schema()),
        registration("cat")
            .with_description("Print file")
            .with_parameter_schema(empty_object_schema()),
        registration("sed")
            .with_description("Stream edit")
            .with_parameter_schema(empty_object_schema()),
        registration("awk")
            .with_description("Process text")
            .with_parameter_schema(empty_object_schema()),
    ];

    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let mut config = SessionToolsConfig::full_public(
        SessionSurface::AgentRunner,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    );
    config.planning_active = false;
    let names = catalog.public_tool_names(config);

    assert_eq!(
        names,
        vec![
            tools::EXEC_COMMAND.to_string(),
            tools::WRITE_STDIN.to_string(),
            tools::APPLY_PATCH.to_string(),
            tools::SEARCH_TOOLS.to_string(),
        ]
    );
    for command in ["ls", "rg", "find", "cat", "sed", "awk"] {
        assert!(
            !names.contains(&command.to_string()),
            "{command} must stay an exec_command.cmd example, not a default tool"
        );
    }
    for file_tool in [
        tools::READ_FILE,
        tools::WRITE_FILE,
        tools::DELETE_FILE,
        tools::MOVE_FILE,
        tools::COPY_FILE,
        tools::UNIFIED_FILE,
    ] {
        assert!(!names.contains(&file_tool.to_string()), "{file_tool} must stay out of the default file surface");
    }
}
#[test]
fn default_profile_exposes_planning_tools_during_planning() {
    let registrations = vec![
        registration(tools::EXEC_COMMAND)
            .with_description("Run command")
            .with_parameter_schema(empty_object_schema()),
        registration(tools::WRITE_STDIN)
            .with_description("Write stdin")
            .with_parameter_schema(empty_object_schema()),
        registration(tools::APPLY_PATCH)
            .with_llm_visibility(false)
            .with_description("Apply patch")
            .with_parameter_schema(apply_patch_parameters())
            .with_behavior(ToolBehavior::apply_patch(ToolMutationModel::Mutating, false, true)),
        registration(tools::SEARCH_TOOLS)
            .with_description("Discover deferred tools")
            .with_parameter_schema(empty_object_schema()),
        registration(tools::CODE_SEARCH)
            .with_description("Search code")
            .with_parameter_schema(empty_object_schema()),
        registration(tools::READ_FILE)
            .with_description("Read file")
            .with_parameter_schema(empty_object_schema()),
        registration(tools::LIST_FILES)
            .with_description("List files")
            .with_parameter_schema(empty_object_schema()),
        registration(tools::GREP_FILE)
            .with_description("Grep file")
            .with_parameter_schema(empty_object_schema()),
        registration(tools::REQUEST_USER_INPUT)
            .with_description("Ask the user")
            .with_parameter_schema(empty_object_schema()),
    ];

    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let normal_names = catalog.public_tool_names(SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    ));
    assert_eq!(
        normal_names,
        vec![
            tools::EXEC_COMMAND.to_string(),
            tools::WRITE_STDIN.to_string(),
            tools::APPLY_PATCH.to_string(),
            tools::SEARCH_TOOLS.to_string(),
        ]
    );

    let planning_names = catalog.public_tool_names(
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_planning_active(true),
    );

    // Planning must expose the full read-only inspection surface plus the
    // interview tool — never collapse to a bare `code_search` catalog
    // (turn_912/913 regression). The Interactive surface additionally
    // hides read_file/list_files (they stay reachable on AgentRunner);
    // interactive planners read files through exec_command per the
    // planning read-only notice.
    assert_eq!(
        planning_names,
        vec![
            tools::EXEC_COMMAND.to_string(),
            tools::CODE_SEARCH.to_string(),
            tools::GREP_FILE.to_string(),
            tools::REQUEST_USER_INPUT.to_string(),
        ]
    );

    let agent_runner_planning_names = catalog.public_tool_names(
        SessionToolsConfig::full_public(
            SessionSurface::AgentRunner,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_planning_active(true),
    );
    assert_eq!(
        agent_runner_planning_names,
        vec![
            tools::EXEC_COMMAND.to_string(),
            tools::CODE_SEARCH.to_string(),
            tools::READ_FILE.to_string(),
            tools::LIST_FILES.to_string(),
            tools::GREP_FILE.to_string(),
            tools::REQUEST_USER_INPUT.to_string(),
        ]
    );
}
/// Turn_912/913 end-to-end regression: compose the same three wire
/// filters the binary runloop applies (catalog profile+surface, primary
/// agent tool policy, permission advertisement) for the built-in plan
/// agent on the Interactive surface and assert the resulting planning
/// catalog. Before the fix this collapsed to `["code_search"]`.
#[test]
fn plan_agent_interactive_wire_catalog_survives_all_filters() {
    use crate::config::PermissionsConfig;
    use crate::permissions::{build_advertised_permission_requests, evaluate_effective_permissions};
    use crate::primary_agent::{ActivePrimaryAgent, primary_agent_allows_tool};

    let registrations = [
        tools::EXEC_COMMAND,
        tools::CODE_SEARCH,
        tools::GREP_FILE,
        tools::READ_FILE,
        tools::LIST_FILES,
        tools::REQUEST_USER_INPUT,
        tools::APPLY_PATCH,
        tools::WRITE_FILE,
    ]
    .into_iter()
    .map(|name| {
        registration(name)
            .with_description("test tool")
            .with_parameter_schema(empty_object_schema())
    })
    .collect::<Vec<_>>();
    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let names = catalog.public_tool_names(
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_planning_active(true),
    );

    let agent = ActivePrimaryAgent::from_spec(&vtcode_config::builtin_plan_agent());
    let global = PermissionsConfig::default();
    let temp = tempfile::TempDir::new().expect("tempdir");
    let workspace = temp.path();
    let visible: Vec<String> = names
        .iter()
        .filter(|name| primary_agent_allows_tool(&agent, name))
        .filter(|name| {
            let requests = build_advertised_permission_requests(workspace, workspace, name);
            requests.is_empty()
                || requests.iter().any(|request| {
                    evaluate_effective_permissions(&global, &agent.permissions, workspace, workspace, request)
                        != crate::permissions::ResolvedPermissionDecision::Deny
                })
        })
        .cloned()
        .collect();

    assert_eq!(
        visible,
        vec![
            tools::EXEC_COMMAND.to_string(),
            tools::CODE_SEARCH.to_string(),
            tools::GREP_FILE.to_string(),
            tools::REQUEST_USER_INPUT.to_string(),
        ],
        "plan agent Interactive wire catalog must keep the read-only inspection + interview set"
    );
}
#[test]
fn exec_command_schema_models_unix_tools_as_cmd_examples() {
    let registration = registration(tools::EXEC_COMMAND)
        .with_description("Run command")
        .with_parameter_schema(exec_command_parameters());
    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
    let entries = catalog.schema_entries(SessionToolsConfig::full_public(
        SessionSurface::AgentRunner,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    ));
    let entry = entries
        .iter()
        .find(|entry| entry.name == tools::EXEC_COMMAND)
        .expect("exec_command schema entry");
    let properties = &entry.parameters["properties"];

    assert_eq!(entry.parameters["required"], json!(["cmd"]));
    // The example commands live once in EXEC_COMMAND_DESCRIPTION; the
    // `cmd` property defers to the tool description instead of repeating
    // the list on every request.
    assert!(
        properties["cmd"]["description"]
            .as_str()
            .is_some_and(|text| text.contains("tool description lists covered tools"))
    );
    assert_eq!(properties["tty"]["type"], "boolean");
    for command in ["ls", "rg", "find", "cat", "sed", "awk"] {
        assert!(properties.get(command).is_none(), "{command} must not be modelled as a separate schema property");
    }
}
