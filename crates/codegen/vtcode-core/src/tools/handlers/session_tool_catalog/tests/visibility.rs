use super::*;

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
