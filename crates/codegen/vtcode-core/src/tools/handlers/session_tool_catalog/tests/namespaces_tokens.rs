use super::*;

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
