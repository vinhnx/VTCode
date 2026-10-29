use super::*;

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
