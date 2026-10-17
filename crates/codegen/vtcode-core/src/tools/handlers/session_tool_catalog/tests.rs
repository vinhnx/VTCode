use super::*;
use crate::config::VTCodeConfig;
use crate::tools::constants::empty_object_schema;
use crate::tools::registry::ToolRegistration;
use crate::tools::request_user_input::RequestUserInputTool;
use crate::tools::tool_intent::{ToolBehavior, ToolMutationModel};
use crate::tools::traits::Tool;
use serde_json::json;

fn registration(name: &'static str) -> ToolRegistration {
    ToolRegistration::new(name, CapabilityLevel::CodeSearch, false, |_, _| Box::pin(async { Ok(Value::Null) }))
}

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

#[test]
fn advanced_profile_exposes_code_search_without_internal_search_names() {
    let registrations = vec![
        registration(tools::CODE_SEARCH)
            .with_description("Search code")
            .with_parameter_schema(empty_object_schema()),
        registration(tools::LIST_FILES)
            .with_llm_visibility(false)
            .with_description("List files")
            .with_parameter_schema(list_files_parameters()),
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
    ];

    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let names = catalog.public_tool_names(
        SessionToolsConfig::full_public(
            SessionSurface::AgentRunner,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode),
    );

    assert_eq!(names, vec![tools::CODE_SEARCH.to_string()]);
}

#[test]
fn advanced_profile_retains_eligible_specialised_and_dynamic_tools() {
    let registrations = vec![
        registration(tools::CODE_SEARCH)
            .with_description("Search code")
            .with_parameter_schema(empty_object_schema()),
        registration("mcp::context7::search")
            .with_catalog_source(ToolCatalogSource::Mcp)
            .with_llm_visibility(false)
            .with_description("Search documentation")
            .with_parameter_schema(empty_object_schema())
            .with_aliases(["mcp__context7__search"]),
        registration(tools::LOAD_SKILL)
            .with_catalog_source(ToolCatalogSource::Builtin)
            .with_description("Load a skill")
            .with_parameter_schema(empty_object_schema()),
        registration(tools::START_PLANNING)
            .with_catalog_source(ToolCatalogSource::Builtin)
            .with_description("Start planning")
            .with_parameter_schema(empty_object_schema()),
        registration(tools::SPAWN_AGENT)
            .with_catalog_source(ToolCatalogSource::Builtin)
            .with_description("Spawn an agent")
            .with_parameter_schema(empty_object_schema()),
        registration(tools::CRON_CREATE)
            .with_catalog_source(ToolCatalogSource::Builtin)
            .with_description("Create a scheduled prompt")
            .with_parameter_schema(empty_object_schema()),
        registration("dynamic_plugin_tool")
            .with_catalog_source(ToolCatalogSource::Dynamic)
            .with_description("Run a dynamic plugin tool")
            .with_parameter_schema(empty_object_schema()),
    ];

    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let names = catalog.public_tool_names(
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode),
    );

    assert_eq!(
        names,
        vec![
            tools::CODE_SEARCH.to_string(),
            "mcp__context7__search".to_string(),
            tools::LOAD_SKILL.to_string(),
            tools::START_PLANNING.to_string(),
            tools::SPAWN_AGENT.to_string(),
            tools::CRON_CREATE.to_string(),
            "dynamic_plugin_tool".to_string(),
        ]
    );
}

#[test]
fn acp_surface_exposes_code_search_with_advanced_profile() {
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
        registration(tools::LOAD_SKILL)
            .with_description("Load a skill")
            .with_parameter_schema(empty_object_schema()),
    ];

    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let names = catalog.public_tool_names(
        SessionToolsConfig::full_public(
            SessionSurface::Acp,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode),
    );

    assert_eq!(
        names,
        vec![
            tools::EXEC_COMMAND.to_string(),
            tools::WRITE_STDIN.to_string(),
            tools::APPLY_PATCH.to_string(),
            tools::CODE_SEARCH.to_string(),
        ]
    );
}

#[test]
fn rebuild_catalog_uses_public_mcp_alias() {
    let registration = registration("mcp::context7::search")
        .with_catalog_source(ToolCatalogSource::Mcp)
        .with_llm_visibility(false)
        .with_description("search docs")
        .with_parameter_schema(empty_object_schema())
        .with_aliases(["mcp__context7__search"]);

    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
    let names = catalog.public_tool_names(
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode),
    );

    assert_eq!(names, vec!["mcp__context7__search".to_string()]);
}

#[test]
fn schema_entries_hide_request_user_input_when_disabled() {
    let registration = registration(tools::REQUEST_USER_INPUT)
        .with_description("Ask the user")
        .with_parameter_schema(empty_object_schema());

    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
    let names = catalog.public_tool_names(SessionToolsConfig {
        surface: SessionSurface::Interactive,
        capability_level: CapabilityLevel::CodeSearch,
        documentation_mode: ToolDocumentationMode::Full,
        planning_active: true,
        request_user_input_enabled: false,
        model_capabilities: ToolModelCapabilities::default(),
        deferred_tool_policy: DeferredToolPolicy::default(),
        anthropic_native_memory_enabled: false,
        tool_profile: ToolProfile::VtCode,
    });

    assert!(names.is_empty());
}

#[test]
fn task_tracker_stays_visible_outside_planning_workflow() {
    let registration = registration(tools::TASK_TRACKER)
        .with_description("Track plan tasks")
        .with_parameter_schema(empty_object_schema());

    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
    let names = catalog.public_tool_names(SessionToolsConfig {
        surface: SessionSurface::Interactive,
        capability_level: CapabilityLevel::CodeSearch,
        documentation_mode: ToolDocumentationMode::Full,
        planning_active: false,
        request_user_input_enabled: true,
        model_capabilities: ToolModelCapabilities::default(),
        deferred_tool_policy: DeferredToolPolicy::default(),
        anthropic_native_memory_enabled: false,
        tool_profile: ToolProfile::AdvancedVtCode,
    });

    assert_eq!(names, vec![tools::TASK_TRACKER.to_string()]);
}

#[test]
fn memory_tool_is_hidden_unless_anthropic_native_memory_is_enabled() {
    let registration = registration(tools::MEMORY)
        .with_description("Native memory")
        .with_parameter_schema(empty_object_schema());
    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);

    let hidden = catalog.public_tool_names(SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    ));
    assert!(hidden.is_empty());

    let visible = catalog.public_tool_names(
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode)
        .with_anthropic_native_memory_enabled(true),
    );
    assert_eq!(visible, vec![tools::MEMORY.to_string()]);
}

#[test]
fn memory_tool_uses_anthropic_native_definition_when_visible() {
    let registration = registration(tools::MEMORY)
        .with_description("Native memory")
        .with_parameter_schema(empty_object_schema());
    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);

    let definitions = catalog.model_tools(
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode)
        .with_anthropic_native_memory_enabled(true),
    );

    assert_eq!(definitions.len(), 1);
    assert_eq!(definitions[0].tool_type, "memory_20250818");
    assert_eq!(definitions[0].function_name(), tools::MEMORY);
}

#[test]
fn apply_patch_uses_special_tool_when_supported() {
    let registration = registration(tools::APPLY_PATCH)
        .with_llm_visibility(false)
        .with_description("Apply patch")
        .with_parameter_schema(apply_patch_parameters())
        .with_behavior(ToolBehavior::apply_patch(ToolMutationModel::Mutating, false, true));

    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
    let tools = catalog.model_tools(SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        ToolModelCapabilities { supports_apply_patch_tool: true },
    ));

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].tool_type, "apply_patch");
}

#[test]
fn apply_patch_falls_back_to_function_tool_when_unsupported() {
    let registration = registration(tools::APPLY_PATCH)
        .with_llm_visibility(false)
        .with_description("Apply patch")
        .with_parameter_schema(apply_patch_parameters())
        .with_behavior(ToolBehavior::apply_patch(ToolMutationModel::Mutating, false, true));

    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
    let tools = catalog.model_tools(SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    ));

    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].tool_type, "function");
}

#[test]
fn apply_patch_stays_eager_for_json_and_native_models_under_deferral() {
    let patch = registration(tools::APPLY_PATCH)
        .with_llm_visibility(false)
        .with_description("Apply patch")
        .with_parameter_schema(apply_patch_parameters())
        .with_behavior(ToolBehavior::apply_patch(ToolMutationModel::Mutating, false, true));
    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![patch]);
    for native in [false, true] {
        for policy in [
            DeferredToolPolicy::client_local(Vec::new()),
            DeferredToolPolicy::anthropic(ToolSearchAlgorithm::Regex, Vec::new()),
        ] {
            let config = SessionToolsConfig::full_public(
                SessionSurface::AgentRunner,
                CapabilityLevel::CodeSearch,
                ToolDocumentationMode::Full,
                ToolModelCapabilities { supports_apply_patch_tool: native },
            )
            .with_deferred_tool_policy(policy);
            let definitions = catalog.model_tools(config.clone());
            let patch = definitions
                .iter()
                .find(|tool| tool.function_name() == tools::APPLY_PATCH)
                .expect("patch stays available");
            assert_eq!(patch.defer_loading, None);
            assert_eq!(patch.tool_type, if native { "apply_patch" } else { "function" });
            assert!(
                !catalog
                    .model_tools(config.with_planning_active(true))
                    .iter()
                    .any(|tool| tool.function_name() == tools::APPLY_PATCH)
            );
        }
    }
}

#[test]
fn agent_runner_default_hides_legacy_browse_tools() {
    let read_file = registration(tools::READ_FILE)
        .with_llm_visibility(false)
        .with_description("Read file contents in chunks")
        .with_parameter_schema(empty_object_schema());
    let list_files = registration(tools::LIST_FILES)
        .with_llm_visibility(false)
        .with_description("List files with pagination")
        .with_parameter_schema(list_files_parameters());
    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![read_file, list_files]);

    let interactive_names = catalog.public_tool_names(SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    ));
    assert!(!interactive_names.contains(&tools::READ_FILE.to_string()));
    assert!(!interactive_names.contains(&tools::LIST_FILES.to_string()));

    let agent_runner_names = catalog.public_tool_names(SessionToolsConfig::full_public(
        SessionSurface::AgentRunner,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    ));
    assert!(!agent_runner_names.contains(&tools::READ_FILE.to_string()));
    assert!(!agent_runner_names.contains(&tools::LIST_FILES.to_string()));
}

#[test]
fn parallel_support_comes_from_behavior_metadata() {
    let registration = registration("parallel_catalog_tool")
        .with_description("parallel-safe test tool")
        .with_parameter_schema(empty_object_schema())
        .with_behavior(ToolBehavior::function(ToolMutationModel::ReadOnly, true, false));

    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
    assert_eq!(catalog.entries().len(), 1);
    assert!(catalog.entries()[0].supports_parallel_tool_calls);
}

#[test]
fn model_tool_serialization_keeps_output_cap_separate_from_approval_policy() {
    let registration = registration(tools::EXEC_COMMAND)
        .with_description("Run the policy surface test")
        .with_parameter_schema(empty_object_schema())
        .with_permission(ToolPolicy::Allow);
    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
    let definitions = catalog.model_tools(
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode),
    );
    let tool = definitions.first().expect("model tool definition");
    let serialized = serde_json::to_value(tool).expect("serialize model tool definition");

    assert_eq!(
        tool.function.as_ref().map(|function| function.description.as_str()),
        Some("Run the policy surface test")
    );
    assert_eq!(
        tool.function
            .as_ref()
            .map(|function| function.parameters["properties"]["max_output_tokens"]["default"].clone()),
        Some(json!(vtcode_utility_tool_specs::DEFAULT_MAX_OUTPUT_TOKENS))
    );
    for key in [
        "approval_policy",
        "default_permission",
        "permission",
        "tool_policy",
        "allow_patterns",
        "deny_patterns",
    ] {
        assert!(!contains_json_key(&serialized, key), "approval metadata leaked into model schema: {key}");
    }
}

fn contains_json_key(value: &Value, key: &str) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(name, value)| name == key || contains_json_key(value, key)),
        Value::Array(values) => values.iter().any(|value| contains_json_key(value, key)),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}

#[test]
fn configured_spec_preserves_json_schema_field_names() {
    let registration = registration("schema_contract_tool")
        .with_description("schema contract")
        .with_parameter_schema(json!({
            "type": "object",
            "properties": {
                "input": {"type": "string"}
            },
            "additionalProperties": false,
            "anyOf": [
                {"required": ["input"]},
                {"required": ["patch"]}
            ]
        }));

    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![registration]);
    let entry = &catalog.entries()[0];
    let ToolSpec::Function(tool) = &entry.configured_spec.spec else {
        panic!("expected function tool spec");
    };

    let serialized = serde_json::to_value(&tool.parameters).expect("serialize parameters");
    assert_eq!(serialized["additionalProperties"], Value::Bool(false));
    assert!(serialized["anyOf"].is_array());
    assert!(serialized.get("additional_properties").is_none());
    assert!(serialized.get("any_of").is_none());
}

#[test]
fn compact_parameters_preserves_property_named_description() {
    let schema = RequestUserInputTool.parameter_schema().expect("request_user_input schema");

    let compacted = compact_parameters(schema, ToolDocumentationMode::Progressive);
    let description_property =
        &compacted["properties"]["questions"]["items"]["properties"]["options"]["items"]["properties"]["description"];

    assert!(description_property.is_object());
    assert_eq!(
        compacted["properties"]["questions"]["items"]["properties"]["options"]["items"]["required"],
        json!(["label", "description"])
    );
}

#[test]
fn cached_projections_preserve_schema_and_deferral_across_documentation_modes() {
    let exec_command = registration(tools::EXEC_COMMAND)
        .with_description("Run a command. Use the workspace shell safely.")
        .with_parameter_schema(json!({
            "type": "object",
            "properties": {
                "cmd": {"type": "string", "description": "Command to run."}
            },
            "required": ["cmd"]
        }));
    let mcp_tool = registration("mcp::context7::search")
        .with_catalog_source(ToolCatalogSource::Mcp)
        .with_llm_visibility(false)
        .with_description(
            "Search the documentation server. This description is intentionally longer than minimal mode.",
        )
        .with_parameter_schema(json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Documentation query."}
            }
        }))
        .with_aliases(["mcp__context7__search"]);
    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![exec_command, mcp_tool]);

    for documentation_mode in [
        ToolDocumentationMode::Minimal,
        ToolDocumentationMode::Progressive,
        ToolDocumentationMode::Full,
    ] {
        let config = SessionToolsConfig::full_public(
            SessionSurface::AgentRunner,
            CapabilityLevel::CodeSearch,
            documentation_mode,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode)
        .with_deferred_tool_policy(DeferredToolPolicy::client_local(Vec::new()));

        let expected_schema = catalog
            .entries()
            .iter()
            .filter(|entry| entry.is_visible(&config))
            .map(|entry| ToolSchemaEntry {
                name: entry.public_name.clone(),
                description: compact_tool_description(
                    entry.description.as_str(),
                    documentation_mode,
                    entry.max_description_length,
                ),
                parameters: compact_parameters(entry.parameters.clone(), documentation_mode),
            })
            .collect::<Vec<_>>();
        assert_eq!(catalog.schema_entries(config.clone()), expected_schema);

        let visible_entries = catalog
            .entries()
            .iter()
            .filter(|entry| entry.is_visible(&config))
            .collect::<Vec<_>>();
        let estimated_schema_tokens = expected_schema
            .iter()
            .map(|entry| serde_json::to_string(entry).expect("serialize expected schema").len() / 4)
            .sum();
        let deferable_tool_count = visible_entries
            .iter()
            .filter(|entry| should_defer_tool_loading(entry, &config))
            .count();
        let expose_tools_directly = config.deferred_tool_policy.is_client_local()
            && !catalog_would_benefit_from_deferral(
                visible_entries
                    .iter()
                    .any(|entry| matches!(entry.source, ToolCatalogSource::Mcp)),
                deferable_tool_count,
                estimated_schema_tokens,
            );
        let definitions = catalog.model_tools(config.clone());

        for entry in visible_entries {
            let expected_deferred = should_defer_tool_loading(entry, &config) && !expose_tools_directly;
            let definition = definitions
                .iter()
                .find(|tool| tool.function_name() == entry.public_name)
                .unwrap_or_else(|| panic!("missing model definition for {}", entry.public_name));
            let actual_deferred = definition.defer_loading == Some(true);
            assert_eq!(actual_deferred, expected_deferred, "deferral changed for {}", entry.public_name);
        }

        assert_eq!(catalog.model_tools(config.clone()), definitions);
        assert!(definitions.iter().any(|tool| tool.function_name() == tools::EXEC_COMMAND));
    }
}

#[test]
fn hosted_deferral_skips_schema_estimation_without_changing_tool_output() {
    let registrations = (0..(DIRECT_TOOL_EXPOSURE_THRESHOLD + 4))
        .map(|index| {
            ToolRegistration::new(format!("hosted_catalog_tool_{index}"), CapabilityLevel::CodeSearch, false, |_, _| {
                Box::pin(async { Ok(Value::Null) })
            })
            .with_description(format!("Search the hosted catalog for item {index}."))
            .with_parameter_schema(json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "Search query." }
                },
                "required": ["query"]
            }))
        })
        .collect();
    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let config = SessionToolsConfig::full_public(
        SessionSurface::AgentRunner,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Progressive,
        ToolModelCapabilities::default(),
    )
    .with_tool_profile(ToolProfile::AdvancedVtCode)
    .with_deferred_tool_policy(DeferredToolPolicy::openai_hosted(Vec::new()));

    let expected_schema = catalog.schema_entries(config.clone());
    let definitions = catalog.model_tools(config.clone());

    assert_eq!(definitions.len(), expected_schema.len() + 1, "hosted search must be added to the catalog");
    assert!(definitions.iter().any(ToolDefinition::is_tool_search));
    for schema in expected_schema {
        let definition = definitions
            .iter()
            .find(|tool| tool.function_name() == schema.name)
            .unwrap_or_else(|| panic!("missing hosted definition for {}", schema.name));
        let function = definition.function.as_ref().expect("hosted catalog function definition");
        assert_eq!(function.name, schema.name);
        assert_eq!(function.description, schema.description);
        assert_eq!(function.parameters, schema.parameters);
        assert_eq!(definition.defer_loading, Some(true));
    }

    let estimates_initialized = catalog.visible_entry_indices(&config).into_iter().any(|index| {
        let entry = &catalog.entries[index];
        catalog
            .projection(index, entry, config.documentation_mode)
            .has_serialized_token_estimate()
    });
    assert!(!estimates_initialized, "hosted policies must not serialize schema-token estimates");
}

#[test]
fn eager_and_client_local_policies_still_estimate_schema_tokens_when_needed() {
    for deferred_tool_policy in [
        DeferredToolPolicy::default(),
        DeferredToolPolicy::client_local(Vec::new()),
    ] {
        let registrations = (0..(DIRECT_TOOL_EXPOSURE_THRESHOLD + 1))
            .map(|index| {
                ToolRegistration::new(
                    format!("estimated_catalog_tool_{index}"),
                    CapabilityLevel::CodeSearch,
                    false,
                    |_, _| Box::pin(async { Ok(Value::Null) }),
                )
                .with_description(format!("Estimate this catalog entry {index}."))
                .with_parameter_schema(json!({
                    "type": "object",
                    "properties": { "path": { "type": "string" } }
                }))
            })
            .collect();
        let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
        let config = SessionToolsConfig::full_public(
            SessionSurface::AgentRunner,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Progressive,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode)
        .with_deferred_tool_policy(deferred_tool_policy);

        let _ = catalog.model_tools(config.clone());
        let estimates_initialized = catalog.visible_entry_indices(&config).into_iter().any(|index| {
            let entry = &catalog.entries[index];
            catalog
                .projection(index, entry, config.documentation_mode)
                .has_serialized_token_estimate()
        });
        assert!(estimates_initialized, "the active policy must retain its schema-token decision");
    }
}

#[test]
fn anthropic_policy_injects_tool_search_and_defers_non_core_tools() {
    let exec_command = registration(tools::EXEC_COMMAND)
        .with_description("Run command")
        .with_parameter_schema(empty_object_schema());
    let apply_patch = registration(tools::APPLY_PATCH)
        .with_llm_visibility(false)
        .with_description("Apply patch")
        .with_parameter_schema(apply_patch_parameters())
        .with_behavior(ToolBehavior::apply_patch(ToolMutationModel::Mutating, false, true));
    let mcp_tool = registration("mcp::context7::search")
        .with_catalog_source(ToolCatalogSource::Mcp)
        .with_llm_visibility(false)
        .with_description("search docs")
        .with_parameter_schema(empty_object_schema())
        .with_aliases(["mcp__context7__search"]);

    let mut registrations = vec![exec_command, apply_patch, mcp_tool];
    for index in 0..DIRECT_TOOL_EXPOSURE_THRESHOLD {
        let name: &'static str = Box::leak(format!("mcp::context7::resolve_{index}").into_boxed_str());
        let alias = format!("mcp__context7__resolve_{index}");
        registrations.push(
            registration(name)
                .with_catalog_source(ToolCatalogSource::Mcp)
                .with_llm_visibility(false)
                .with_description(format!("resolve docs {index}"))
                .with_parameter_schema(empty_object_schema())
                .with_aliases([alias]),
        );
    }

    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let definitions = catalog.model_tools(
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode)
        .with_deferred_tool_policy(DeferredToolPolicy::anthropic(ToolSearchAlgorithm::Regex, Vec::new())),
    );

    assert!(
        definitions
            .iter()
            .any(|tool| tool.tool_type == "tool_search_tool_regex_20251119"),
        "anthropic tool search should be injected when deferred tools exist"
    );
    let exec_tool = definitions
        .iter()
        .find(|tool| tool.function_name() == tools::EXEC_COMMAND)
        .expect("exec_command should be present");
    assert_eq!(exec_tool.defer_loading, None);

    let apply_patch = definitions
        .iter()
        .find(|tool| tool.function_name() == tools::APPLY_PATCH)
        .expect("apply_patch fallback should be present");
    assert_eq!(apply_patch.defer_loading, None);

    let mcp_tool = definitions
        .iter()
        .find(|tool| tool.function_name() == "mcp__context7__search")
        .expect("mcp tool should be present");
    assert_eq!(mcp_tool.defer_loading, Some(true));
}

#[test]
fn mcp_tool_registration_derives_namespace_from_server_name() {
    let mcp_tool = registration("mcp::context7::search")
        .with_catalog_source(ToolCatalogSource::Mcp)
        .with_llm_visibility(false)
        .with_description("search docs")
        .with_parameter_schema(empty_object_schema())
        .with_aliases(["mcp__context7__search"]);

    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![mcp_tool]);
    let entry = catalog
        .entries()
        .iter()
        .find(|entry| entry.public_name == "mcp__context7__search")
        .expect("mcp entry should be present");

    let namespace = entry
        .namespace
        .as_ref()
        .expect("mcp tool should derive a namespace from its server name");
    assert_eq!(namespace.name, "context7");
    assert_eq!(namespace.description, "Tools provided by MCP server 'context7'");
}

#[test]
fn core_tool_registration_has_no_namespace() {
    let exec_command = registration(tools::EXEC_COMMAND)
        .with_description("Run command")
        .with_parameter_schema(empty_object_schema());

    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![exec_command]);
    let entry = catalog
        .entries()
        .iter()
        .find(|entry| entry.public_name == tools::EXEC_COMMAND)
        .expect("core tool entry should be present");

    assert!(entry.namespace.is_none(), "core/builtin tools should not derive a namespace");
}

#[test]
fn model_tools_attach_namespace_only_to_deferred_mcp_tools() {
    let exec_command = registration(tools::EXEC_COMMAND)
        .with_description("Run command")
        .with_parameter_schema(empty_object_schema());
    let mcp_tool = registration("mcp::context7::search")
        .with_catalog_source(ToolCatalogSource::Mcp)
        .with_llm_visibility(false)
        .with_description("search docs")
        .with_parameter_schema(empty_object_schema())
        .with_aliases(["mcp__context7__search"]);

    let mut registrations = vec![exec_command, mcp_tool];
    for index in 0..DIRECT_TOOL_EXPOSURE_THRESHOLD {
        let name: &'static str = Box::leak(format!("mcp::context7::resolve_{index}").into_boxed_str());
        let alias = format!("mcp__context7__resolve_{index}");
        registrations.push(
            registration(name)
                .with_catalog_source(ToolCatalogSource::Mcp)
                .with_llm_visibility(false)
                .with_description(format!("resolve docs {index}"))
                .with_parameter_schema(empty_object_schema())
                .with_aliases([alias]),
        );
    }

    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let definitions = catalog.model_tools(
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode)
        .with_deferred_tool_policy(DeferredToolPolicy::anthropic(ToolSearchAlgorithm::Regex, Vec::new())),
    );

    let core_tool = definitions
        .iter()
        .find(|tool| tool.function_name() == tools::EXEC_COMMAND)
        .expect("exec_command should be present");
    assert_eq!(core_tool.defer_loading, None);
    assert!(core_tool.namespace.is_none(), "non-deferred core tools should never carry namespace metadata");

    let deferred_mcp_tool = definitions
        .iter()
        .find(|tool| tool.function_name() == "mcp__context7__search")
        .expect("deferred mcp tool should be present");
    assert_eq!(deferred_mcp_tool.defer_loading, Some(true));
    let namespace = deferred_mcp_tool
        .namespace
        .as_ref()
        .expect("deferred mcp tool should carry namespace metadata");
    assert_eq!(namespace.name, "context7");
}

#[test]
fn small_mcp_catalog_is_deferred_despite_low_tool_count() {
    let exec_command = registration(tools::EXEC_COMMAND)
        .with_description("Run command")
        .with_parameter_schema(empty_object_schema());
    let mcp_tool = registration("mcp::context7::search")
        .with_catalog_source(ToolCatalogSource::Mcp)
        .with_llm_visibility(false)
        .with_description("search docs")
        .with_parameter_schema(empty_object_schema())
        .with_aliases(["mcp__context7__search"]);

    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![exec_command, mcp_tool]);
    let definitions = catalog.model_tools(
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode)
        .with_deferred_tool_policy(DeferredToolPolicy::anthropic(ToolSearchAlgorithm::Regex, Vec::new())),
    );

    let mcp_definition = definitions
        .iter()
        .find(|tool| tool.function_name() == "mcp__context7__search")
        .expect("mcp tool should be present");
    assert_eq!(
        mcp_definition.defer_loading,
        Some(true),
        "even a single MCP tool should be deferred to avoid schema tax"
    );
}

#[test]
fn client_local_policy_deferred_for_small_mcp_catalog() {
    let exec_command = registration(tools::EXEC_COMMAND)
        .with_description("Run command")
        .with_parameter_schema(empty_object_schema());
    let mcp_search_tools = registration(tools::MCP_SEARCH_TOOLS)
        .with_description("Search MCP tools")
        .with_parameter_schema(empty_object_schema());
    let mcp_tool = registration("mcp::context7::search")
        .with_catalog_source(ToolCatalogSource::Mcp)
        .with_llm_visibility(false)
        .with_description("search docs")
        .with_parameter_schema(empty_object_schema())
        .with_aliases(["mcp__context7__search"]);

    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![exec_command, mcp_search_tools, mcp_tool]);
    let definitions = catalog.model_tools(
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode)
        .with_deferred_tool_policy(DeferredToolPolicy::client_local(Vec::new())),
    );

    assert!(
        definitions.iter().any(|tool| tool.function_name() == "mcp__context7__search"),
        "mcp tool should still be listed in the model-facing catalog for client-local search"
    );
    let mcp_definition = definitions
        .iter()
        .find(|tool| tool.function_name() == "mcp__context7__search")
        .expect("mcp tool should be present");
    assert_eq!(
        mcp_definition.defer_loading,
        Some(true),
        "client-local deferral should also apply to small MCP catalogs"
    );
    let search_definition = definitions
        .iter()
        .find(|tool| tool.function_name() == tools::MCP_SEARCH_TOOLS)
        .expect("client-local MCP search should remain available");
    assert_eq!(search_definition.defer_loading, None);
}

#[test]
fn client_local_policy_exposes_small_builtin_catalog_directly() {
    // Plan-mode policy filtering shrinks the visible catalog to a handful
    // of read-only builtins. Deferring that tiny set drops every tool from
    // the wire payload (on_wire_tools = 0), so the model improvises
    // textual XML tool calls instead of native ones (turn_887/turn_888).
    // Small catalogs must stay eager: deferral exists to shed schema tax,
    // not to hide the whole catalog.
    let exec_command = registration(tools::EXEC_COMMAND)
        .with_description("Run command")
        .with_parameter_schema(empty_object_schema());
    let code_search = registration(tools::CODE_SEARCH)
        .with_description("Search code")
        .with_parameter_schema(empty_object_schema());
    let read_file = registration(tools::READ_FILE)
        .with_description("Read file")
        .with_parameter_schema(empty_object_schema());

    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![exec_command, code_search, read_file]);
    let definitions = catalog.model_tools(
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode)
        .with_deferred_tool_policy(DeferredToolPolicy::client_local(Vec::new())),
    );

    assert!(
        definitions.iter().all(|tool| tool.defer_loading.is_none()),
        "a small builtin-only catalog gains nothing from client-local deferral; every tool must stay on the wire"
    );
}

#[test]
fn client_local_policy_defers_large_builtin_catalog() {
    let exec_command = registration(tools::EXEC_COMMAND)
        .with_description("Run command")
        .with_parameter_schema(empty_object_schema());
    let code_search = registration(tools::CODE_SEARCH)
        .with_description("Search code")
        .with_parameter_schema(empty_object_schema());

    let mut registrations = vec![exec_command, code_search];
    for index in 0..DIRECT_TOOL_EXPOSURE_THRESHOLD {
        let name: &'static str = Box::leak(format!("extra_builtin_{index}").into_boxed_str());
        registrations.push(
            registration(name)
                .with_description(format!("extra builtin {index}"))
                .with_parameter_schema(empty_object_schema()),
        );
    }

    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let definitions = catalog.model_tools(
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode)
        .with_deferred_tool_policy(DeferredToolPolicy::client_local(Vec::new())),
    );

    let exec_tool = definitions
        .iter()
        .find(|tool| tool.function_name() == tools::EXEC_COMMAND)
        .expect("exec_command should be present");
    assert_eq!(exec_tool.defer_loading, None, "core tools stay eager even in large catalogs");
    let search_tool = definitions
        .iter()
        .find(|tool| tool.function_name() == tools::CODE_SEARCH)
        .expect("code_search should be present");
    assert_eq!(
        search_tool.defer_loading, None,
        "structured search stays eager even in large catalogs (session-efficiency)"
    );
    let extra_tool = definitions
        .iter()
        .find(|tool| tool.function_name() == "extra_builtin_0")
        .expect("extra builtin should be present");
    assert_eq!(
        extra_tool.defer_loading,
        Some(true),
        "large builtin catalogs still defer non-core tools under client-local policy"
    );
}

/// Serialize the on-wire tool schemas (the definitions the model actually
/// receives) and estimate their token cost at ~4 chars/token, matching the
/// convention in `estimate_schema_tokens` and the first-request budget test
/// in `tools/registry/builtins.rs`. Deferred tools (`defer_loading ==
/// Some(true)`) are omitted under the client-local policy, so they never
/// reach the wire payload and are excluded here.
fn on_wire_schema_tokens(catalog: &SessionToolCatalog, config: SessionToolsConfig) -> usize {
    #[derive(Serialize)]
    struct Estimate<'a> {
        name: &'a str,
        description: &'a str,
        parameters: &'a Value,
    }
    let on_wire: FxHashSet<String> = catalog
        .model_tools(config.clone())
        .into_iter()
        .filter(|tool| tool.defer_loading != Some(true))
        .map(|tool| tool.function_name().to_string())
        .collect();
    catalog
        .schema_entries(config)
        .into_iter()
        .filter(|entry| on_wire.contains(&entry.name))
        .map(|entry| {
            serde_json::to_string(&Estimate {
                name: &entry.name,
                description: &entry.description,
                parameters: &entry.parameters,
            })
            .map(|s| s.len() / 4)
            .unwrap_or(0)
        })
        .sum()
}

#[test]
fn task_tracker_schema_token_estimate_tracks_workflow_specific_parameters() {
    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![
        registration(tools::TASK_TRACKER)
            .with_description("Track plan tasks")
            .with_parameter_schema(empty_object_schema()),
    ]);
    let base_config = SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    )
    .with_tool_profile(ToolProfile::AdvancedVtCode);

    let standard_config = base_config.clone().with_planning_active(false);
    let standard_visible = catalog.visible_entry_indices(&standard_config);
    assert_eq!(
        catalog.estimate_schema_tokens(&standard_visible, &standard_config),
        on_wire_schema_tokens(&catalog, standard_config),
        "inactive task_tracker token estimate should match the emitted schema",
    );

    let planning_config = base_config.with_planning_active(true);
    let planning_visible = catalog.visible_entry_indices(&planning_config);
    assert_eq!(
        catalog.estimate_schema_tokens(&planning_visible, &planning_config),
        on_wire_schema_tokens(&catalog, planning_config),
        "planning task_tracker token estimate should match the emitted schema",
    );
}

#[test]
fn task_tracker_schema_keeps_max_output_tokens_in_standard_and_planning_modes() {
    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![
        registration(tools::TASK_TRACKER)
            .with_description("Track plan tasks")
            .with_parameter_schema(empty_object_schema()),
    ]);
    let base_config = SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    )
    .with_tool_profile(ToolProfile::AdvancedVtCode);

    for planning_active in [false, true] {
        let function = catalog
            .model_tools(base_config.clone().with_planning_active(planning_active))
            .into_iter()
            .find_map(|tool| (tool.function_name() == tools::TASK_TRACKER).then_some(tool.function).flatten())
            .unwrap_or_else(|| panic!("missing task_tracker function for planning_active={planning_active}"));

        assert_eq!(
            function.parameters["properties"]["max_output_tokens"]["default"],
            json!(vtcode_utility_tool_specs::DEFAULT_MAX_OUTPUT_TOKENS),
            "task_tracker schema must keep max_output_tokens when planning_active={planning_active}",
        );
    }
}

/// Build a simulated MCP tool registration for `server`/`tool` with a
/// realistic one-line description and a two-parameter schema, so the eager
/// schema tax is pronounced enough to assert on.
fn mcp_server_tool_registration(server: &str, tool: &str, description: &str) -> ToolRegistration {
    let name: &'static str = Box::leak(format!("mcp::{server}::{tool}").into_boxed_str());
    let alias = format!("mcp__{server}__{tool}");
    registration(name)
        .with_catalog_source(ToolCatalogSource::Mcp)
        .with_llm_visibility(false)
        .with_description(description.to_string())
        .with_parameter_schema(json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "The lookup query." },
                "limit": { "type": "integer", "description": "Max results to return." }
            },
            "required": ["query"]
        }))
        .with_aliases([alias])
}

/// Phase 1.1 / 2.x regression: with several MCP servers attached, the
/// client-local deferred-loading path must keep the first-request wire
/// payload near the no-MCP baseline (because MCP schemas are omitted),
/// while eager exposure would balloon it. This proves the win the
/// `DIRECT_TOOL_EXPOSURE_THRESHOLD` / `has_mcp_tools` gating delivers.
#[test]
fn mcp_deferral_keeps_first_request_wire_payload_near_baseline() {
    let core_registrations = || {
        vec![
            registration(tools::EXEC_COMMAND)
                .with_description("Run a shell command in the workspace sandbox.")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::APPLY_PATCH)
                .with_llm_visibility(false)
                .with_description("Apply a structured patch to files.")
                .with_parameter_schema(apply_patch_parameters())
                .with_behavior(ToolBehavior::apply_patch(ToolMutationModel::Mutating, false, true)),
            registration(tools::MCP_SEARCH_TOOLS)
                .with_description("Search across deferred MCP tools by keyword.")
                .with_parameter_schema(empty_object_schema()),
            registration(tools::CODE_SEARCH)
                .with_description("Search the codebase symbol index.")
                .with_parameter_schema(empty_object_schema()),
        ]
    };

    // Five simulated MCP servers, four tools each = 20 deferred MCP tools.
    let mut registrations = core_registrations();
    for server in ["context7", "filesystem", "github", "slack", "postgres"] {
        for index in 0..4 {
            let tool = format!("op_{index}");
            let description =
                format!("{server} operation {index}: query and mutate {server} resources with paging and filters.");
            registrations.push(mcp_server_tool_registration(server, &tool, &description));
        }
    }
    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);

    let make_config = |policy: DeferredToolPolicy| {
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode)
        .with_deferred_tool_policy(policy)
    };

    // Eager: no deferral, so all 20 MCP schemas travel on the first request.
    let eager_tokens = on_wire_schema_tokens(&catalog, make_config(DeferredToolPolicy::default()));
    // Client-local deferral: MCP tools are omitted from the wire payload.
    let deferred_tokens = on_wire_schema_tokens(&catalog, make_config(DeferredToolPolicy::client_local(Vec::new())));
    // Baseline: the same core tools, no MCP servers, under deferral.
    let baseline_catalog = SessionToolCatalog::rebuild_from_registrations(core_registrations());
    let baseline_tokens =
        on_wire_schema_tokens(&baseline_catalog, make_config(DeferredToolPolicy::client_local(Vec::new())));

    assert!(
        eager_tokens >= deferred_tokens * 3,
        "eager payload ({eager_tokens}) must be at least 3x the deferred payload \
         ({deferred_tokens}) -- otherwise the MCP schema tax deferral removes is not real"
    );
    assert!(
        deferred_tokens <= baseline_tokens * 5 / 4,
        "with 5 MCP servers under deferral, the first-request payload ({deferred_tokens}) \
         must stay within 25% of the no-MCP baseline ({baseline_tokens})"
    );
    assert!(
        deferred_tokens < eager_tokens,
        "deferral must shrink the first-request wire payload: \
         eager={eager_tokens} deferred={deferred_tokens}"
    );
}

/// Phase 7.2: the advisory warning condition for "deferred loading is
/// disabled but the catalog would benefit from it" is pure logic in
/// `catalog_would_benefit_from_deferral`. A disabled policy + a catalog
/// that would defer (MCP present, over the count threshold, or over the
/// schema-token budget) is the only non-noisy warning case -- the count/
/// budget thresholds *triggering* deferral when enabled is correct
/// behavior, not a warning condition.
#[test]
fn catalog_would_benefit_from_deferral_detects_each_trigger() {
    // Small builtin-only catalog: no benefit (deferral would not engage).
    assert!(
        !catalog_would_benefit_from_deferral(false, 3, 500),
        "a small builtin-only catalog does not benefit from deferral"
    );
    // Any MCP tool present -> benefit (MCP schemas are the dominant cost).
    assert!(catalog_would_benefit_from_deferral(true, 1, 100), "any MCP tool means deferral would engage");
    // At the count threshold -> benefit.
    assert!(
        catalog_would_benefit_from_deferral(false, DIRECT_TOOL_EXPOSURE_THRESHOLD, 500),
        "meeting the count threshold means deferral would engage"
    );
    // Just under the count threshold but over the token budget -> benefit
    // (the single-large-server backstop).
    assert!(
        catalog_would_benefit_from_deferral(
            false,
            DIRECT_TOOL_EXPOSURE_THRESHOLD - 1,
            DIRECT_TOOL_EXPOSURE_TOKEN_BUDGET + 1,
        ),
        "exceeding the schema-token budget means deferral would engage"
    );
    // Exactly at the token budget (<=, not >) and under the count threshold,
    // no MCP -> no benefit (boundary matches the `<=` in `model_tools`).
    assert!(
        !catalog_would_benefit_from_deferral(
            false,
            DIRECT_TOOL_EXPOSURE_THRESHOLD - 1,
            DIRECT_TOOL_EXPOSURE_TOKEN_BUDGET,
        ),
        "at exactly the token budget and below the count threshold, \
        deferral does not engage (boundary is <=)"
    );
}

#[test]
fn planning_read_tools_stay_core_with_mcp_deferral() {
    let registrations = [
        tools::EXEC_COMMAND,
        tools::READ_FILE,
        tools::LIST_FILES,
        tools::GREP_FILE,
        tools::CODE_SEARCH,
        tools::REQUEST_USER_INPUT,
    ]
    .into_iter()
    .map(|name| {
        registration(name)
            .with_description("planning read tool")
            .with_parameter_schema(empty_object_schema())
    })
    .chain(std::iter::once(
        registration("mcp::context7::search")
            .with_catalog_source(ToolCatalogSource::Mcp)
            .with_llm_visibility(false)
            .with_description("search docs")
            .with_parameter_schema(empty_object_schema())
            .with_aliases(["mcp__context7__search"]),
    ))
    .collect::<Vec<_>>();
    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let planning_config = SessionToolsConfig::full_public(
        SessionSurface::AgentRunner,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    )
    .with_planning_active(true)
    .with_deferred_tool_policy(DeferredToolPolicy::client_local(Vec::new()));
    let planning_definitions = catalog.model_tools(planning_config);
    for tool in [
        tools::READ_FILE,
        tools::LIST_FILES,
        tools::GREP_FILE,
        tools::CODE_SEARCH,
    ] {
        let definition = planning_definitions
            .iter()
            .find(|definition| definition.function_name() == tool)
            .unwrap_or_else(|| panic!("missing planning definition for {tool}"));
        assert_eq!(definition.defer_loading, None, "{tool} must stay on wire in planning even with MCP deferral");
    }

    let exec_config = SessionToolsConfig::full_public(
        SessionSurface::AgentRunner,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    )
    .with_deferred_tool_policy(DeferredToolPolicy::client_local(Vec::new()));
    let grep_entry = catalog
        .entries()
        .iter()
        .find(|entry| entry.public_name == tools::GREP_FILE)
        .expect("missing grep catalog entry");
    // Structured search is always-eager (session-efficiency 2026-09-28):
    // deferred search pushed models to shell out via exec_command.
    assert!(!should_defer_tool_loading(grep_entry, &exec_config), "grep_file stays eager outside planning");

    let interactive_planning_config = SessionToolsConfig::full_public(
        SessionSurface::Interactive,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        ToolModelCapabilities::default(),
    )
    .with_planning_active(true)
    .with_deferred_tool_policy(DeferredToolPolicy::client_local(Vec::new()));
    let interactive_definitions = catalog.model_tools(interactive_planning_config);
    for tool in [tools::GREP_FILE, tools::CODE_SEARCH] {
        let definition = interactive_definitions
            .iter()
            .find(|definition| definition.function_name() == tool)
            .unwrap_or_else(|| panic!("missing interactive planning definition for {tool}"));
        assert_eq!(
            definition.defer_loading, None,
            "{tool} must stay on wire for Interactive planning with MCP deferral (session-20260923)"
        );
    }
}

#[test]
fn lean_defaults_defer_planner_and_skills_tools_outside_planning() {
    let always_eager = [
        tools::EXEC_COMMAND,
        tools::WRITE_STDIN,
        tools::SEARCH_TOOLS,
        tools::APPLY_PATCH,
        tools::CODE_SEARCH,
        tools::GREP_FILE,
        // Workflow-coordination control-plane tools stay directly callable in
        // every mode: full-auto references them in `allowed_tools` and OpenAI
        // rejects deferred references with 400 "not found in 'tools'".
        tools::TASK_TRACKER,
        tools::START_PLANNING,
    ];
    let deferrable = [
        tools::AGENT,
        tools::LIST_SKILLS,
        tools::LOAD_SKILL,
        tools::LOAD_SKILL_RESOURCE,
    ];
    let registrations = always_eager
        .into_iter()
        .chain(deferrable)
        .map(|name| {
            registration(name)
                .with_description("tool")
                .with_parameter_schema(empty_object_schema())
        })
        .collect::<Vec<_>>();
    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let policy = DeferredToolPolicy::client_local(Vec::new());
    let caps = ToolModelCapabilities { supports_apply_patch_tool: true };
    let idle_config = SessionToolsConfig::full_public(
        SessionSurface::AgentRunner,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        caps,
    )
    .with_deferred_tool_policy(policy.clone());

    for name in always_eager {
        let entry = catalog
            .entries()
            .iter()
            .find(|entry| entry.public_name == name)
            .unwrap_or_else(|| panic!("missing {name}"));
        assert!(!should_defer_tool_loading(entry, &idle_config), "{name} must stay eager");
    }
    for name in deferrable {
        let entry = catalog
            .entries()
            .iter()
            .find(|entry| entry.public_name == name)
            .unwrap_or_else(|| panic!("missing {name}"));
        assert!(should_defer_tool_loading(entry, &idle_config), "{name} must defer outside planning");
    }

    let planning_config = SessionToolsConfig::full_public(
        SessionSurface::AgentRunner,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        caps,
    )
    .with_planning_active(true)
    .with_deferred_tool_policy(policy);
    for name in [tools::TASK_TRACKER, tools::START_PLANNING] {
        let entry = catalog
            .entries()
            .iter()
            .find(|entry| entry.public_name == name)
            .unwrap_or_else(|| panic!("missing {name}"));
        assert!(
            !should_defer_tool_loading(entry, &planning_config),
            "{name} must stay eager while planning is active"
        );
    }
}

#[test]
fn workflow_coordination_tools_stay_eager_under_hosted_deferral_outside_planning() {
    // Regression guard for full-auto turn 1 on OpenAI (gpt-6-luna,
    // gpt-5.6-sol): `task_tracker`/`start_planning` deferred outside planning
    // made the Responses `allowed_tools` reference unresolvable, and the
    // backend rejected the request with 400 "Tool choice 'task_tracker' not
    // found in 'tools' parameter". Workflow-coordination tools bypass the
    // full-auto allow-list, so they must stay directly callable in every
    // mode without a prior discovery round-trip.
    let registrations = [tools::TASK_TRACKER, tools::START_PLANNING, tools::AGENT]
        .into_iter()
        .map(|name| {
            registration(name)
                .with_description("tool")
                .with_parameter_schema(empty_object_schema())
        })
        .collect::<Vec<_>>();
    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let policy = DeferredToolPolicy::openai_hosted(Vec::<String>::new());
    let caps = ToolModelCapabilities { supports_apply_patch_tool: true };
    // Real full-auto runs under the AdvancedVtCode profile, where these tools
    // are visible: assert through `model_tools` (not just raw entries) so the
    // guard also fails if the tools ever drop out of the visible catalog.
    let execution_config = SessionToolsConfig::full_public(
        SessionSurface::AgentRunner,
        CapabilityLevel::CodeSearch,
        ToolDocumentationMode::Full,
        caps,
    )
    .with_tool_profile(ToolProfile::AdvancedVtCode)
    .with_deferred_tool_policy(policy);

    for name in [tools::TASK_TRACKER, tools::START_PLANNING] {
        let entry = catalog
            .entries()
            .iter()
            .find(|entry| entry.public_name == name)
            .unwrap_or_else(|| panic!("missing {name}"));
        assert!(
            !should_defer_tool_loading(entry, &execution_config),
            "{name} must stay eager outside planning so allowed_tools references resolve"
        );
    }

    // Non-workflow tools keep the lean-defaults deferral outside planning.
    let entry = catalog
        .entries()
        .iter()
        .find(|entry| entry.public_name == tools::AGENT)
        .expect("missing agent");
    assert!(should_defer_tool_loading(entry, &execution_config), "agent must still defer outside planning");

    // End-to-end through the real pipeline: the workflow tools must reach the
    // wire catalog as eager definitions (no `defer_loading` marker), with the
    // hosted discovery tool present for the still-deferred `agent` tool.
    let wire = catalog.model_tools(execution_config);
    for name in [tools::TASK_TRACKER, tools::START_PLANNING] {
        let tool = wire
            .iter()
            .find(|tool| tool.function_name() == name)
            .unwrap_or_else(|| panic!("{name} must be visible in model_tools under AdvancedVtCode"));
        assert!(tool.defer_loading.is_none(), "{name} must be eager on the wire so allowed_tools references resolve");
    }
    assert!(
        wire.iter().any(|tool| tool.is_tool_search()),
        "hosted tool_search must be injected while deferrable tools remain"
    );
}

#[test]
fn openai_policy_injects_tool_search_for_large_catalogs() {
    let exec_command = registration(tools::EXEC_COMMAND)
        .with_description("Run command")
        .with_parameter_schema(empty_object_schema());
    let mcp_tool = registration("mcp::context7::search")
        .with_catalog_source(ToolCatalogSource::Mcp)
        .with_llm_visibility(false)
        .with_description("search docs")
        .with_parameter_schema(empty_object_schema())
        .with_aliases(["mcp__context7__search"]);

    let mut registrations = vec![exec_command, mcp_tool];
    for index in 0..DIRECT_TOOL_EXPOSURE_THRESHOLD {
        let name: &'static str = Box::leak(format!("mcp::context7::resolve_{index}").into_boxed_str());
        let alias = format!("mcp__context7__resolve_{index}");
        registrations.push(
            registration(name)
                .with_catalog_source(ToolCatalogSource::Mcp)
                .with_llm_visibility(false)
                .with_description(format!("resolve docs {index}"))
                .with_parameter_schema(empty_object_schema())
                .with_aliases([alias]),
        );
    }

    let catalog = SessionToolCatalog::rebuild_from_registrations(registrations);
    let definitions = catalog.model_tools(
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities { supports_apply_patch_tool: true },
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode)
        .with_deferred_tool_policy(DeferredToolPolicy::openai_hosted(vec!["mcp__context7__search".to_string()])),
    );

    assert!(
        definitions.iter().any(|tool| tool.tool_type == "tool_search"),
        "openai hosted tool search should be injected when deferred tools exist"
    );
    let mcp_tool = definitions
        .iter()
        .find(|tool| tool.function_name() == "mcp__context7__search")
        .expect("mcp tool should be present");
    assert_eq!(mcp_tool.defer_loading, None);

    let deferred_mcp_tool = definitions
        .iter()
        .find(|tool| tool.function_name() == "mcp__context7__resolve_0")
        .expect("deferred mcp tool should be present");
    assert_eq!(deferred_mcp_tool.defer_loading, Some(true));
}

#[test]
fn openai_policy_deferred_for_small_mcp_catalog() {
    let mcp_tool = registration("mcp::context7::search")
        .with_catalog_source(ToolCatalogSource::Mcp)
        .with_llm_visibility(false)
        .with_description("search docs")
        .with_parameter_schema(empty_object_schema())
        .with_aliases(["mcp__context7__search"]);
    let second_mcp_tool = registration("mcp::context7::resolve")
        .with_catalog_source(ToolCatalogSource::Mcp)
        .with_llm_visibility(false)
        .with_description("resolve docs")
        .with_parameter_schema(empty_object_schema())
        .with_aliases(["mcp__context7__resolve"]);

    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![mcp_tool, second_mcp_tool]);
    let definitions = catalog.model_tools(
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode)
        .with_deferred_tool_policy(DeferredToolPolicy::openai_hosted(vec!["mcp__context7__search".to_string()])),
    );

    assert!(
        definitions.iter().any(|tool| tool.tool_type == "tool_search"),
        "MCP presence should trigger tool search even for a small catalog"
    );
    let mcp_tool = definitions
        .iter()
        .find(|tool| tool.function_name() == "mcp__context7__search")
        .expect("mcp tool should be present");
    assert_eq!(mcp_tool.defer_loading, None, "always-available tool stays eager");

    let direct_mcp_tool = definitions
        .iter()
        .find(|tool| tool.function_name() == "mcp__context7__resolve")
        .expect("deferred mcp tool should be present");
    assert_eq!(direct_mcp_tool.defer_loading, Some(true), "non-always-available MCP tool should be deferred");
}

#[test]
fn always_available_tools_match_registration_names_and_aliases() {
    let mcp_tool = registration("mcp::context7::search")
        .with_catalog_source(ToolCatalogSource::Mcp)
        .with_llm_visibility(false)
        .with_description("search docs")
        .with_parameter_schema(empty_object_schema())
        .with_aliases(["mcp__context7__search"]);
    let dynamic_tool = registration("dynamic_skill_tool")
        .with_description("dynamic skill tool")
        .with_parameter_schema(empty_object_schema());

    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![mcp_tool, dynamic_tool]);
    let definitions = catalog.model_tools(
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode)
        .with_deferred_tool_policy(DeferredToolPolicy::openai_hosted(vec![
            "mcp::context7::search".to_string(),
            "dynamic_skill_tool".to_string(),
        ])),
    );

    let mcp_tool = definitions
        .iter()
        .find(|tool| tool.function_name() == "mcp__context7__search")
        .expect("mcp tool should be present");
    assert_eq!(mcp_tool.defer_loading, None);

    let dynamic_tool = definitions
        .iter()
        .find(|tool| tool.function_name() == "dynamic_skill_tool")
        .expect("dynamic tool should be present");
    assert_eq!(dynamic_tool.defer_loading, None);
}

#[test]
fn unsupported_providers_keep_catalog_eager() {
    let exec_command = registration(tools::EXEC_COMMAND)
        .with_description("Run command")
        .with_parameter_schema(empty_object_schema());
    let mcp_tool = registration("mcp::context7::search")
        .with_catalog_source(ToolCatalogSource::Mcp)
        .with_llm_visibility(false)
        .with_description("search docs")
        .with_parameter_schema(empty_object_schema())
        .with_aliases(["mcp__context7__search"]);

    let catalog = SessionToolCatalog::rebuild_from_registrations(vec![exec_command, mcp_tool]);
    let definitions = catalog.model_tools(
        SessionToolsConfig::full_public(
            SessionSurface::Interactive,
            CapabilityLevel::CodeSearch,
            ToolDocumentationMode::Full,
            ToolModelCapabilities::default(),
        )
        .with_tool_profile(ToolProfile::AdvancedVtCode),
    );

    assert!(!definitions.iter().any(|tool| tool.is_tool_search()));
    assert!(
        definitions.iter().all(|tool| tool.defer_loading.is_none()),
        "unsupported providers should keep the eager catalog"
    );
}

#[test]
fn deferred_tool_policy_uses_provider_defaults() {
    let config = VTCodeConfig::default();

    let anthropic = deferred_tool_policy_for_runtime(Some(Provider::Anthropic), false, Some(&config));
    assert!(anthropic.is_enabled());
    assert_eq!(
        anthropic.tool_search_definition().map(|tool| tool.tool_type),
        Some("tool_search_tool_regex_20251119".to_string())
    );

    let openai = deferred_tool_policy_for_runtime(Some(Provider::OpenAI), true, Some(&config));
    assert!(openai.is_enabled());
    assert_eq!(openai.tool_search_definition().map(|tool| tool.tool_type), Some("tool_search".to_string()));

    // OpenAI without Responses compaction, and no explicit provider-hosted
    // tool search, falls through to client-local deferral now that
    // `client_tool_search` defaults to `true`.
    let unsupported = deferred_tool_policy_for_runtime(Some(Provider::OpenAI), false, Some(&config));
    assert!(unsupported.is_enabled());
    assert!(unsupported.is_client_local());
}

#[test]
fn client_local_policy_selected_when_flag_enabled_for_unsupported_provider() {
    let mut config = VTCodeConfig::default();
    config.tools.client_tool_search = true;

    let gemini = deferred_tool_policy_for_runtime(Some(Provider::Gemini), false, Some(&config));
    assert!(gemini.is_enabled());
    assert!(gemini.is_client_local());
    assert_eq!(gemini.tool_search_definition(), None);

    // No provider inferred (e.g. unknown/custom model) is also covered
    // by the fallthrough arm.
    let no_provider = deferred_tool_policy_for_runtime(None, false, Some(&config));
    assert!(no_provider.is_enabled());
    assert!(no_provider.is_client_local());
}

#[test]
fn client_local_policy_not_selected_when_flag_disabled() {
    let mut config = VTCodeConfig::default();
    // Default is enabled; explicitly disable it to test the fallback path.
    config.tools.client_tool_search = false;
    assert!(!config.tools.client_tool_search);

    let gemini = deferred_tool_policy_for_runtime(Some(Provider::Gemini), false, Some(&config));
    assert!(!gemini.is_enabled());
    assert!(!gemini.is_client_local());

    let no_config = deferred_tool_policy_for_runtime(Some(Provider::Gemini), false, None);
    assert!(!no_config.is_enabled());
    assert!(!no_config.is_client_local());
}

#[test]
fn anthropic_native_memory_runtime_flag_tracks_provider_and_config() {
    let mut config = VTCodeConfig::default();
    config.provider.anthropic.memory.enabled = true;

    assert!(anthropic_native_memory_enabled_for_runtime(
        Some(Provider::Anthropic),
        "claude-sonnet-5",
        Some(&config),
    ));
    assert!(!anthropic_native_memory_enabled_for_runtime(
        Some(Provider::OpenAI),
        "claude-sonnet-5",
        Some(&config),
    ));
    assert!(!anthropic_native_memory_enabled_for_runtime(Some(Provider::Anthropic), "gpt-5", Some(&config),));
    assert!(anthropic_native_memory_enabled_for_runtime(
        Some(Provider::Anthropic),
        "my-private-claude-build",
        Some(&config),
    ));
}
